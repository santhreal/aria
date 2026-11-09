//! Label colors: assigning one, clearing it, and selecting by it.
//!
//! A label is organizational only — nothing renders it into a frame — so this is
//! the whole feature: one undo step per labelling, and a way to get back every clip
//! drawn in a color. The palette is [`aria_models::LabelColor`]. A clip with
//! no label of its own belongs to the group it is drawn in, so "the Forest
//! group" reaches every audio clip nobody relabelled.

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{ClipId, LabelColor, MediaId};

use super::AppState;
use super::decline::Decline;

impl AppState {
    /// Set (`Some`) or clear (`None`) the label on every selected clip, as one
    /// undo step.
    ///
    /// Writes only the clips [`Self::label_targets`] returns, and declines
    /// when it returns none: a step that changes no label would spend the next
    /// Undo on nothing. A failure part way reverts the clips already written.
    pub fn set_selection_label(&mut self, label: Option<LabelColor>) -> Result<(), EngineError> {
        let clips = match self.label_targets(label) {
            Ok(clips) => clips,
            Err(why) => {
                self.decline(why);
                return Ok(());
            }
        };
        self.one_step_as("Set label color", |state| {
            for clip in clips {
                state.apply_command(Command::Edit(EditCommand::SetClipLabel { clip, label }))?;
            }
            Ok(())
        })
    }

    /// The selected clips whose own label is not `label`: the clips a Label
    /// row writes. The row's guard and its body both read this.
    pub(crate) fn label_targets(&self, label: Option<LabelColor>) -> Result<Vec<ClipId>, Decline> {
        if self.selection.is_empty() {
            return Err(Decline::NoSelection);
        }
        let targets: Vec<ClipId> = self
            .selection
            .iter()
            .copied()
            .filter(|&clip| self.ui_clip_ref(clip).is_none_or(|ui| ui.label != label))
            .collect();
        if targets.is_empty() {
            return Err(Decline::SameLabel);
        }
        Ok(targets)
    }

    /// The label on the selected clip, if it has one.
    pub fn selected_label(&self) -> Option<LabelColor> {
        self.selected_ui_clip()?.label
    }

    /// Replace the selection with every clip carrying `label`, in timeline
    /// order. This is what makes a label useful rather than decorative: color a
    /// group once, then reach all of it later.
    pub fn select_by_label(&mut self, label: LabelColor) {
        let mut hits: Vec<(i64, u64)> = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.drawn_label() == label)
            .map(|clip| (clip.timeline_start_ticks, clip.id))
            .collect();
        hits.sort_unstable();
        if hits.is_empty() {
            self.status_message = Some("No clips with that label".into());
            return;
        }
        let count = hits.len();
        self.selection = hits
            .into_iter()
            .map(|(_, id)| ClipId::from_raw(id))
            .collect();
        self.selected_edge = None;
        self.selected_gap = None;
        self.status_message = Some(format!("Selected {}", counted(count, "clip", "clips")));
        self.refresh_projected_selection();
    }

    /// How many clips are drawn in each label, in palette order, for a
    /// surface that offers only the colors in use. One walk of the sequence.
    pub fn label_counts(&self) -> Vec<(LabelColor, usize)> {
        let mut counts = [0usize; LabelColor::ALL.len()];
        for clip in self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
        {
            counts[clip.drawn_label() as usize] += 1;
        }
        LabelColor::ALL
            .into_iter()
            .filter_map(|color| {
                let count = counts[color as usize];
                (count > 0).then_some((color, count))
            })
            .collect()
    }

    /// Set (`Some`) or clear (`None`) the label on one media-pool entry, or
    /// decline when the entry already has `label`.
    ///
    /// `media` is the projected row id, which is the raw [`aria_models::MediaId`]
    /// the bin already draws with.
    pub fn set_media_label(
        &mut self,
        media: u64,
        label: Option<LabelColor>,
    ) -> Result<(), EngineError> {
        let known = self.store.media.iter().any(|item| item.id == media);
        if known && self.media_label(media) == label {
            self.decline(Decline::SameLabel);
            return Ok(());
        }
        self.apply_command(Command::Project(ProjectCommand::SetMediaLabel {
            media: MediaId::from_raw(media),
            label,
        }))?;
        Ok(())
    }

    /// The label on one pool row, if it has one.
    pub fn media_label(&self, media: u64) -> Option<LabelColor> {
        self.store
            .media
            .iter()
            .find(|item| item.id == media)
            .and_then(|item| item.label)
    }
}
