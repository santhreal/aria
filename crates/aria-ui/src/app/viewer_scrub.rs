//! Drags that start on a playhead field or the Viewer's mini ruler and outlast
//! the element pressed.
//!
//! A playhead field is the one in the Viewer's control row or the one in the
//! timeline header, and both are scrubbable. A press arms it; travel past
//! [`CLICK_SLOP_PX`] moves the playhead one frame per
//! [`PX_PER_UNIT`](super::value_scrub::PX_PER_UNIT) pixels,
//! ten with Shift and a tenth of one with Ctrl, and letting go without moving
//! opens the field for typing. The travel is relative, so the shell locks the
//! pointer for the drag.
//!
//! The mini ruler spans the tab in front: the sequence on Program, the take on
//! a Source tab. A press puts that tab on the frame under the pointer and a
//! drag follows the pointer across the strip.
//!
//! The press is held here rather than on the element, so a drag keeps going
//! when the pointer leaves the field or the 12 px strip. Escape during either
//! puts the tab back on the frame the press found.

use super::AppState;
use super::clock::ClockSite;
use super::pool_preview::scrub_tick;
use super::value_scrub::{CLICK_SLOP_PX, rate};

/// A press on a playhead field or the mini ruler, until it is released.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewerPress {
    /// A playhead field.
    Clock {
        /// The field pressed, which opens for typing on a click.
        site: ClockSite,
        /// The playhead when the press landed.
        from: i64,
        /// Pointer travel, in pixels, until the drag passes the click slop.
        offset: f32,
        /// Frames travelled so far, rate changes included.
        travel: f32,
        /// Past the click slop: this press is a drag and will not open the
        /// field.
        dragging: bool,
    },
    /// The mini ruler, with the frame the tab in front was on at the press.
    Ruler { from: i64 },
}

/// The held press, and where the mini ruler was last painted.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewerScrub {
    press: Option<ViewerPress>,
    /// `(left, width)` in window pixels. A pointer position means nothing
    /// without it, and a mouse move carries no element bounds.
    ruler: Option<(f32, f32)>,
}

impl ViewerScrub {
    /// The held press, if any.
    pub fn press(&self) -> Option<ViewerPress> {
        self.press
    }

    /// Whether a playhead field is being dragged. The shell locks the
    /// pointer while it is.
    pub fn drag_held(&self) -> bool {
        matches!(self.press, Some(ViewerPress::Clock { dragging: true, .. }))
    }
}

impl AppState {
    /// Record where the mini ruler is painted. Layout, not content: this never
    /// asks for a repaint.
    pub fn record_viewer_ruler(&mut self, left: f32, width: f32) {
        self.viewer_scrub.ruler = Some((left, width));
    }

    /// Press on the playhead field at `site`. An open field keeps the press
    /// for its caret, so a press while typing never starts a drag; a press on
    /// the other field drops the draft and starts there.
    pub fn press_clock(&mut self, site: ClockSite) {
        if self.clock_focused() {
            if self.clock_site == site {
                return;
            }
            self.focus_clock(false);
        }
        self.viewer_scrub.press = Some(ViewerPress::Clock {
            site,
            from: self.playhead.value,
            offset: 0.0,
            travel: 0.0,
            dragging: false,
        });
    }

    /// Press on the mini ruler at window `x`: the tab in front goes to the
    /// frame under the pointer. Returns whether it moved.
    pub fn press_viewer_ruler(&mut self, x: f32) -> bool {
        self.viewer_scrub.press = Some(ViewerPress::Ruler {
            from: self.ruler_frame(),
        });
        self.ruler_to(x)
    }

