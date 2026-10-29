//! Replace-media and template-slot commands.
//!
//! Three engine commands had no operator path: `SetClipMedia` (repoint a clip at
//! other pooled media, keeping its placement, transform, and effects),
//! `SetReplaceable` (mark a clip as a fillable template slot), and
//! `SetTextEditable` (let a template's text be retyped).
//!
//! The model validates each of them, so the setters here refuse the same cases
//! ahead of the command rather than letting an error surface from a click: a
//! freeze-frame clip cannot be repointed, a slot must sit on a media clip whose
//! lane matches the slot's media restriction, and text editability applies only
//! to text generators.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, MediaId, Replaceable, SlotMedia, TimeRange, TrackId};

use super::AppState;

/// Every slot restriction, with its label, in one place so a picker and a test
/// read the same list.
pub(crate) const SLOT_MEDIA: [(SlotMedia, &str); 4] = [
    (SlotMedia::Any, "Any visual"),
    (SlotMedia::VideoOnly, "Video only"),
    (SlotMedia::ImageOnly, "Image only"),
    (SlotMedia::AudioOnly, "Audio only"),
];

/// The label of one restriction.
pub(crate) fn slot_media_label(accepts: SlotMedia) -> &'static str {
    SLOT_MEDIA
        .iter()
        .find(|(kind, _)| *kind == accepts)
        .map(|(_, label)| *label)
        .unwrap_or("Any visual")
}

