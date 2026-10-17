//! How the preview is looked at: guides, zoom, pan, fullscreen, and the
//! transparency grid.
//!
//! None of this is an edit, so none of it goes through the undo stack: it is
//! the viewing position. One owner, because the Viewer's chips, the View menu,
//! and the palette all reach the same toggles and the same zoom, and a second
//! definition would let the menu tick disagree with the overlay on screen.

use super::AppState;

/// Zoom steps the buttons walk. Fit is a view, not a step, so it is `None`
/// rather than a magic number in this list.
pub const ZOOM_STEPS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

/// How the preview is being looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewView {
    /// `None` fits the frame to the pane; `Some(scale)` draws at that scale.
    pub zoom: Option<f32>,
    /// Pan offset in pixels, only meaningful while zoomed.
    pub pan: (f32, f32),
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
            title_safe: false,
            action_safe: false,
            grid: false,
            fullscreen: false,
        }
    }
}

impl PreviewView {
    /// The scale the frame is drawn at, with fit reported as 1.0: the pane's own
    /// `Contain` fit does the fitting, so a fit view needs no scale of its own.
    pub fn scale(&self) -> f32 {
        self.zoom.unwrap_or(1.0)
    }

    /// Whether the frame can be panned. A fitted frame has nowhere to go.
    pub fn pannable(&self) -> bool {
        self.zoom.is_some_and(|zoom| zoom > 1.0)
    }
}

impl AppState {
    pub fn toggle_title_safe(&mut self) {
        self.preview_view.title_safe = !self.preview_view.title_safe;
    }

    pub fn toggle_action_safe(&mut self) {
        self.preview_view.action_safe = !self.preview_view.action_safe;
    }

    /// Whether either safe margin is drawn.
    pub fn safe_margins_on(&self) -> bool {
        self.preview_view.title_safe || self.preview_view.action_safe
    }

    /// The Viewer's Safe chip: both margins off while either is drawn,
    /// otherwise both on.
    pub fn toggle_safe_margins(&mut self) {
        let on = !self.safe_margins_on();
        self.preview_view.title_safe = on;
        self.preview_view.action_safe = on;
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

    /// Back to fitting the pane. Also drops the pan, which has no meaning once
    /// the whole frame is visible.
    pub fn preview_zoom_fit(&mut self) {
        self.preview_view.zoom = None;
        self.preview_view.pan = (0.0, 0.0);
    }

    /// Draw the frame at its own pixels.
    pub fn preview_zoom_actual(&mut self) {
        self.preview_view.zoom = Some(1.0);
        self.preview_view.pan = (0.0, 0.0);
    }

    pub fn preview_zoom_in(&mut self) {
        self.step_zoom(1);
    }

    pub fn preview_zoom_out(&mut self) {
        self.step_zoom(-1);
    }

    /// Move the frame under a zoomed view. Ignored while fitted, so a stray drag
    /// on a fitted preview cannot leave the picture off-centre.
    pub fn pan_preview(&mut self, dx: f32, dy: f32) {
        if !self.preview_view.pannable() {
            return;
        }
        let (x, y) = self.preview_view.pan;
        self.preview_view.pan = (x + dx, y + dy);
    }

    /// The zoom label the panel shows: `Fit` or a percentage.
    pub fn preview_zoom_label(&self) -> String {
        match self.preview_view.zoom {
            None => "Fit".to_string(),
            Some(zoom) => format!("{}%", (zoom * 100.0).round() as i32),
        }
    }

    /// Walk `ZOOM_STEPS` from where the view is now. Fit enters the list at
    /// 100%, so the first zoom-in from a fitted view is a real step rather than
    /// a jump to the end.
    fn step_zoom(&mut self, direction: i32) {
        let current = self.preview_view.zoom.unwrap_or(1.0);
        let index = ZOOM_STEPS
            .iter()
            .position(|step| (step - current).abs() < 1e-4)
            .unwrap_or(3);
        let next = (index as i32 + direction).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
        self.preview_view.zoom = Some(ZOOM_STEPS[next]);
        if !self.preview_view.pannable() {
            self.preview_view.pan = (0.0, 0.0);
        }
    }
}
