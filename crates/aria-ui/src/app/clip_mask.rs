//! Mask, chroma-key, and track-matte commands for the selected clip.
//!
//! A clip holds a stack of masks. `AddClipMask`, `SetClipMask`,
//! `RemoveClipMask` and `MoveClipMask` address one mask by its index, and
//! `SetClipMask` carries that whole mask, so every setter here starts from
//! the model mask and changes one field: a keyframed property keeps its keys
//! through an edit to another. `SetClipChroma` and `SetClipMatte` carry the
//! whole value and are `Option`, so clearing is the same command with `None`.
//!
//! The numeric knobs are *not* here. A scalar edit goes through
//! `clip_params.rs`, which addresses the one property. What is left is the
//! structural part — which shape, which mode, which key colour, which stencil.
//!
//! One mask of the selected clip is active: the Viewer handles, the Pen, and
//! the tracker edit it. A new mask becomes active; removing or moving masks
//! keeps the active one pointing at the same mask.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ChromaKey, Easing, Mask, MaskKind, MaskMode, MatteSource, Param, SegmentSample, ShapePathPoint,
    TrackMatte,
};

use super::AppState;
use crate::types::{UiChroma, UiMask, UiMatte};

fn chroma_from_ui(ui: &UiChroma) -> ChromaKey {
    ChromaKey {
        rgb: ui.rgb,
        strength: Param::Constant(ui.strength),
        shadow: Param::Constant(ui.shadow),
    }
}

/// Write `value` into `param` at clip-relative `tick`: a keyframe there,
/// keeping the easing of one already there, when the property is keyframed,
/// and the value itself when it is not.
fn land<T: SegmentSample>(param: &mut Param<T>, tick: i64, value: T) {
    if param.is_animated() {
        let easing = param.easing_at(tick).unwrap_or(Easing::Linear);
        param.set_keyframe(tick, value, easing);
    } else {
        *param = Param::Constant(value);
    }
}

fn matte_from_ui(ui: &UiMatte) -> TrackMatte {
    TrackMatte {
        source: ui.source,
        invert: ui.invert,
        softness: Param::Constant(ui.softness),
    }
}

impl AppState {
    /// The index of the selected clip's active mask, or `None` when the clip
    /// has no mask. An index past the end of the stack reads as the last mask.
    pub fn active_mask(&self) -> Option<u32> {
        let clip = self.selected_clip()?;
        let count = self.selected_model()?.masks.len();
        let last = count.checked_sub(1)?;
        let index = match self.tool_session.active_mask {
            Some((owner, index)) if owner == clip => index as usize,
            _ => 0,
        };
        Some(index.min(last) as u32)
    }

    /// Make the mask at `index` of the selected clip the one the Viewer
    /// handles, the Pen, and the tracker edit.
    pub fn set_active_mask(&mut self, index: u32) {
        let Some(clip) = self.selected_clip() else {
            return;
        };
        if self.tool_session.active_mask != Some((clip, index)) {
            // A path edit belongs to the mask it started on.
            self.tool_session.mask_path = None;
        }
        self.tool_session.active_mask = Some((clip, index));
    }

    /// The selected clip's active mask as projected.
    pub fn active_ui_mask(&self) -> Option<UiMask> {
        let index = self.active_mask()?;
        self.selected_ui_clip_ref()?
            .masks
            .get(index as usize)
            .copied()
    }

    /// The selected clip's active mask in the model.
    pub(crate) fn active_model_mask(&self) -> Option<&Mask> {
        let index = self.active_mask()?;
        self.selected_model()?.masks.get(index as usize)
    }

    /// The selected clip's parameter tick under the playhead: where its masks
    /// are read and where an edit to a keyframed mask property is keyed.
    pub(crate) fn selected_param_tick(&self) -> Option<i64> {
        Some(self.selected_model()?.param_tick(self.playhead.value))
    }

    /// The active mask's centre, size, and rotation at the playhead, which is
    /// where the Viewer draws its handles and where a drag of them starts.
    pub fn active_mask_pose(&self) -> Option<([f32; 2], [f32; 2], f32)> {
        let mask = self.active_model_mask()?;
        let tick = self.selected_param_tick()?;
        Some((
            mask.center.sample(tick),
            mask.size.sample(tick),
            mask.rotation.sample(tick),
        ))
    }

