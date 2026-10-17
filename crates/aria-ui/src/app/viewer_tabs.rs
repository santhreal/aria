//! The Viewer's tabs: Program first, then one tab per open source, and the
//! split that shows a Source tab beside Program.
//!
//! A Source tab keeps its playhead and marks while it stays open, so moving
//! between tabs never loses a marked range. The keys reach the focused tab:
//! Program, or one Source tab. Shift+2 swaps Program and the Source tab
//! focused last, and the split's left pane shows that same tab.

use std::time::Instant;

use super::AppState;
use super::pool_preview::SourceView;

/// The focused Source tab playing: where its clock started and where it stops.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SourcePlay {
    /// The tick on screen when the clock started.
    anchor: i64,
    started: Instant,
    /// Take frames per frame of real time, signed: `-2` plays backward at
    /// double speed.
    rate: i32,
    /// Forward playback stops on the frame before this one; `None` plays to
    /// the end of the take.
    until: Option<i64>,
}

/// The open Source tabs and which tab the Viewer focuses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceTabs {
    /// Open Source tabs in tab-row order.
    tabs: Vec<SourceView>,
    /// The focused Source tab; `None` focuses Program.
    focus: Option<usize>,
    /// The Source tab focused last: Shift+2 returns to it, and the split's
    /// left pane shows it.
    recent: Option<usize>,
    /// Source on the left and Program on the right in one frame.
    split: bool,
    /// The focused Source tab's playback. A change of focus stops it.
    play: Option<SourcePlay>,
    /// The token of the vsync loop that moves the playing tab. See
    /// [`Self::drive`].
    driver: u64,
}

impl SourceTabs {
    /// The open Source tabs, in tab-row order.
    pub fn tabs(&self) -> &[SourceView] {
        &self.tabs
    }

    /// The focused Source tab, or `None` while Program is focused.
    pub fn focused(&self) -> Option<&SourceView> {
        self.tabs.get(self.focus?)
    }

    pub fn focused_mut(&mut self) -> Option<&mut SourceView> {
        self.tabs.get_mut(self.focus?)
    }

    pub fn focus_index(&self) -> Option<usize> {
        self.focus
    }

    /// The Source tab focused last.
    pub fn recent(&self) -> Option<&SourceView> {
        self.tabs.get(self.recent?)
    }

    pub fn recent_index(&self) -> Option<usize> {
        self.recent
    }

    pub fn split(&self) -> bool {
        self.split
    }

    /// The Source tab the split's left pane shows, while split.
    pub fn split_source(&self) -> Option<&SourceView> {
        self.recent().filter(|_| self.split)
    }

    /// Focus the tab open on `view`'s source, or open `view` as a new tab and
    /// focus it. An open tab keeps its own position and marks. A tab linked to
    /// a timeline clip is its own tab: the same take opened from the pool, or
    /// from another clip, opens beside it. Returns the tab's index.
    pub fn open(&mut self, view: SourceView) -> usize {
        let index = match self
            .tabs
            .iter()
            .position(|tab| tab.media == view.media && tab.clip == view.clip)
        {
            Some(index) => index,
            None => {
                self.tabs.push(view);
                self.tabs.len() - 1
            }
        };
        self.focus(index);
        index
    }

    /// Focus Source tab `index`. An index past the last tab changes nothing.
    pub fn focus(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.focus != Some(index) {
            self.play = None;
        }
        self.focus = Some(index);
        self.recent = Some(index);
        true
    }

    /// Focus Program. The Source tabs stay open, and the one that had the
    /// focus stops playing.
    pub fn focus_program(&mut self) {
        self.focus = None;
        self.play = None;
    }

    /// Shift+2: from a Source tab to Program, or from Program to the Source
    /// tab focused last. Returns whether a tab changed.
    pub fn swap_program_and_recent(&mut self) -> bool {
        if self.focus.is_some() {
            self.focus_program();
            return true;
        }
        match self.recent {
            Some(index) => self.focus(index),
            None => false,
        }
    }

    /// Close Source tab `index`. A closed focused tab hands the focus to
    /// Program, and the tab beside it becomes the one Shift+2 returns to.
    pub fn close(&mut self, index: usize) -> Option<SourceView> {
        if index >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(index);
        if self.focus == Some(index) {
            self.play = None;
        }
        let shift = |at: usize| match at.cmp(&index) {
            core::cmp::Ordering::Less => Some(at),
            core::cmp::Ordering::Equal => None,
            core::cmp::Ordering::Greater => Some(at - 1),
        };
        self.focus = self.focus.and_then(shift);
        self.recent = match self.recent {
            Some(at) if at == index => {
                (!self.tabs.is_empty()).then(|| index.min(self.tabs.len() - 1))
            }
            other => other.and_then(shift),
        };
        if self.tabs.is_empty() {
            self.split = false;
        }
        Some(closed)
    }