    /// The pointer moved `dx` pixels with the press held, and is now at window
    /// `x`. Returns whether the tab in front moved.
    pub fn drag_viewer(&mut self, dx: f32, x: f32, shift: bool, ctrl: bool) -> bool {
        let end = self.store.sequence.duration_ticks.max(0);
        let to = match self.viewer_scrub.press.as_mut() {
            None => return false,
            Some(ViewerPress::Ruler { .. }) => return self.ruler_to(x),
            Some(ViewerPress::Clock {
                from,
                offset,
                travel,
                dragging,
                ..
            }) => {
                let mut moved = dx;
                if !*dragging {
                    *offset += dx;
                    if offset.abs() < CLICK_SLOP_PX {
                        return false;
                    }
                    *dragging = true;
                    // The travel inside the slop counts toward the move.
                    moved = *offset;
                }
                // Travel past either end of the sequence is not banked, so
                // the way back moves the playhead at once.
                let low = -(*from as f32);
                let high = ((end - *from) as f32).max(low);
                *travel = (*travel + moved * rate(shift, ctrl)).clamp(low, high);
                from.saturating_add(travel.round() as i64)
            }
        };
        self.program_to(to)
    }

    /// Whether a drag of the held press moves the take of the Source tab in
    /// front rather than the playhead: a press on the mini ruler over a Source
    /// tab. A playhead field moves the playhead from either tab.
    pub fn viewer_drag_moves_take(&self) -> bool {
        matches!(self.viewer_scrub.press, Some(ViewerPress::Ruler { .. }))
            && self.sources.focused().is_some()
    }

    /// Let go of the press. A press on a playhead field that never moved
    /// opens that field for typing. Returns whether a press was held.
    pub fn release_viewer(&mut self) -> bool {
        let Some(press) = self.viewer_scrub.press.take() else {
            return false;
        };
        if let ViewerPress::Clock {
            site,
            dragging: false,
            ..
        } = press
        {
            self.clock_site = site;
            self.focus_clock(true);
        }
        true
    }

    /// Escape with a press held: the tab in front goes back to the frame the
    /// press found, and the press ends. Returns whether a press was held.
    pub fn cancel_viewer_drag(&mut self) -> bool {
        let Some(press) = self.viewer_scrub.press.take() else {
            return false;
        };
        match press {
            ViewerPress::Clock { from, .. } => {
                self.program_to(from);
            }
            ViewerPress::Ruler { from } => match self.sources.focused().map(|view| view.media) {
                Some(media) => {
                    self.hover_pool_item(media, from);
                }
                None => {
                    self.program_to(from);
                }
            },
        }
        true
    }

    /// The frame the mini ruler shows: the focused Source tab's, the
    /// playhead's otherwise.
    pub fn ruler_frame(&self) -> i64 {
        self.sources
            .focused()
            .map_or(self.playhead.value, |view| view.tick)
    }

    /// The tab in front to the frame under window `x` on the painted ruler.
    /// The take's last frame and the sequence end sit at the right edge.
    fn ruler_to(&mut self, x: f32) -> bool {
        let Some((left, width)) = self.viewer_scrub.ruler else {
            return false;
        };
        let source = self
            .sources
            .focused()
            .map(|view| (view.media, view.duration_ticks, view.tick));
        match source {
            Some((media, duration, tick)) => {
                let to = scrub_tick(x - left, width, duration);
                if to == tick {
                    return false;
                }
                // The pool tile follows the ruler, so the two pictures agree.
                self.hover_pool_item(media, to);
                true
            }
            None => {
                let end = self.store.sequence.duration_ticks.max(0);
                self.program_to(scrub_tick(x - left, width, end.saturating_add(1)))
            }
        }
    }

    /// The playhead to `tick`, clamped into the sequence, with an audio grain.
    /// Returns whether it moved.
    fn program_to(&mut self, tick: i64) -> bool {
        let tick = tick.clamp(0, self.store.sequence.duration_ticks.max(0));
        if tick == self.playhead.value {
            return false;
        }
        self.scrub_playhead(tick);
        true
    }
}

#[cfg(test)]
#[path = "viewer_scrub_tests.rs"]
mod tests;
