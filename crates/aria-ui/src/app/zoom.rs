//! Fitting the timeline to something: the whole sequence, or the selection.
//!
//! Zoom used to be incremental only, so reading a two-hour sequence or finding a
//! four-frame clip meant holding a key. A fit is one action: it computes the
//! zoom that puts a tick span inside the timeline viewport and scrolls to its
//! start, so the result is a view rather than a number of steps.
//!
//! The viewport width is derived from the same chrome the shell lays out with
//! (`transport::editor_timeline_viewport_px`), which is why the panel widths are
//! read from `ui_settings` here instead of being passed in: a collapsed pane is
//! width zero for both the layout and the fit.
//!
//! The same viewport width answers two more questions: how far out the zoom
//! goes, which is never short of the whole sequence, and whether the playhead
//! is still on screen. Following it during playback, and bringing it back into
//! view after a seek, are scrolls, not zooms, so they live here beside the one
//! writer of the scroll offset.

use super::AppState;
use crate::timeline_display::Follow;
use crate::transport;

/// Fraction of the viewport a fitted selection leaves as margin, so a fitted
/// clip does not sit flush against both edges with no context around it.
const SELECTION_MARGIN: f32 = 0.12;

impl AppState {
    /// The timeline's drawable width, in pixels: the Timeline frame as last
    /// drawn, less the outline column. The panel reads it too: what is on
    /// screen decides what a fit shows and what a draw has to build.
    ///
    /// Before the frame is first drawn, the width the editor layout leaves
    /// beside the side panes. A frame that is drawn at a new width redraws
    /// (see `AppState::record_timeline_frame`), so a draw never builds the
    /// lanes for a width other than the one on screen.
    pub fn timeline_viewport_px(&self) -> f32 {
        let outline = self.outline_drawn_width();
        if let Some([_, _, width, _]) = self.timeline_frame {
            return (width - outline).max(1.0);
        }
        let sidebar = if self.ui_settings.sidebar_collapsed {
            0.0
        } else {
            self.ui_settings.sidebar_width
        };
        let inspector = if self.ui_settings.inspector_collapsed {
            0.0
        } else {
            self.ui_settings.inspector_width
        };
        transport::editor_timeline_viewport_px(self.last_window_w, sidebar, inspector, outline)
    }

    /// Record where the timeline panel is drawn, in window pixels.
    ///
    /// True when the rectangle moved, which is the caller's cue to repaint: a
    /// notify on every measurement is a repaint loop. Same shape as
    /// [`AppState::record_ramp_frame`], for the same reason — a pointer is
    /// reported in window coordinates and a panel that converts one has to know
    /// where it starts.
    pub(crate) fn record_timeline_frame(&mut self, frame: [f32; 4]) -> bool {
        let next = Some(frame);
        let moved = next != self.timeline_frame;
        self.timeline_frame = next;
        moved
    }

    /// The window x of the timeline's first tick: the panel's left edge plus the
    /// outline column that sits before the scrolling content.
    ///
    /// `None` before the panel has been drawn. Every pointer-to-tick conversion
    /// goes through here, so a click on the ruler and a rubber band on a lane
    /// agree about where tick zero is. Subtracting the header width alone put
    /// the whole media rail's width into the sequence: a click on the first
    /// frame landed three seconds in.
    pub fn timeline_content_left(&self) -> Option<f32> {
        let outline = self.outline_drawn_width();
        self.timeline_frame.map(|[x, _, _, _]| x + outline)
    }

    /// Move the timeline's horizontal scroll, and record whether the drawn
    /// offset should ease its way there.
    ///
    /// Every writer goes through here, so the model value and the store's copy
    /// are one edit and `eased` is the only decision a caller has to make. A
    /// view moved by hand (a wheel notch, a pan drag, a pointer-anchored zoom)
    /// has to land under that input and is not eased. A
    /// view the editor chose — a fit, playback catching up — arrives instead.
    pub(crate) fn scroll_timeline_to(&mut self, x: f32, eased: bool) {
        let x = x.max(0.0);
        self.scroll_x = x;
        self.store_mut().scroll_x = x;
        self.scroll_eased = eased;
        // A scroll from anywhere else ends the settle a release started: the
        // overshoot belongs to the gesture, not to the offset.
        self.scroll_fling = false;
    }