    /// Keep the tabs `keep` accepts, updating each in place, and close the
    /// rest the way [`Self::close`] does.
    pub fn retain(&mut self, mut keep: impl FnMut(&mut SourceView) -> bool) {
        let mut index = 0;
        while index < self.tabs.len() {
            if keep(&mut self.tabs[index]) {
                index += 1;
            } else {
                self.close(index);
            }
        }
    }

    /// The focused tab's playback speed while it plays, in take frames per
    /// frame of real time.
    pub fn play_rate(&self) -> Option<i32> {
        self.play.map(|play| play.rate)
    }

    /// Play the focused tab from its frame at `rate`, stopping on the frame
    /// before `until` going forward, or at an end of the take. An `until` at or
    /// behind the frame on screen plays to the end. A zero rate, or Program
    /// focused, stops instead. Returns whether a tab plays.
    pub fn play(&mut self, rate: i32, until: Option<i64>, now: Instant) -> bool {
        let Some(tick) = self.focused().map(|view| view.tick).filter(|_| rate != 0) else {
            self.play = None;
            return false;
        };
        self.play = Some(SourcePlay {
            anchor: tick,
            started: now,
            rate,
            until: until.filter(|until| *until > tick),
        });
        true
    }

    /// Hand the playing tab to a new vsync loop and return its token, or
    /// `None` while nothing plays. The loop holding the previous token ends on
    /// its next vsync, so one loop moves the frame however often playback is
    /// started.
    pub fn drive(&mut self) -> Option<u64> {
        self.play?;
        self.driver = self.driver.wrapping_add(1);
        Some(self.driver)
    }

    /// Whether the loop holding `token` still moves a playing tab.
    pub fn driven_by(&self, token: u64) -> bool {
        self.play.is_some() && self.driver == token
    }

    /// Stop the focused tab where it is.
    pub fn stop(&mut self) {
        self.play = None;
    }

    /// Move the playing tab to the frame its clock reaches at `now`, for a take
    /// of `fps` frames per second. The clock stops on the last frame it may
    /// show: the frame before `until` or the take's last frame going forward,
    /// the first frame going backward. Returns whether the tab still plays.
    pub fn advance(&mut self, fps: f64, now: Instant) -> bool {
        let Some(play) = self.play else {
            return false;
        };
        let focus = self.focus;
        let Some(view) = focus.and_then(|index| self.tabs.get_mut(index)) else {
            self.play = None;
            return false;
        };
        let last = view.duration_ticks.saturating_sub(1).max(0);
        let elapsed = now.saturating_duration_since(play.started).as_secs_f64();
        let moved = (elapsed * fps * f64::from(play.rate)).trunc() as i64;
        let tick = play.anchor.saturating_add(moved);
        let (stop, done) = if play.rate > 0 {
            let stop = play
                .until
                .map_or(last, |until| until.saturating_sub(1))
                .min(last);
            (stop, tick >= stop)
        } else {
            (0, tick <= 0)
        };
        view.tick = if done { stop } else { tick };
        if done {
            self.play = None;
        }
        !done
    }

    /// Split the Viewer, or return it to tabs. Splitting needs a Source tab to
    /// show on the left; without one the Viewer stays as it is.
    pub fn toggle_split(&mut self) -> bool {
        if !self.split && self.recent.is_none() {
            return false;
        }
        self.split = !self.split;
        true
    }
}

impl AppState {
    /// Focus Program. The Source tabs stay open, and Shift+2 returns to the
    /// one focused last with its position and marks.
    pub fn show_program(&mut self) {
        self.sources.focus_program();
    }

    /// Focus Source tab `index`, stopping sequence playback: the transport
    /// keys now reach the source.
    pub fn focus_source_tab(&mut self, index: usize) {
        if self.sources.focus_index() == Some(index) {
            return;
        }
        if self.playing {
            self.toggle_playback();
        }
        if self.sources.focus(index) {
            self.reconcile_source_view();
        }
    }

    /// Close Source tab `index` and report which one closed.
    pub fn close_source_tab(&mut self, index: usize) {
        if let Some(closed) = self.sources.close(index) {
            self.status_message = Some(format!("Closed {}", closed.name));
        }
    }

    /// Viewer > Split: the Source tab focused last on the left and Program on
    /// the right, each with its own transport.
    pub fn toggle_viewer_split(&mut self) {
        if self.sources.toggle_split() {
            self.status_message = Some(
                if self.sources.split() {
                    "Viewer split: Source and Program"
                } else {
                    "Viewer tabs"
                }
                .into(),
            );
        } else {
            self.status_message = Some("Open a source in the Viewer to split it".into());
        }
    }
}

#[cfg(test)]
#[path = "viewer_tabs_tests.rs"]
mod tests;
