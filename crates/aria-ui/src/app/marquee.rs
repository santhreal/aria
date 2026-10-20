//! Rubber-band selection on the timeline.
//!
//! A band is two corners, and each corner is a lane plus a tick rather than a
//! pixel: the lane the pointer is over reports itself, so the selection never
//! needs the timeline's row geometry and cannot disagree with it after a lane is
//! resized or collapsed mid-drag.
//!
//! The band is session state, not an edit: it is drawn while the pointer moves
//! and it becomes a selection when the button is released.

use aria_models::ClipId;

use super::AppState;

/// A live rubber band: where it started, and where the pointer is now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marquee {
    pub from_track: u64,
    pub from_tick: i64,
    from_row: usize,
    pub to_track: u64,
    pub to_tick: i64,
    to_row: usize,
}

impl Marquee {
    /// The tick span the band covers, low first.
    pub fn ticks(&self) -> (i64, i64) {
        if self.from_tick <= self.to_tick {
            (self.from_tick, self.to_tick)
        } else {
            (self.to_tick, self.from_tick)
        }
    }

    /// The row span the band covers, low first.
    fn rows(&self) -> (usize, usize) {
        if self.from_row <= self.to_row {
            (self.from_row, self.to_row)
        } else {
            (self.to_row, self.from_row)
        }
    }
}

impl AppState {
    /// Start a band at a lane and tick. Clears the selection: a band that ends
    /// on empty space is how a deselect works.
    pub fn begin_marquee(&mut self, track: u64, tick: i64) {
        let Some(row) = self.marquee_track_row(track) else {
            return;
        };
        self.marquee = Some(Marquee {
            from_track: track,
            from_tick: tick,
            from_row: row,
            to_track: track,
            to_tick: tick,
            to_row: row,
        });
    }

    /// Move the band's free corner. Does nothing when no band is in flight, so
    /// a plain pointer move over a lane costs nothing.
    pub fn drag_marquee(&mut self, track: u64, tick: i64) -> bool {
        let Some(row) = self.marquee_track_row(track) else {
            return false;
        };
        let Some(band) = self.marquee.as_mut() else {
            return false;
        };
        if band.to_track == track && band.to_tick == tick {
            return false;
        }
        band.to_track = track;
        band.to_tick = tick;
        band.to_row = row;
        true
    }

    /// Whether a lane is inside the band's row span, so it should draw the
    /// band's shading. Row order comes from the projected track list.
    pub fn marquee_covers_row(&self, row: usize) -> bool {
        self.marquee
            .is_some_and(|band| (band.rows().0..=band.rows().1).contains(&row))
    }

    /// Release the band: select every unlocked clip it touches.
    ///
    /// A clip counts when its extent overlaps the band's tick span at all, which
    /// is what a band drawn across the middle of a row looks like it should do.
    /// A locked lane is skipped rather than selected-and-refused later.
    pub fn commit_marquee(&mut self) -> usize {
        let Some(band) = self.marquee.take() else {
            return 0;
        };
        let (low, high) = band.ticks();
        if low == high {
            // A press with no drag: the band has no width, so it encloses
            // nothing and the click reads as a deselect.
            if !self.selection.is_empty() {
                self.selection.clear();
                self.rebuild_store();
            }
            return 0;
        }
        let (first_row, last_row) = band.rows();
        let covered: Vec<u64> = self
            .store
            .sequence
            .tracks
            .iter()
            .enumerate()
            .filter(|(row, _)| (first_row..=last_row).contains(row))
            .map(|(_, track)| track)
            .filter(|track| !track.locked)
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.timeline_start_ticks < high && clip.timeline_end_ticks > low)
            .map(|clip| clip.id)
            .collect();
        self.selection = covered.iter().map(|id| ClipId::from_raw(*id)).collect();
        self.rebuild_store();
        covered.len()
    }

    /// Abandon the band without changing the selection.
    pub fn cancel_marquee(&mut self) {
        self.marquee = None;
    }

    fn marquee_track_row(&self, track: u64) -> Option<usize> {
        self.store
            .sequence
            .tracks
            .iter()
            .position(|candidate| candidate.id == track)
    }
}
