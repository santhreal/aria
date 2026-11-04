//! Dragging a marker along the timeline ruler.
//!
//! A press on a marker arms the drag. Travel past [`CLICK_SLOP_PX`] moves the
//! marker by the travel, read at the timeline's zoom and rounded to whole
//! frames, so the grabbed point stays under the pointer. A sequence marker
//! stops at the sequence start and where its end meets the sequence end; a
//! clip marker stops at its clip's edges. Letting go with no travel is a click,
//! which moves the playhead to the marker.
//!
//! A drag is one undo step, listed as `Move marker`: every move rolls back
//! what the previous move wrote and writes the whole travel again. Escape puts
//! the marker back.

use aria_models::ClipId;
use aria_plugin::EditGroup;

use super::AppState;
use super::value_scrub::CLICK_SLOP_PX;
use crate::transport::TIMELINE_BASE_PPS;

/// The undo step a marker drag writes.
pub const MOVE_MARKER: &str = "Move marker";

/// A press held on a ruler marker.
pub(crate) struct MarkerDrag {
    marker: u64,
    /// The marker's tick when the press landed.
    from: i64,
    /// The first and last tick the marker may stand on.
    span: (i64, i64),
    /// The pointer's window x at the press.
    x: f32,
    /// Whether the pointer has left the click slop.
    dragged: bool,
    /// What the open group holds.
    group: Option<EditGroup>,
    /// The tick the open group moved the marker to.
    applied: i64,
}

impl AppState {
    /// Whether a press holds a ruler marker.
    pub fn marker_held(&self) -> bool {
        self.marker_drag.is_some()
    }

    /// Press on `marker` at window `x`. Returns whether the marker is in the
    /// sequence.
    pub fn press_marker(&mut self, marker: u64, x: f32) -> bool {
        self.cancel_marker_drag();
        let Some((from, clip, length)) = self
            .store
            .sequence
            .markers
            .iter()
            .find(|row| row.id == marker)
            .map(|row| (row.tick, row.clip_id, row.duration_ticks))
        else {
            return false;
        };
        let (first, last) = match clip {
            Some(clip) => {
                let Some(clip) = self.ui_clip_ref(ClipId::from_raw(clip)) else {
                    return false;
                };
                (clip.timeline_start_ticks, clip.timeline_end_ticks - 1)
            }
            None => (0, self.store.sequence.duration_ticks - length.max(0)),
        };
        self.marker_drag = Some(MarkerDrag {
            marker,
            from,
            // A marker already past its span, on a sequence shortened under
            // it, does not jump at the first move.
            span: (first.min(from), last.max(from)),
            x,
            dragged: false,
            group: None,
            applied: from,
        });
        true
    }

    /// The pointer is at window `x` with the press held. Returns whether the
    /// marker moved.
    pub fn drag_marker(&mut self, x: f32) -> bool {
        let pps = (TIMELINE_BASE_PPS * self.zoom).max(f32::EPSILON);
        let fps = self.store.sequence.ticks_per_second();
        let Some(mut drag) = self.marker_drag.take() else {
            return false;
        };
        let travel = x - drag.x;
        drag.dragged |= travel.abs() >= CLICK_SLOP_PX;
        let to = drag
            .from
            .saturating_add((travel / pps * fps).round() as i64)
            .clamp(drag.span.0, drag.span.1);
        let moved = drag.dragged && to != drag.applied;
        if moved {
            if let Some(group) = drag.group.take() {
                self.roll_back(group);
            }
            drag.applied = drag.from;
            let marker = drag.marker;
            let wrote =
                (to != drag.from).then(|| self.edit_group(|state| state.move_marker(marker, to)));
            // A move that landed has projected, so this poll projects only a
            // rollback with nothing written over it.
            self.sync_from_engine();
            match wrote {
                Some(Ok((group, ()))) => {
                    drag.group = Some(group);
                    drag.applied = to;
                }
                Some(Err(err)) => self.report_error(&err),
                None => {}
            }
        }
        self.marker_drag = Some(drag);
        moved
    }

    /// Let go of the marker. A drag keeps what it wrote as one undo step; a
    /// press with no travel moves the playhead to the marker. Returns whether
    /// a press was held.
    pub fn release_marker(&mut self) -> bool {
        let Some(drag) = self.marker_drag.take() else {
            return false;
        };
        match drag.group {
            Some(group) => {
                group.commit_as(MOVE_MARKER);
                self.sync_from_engine();
            }
            None if !drag.dragged => self.set_playhead(drag.from),
            None => {}
        }
        true
    }

    /// Escape during a drag on a marker: put it back where the press found it.
    /// Returns whether a press was held.
    pub fn cancel_marker_drag(&mut self) -> bool {
        let Some(drag) = self.marker_drag.take() else {
            return false;
        };
        if let Some(group) = drag.group {
            self.roll_back(group);
            self.sync_from_engine();
        }
        true
    }
}

#[cfg(test)]
#[path = "marker_drag_tests.rs"]
mod tests;
