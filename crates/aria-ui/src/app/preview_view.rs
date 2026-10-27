//! How the preview is looked at: guides, zoom, pan, fullscreen, and the
//! transparency grid.
//!
//! None of this is an edit, so none of it goes through the undo stack: it is
//! the viewing position. One owner, because the Viewer's chips, the View menu,
//! and the palette all reach the same toggles and the same zoom, and a second
//! definition would let the menu tick disagree with the overlay on screen.

use super::AppState;
use super::canvas_overlay::{Rect, fit_frame, view_frame};

/// The zoom levels the Viewer offers, as device pixels per canvas pixel: at
/// 100% one canvas pixel covers one device pixel. Fit is a view, not a level,
/// so it is `None` rather than a number in this list.
pub const ZOOM_STEPS: [f32; 9] = [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0, 8.0];

/// How far apart two zoom levels must be to count as different steps: a
/// fitted frame drawn at 49.99% steps in to 75%, not to 50%.
const STEP_SLACK: f32 = 1e-3;

/// How the preview is being looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewView {
    /// `None` fits the frame to the pane; `Some(zoom)` draws each canvas pixel
    /// over `zoom` device pixels, about the pane's center.
    pub zoom: Option<f32>,
    /// How far the frame's center stands from the pane's center, in window
    /// pixels. Nonzero only along an axis where the zoomed frame is larger
    /// than the pane.
    pub pan: (f32, f32),
    /// Device pixels per window pixel on the screen the Viewer last painted
    /// on, which converts a zoom level to window pixels.
    pub pixel_ratio: f32,
    pub title_safe: bool,
    pub action_safe: bool,
    pub grid: bool,
    pub fullscreen: bool,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: None,
            pan: (0.0, 0.0),
            pixel_ratio: 1.0,
            title_safe: false,
            action_safe: false,
            grid: false,
            fullscreen: false,
        }
    }
}

/// A list the Viewer's control row opens above one of its chips. One is open
/// at a time: opening one closes the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerMenu {
    Zoom,
    Resolution,
}

impl AppState {
    pub fn toggle_title_safe(&mut self) {
        self.preview_view.title_safe = !self.preview_view.title_safe;
    }

    pub fn toggle_action_safe(&mut self) {
        self.preview_view.action_safe = !self.preview_view.action_safe;
    }

    pub fn toggle_preview_grid(&mut self) {
        self.preview_view.grid = !self.preview_view.grid;
    }

    /// Whether the Viewer shows the transparency grid where no layer covers
    /// the canvas.
    pub fn transparency_grid_on(&self) -> bool {
        self.ui_settings.transparency_grid
    }

    /// Show the transparency grid where no layer covers the canvas, or the
    /// sequence's background colour; save the choice and render the frame
    /// again. Export, thumbnails, and Export Frame keep the background.
    pub fn toggle_transparency_grid(&mut self, cx: &mut gpui::Context<Self>) {
        let on = !self.ui_settings.transparency_grid;
        self.ui_settings.transparency_grid = on;
        self.engine.lock().set_preview_transparency(on);
        self.persist_settings_later(cx);
        // Frames rendered over the other background are stale.
        self.picture_changed();
        if self.playback_cadence.is_none() {
            let state = cx.entity();
            cx.defer(move |app| super::spawn_preview(state, app));
        }
        cx.notify();
    }

    /// Fill the window with the preview, or give the panels back.
    pub fn toggle_preview_fullscreen(&mut self) {
        self.preview_view.fullscreen = !self.preview_view.fullscreen;
        if self.preview_view.fullscreen {
            self.active_panel = crate::types::ActivePanel::Preview;
        }
    }

    /// Esc's meaning for the preview: leave fullscreen if it is on, and report
    /// whether it did, so the shell can fall through to its other gestures.
    pub fn leave_preview_fullscreen(&mut self) -> bool {
        if !self.preview_view.fullscreen {
            return false;
        }
        self.preview_view.fullscreen = false;
        true
    }

    /// Open `menu`, or close it when it is the open one.
    pub fn toggle_viewer_menu(&mut self, menu: ViewerMenu) {
        self.viewer_menu = (self.viewer_menu != Some(menu)).then_some(menu);
    }

    /// Back to fitting the pane. Also drops the pan, which has no meaning once
    /// the whole frame is visible.
    pub fn preview_zoom_fit(&mut self) {
        self.set_preview_zoom(None);
    }

    /// Draw each canvas pixel over one device pixel.
    pub fn preview_zoom_actual(&mut self) {
        self.set_preview_zoom(Some(1.0));
    }

    pub fn preview_zoom_in(&mut self) {
        self.step_zoom(1);
    }

    pub fn preview_zoom_out(&mut self) {
        self.step_zoom(-1);
    }