    /// Send `mask` as the mask at `index` of the selected clip, starting from
    /// the model mask so the fields `edit` leaves alone keep their keyframes.
    fn edit_mask(&mut self, index: u32, edit: impl FnOnce(&mut Mask)) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(mut mask) = self
            .selected_model()
            .and_then(|model| model.masks.get(index as usize))
            .cloned()
        else {
            return Ok(());
        };
        edit(&mut mask);
        self.apply_command(Command::Edit(EditCommand::SetClipMask {
            clip,
            index,
            mask,
        }))?;
        Ok(())
    }

    /// Put `mask` at the bottom of the selected clip's stack and make it the
    /// active mask.
    fn push_mask(&mut self, mask: Mask) -> Result<(), EngineError> {
        let (Some(clip), Some(count)) = (
            self.selected_clip(),
            self.selected_model().map(|model| model.masks.len()),
        ) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::AddClipMask { clip, mask }))?;
        self.tool_session.active_mask = Some((clip, count as u32));
        self.tool_session.mask_path = None;
        Ok(())
    }

    /// Add a mask of `kind` to the selected clip.
    pub fn add_selected_mask(&mut self, kind: MaskKind) -> Result<(), EngineError> {
        self.push_mask(Mask::new(kind))
    }

    /// How the mask at `index` combines with the masks above it.
    pub fn set_selected_mask_mode(
        &mut self,
        index: u32,
        mode: MaskMode,
    ) -> Result<(), EngineError> {
        self.edit_mask(index, |mask| mask.mode = mode)
    }

    /// Keep the outside of the mask at `index` instead of the inside.
    pub fn toggle_selected_mask_invert(&mut self, index: u32) -> Result<(), EngineError> {
        self.edit_mask(index, |mask| mask.invert = !mask.invert)
    }

    /// Remove the mask at `index` from the selected clip.
    pub fn remove_selected_mask(&mut self, index: u32) -> Result<(), EngineError> {
        let (Some(clip), Some(active)) = (self.selected_clip(), self.active_mask()) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::RemoveClipMask { clip, index }))?;
        // The active mask stays the same mask, or becomes the one above a
        // removed active mask.
        let active = match active.cmp(&index) {
            std::cmp::Ordering::Greater => active - 1,
            std::cmp::Ordering::Equal => index.saturating_sub(1),
            std::cmp::Ordering::Less => active,
        };
        self.tool_session.active_mask = Some((clip, active));
        self.tool_session.mask_path = None;
        self.status_message = Some(format!("Removed mask {}", index + 1));
        Ok(())
    }

    /// Move the mask at `from` to `to` in the selected clip's stack.
    pub fn move_selected_mask(&mut self, from: u32, to: u32) -> Result<(), EngineError> {
        let (Some(clip), Some(active)) = (self.selected_clip(), self.active_mask()) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::MoveClipMask {
            clip,
            from_index: from,
            to_index: to,
        }))?;
        self.tool_session.active_mask = Some((clip, moved_index(active, from, to)));
        Ok(())
    }

    /// Remove the active mask.
    pub fn clear_selected_mask(&mut self) -> Result<(), EngineError> {
        match self.active_mask() {
            Some(index) => self.remove_selected_mask(index),
            None => Ok(()),
        }
    }

    /// Invert the active mask.
    pub fn toggle_active_mask_invert(&mut self) -> Result<(), EngineError> {
        match self.active_mask() {
            Some(index) => self.toggle_selected_mask_invert(index),
            None => Ok(()),
        }
    }

    /// Set the active mask's center, size, and rotation at the playhead,
    /// keeping the shape and the remaining knobs. The on-canvas handles build
    /// all three, so a drag is one edit rather than one per pointer move.
    ///
    /// Each of the three that changed is keyed at the playhead when it is
    /// keyframed, so a tracked or animated mask keeps its curve, and replaced
    /// when it is not. One that did not change keeps its keyframes untouched.
    pub fn set_selected_mask_geometry(
        &mut self,
        center: [f32; 2],
        size: [f32; 2],
        rotation: f32,
    ) -> Result<(), EngineError> {
        let (Some(index), Some(before), Some(tick)) = (
            self.active_mask(),
            self.active_mask_pose(),
            self.selected_param_tick(),
        ) else {
            return Ok(());
        };
        if !center.iter().chain(size.iter()).all(|v| v.is_finite()) || !rotation.is_finite() {
            return Ok(());
        }
        let size = [size[0].clamp(0.01, 10.0), size[1].clamp(0.01, 10.0)];
        let rotation = rotation.clamp(-3600.0, 3600.0);
        if before == (center, size, rotation) {
            return Ok(());
        }
        self.edit_mask(index, |mask| {
            if center != before.0 {
                land(&mut mask.center, tick, center);
            }
            if size != before.1 {
                land(&mut mask.size, tick, size);
            }
            if rotation != before.2 {
                land(&mut mask.rotation, tick, rotation);
            }
        })
    }

    /// Reshape the active mask to the path `points` and `closed` state at the
    /// playhead: the keyframe there when the path is keyframed, added when none
    /// sits there, or the path itself when it is not. The mask keeps everything
    /// else: its feather, centre, rotation and their keyframes, its other path
    /// keyframes, its mode, and its invert.
    pub fn set_selected_mask_path(
        &mut self,
        points: Vec<ShapePathPoint>,
        closed: bool,
    ) -> Result<(), EngineError> {
        self.reshape_selected_mask_path(closed, |mask, tick| mask.set_path_at(tick, points))
    }

    /// Change the active mask's path with `reshape`, handed the clip's
    /// parameter tick under the playhead, as one edit that leaves it a path
    /// mask, closed or open as `closed` says.
    pub(super) fn reshape_selected_mask_path(
        &mut self,
        closed: bool,
        reshape: impl FnOnce(&mut Mask, i64),
    ) -> Result<(), EngineError> {
        let (Some(index), Some(tick)) = (self.active_mask(), self.selected_param_tick()) else {
            return Ok(());
        };
        self.edit_mask(index, |mask| {
            mask.kind = MaskKind::Path;
            reshape(mask, tick);
            mask.path_closed = closed;
        })
    }

    /// Add a closed path mask through `points` to the selected clip.
    pub fn add_selected_mask_path(
        &mut self,
        points: Vec<ShapePathPoint>,
    ) -> Result<(), EngineError> {
        self.push_mask(Mask::from_path(points, true))
    }

    fn send_chroma(&mut self, chroma: Option<ChromaKey>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipChroma { clip, chroma }))?;
        Ok(())
    }

    /// Key on a colour, seeding a usable tolerance so the first click does
    /// something visible.
    pub fn set_selected_chroma_color(&mut self, rgb: [u8; 3]) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let chroma = match &ui.chroma {
            Some(existing) => {
                let mut chroma = chroma_from_ui(existing);
                chroma.rgb = rgb;
                chroma
            }
            None => ChromaKey::new(rgb),
        };
        self.send_chroma(Some(chroma))
    }

    pub fn clear_selected_chroma(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_chroma(None)?;
        self.status_message = Some("Cleared chroma key".into());
        Ok(())
    }

    fn send_matte(&mut self, matte: Option<TrackMatte>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMatte { clip, matte }))?;
        Ok(())
    }

    /// Whether a clip on a higher lane overlaps the selected clip.
    ///
    /// The stencil is not chosen in the inspector: the renderer reads whatever
    /// layer sits directly above, and a matted clip with nothing above it draws
    /// unmatted. So this is what makes the difference between a matte that cuts
    /// and one that is inert, and the setters say which the operator just got.
    pub fn matte_stencil_ready(&self) -> bool {
        let Some(ui) = self.selected_ui_clip() else {
            return false;
        };
        let tracks = &self.store.sequence.tracks;
        let Some(lane) = tracks.iter().position(|t| t.id == ui.track_id) else {
            return false;
        };
        // Tracks are held top-first, so every lane above this one has a
        // smaller index.
        tracks[..lane].iter().flat_map(|t| t.clips.iter()).any(|c| {
            c.timeline_start_ticks < ui.timeline_end_ticks
                && c.timeline_end_ticks > ui.timeline_start_ticks
        })
    }

    /// Cut the selected clip with the lane above, reading `source`.
    ///
    /// Seeds a soft edge of zero: a hard cut is what a matte means before the
    /// operator asks for a falloff.
    pub fn set_selected_matte_source(&mut self, source: MatteSource) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let matte = match ui.matte.as_ref() {
            Some(existing) => TrackMatte {
                source,
                ..matte_from_ui(existing)
            },
            None => TrackMatte {
                source,
                invert: false,
                softness: Param::Constant(0.0),
            },
        };
        let ready = self.matte_stencil_ready();
        self.send_matte(Some(matte))?;
        self.status_message = Some(if ready {
            format!("Track matte reads {} above", source.label())
        } else {
            format!(
                "Track matte set to {}; nothing above this clip to cut with",
                source.label()
            )
        });
        Ok(())
    }

    pub fn clear_selected_matte(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_matte(None)?;
        self.status_message = Some("Removed track matte".into());
        Ok(())
    }

    /// Flip which side of the stencil keeps the content.
    pub fn toggle_selected_matte_invert(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(existing) = ui.matte.as_ref() else {
            return Ok(());
        };
        let mut matte = matte_from_ui(existing);
        matte.invert = !matte.invert;
        self.send_matte(Some(matte))
    }
}

/// Where the mask at `index` sits after the mask at `from` moves to `to`.
const fn moved_index(index: u32, from: u32, to: u32) -> u32 {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}
