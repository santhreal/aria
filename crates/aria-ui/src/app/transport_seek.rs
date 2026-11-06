//! A run that goes on from wherever the playhead is put while it plays.
//!
//! The clock of a run counts from an anchor tick, and each vsync places the
//! playhead where the clock reads. Anything else that moves the playhead
//! during the run (Home, End, an arrow key, a marker jump, a typed timecode, a
//! ruler press, a plugin) leaves it somewhere the clock did not put it; the
//! next vsync re-anchors the clock there instead of placing the playhead back.

use std::time::{Duration, Instant};

use super::AppState;

/// How long the playhead has to hold still under playback before a mixer
/// that follows the transport restarts from it. A ruler drag moves the
/// playhead every pointer event, and a restart opens the audio device.
pub(super) const SEEK_AUDIO_SETTLE: Duration = Duration::from_millis(150);

impl AppState {
    /// Count the run's clock from the playhead as it stands.
    pub(super) fn anchor_clock_here(&mut self) {
        self.playback_anchor_tick = self.playhead.value;
        self.playback_started = Some(Instant::now());
        self.playback_placed = Some(self.playhead.value);
    }

    /// Re-anchor the run on a playhead its clock did not place, and restart
    /// the mixer once the moves settle. Called once per vsync, before the
    /// clock is read.
    pub(super) fn follow_moved_playhead(&mut self) {
        if self
            .playback_placed
            .is_some_and(|placed| placed != self.playhead.value)
        {
            self.anchor_clock_here();
            // The mixer goes quiet: until it restarts, the frame clock drives
            // the playhead, and the old stream's clock is not read.
            if self.audio_follows() {
                if self.playback_audio_resume.is_none() {
                    self.audio.publish_stop();
                }
                self.playback_audio_resume = Some(Instant::now());
            }
            if self.playback_cadence.is_some() {
                self.arm_presenting();
            }
        }
        if self
            .playback_audio_resume
            .is_some_and(|at| at.elapsed() >= SEEK_AUDIO_SETTLE)
        {
            self.playback_audio_resume = None;
            self.anchor_clock_here();
            self.republish_audio();
        }
    }
}

#[cfg(test)]
#[path = "transport_seek_tests.rs"]
mod tests;