    /// Show the frame at `zoom`, `None` to fit it. The canvas point at the
    /// pane's center stays there, as far as the pan reaches.
    pub fn set_preview_zoom(&mut self, zoom: Option<f32>) {
        let before = self.preview_frame().map(|frame| frame.w);
        self.preview_view.zoom = zoom;
        let after = self.preview_frame().map(|frame| frame.w);
        let (x, y) = self.preview_view.pan;
        self.preview_view.pan = match (before, after) {
            (Some(before), Some(after)) if before > 0.0 => {
                let grown = after / before;
                (x * grown, y * grown)
            }
            _ => (0.0, 0.0),
        };
        self.clamp_preview_pan();
    }

    /// Move a zoomed frame that is larger than the pane, no farther than its
    /// edge meeting the pane's edge. Ignored while the frame fits the pane, so
    /// a stray drag cannot leave the picture off-center.
    pub fn pan_preview(&mut self, dx: f32, dy: f32) {
        if !self.preview_pannable() {
            return;
        }
        let (x, y) = self.preview_view.pan;
        self.preview_view.pan = (x + dx, y + dy);
        self.clamp_preview_pan();
    }

    /// Whether a press on the picture pans it: the zoomed frame is larger than
    /// the pane along at least one axis.
    pub fn preview_pannable(&self) -> bool {
        let (x, y) = self.preview_pan_reach();
        x > 0.0 || y > 0.0
    }

    /// The zoom a fitted frame is drawn at, once the pane has been painted.
    pub fn preview_fit_zoom(&self) -> Option<f32> {
        let container = self.overlay_frame.container?;
        let canvas = self.canvas_size();
        let width = canvas[0] as f32;
        (width > 0.0)
            .then(|| fit_frame(container, canvas).w * self.preview_view.pixel_ratio / width)
    }

    /// Where the frame is drawn inside the painted pane, from the pane's
    /// top left, while zoomed. `None` while it fits, which `ObjectFit::Contain`
    /// draws, and before the pane is painted.
    pub(crate) fn preview_zoomed_frame(&self) -> Option<Rect> {
        let container = self.overlay_frame.container?;
        self.preview_zoomed_frame_in([container.w, container.h])
    }

    /// [`Self::preview_zoomed_frame`] in a pane `size` window pixels across.
    pub(crate) fn preview_zoomed_frame_in(&self, [w, h]: [f32; 2]) -> Option<Rect> {
        self.preview_view.zoom?;
        Some(view_frame(
            Rect::new(0.0, 0.0, w, h),
            self.preview_view,
            self.canvas_size(),
        ))
    }

    /// Where the frame is drawn in the painted pane, in window pixels.
    fn preview_frame(&self) -> Option<Rect> {
        let container = self.overlay_frame.container?;
        Some(view_frame(container, self.preview_view, self.canvas_size()))
    }

    /// How far the pan reaches from center along each axis: half of what the
    /// zoomed frame overhangs the pane, zero along an axis where it fits.
    fn preview_pan_reach(&self) -> (f32, f32) {
        let Some(container) = self.overlay_frame.container else {
            return (0.0, 0.0);
        };
        if self.preview_view.zoom.is_none() {
            return (0.0, 0.0);
        }
        let centered = PreviewView {
            pan: (0.0, 0.0),
            ..self.preview_view
        };
        let frame = view_frame(container, centered, self.canvas_size());
        (
            ((frame.w - container.w) * 0.5).max(0.0),
            ((frame.h - container.h) * 0.5).max(0.0),
        )
    }

    pub(super) fn clamp_preview_pan(&mut self) {
        let (reach_x, reach_y) = self.preview_pan_reach();
        let (x, y) = self.preview_view.pan;
        self.preview_view.pan = (x.clamp(-reach_x, reach_x), y.clamp(-reach_y, reach_y));
    }

    /// Step to the next level in [`ZOOM_STEPS`] past the zoom on screen: from
    /// fit, the next level above or below the fitted frame's own zoom. Stays
    /// put past either end.
    fn step_zoom(&mut self, direction: i32) {
        let current = self
            .preview_view
            .zoom
            .or_else(|| self.preview_fit_zoom())
            .unwrap_or(1.0);
        let next = if direction > 0 {
            ZOOM_STEPS
                .into_iter()
                .find(|step| *step > current * (1.0 + STEP_SLACK))
        } else {
            ZOOM_STEPS
                .into_iter()
                .rev()
                .find(|step| *step < current * (1.0 - STEP_SLACK))
        };
        if let Some(next) = next {
            self.set_preview_zoom(Some(next));
        }
    }
}

/// `Fit`, or `zoom` as a whole percentage.
pub fn zoom_label(zoom: Option<f32>) -> String {
    match zoom {
        None => "Fit".to_string(),
        Some(zoom) => format!("{}%", (zoom * 100.0).round() as i32),
    }
}