    /// Move the timeline's scroll the way a released pan does.
    ///
    /// The distance is the momentum the release was carrying
    /// ([`gpui::motion::fling::fling_distance`]); the settle is the one spec that
    /// overshoots, so the offset passes the target it was thrown at and comes
    /// back. Goes through [`Self::scroll_timeline_to`], so the clamp and the two
    /// copies are still that method's business and this adds only the feel.
    pub(crate) fn fling_timeline_to(&mut self, x: f32) {
        self.scroll_timeline_to(x, true);
        self.scroll_fling = true;
    }

    /// The one writer of the zoom level.
    ///
    /// Two copies exist: the state's, which every verb and the fit read, and the
    /// projection's, which the panel draws from. A caller that wrote one and
    /// forgot the other left the timeline drawing at a zoom nothing else agreed
    /// with, which is why the clamp to [`Self::zoom_floor`] and
    /// [`transport::ZOOM_MAX`] lives here too: a key press, a menu row, a wheel
    /// notch, the zoom bar and a fit all arrive through this.
    ///
    /// `eased` is the same decision [`Self::scroll_timeline_to`] takes: a
    /// pointer-anchored wheel notch has to land at the scale the pointer was
    /// anchored to, so it is not eased. A keyed step, a menu row, and a fit are
    /// the editor changing the scale, and they travel there.
    pub(crate) fn set_zoom(&mut self, zoom: f32, eased: bool) {
        let zoom = zoom.clamp(self.zoom_floor(), transport::ZOOM_MAX);
        self.zoom = zoom;
        self.store_mut().zoom = zoom;
        self.zoom_eased = eased;
    }

    /// The narrowest zoom: [`transport::ZOOM_MIN`], or the zoom that fits the
    /// whole sequence in the view when that is narrower, so zooming out always
    /// ends on all of it.
    pub(crate) fn zoom_floor(&self) -> f32 {
        transport::fit_zoom(
            self.store.sequence.duration_ticks,
            self.store.sequence.ticks_per_second(),
            self.timeline_viewport_px(),
            transport::TIMELINE_BASE_PPS,
        )
        .min(transport::ZOOM_MIN)
    }

    /// Zoom by `factor` about the playhead (`=` / `-`): a playhead on screen
    /// keeps its pixel, and one off screen lands in the middle of the view.
    pub fn zoom_around_playhead(&mut self, factor: f32) {
        let width = self.timeline_viewport_px();
        let at = self.playhead_px() - self.scroll_x;
        let screen_x = if (0.0..=width).contains(&at) {
            at
        } else {
            width / 2.0
        };
        let seconds = self.playhead.value.max(0) as f32 / self.store.sequence.ticks_per_second();
        self.zoom_about(self.zoom * factor, seconds, screen_x, true);
    }

    /// Zoom by `factor` about window x `x`, the second under the pointer held
    /// under it: a wheel notch, or a click with the Zoom tool. Not eased, so
    /// the frame it lands on is drawn at the scale the pointer anchored.
    /// Returns false, and does nothing, before the timeline is drawn.
    pub(crate) fn zoom_at_pointer(&mut self, x: f32, factor: f32) -> bool {
        let Some(left) = self.timeline_content_left() else {
            return false;
        };
        let pointer = (x - left).max(0.0);
        let seconds = (self.scroll_x + pointer) / (transport::TIMELINE_BASE_PPS * self.zoom);
        self.zoom_about(self.zoom * factor, seconds, pointer, false);
        true
    }

    /// Zoom to `zoom` with the timeline second `seconds` drawn `screen_x`
    /// pixels into the view. A second inside the sequence moves right of
    /// `screen_x` when holding it there would scroll the sequence's start away
    /// with empty view past its end, so a zoom out at the floor shows the whole
    /// sequence from its first frame. A second past the end holds its pixel.
    fn zoom_about(&mut self, zoom: f32, seconds: f32, screen_x: f32, eased: bool) {
        self.set_zoom(zoom, eased);
        let pps = transport::TIMELINE_BASE_PPS * self.zoom;
        let fps = self.store.sequence.ticks_per_second();
        let end = self.store.sequence.duration_ticks.max(0) as f32 / fps * pps;
        let at = seconds * pps;
        let held = at - screen_x;
        let scroll = if at <= end {
            held.min(end - self.timeline_viewport_px())
        } else {
            held
        };
        self.scroll_timeline_to(scroll, eased);
    }

