//! The zoom scroll bar along the bottom of the timeline: the sequence as a
//! strip, with the stretch on screen as a thumb a pointer moves and resizes.
//!
//! A press on the thumb scrolls the timeline, a press on either end of the
//! thumb zooms with the other end held in place, and a press on the strip
//! beside the thumb centers the view on the pressed tick and scrolls from
//! there. The press keeps the strip's mapping from pointer to tick, so the
//! grabbed point stays under the pointer while the zoom changes, and is held
//! here rather than on the element, so a drag keeps going when the pointer
//! leaves the bar; the shell routes the pointer until the button comes up.
//! Escape during the drag puts the zoom and the scroll back.

use super::AppState;
use crate::transport::TIMELINE_BASE_PPS;

/// Width of the grip at each end of a thumb wide enough for three grips, in
/// pixels.
const HANDLE_W: f32 = 6.0;
/// How far past the thumb's ends a press still takes its grip, in pixels, so
/// a grip 6 px wide is not a target the pointer has to land on exactly.
const GRIP_SLOP: f32 = 3.0;
/// The narrowest the thumb is drawn, in pixels.
const MIN_THUMB_W: f32 = 18.0;

/// Width of the grip at each end of a thumb `thumb_w` pixels wide: a third of
/// a narrow thumb, so its middle third still scrolls.
pub fn zoom_bar_grip_w(thumb_w: f32) -> f32 {
    HANDLE_W.min(thumb_w / 3.0)
}

/// The part of the zoom scroll bar a press holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomBarGrip {
    /// The thumb between its ends: scrolls.
    Thumb,
    /// The thumb's left end: the view's first tick.
    Start,
    /// The thumb's right end: the view's last tick.
    End,
}

/// The strip under the timeline at one moment: where it is in the window and
/// which ticks it spans.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Strip {
    /// Window x of the strip's left edge.
    left: f32,
    /// Width in pixels.
    width: f32,
    /// Ticks from the strip's left edge to its right edge.
    span: f64,
}

impl Strip {
    /// The tick under window `x`, on the strip's scale past its ends too.
    fn tick_at(self, x: f32) -> f64 {
        f64::from((x - self.left) / self.width) * self.span
    }

    /// The strip pixel of `tick`.
    fn x_of(self, tick: f64) -> f32 {
        (tick / self.span * f64::from(self.width)) as f32
    }
}

/// A press on the zoom scroll bar, until it is released.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomBarPress {
    grip: ZoomBarGrip,
    strip: Strip,
    /// The pressed tick less the tick the grip moves: the view's first tick
    /// for [`ZoomBarGrip::Thumb`] and [`ZoomBarGrip::Start`], its last for
    /// [`ZoomBarGrip::End`]. A press a few pixels off an end moves that end
    /// from where it is, not from the pointer.
    grab: f64,
    /// The end the grip holds still: the view's last tick for
    /// [`ZoomBarGrip::Start`], its first for the others.
    held: f64,
    /// The zoom and the scroll when the press landed.
    from: (f32, f32),
}

impl AppState {
    /// Ticks per pixel at the timeline's zoom.
    fn ticks_per_px(&self) -> f64 {
        self.ticks_per_px_at(self.zoom)
    }

    /// Ticks per pixel at `zoom`.
    fn ticks_per_px_at(&self, zoom: f32) -> f64 {
        let fps = f64::from(self.store.sequence.ticks_per_second());
        fps / f64::from(TIMELINE_BASE_PPS * zoom).max(f64::EPSILON)
    }

    /// The first and last tick of a view scrolled to `scroll_x` at `zoom`.
    fn view_at(&self, scroll_x: f32, zoom: f32) -> (f64, f64) {
        let per_px = self.ticks_per_px_at(zoom);
        let start = f64::from(scroll_x) * per_px;
        (
            start,
            start + f64::from(self.timeline_viewport_px()) * per_px,
        )
    }

    /// The view's first and last tick.
    fn view_ticks(&self) -> (f64, f64) {
        self.view_at(self.scroll_x, self.zoom)
    }

    /// The strip for a view that ends at `view_end`: the sequence, and past
    /// its end the view when the view reaches further. A held press keeps the
    /// span it found, so the strip does not rescale under the pointer. `None`
    /// before the timeline is drawn.
    fn zoom_strip(&self, view_end: f64) -> Option<Strip> {
        let left = self.timeline_content_left()?;
        let width = self.timeline_viewport_px().max(1.0);
        let span = match self.zoom_bar_press {
            Some(press) => press.strip.span,
            None => {
                let duration = self.store.sequence.duration_ticks.max(0) as f64;
                duration.max(view_end).max(1.0)
            }
        };
        Some(Strip { left, width, span })
    }

