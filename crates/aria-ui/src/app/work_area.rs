//! The work area bar under the timeline ruler: the In to Out range as a bar a
//! pointer moves and resizes.
//!
//! A press on the bar moves both points, a press on either end moves that one,
//! and a double-click sets the range to the whole sequence. The press is held
//! here rather than on the element, so a drag keeps going when the pointer
//! leaves the 8 px bar; the shell routes the pointer until the button comes up.
//! Escape during the drag puts the range back where the press found it.

use super::AppState;
use crate::transport::TIMELINE_BASE_PPS;

/// The part of the work area bar a press holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkAreaGrip {
    /// The left end: the In point.
    Start,
    /// The right end: the Out point.
    End,
    /// The bar between them: both points.
    Bar,
}

/// A press on the work area bar, until it is released.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkAreaPress {
    grip: WorkAreaGrip,
    /// The range when the press landed, as `(start, end)` ticks.
    from: (i64, i64),
    /// The pointer's window x at the press.
    x: f32,
}

impl AppState {
    /// Press on the work area bar at window `x`.
    pub fn press_work_area(&mut self, grip: WorkAreaGrip, x: f32) {
        self.work_area_press = Some(WorkAreaPress {
            grip,
            from: self.work_area(),
            x,
        });
    }

    /// Whether a press on the work area bar is held.
    pub fn work_area_held(&self) -> bool {
        self.work_area_press.is_some()
    }

    /// The pointer is at window `x` with the press held. The travel is read at
    /// the timeline's zoom, so the grabbed point stays under the pointer.
    /// Returns whether the range moved.
    pub fn drag_work_area(&mut self, x: f32) -> bool {
        let Some(press) = self.work_area_press else {
            return false;
        };
        let duration = self.store.sequence.duration_ticks.max(0);
        if duration == 0 {
            return false;
        }
        let pps = (TIMELINE_BASE_PPS * self.zoom).max(f32::EPSILON);
        let fps = self.store.sequence.ticks_per_second();
        let delta = ((x - press.x) / pps * fps).round() as i64;
        let (start, end) = press.from;
        // An end stops one frame short of the other, so the range never
        // collapses to nothing.
        let (start, end) = match press.grip {
            WorkAreaGrip::Bar => {
                let delta = delta.clamp(-start, duration - end);
                (start + delta, end + delta)
            }
            WorkAreaGrip::Start => ((start + delta).clamp(0, (end - 1).max(0)), end),
            WorkAreaGrip::End => (
                start,
                (end + delta).clamp((start + 1).min(duration), duration),
            ),
        };
        self.set_work_area(start, end)
    }

    /// Let go of the press. Returns whether a press was held.
    pub fn release_work_area(&mut self) -> bool {
        self.work_area_press.take().is_some()
    }

    /// Escape with a press held: the range goes back to where the press found
    /// it, and the press ends. Returns whether a press was held.
    pub fn cancel_work_area_drag(&mut self) -> bool {
        let Some(press) = self.work_area_press.take() else {
            return false;
        };
        self.set_work_area(press.from.0, press.from.1);
        true
    }

    /// A double-click on the bar: the work area spans the whole sequence.
    pub fn work_area_to_sequence(&mut self) {
        let duration = self.store.sequence.duration_ticks.max(0);
        self.set_work_area(0, duration);
        self.status_message = Some("Work area set to the sequence".into());
    }

    /// Put In at `start` and Out at `end`. A loop or a work area play that is
    /// running ends at the new Out. Returns whether either point moved.
    fn set_work_area(&mut self, start: i64, end: i64) -> bool {
        let next = (Some(start), Some(end));
        if (self.mark_in, self.mark_out) == next {
            return false;
        }
        (self.mark_in, self.mark_out) = next;
        let end = self.active_playback_end();
        if let Some(cadence) = self.playback_cadence.as_mut() {
            cadence.end = end;
        }
        true
    }
}

#[cfg(test)]
#[path = "work_area_tests.rs"]
mod tests;