    /// Keep the playhead on screen while it moves, reporting whether the view
    /// followed it.
    ///
    /// This belongs to the transport tick rather than the draw: the drawn offset
    /// is a glide toward the committed one, and a draw that moved its own input
    /// would chase itself for as long as the glide lasted. How the view moves is
    /// the Timeline Display Settings follow: a page jump near an edge, or a
    /// scroll that holds the playhead centred. A view moved by any other writer
    /// since the last follow holds still until the playhead leaves it.
    pub(crate) fn follow_playhead(&mut self) -> bool {
        let width = self.timeline_viewport_px();
        if width <= 0.0 {
            return false;
        }
        let at = self.playhead_px();
        let moved_by_hand = self
            .follow_scroll
            .is_some_and(|followed| (followed - self.scroll_x).abs() >= 0.5);
        if moved_by_hand && at >= self.scroll_x && at <= self.scroll_x + width {
            return false;
        }
        let follow = self.ui_settings.timeline_display.follow;
        let Some(target) = follow.target(at, self.scroll_x, width) else {
            self.follow_scroll = Some(self.scroll_x);
            return false;
        };
        self.scroll_timeline_to(target, follow.glides());
        self.follow_scroll = Some(self.scroll_x);
        true
    }

    /// Bring the playhead into view after a seek: a playhead that landed off
    /// screen pages the view to rest it where a page follow rests it, whatever
    /// the follow setting, which is about playback. A playhead on screen leaves
    /// the view where it is.
    pub(super) fn reveal_playhead(&mut self) {
        let width = self.timeline_viewport_px();
        let at = self.playhead_px();
        if width <= 0.0 || (self.scroll_x..=self.scroll_x + width).contains(&at) {
            return;
        }
        if let Some(target) = Follow::Page.target(at, self.scroll_x, width) {
            self.scroll_timeline_to(target, true);
            self.follow_scroll = Some(self.scroll_x);
        }
    }

    /// The playhead's offset from the start of the timeline, in pixels at the
    /// committed zoom, computed the way the lanes draw it: ticks over the ticks
    /// in a second, times pixels per second. A follow and a reveal decide by
    /// the pixels on screen rather than a second opinion about the playhead.
    fn playhead_px(&self) -> f32 {
        let fps = self.store.sequence.ticks_per_second();
        let pps = transport::TIMELINE_BASE_PPS * self.zoom;
        (self.playhead.value.max(0) as f32 / fps) * pps
    }

    /// Fit `[start, start + duration)` and scroll so `start` is at the left
    /// edge. A zero or negative span is inert: there is nothing to fit.
    fn fit_span(&mut self, start_ticks: i64, duration_ticks: i64) -> bool {
        if duration_ticks <= 0 {
            return false;
        }
        let fps = self.store.sequence.ticks_per_second();
        let zoom = transport::fit_zoom(
            duration_ticks,
            fps,
            self.timeline_viewport_px(),
            transport::TIMELINE_BASE_PPS,
        );
        let pps = transport::TIMELINE_BASE_PPS * zoom;
        let scroll = ((start_ticks.max(0) as f32 / fps.max(1.0)) * pps).max(0.0);
        self.set_zoom(zoom, true);
        self.scroll_timeline_to(scroll, true);
        // A fit is a visible view change; the previous status line
        // is about something else and would read as this action's result.
        self.status_message = None;
        true
    }

    /// Fit the whole sequence. Inert on an empty timeline, where there is no
    /// content to frame and a fit would only reset the zoom.
    pub fn zoom_fit_sequence(&mut self) {
        let duration = self.store.sequence.duration_ticks;
        if !self.fit_span(0, duration) {
            self.status_message = Some("Nothing on the timeline to fit".into());
        }
    }

    /// Fit the selection, with a margin on each side. Inert with no selection.
    ///
    /// The span is the union of every selected clip, so a multi-lane selection
    /// frames all of it rather than the first clip found.
    pub fn zoom_fit_selection(&mut self) {
        let Some((start, end)) = self.selection_span() else {
            self.status_message = Some("Select a clip to fit".into());
            return;
        };
        let span = (end - start).max(1);
        let margin = ((span as f32) * SELECTION_MARGIN) as i64;
        if !self.fit_span(start - margin, span + margin * 2) {
            self.status_message = Some("Select a clip to fit".into());
        }
    }

    /// The tick span the selection covers, or nothing when nothing is selected.
    pub fn selection_span(&self) -> Option<(i64, i64)> {
        let mut span: Option<(i64, i64)> = None;
        for clip in self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.selected)
        {
            let (start, end) = (clip.timeline_start_ticks, clip.timeline_end_ticks);
            span = Some(match span {
                Some((lo, hi)) => (lo.min(start), hi.max(end)),
                None => (start, end),
            });
        }
        span
    }
}