    /// The thumb of a view scrolled to `scroll_x` at `zoom`, as `(x, width)`
    /// strip pixels, and the share of the strip's width the sequence's clips
    /// are drawn across. The timeline draws at an eased scroll and zoom, and
    /// the thumb is drawn at the same ones.
    pub fn zoom_bar_thumb(&self, scroll_x: f32, zoom: f32) -> ((f32, f32), f32) {
        let (start, end) = self.view_at(scroll_x, zoom);
        let Some(strip) = self.zoom_strip(end) else {
            return ((0.0, 0.0), 1.0);
        };
        // A long sequence at a close zoom still draws a thumb a pointer can
        // find, centered on the view.
        let x = strip.x_of(start);
        let w = strip.x_of(end) - x;
        let shown = w.max(MIN_THUMB_W).min(strip.width);
        let x = (x + (w - shown) / 2.0).clamp(0.0, strip.width - shown);
        let (x, w) = (x, shown);
        let duration = self.store.sequence.duration_ticks.max(0) as f64;
        ((x, w), (duration / strip.span).clamp(0.0, 1.0) as f32)
    }

    /// Press on the zoom scroll bar at window `x`. A press beside the thumb
    /// centers the view on the pressed tick first and then holds the thumb.
    pub fn press_zoom_bar(&mut self, x: f32) {
        let (start, end) = self.view_ticks();
        let Some(strip) = self.zoom_strip(end) else {
            return;
        };
        let from = (self.zoom, self.scroll_x);
        let ((thumb_x, thumb_w), _) = self.zoom_bar_thumb(self.scroll_x, self.zoom);
        let at = x - strip.left;
        let grip_w = zoom_bar_grip_w(thumb_w);
        let thumb_end = thumb_x + thumb_w;
        let grip = if (thumb_x - GRIP_SLOP..=thumb_x + grip_w).contains(&at) {
            ZoomBarGrip::Start
        } else if (thumb_end - grip_w..=thumb_end + GRIP_SLOP).contains(&at) {
            ZoomBarGrip::End
        } else {
            ZoomBarGrip::Thumb
        };
        if grip == ZoomBarGrip::Thumb && !(thumb_x..=thumb_end).contains(&at) {
            let tick = strip.tick_at(x);
            self.scroll_view_to(tick - (end - start) / 2.0, strip);
        }
        let (start, end) = self.view_ticks();
        let moved = if grip == ZoomBarGrip::End { end } else { start };
        self.zoom_bar_press = Some(ZoomBarPress {
            grip,
            strip,
            grab: strip.tick_at(x) - moved,
            held: if grip == ZoomBarGrip::Start {
                end
            } else {
                start
            },
            from,
        });
    }

    /// Whether a press on the zoom scroll bar is held.
    pub fn zoom_bar_held(&self) -> bool {
        self.zoom_bar_press.is_some()
    }

    /// The pointer is at window `x` with the press held: the thumb follows it,
    /// or the grabbed end does and the zoom changes to fit. Returns whether
    /// the view moved.
    pub fn drag_zoom_bar(&mut self, x: f32) -> bool {
        let Some(press) = self.zoom_bar_press else {
            return false;
        };
        let before = (self.zoom, self.scroll_x);
        // The grabbed point stays under the pointer, so a grip pressed inside
        // a thumb drawn wider than the view reaches the strip's end once the
        // pointer passes it by as much.
        let tick = (press.strip.tick_at(x) - press.grab).clamp(0.0, press.strip.span);
        let fps = f64::from(self.store.sequence.ticks_per_second());
        let view_w = f64::from(self.timeline_viewport_px());
        // The zoom that shows `ticks` across the view; `set_zoom` holds it to
        // the range the timeline draws.
        let zoom_for =
            |ticks: f64| (view_w * fps / (ticks.max(1.0) * f64::from(TIMELINE_BASE_PPS))) as f32;
        match press.grip {
            ZoomBarGrip::Thumb => self.scroll_view_to(tick, press.strip),
            ZoomBarGrip::End => {
                self.set_zoom(zoom_for(tick - press.held), false);
                self.scroll_timeline_to((press.held / self.ticks_per_px()) as f32, false);
            }
            ZoomBarGrip::Start => {
                self.set_zoom(zoom_for(press.held - tick), false);
                let end_px = press.held / self.ticks_per_px();
                self.scroll_timeline_to((end_px - view_w) as f32, false);
            }
        }
        (self.zoom, self.scroll_x) != before
    }

    /// Let go of the press. Returns whether a press was held.
    pub fn release_zoom_bar(&mut self) -> bool {
        self.zoom_bar_press.take().is_some()
    }

    /// Escape with a press held: the zoom and the scroll go back to where the
    /// press found them, and the press ends. Returns whether a press was held.
    pub fn cancel_zoom_bar_drag(&mut self) -> bool {
        let Some(press) = self.zoom_bar_press.take() else {
            return false;
        };
        self.set_zoom(press.from.0, false);
        self.scroll_timeline_to(press.from.1, false);
        true
    }

    /// Scroll so the view's first tick is `start`, held inside `strip`.
    fn scroll_view_to(&mut self, start: f64, strip: Strip) {
        let (first, last) = self.view_ticks();
        let start = start.clamp(0.0, (strip.span - (last - first)).max(0.0));
        self.scroll_timeline_to((start / self.ticks_per_px()) as f32, false);
    }
}

#[cfg(test)]
#[path = "zoom_bar_tests.rs"]
mod tests;
