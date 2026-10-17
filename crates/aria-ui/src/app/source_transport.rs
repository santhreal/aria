//! A Source tab's own transport: play, J/K/L, step, and the jumps to In, Out,
//! and either end of the take.
//!
//! The transport keys reach the Source tab while the Viewer is the active panel
//! and a Source tab is in front of it; anywhere else they reach the sequence,
//! as they always did. One clock runs at a time: starting the source stops the
//! sequence, and starting the sequence stops the source.
//!
//! The clock is [`SourceTabs`](super::viewer_tabs::SourceTabs)' own. It moves
//! the tab's frame on each vsync from the take's frame rate, and the frame on
//! screen follows through the same render request a scrub makes, so a decode
//! that cannot keep up shows the newest frame it has rather than falling
//! behind.

use std::time::Instant;

use super::AppState;
use super::pool_preview::SourceView;
use crate::types::ActivePanel;

impl AppState {
    /// Whether the transport keys reach a Source tab: the Viewer is the
    /// active panel and a Source tab is in front of it.
    pub fn source_has_keys(&self) -> bool {
        self.active_panel == ActivePanel::Preview && self.sources.focused().is_some()
    }

    /// The focused Source tab's take, in frames per second.
    pub(super) fn source_fps(&self) -> Option<f64> {
        let media = self.sources.focused()?.media;
        let rate = self
            .store
            .media
            .iter()
            .find(|item| item.id == media)?
            .frame_rate;
        let fps = f64::from(rate.num) / f64::from(rate.den);
        (fps.is_finite() && fps > 0.0).then_some(fps)
    }

    /// Stop the sequence before a source plays: one clock at a time.
    fn hand_clock_to_source(&mut self) {
        if self.playing {
            self.toggle_playback();
        }
    }

    /// Space on a Source tab: play the take from its frame at 1x, or stop it.
    /// From the last frame, play starts again at In, or at the first frame.
    pub fn toggle_source_playback(&mut self) {
        if self.sources.play_rate().is_some() {
            self.sources.stop();
            return;
        }
        self.hand_clock_to_source();
        let Some(view) = self.sources.focused_mut() else {
            return;
        };
        let last = last_frame(view);
        if view.tick >= last {
            view.tick = view.mark_in.unwrap_or(0).clamp(0, last);
        }
        self.sources.play(1, None, Instant::now());
    }

    /// Shift+Space on a Source tab: play from In, stopping at Out.
    pub fn play_source_in_to_out(&mut self) {
        self.hand_clock_to_source();
        let Some(view) = self.sources.focused_mut() else {
            return;
        };
        let (start, end) = view.window();
        view.tick = start.clamp(0, last_frame(view));
        self.sources.play(1, Some(end), Instant::now());
    }

    /// J or L on a Source tab: play backward or forward. Another press in the
    /// same direction doubles the speed, up to 8x; the other direction starts
    /// again at 1x.
    pub fn shuttle_source(&mut self, dir: i32) {
        let sign = dir.signum();
        if sign == 0 {
            return;
        }
        self.hand_clock_to_source();
        let rate = match self.sources.play_rate() {
            Some(rate) if rate.signum() == sign => (rate * 2).clamp(-8, 8),
            _ => sign,
        };
        self.sources.play(rate, None, Instant::now());
    }

    /// Whether the tab in front of the Viewer plays: the focused Source tab on
    /// its own clock, or the sequence with Program in front.
    pub fn viewer_playing(&self) -> bool {
        if self.sources.focused().is_some() {
            self.sources.play_rate().is_some()
        } else {
            self.playing
        }
    }

    /// The Viewer's play button: the tab in front of it plays or stops.
    pub fn toggle_viewer_playback(&mut self) {
        if self.sources.focused().is_some() {
            self.toggle_source_playback();
        } else {
            self.toggle_playback();
        }
    }

    /// K on a Source tab: stop where it is.
    pub fn stop_source_playback(&mut self) {
        self.sources.stop();
    }

    /// One vsync of source playback. False once the source has stopped, or
    /// once the sequence took the clock.
    pub(crate) fn advance_source_playback(&mut self, now: Instant) -> bool {
        if self.playing {
            self.sources.stop();
            return false;
        }
        let Some(fps) = self.source_fps() else {
            self.sources.stop();
            return false;
        };
        self.sources.advance(fps, now)
    }

    /// Move the focused Source tab by `frames`, stopping its playback.
    pub fn step_source(&mut self, frames: i64) {
        self.park_source(|view| view.tick.saturating_add(frames));
    }

    /// Put the focused Source tab on the frame `at` picks, stopping its
    /// playback. The frame clamps into the take.
    fn park_source(&mut self, at: impl FnOnce(&SourceView) -> i64) {
        self.sources.stop();
        if let Some(view) = self.sources.focused_mut() {
            view.tick = at(view).clamp(0, last_frame(view));
        }
    }

    /// The focused Source tab to its In, or its first frame with none set.
    pub fn source_to_in(&mut self) {
        self.park_source(|view| view.mark_in.unwrap_or(0));
    }

    /// The focused Source tab to the last frame its Out keeps, or the take's
    /// last frame with none set.
    pub fn source_to_out(&mut self) {
        self.park_source(|view| view.mark_out.map_or(i64::MAX, |out| out - 1));
    }

    /// Left and Right: a frame on the tab that has the keys.
    pub fn step_here(&mut self, frames: i64) {
        if self.source_has_keys() {
            self.step_source(frames);
        } else {
            self.set_playhead(self.playhead.value + frames);
        }
    }

    /// Home: the first frame of the tab that has the keys.
    pub fn go_to_start_here(&mut self) {
        if self.source_has_keys() {
            self.park_source(|_| 0);
        } else {
            self.set_playhead(0);
        }
    }

    /// End: the last frame of the tab that has the keys.
    pub fn go_to_end_here(&mut self) {
        if self.source_has_keys() {
            self.park_source(|_| i64::MAX);
        } else {
            self.set_playhead(self.store.sequence.duration_ticks);
        }
    }
}

/// The last frame of `view`'s take.
fn last_frame(view: &SourceView) -> i64 {
    view.duration_ticks.saturating_sub(1).max(0)
}

#[cfg(test)]
#[path = "source_transport_tests.rs"]
mod tests;