impl AppState {
    /// Repoint `clip` at other pooled media, keeping its timeline placement, its
    /// transform, and its effects. The source window keeps the clip's current
    /// length where the new media is long enough, and otherwise takes as much as
    /// the media has. Returns whether the clip took it.
    ///
    /// Inert on unknown media, on a generated clip, and on a freeze-frame clip,
    /// all of which the model refuses.
    pub fn replace_clip_media(
        &mut self,
        clip: ClipId,
        media: MediaId,
    ) -> Result<bool, EngineError> {
        let Some(ui) = self.ui_clip_ref(clip) else {
            return Ok(false);
        };
        if ui.media_id.is_none() || ui.generator.is_some() {
            return Ok(false);
        }
        let want = (ui.source_end_ticks - ui.source_start_ticks).max(1);
        let source = {
            let engine = self.engine.lock();
            let Some(item) = engine.project().media(media) else {
                return Ok(false);
            };
            // A still repeats one frame for any window, so it can always cover
            // the clip. Real media caps at what it holds.
            let length = if item.is_image {
                want
            } else {
                want.min(item.duration.value)
            };
            if length <= 0 {
                return Ok(false);
            }
            TimeRange::at_rate(0, length, item.frame_rate)
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMedia {
            clip,
            media,
            source,
        }))?;
        self.status_message = Some("Replaced media".into());
        Ok(true)
    }

    /// Repoint the selected clip. Inert with no selection.
    pub fn replace_selected_media(&mut self, media: MediaId) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.replace_clip_media(clip, media)?;
        Ok(())
    }

    /// Repoint whichever clip covers `tick` on `track`. Returns whether one was
    /// there to repoint, so a drop that hit empty lane can fall through to
    /// placing the media instead.
    pub fn replace_media_at(
        &mut self,
        media: MediaId,
        track: TrackId,
        tick: i64,
    ) -> Result<bool, EngineError> {
        let Some(clip) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .and_then(|lane| {
                lane.clips.iter().find(|clip| {
                    tick >= clip.timeline_start_ticks && tick < clip.timeline_end_ticks
                })
            })
            .map(|clip| ClipId::from_raw(clip.id))
        else {
            return Ok(false);
        };
        self.replace_clip_media(clip, media)
    }

    /// Whether the selected clip can carry a template slot at all: the model
    /// takes only media clips, so a generator or a title can never be one.
    pub fn selected_can_be_slot(&self) -> bool {
        self.selected_ui_clip()
            .map(|clip| clip.media_id.is_some() && clip.generator.is_none())
            .unwrap_or(false)
    }

    /// Which slot restrictions the selected clip's lane accepts. Visual
    /// restrictions belong on a video lane and `AudioOnly` on an audio lane, so
    /// offering the other three on an audio clip would only produce a refusal.
    pub fn selected_slot_choices(&self) -> Vec<(SlotMedia, &'static str)> {
        let audio = self
            .selected_ui_clip()
            .map(|clip| clip.kind == crate::types::UiClipKind::Audio)
            .unwrap_or(false);
        SLOT_MEDIA
            .iter()
            .copied()
            .filter(|(kind, _)| (*kind == SlotMedia::AudioOnly) == audio)
            .collect()
    }

    /// The template slot on the selected clip, as projected.
    pub fn selected_slot(&self) -> Option<crate::types::UiSlot> {
        self.selected_ui_clip()?.replaceable
    }

    /// Mark the selected clip as a template slot, or unmark it. The order is the
    /// next free one in the sequence, so authoring in click order fills in that
    /// order.
    pub fn toggle_selected_replaceable(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if self.selected_slot().is_some() {
            self.apply_command(Command::Edit(EditCommand::SetReplaceable {
                clip,
                replaceable: None,
            }))?;
            self.status_message = Some("Cleared template slot".into());
            return Ok(());
        }
        if !self.selected_can_be_slot() {
            self.status_message = Some("Only media clips can be template slots".into());
            return Ok(());
        }
        let accepts = self
            .selected_slot_choices()
            .first()
            .map(|(kind, _)| *kind)
            .unwrap_or(SlotMedia::Any);
        let order = self.next_slot_order();
        self.apply_command(Command::Edit(EditCommand::SetReplaceable {
            clip,
            replaceable: Some(Replaceable::new(order).with_accepts(accepts)),
        }))?;
        self.status_message = Some(format!("Template slot {order}"));
        Ok(())
    }

    /// The lowest order not already taken by a slot in the sequence.
    fn next_slot_order(&self) -> u32 {
        let mut taken: Vec<u32> = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter_map(|clip| clip.replaceable.as_ref().map(|slot| slot.order))
            .collect();
        taken.sort_unstable();
        let mut order = 0;
        for used in taken {
            if used == order {
                order += 1;
            }
        }
        order
    }

    /// Change what media the selected slot accepts. Inert when the clip has no
    /// slot, or when the lane would refuse the restriction.
    pub fn set_selected_slot_accepts(&mut self, accepts: SlotMedia) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(slot) = self.selected_slot() else {
            return Ok(());
        };
        if !self
            .selected_slot_choices()
            .iter()
            .any(|(kind, _)| *kind == accepts)
        {
            return Ok(());
        }
        let mut marker = Replaceable::new(slot.order).with_accepts(accepts);
        marker.label = slot.label.as_ref().map(|hint| hint.to_string());
        self.apply_command(Command::Edit(EditCommand::SetReplaceable {
            clip,
            replaceable: Some(marker),
        }))?;
        Ok(())
    }

    /// Move the selected slot earlier or later in fill order. Never goes below
    /// zero, and keeps the restriction and the hint.
    pub fn nudge_selected_slot_order(&mut self, delta: i32) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(slot) = self.selected_slot() else {
            return Ok(());
        };
        let order = (slot.order as i64 + delta as i64).max(0) as u32;
        if order == slot.order {
            return Ok(());
        }
        let mut marker = Replaceable::new(order).with_accepts(slot.accepts);
        marker.label = slot.label.as_ref().map(|hint| hint.to_string());
        self.apply_command(Command::Edit(EditCommand::SetReplaceable {
            clip,
            replaceable: Some(marker),
        }))?;
        Ok(())
    }

    /// Set (or, with an empty string, clear) the slot's placeholder hint.
    pub fn set_selected_slot_label(&mut self, label: &str) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(slot) = self.selected_slot() else {
            return Ok(());
        };
        let trimmed = label.trim();
        let mut marker = Replaceable::new(slot.order).with_accepts(slot.accepts);
        marker.label = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if marker.label.as_deref() == slot.label.as_deref() {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetReplaceable {
            clip,
            replaceable: Some(marker),
        }))?;
        Ok(())
    }

    /// Whether the selected clip is a text generator, which is the only clip
    /// whose text a template can offer for retyping.
    pub fn selected_is_text(&self) -> bool {
        self.selected_ui_clip()
            .map(|clip| clip.kind == crate::types::UiClipKind::Text)
            .unwrap_or(false)
    }

    /// Toggle whether this text clip's content is editable in a template.
    pub fn toggle_selected_text_editable(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.selected_is_text() {
            return Ok(());
        }
        let editable = !self
            .selected_ui_clip()
            .map(|clip| clip.text_editable)
            .unwrap_or(false);
        self.apply_command(Command::Edit(EditCommand::SetTextEditable {
            clip,
            editable,
        }))?;
        Ok(())
    }
}
