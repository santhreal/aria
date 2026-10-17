//! The Path row of a path mask: its whole-path keyframes on the timeline.
//!
//! A path keyframe holds every vertex, so the Path row is no catalog property.
//! Each press sends the mask back with its path curve changed, as one
//! `SetClipMask` edit and one undo step. The Viewer reshapes the path at the
//! playhead (`mask_path.rs`); the row keys and unkeys it there, moves and eases
//! its keyframes, and moves the playhead between them. Every press makes its
//! mask the active one, so the Viewer handles the path the row shows.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, ClipId, Easing, Mask, MaskKind};

use super::AppState;
use super::property_rows::{
    KeyframeIconShape, MaskPathKeyDrag, MaskPathKeyMenu, TimelineKeyMark, TimelineMaskPathRow,
};

#[cfg(test)]
#[path = "mask_path_keys_tests.rs"]
mod tests;

/// The indices of `clip`'s path masks, top of the stack first.
pub(crate) fn path_masks(clip: &Clip) -> impl Iterator<Item = u32> + '_ {
    clip.masks
        .iter()
        .enumerate()
        .filter(|(_, mask)| mask.kind == MaskKind::Path)
        .map(|(index, _)| index as u32)
}

/// The name of the Path row of the mask at `index`, as the outline and the
/// property search read it.
pub(crate) fn mask_path_label(index: u32) -> gpui::SharedString {
    format!("Mask {} Path", index + 1).into()
}

/// The mask at `index` of `clip`, when it is a path.
fn path_mask(clip: &Clip, index: u32) -> Option<&Mask> {
    clip.masks
        .get(index as usize)
        .filter(|mask| mask.kind == MaskKind::Path)
}

/// The tick a keyframe drawn at timeline `tick` is stored at: the inverse of
/// [`Clip::param_timeline_tick`], which only shifts.
fn stored_tick(clip: &Clip, tick: i64) -> i64 {
    tick - clip.param_timeline_tick(0)
}

impl AppState {
    /// The Path row of the mask at `mask` on `clip`, when that mask is a path.
    pub(super) fn mask_path_row(&self, clip: &Clip, mask: u32) -> Option<TimelineMaskPathRow> {
        let held = path_mask(clip, mask)?;
        let path = &held.path;
        let at = self.playhead.value;
        let (start, end) = (clip.timeline.start.value, clip.timeline.end_tick());
        let drag = self
            .with_prop_rows(|s| s.mask_path_drag)
            .filter(|drag| drag.clip == clip.id && drag.mask == mask);
        let keyframes = path.keyframes();
        let total = keyframes.len();
        let keys: Vec<TimelineKeyMark> = keyframes
            .iter()
            .enumerate()
            .map(|(index, key)| {
                let drawn = clip.param_timeline_tick(key.tick);
                let dragged = drag.filter(|drag| drag.origin == drawn);
                TimelineKeyMark {
                    tick: dragged.map_or(drawn, |drag| (drawn + drag.delta).clamp(start, end)),
                    shape: KeyframeIconShape::classify(index, total, key.easing),
                    easing: key.easing,
                    selected: dragged.is_some(),
                    roving: false,
                    hold: key.easing.is_hold(),
                    interior: index > 0 && index + 1 < total,
                }
            })
            .collect();
        let vertices = held.path_at(clip.param_tick(at) as f64).len();
        Some(TimelineMaskPathRow {
            clip: clip.id,
            mask,
            clip_start: start,
            clip_end: end,
            label: mask_path_label(mask),
            value_label: format!("{vertices} vertices").into(),
            keyframed: path.is_animated(),
            keyed_at_playhead: path.keyframe_at(clip.param_tick(at)).is_some(),
            has_prev_key: keys.iter().any(|key| key.tick < at),
            has_next_key: keys.iter().any(|key| key.tick > at),
            keys,
        })
    }

    /// Select `clip` and make its mask at `mask` the active one, which the
    /// Viewer handles and the Pen reshapes.
    pub fn focus_mask_path(&mut self, clip: ClipId, mask: u32) {
        self.select_clip(clip);
        self.set_active_mask(mask);
    }

    /// The stopwatch on a Path row: keyframe the path at the playhead, or drop
    /// every keyframe and keep the path shown there.
    pub fn animate_mask_path(&mut self, clip: ClipId, mask: u32) -> Result<(), EngineError> {
        let at = self.playhead.value;
        self.edit_path_mask(clip, mask, |path, model| {
            let tick = model.param_tick(at);
            if path.path.is_animated() {
                path.hold_path(tick);
            } else {
                path.key_path(tick);
            }
            true
        })
    }

    /// The key button on a Path row: keyframe the path at the playhead with
    /// the path shown there, or remove the keyframe there. `keyed` is what the
    /// row drew, so a press answers the button under the pointer.
    pub fn key_mask_path(
        &mut self,
        clip: ClipId,
        mask: u32,
        keyed: bool,
    ) -> Result<(), EngineError> {
        let at = self.playhead.value;
        self.edit_path_mask(clip, mask, |path, model| {
            let tick = model.param_tick(at);
            if keyed {
                path.unkey_path(tick)
            } else {
                path.key_path(tick);
                true
            }
        })
    }

    /// Move the playhead to the path keyframe before it (`forward` false) or
    /// after it, if there is one.
    pub fn step_mask_path_key(&mut self, clip: ClipId, mask: u32, forward: bool) {
        self.focus_mask_path(clip, mask);
        let at = self.playhead.value;
        let Some(row) = self
            .clip_and_lane_kind(clip)
            .and_then(|(model, _)| self.mask_path_row(&model, mask))
        else {
            return;
        };
        let next = if forward {
            row.keys.iter().find(|key| key.tick > at)
        } else {
            row.keys.iter().rev().find(|key| key.tick < at)
        };
        if let Some(key) = next {
            self.set_playhead(key.tick);
        }
    }

