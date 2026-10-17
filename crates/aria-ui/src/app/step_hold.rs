//! A held step button: a frame on the press, then, after the press-and-hold
//! pause, a frame per frame of the tab in front of the Viewer until the button
//! is released or the step stops moving.
//!
//! Each press takes a token. A release, another press, or a step that lands
//! where it started ends the hold, and the repeat loop ends on its next turn
//! once its token is no longer the live one, so no loop outlives its press.

use std::time::Duration;

use gpui::{App, Entity};

use super::{AppState, spawn_preview};
use crate::app::clip_text::tool_hold::TOOL_HOLD_MS;

/// The pause between the press's own step and the first repeat, the same
/// press-and-hold length that opens a tool's variants: a click is one frame.
pub const REPEAT_DELAY: Duration = Duration::from_millis(TOOL_HOLD_MS);

/// The fastest and slowest repeat, so a rate read from a broken file cannot
/// spin the loop or stall it.
const RATE_RANGE: (f64, f64) = (1.0, 240.0);

/// The held step button, if any, and the token the next press takes.
#[derive(Debug, Clone, Copy, Default)]
pub struct StepHold {
    next: u64,
    live: Option<u64>,
}

impl AppState {
    /// Start a hold, ending any other, and return its token.
    fn begin_step_hold(&mut self) -> u64 {
        let hold = &mut self.step_hold;
        hold.next = hold.next.wrapping_add(1);
        hold.live = Some(hold.next);
        hold.next
    }

    /// Release the held step button.
    pub fn end_step_hold(&mut self) {
        self.step_hold.live = None;
    }

    /// Whether a step button is held.
    pub fn step_held(&self) -> bool {
        self.step_hold.live.is_some()
    }

    /// One repeat of hold `token`: step `frames` and return the wait before
    /// the next. Returns nothing, and ends the hold, once the hold was
    /// released or replaced or the step no longer moves.
    fn repeat_step(&mut self, token: u64, frames: i64) -> Option<Duration> {
        if self.step_hold.live != Some(token) {
            return None;
        }
        let before = self.frame_here();
        self.step_here(frames);
        if self.frame_here() == before {
            self.end_step_hold();
            return None;
        }
        Some(self.frame_interval_here())
    }

    /// The frame the transport keys move: the focused Source tab's while it
    /// has the keys, the playhead's otherwise.
    fn frame_here(&self) -> i64 {
        match self.sources.focused() {
            Some(view) if self.source_has_keys() => view.tick,
            _ => self.playhead.value,
        }
    }

    /// One frame of the tab the transport keys move.
    fn frame_interval_here(&self) -> Duration {
        let fps = self
            .source_has_keys()
            .then(|| self.source_fps())
            .flatten()
            .unwrap_or_else(|| self.playhead.rate.as_f64());
        let fps = if fps.is_finite() { fps } else { RATE_RANGE.0 };
        Duration::from_secs_f64(1.0 / fps.clamp(RATE_RANGE.0, RATE_RANGE.1))
    }
}

/// Step the tab in front by `frames` now, and again once per frame while the
/// press lasts.
pub fn hold_step(state: Entity<AppState>, frames: i64, cx: &mut App) {
    let token = state.update(cx, |state, _| {
        state.step_here(frames);
        state.begin_step_hold()
    });
    spawn_preview(state.clone(), cx);
    // Weak, so a held button does not keep a closing session alive.
    let weak = state.downgrade();
    cx.spawn(async move |cx| {
        let mut wait = REPEAT_DELAY;
        loop {
            cx.background_executor().timer(wait).await;
            let Ok(Some(next)) = weak.update(cx, |state, cx| {
                let next = state.repeat_step(token, frames);
                if next.is_some() {
                    cx.notify();
                }
                next
            }) else {
                return;
            };
            let Some(state) = weak.upgrade() else {
                return;
            };
            cx.update(|app| spawn_preview(state, app));
            wait = next;
        }
    })
    .detach();
}

#[cfg(test)]
#[path = "step_hold_tests.rs"]
mod tests;