    /// Shape the segment leaving the path keyframe at timeline `tick`.
    pub fn ease_mask_path_key(
        &mut self,
        clip: ClipId,
        mask: u32,
        tick: i64,
        easing: Easing,
    ) -> Result<(), EngineError> {
        self.edit_path_mask(clip, mask, |path, model| {
            path.set_path_easing(stored_tick(model, tick), easing)
        })
    }

    /// Remove the path keyframe at timeline `tick`. Removing the last one
    /// leaves its path as the mask's path.
    pub fn remove_mask_path_key(
        &mut self,
        clip: ClipId,
        mask: u32,
        tick: i64,
    ) -> Result<(), EngineError> {
        self.edit_path_mask(clip, mask, |path, model| {
            path.unkey_path(stored_tick(model, tick))
        })
    }

    /// Open the right-click menu of a path keyframe.
    pub fn open_mask_path_key_menu(&mut self, menu: MaskPathKeyMenu) {
        self.focus_mask_path(menu.clip, menu.mask);
        self.with_prop_rows_mut(|s| s.mask_path_menu = Some(menu));
    }

    pub fn close_mask_path_key_menu(&mut self) {
        self.with_prop_rows_mut(|s| s.mask_path_menu = None);
    }

    pub fn mask_path_key_menu(&self) -> Option<MaskPathKeyMenu> {
        self.with_prop_rows(|s| s.mask_path_menu)
    }

    /// Press on the path keyframe at timeline `tick`: a drag of it starts.
    pub fn begin_mask_path_key_drag(&mut self, clip: ClipId, mask: u32, tick: i64) {
        self.focus_mask_path(clip, mask);
        self.with_prop_rows_mut(|s| {
            s.mask_path_drag = Some(MaskPathKeyDrag {
                clip,
                mask,
                origin: tick,
                delta: 0,
            });
        });
    }

    /// Carry the dragged path keyframe `raw_delta` ticks from where it was
    /// pressed, inside its clip, snapping to the playhead and to the path's
    /// other keyframes within `threshold` ticks unless `alt` is held. `true`
    /// when the keyframe moved.
    pub fn update_mask_path_key_drag(&mut self, raw_delta: i64, threshold: i64, alt: bool) -> bool {
        let Some(drag) = self.with_prop_rows(|s| s.mask_path_drag) else {
            return false;
        };
        let Some((model, _)) = self.clip_and_lane_kind(drag.clip) else {
            return false;
        };
        let (start, end) = (model.timeline.start.value, model.timeline.end_tick());
        let mut target = (drag.origin + raw_delta).clamp(start, end);
        let mut guide = None;
        if !alt && threshold > 0 {
            let mut candidates = vec![(self.playhead.value, crate::snap::SnapKind::Playhead)];
            if let Some(mask) = path_mask(&model, drag.mask) {
                candidates.extend(
                    mask.path
                        .keyframes()
                        .iter()
                        .map(|key| model.param_timeline_tick(key.tick))
                        .filter(|&tick| tick != drag.origin)
                        .map(|tick| (tick, crate::snap::SnapKind::Keyframe)),
                );
            }
            let nearest = candidates
                .into_iter()
                .filter(|(tick, _)| (*tick - target).abs() <= threshold)
                .min_by_key(|(tick, _)| (*tick - target).abs());
            if let Some((tick, kind)) = nearest {
                target = tick.clamp(start, end);
                guide = Some(crate::snap::SnapCandidate::new(target, kind));
            }
        }
        self.snap_guide = guide;
        let delta = target - drag.origin;
        self.with_prop_rows_mut(|s| match &mut s.mask_path_drag {
            Some(held) if held.delta != delta => {
                held.delta = delta;
                true
            }
            _ => false,
        })
    }

    /// Release the dragged path keyframe: moved, as one edit, or, when it never
    /// moved, a click that brings the playhead to it so the Viewer shows its
    /// path.
    pub fn commit_mask_path_key_drag(&mut self) -> Result<(), EngineError> {
        self.snap_guide = None;
        let Some(drag) = self.with_prop_rows_mut(|s| s.mask_path_drag.take()) else {
            return Ok(());
        };
        if drag.delta == 0 {
            self.set_playhead(drag.origin);
            return Ok(());
        }
        self.edit_path_mask(drag.clip, drag.mask, |path, model| {
            let (start, end) = (model.timeline.start.value, model.timeline.end_tick());
            let to = (drag.origin + drag.delta).clamp(start, end);
            path.move_path_key(stored_tick(model, drag.origin), stored_tick(model, to))
        })
    }

    /// Send the mask at `index` of `clip` back with `edit` applied, when it is
    /// a path and `edit` reports a change. `edit` is handed the clip as well,
    /// to place a time on it.
    fn edit_path_mask(
        &mut self,
        clip: ClipId,
        index: u32,
        edit: impl FnOnce(&mut Mask, &Clip) -> bool,
    ) -> Result<(), EngineError> {
        self.focus_mask_path(clip, index);
        let mask = {
            let engine = self.engine.lock();
            let Some(model) = engine.project().clip(clip) else {
                return Ok(());
            };
            let Some(mut mask) = path_mask(model, index).cloned() else {
                return Ok(());
            };
            if !edit(&mut mask, model) {
                return Ok(());
            }
            mask
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMask {
            clip,
            index,
            mask,
        }))?;
        Ok(())
    }
}
