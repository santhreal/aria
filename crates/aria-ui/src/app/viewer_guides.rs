//! Viewer rulers and guides over the Program frame.
//!
//! Rulers run along the top and left of the Viewer and count canvas pixels.
//! Pressing a ruler pulls out a guide: the top ruler a horizontal one, the
//! left ruler a vertical one. Dragging a guide moves it, and releasing it over
//! its ruler or off the canvas deletes it. With snapping on, a moved layer's
//! edges and center stop on a guide within [`SNAP_PX`] screen pixels.
//!
//! Guides are a view aid, not an edit: they live beside the zoom and the safe
//! margins and stay out of the undo history.

use aria_core::count::counted;

use super::AppState;
use super::canvas_overlay::Rect;

/// Thickness of a ruler strip in screen pixels.
pub const RULER_PX: f32 = 16.0;

/// How close, in screen pixels, a layer edge or center has to come to a guide
/// before it snaps.
pub const SNAP_PX: f32 = 6.0;

/// Canvas-pixel steps a ruler numbers, smallest first. The ruler picks the
/// first step whose numbers stay [`LABEL_SPACING_PX`] apart on screen.
const RULER_STEPS: [u32; 9] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500];

/// Least screen distance between two numbered ruler ticks.
const LABEL_SPACING_PX: f32 = 56.0;

/// Which way a guide runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideAxis {
    /// A horizontal line at a canvas y, pulled from the top ruler.
    Horizontal,
    /// A vertical line at a canvas x, pulled from the left ruler.
    Vertical,
}

/// One guide, at a canvas pixel on its axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub axis: GuideAxis,
    pub at: f32,
}

/// Rulers, guides, and guide snapping for the Program view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewerGuides {
    /// Rulers drawn along the top and left (Ctrl+Shift+R).
    pub rulers: bool,
    /// A moved layer snaps to guides (View > Snap to Guides).
    pub snap: bool,
    /// The guides are kept but neither drawn nor snapped to (View > Show
    /// Guides off). Pulling a new guide shows them again.
    pub hidden: bool,
    pub guides: Vec<Guide>,
    /// The guide under the pointer's drag, by index into `guides`.
    pub dragging: Option<usize>,
}

/// Screen pixels per canvas pixel along x and y for a canvas `size` drawn in
/// `canvas` (window pixels).
fn scale(canvas: Rect, size: [u32; 2]) -> Option<[f32; 2]> {
    let (w, h) = (size[0] as f32, size[1] as f32);
    (w > 0.0 && h > 0.0 && canvas.w > 0.0 && canvas.h > 0.0).then(|| [canvas.w / w, canvas.h / h])
}

/// A guide's screen coordinate on its axis: x for a vertical guide, y for a
/// horizontal one.
pub fn guide_screen(guide: Guide, canvas: Rect, size: [u32; 2]) -> Option<f32> {
    let [sx, sy] = scale(canvas, size)?;
    Some(match guide.axis {
        GuideAxis::Vertical => canvas.x + guide.at * sx,
        GuideAxis::Horizontal => canvas.y + guide.at * sy,
    })
}

/// The canvas pixel under a screen coordinate on `axis`, rounded to a whole
/// pixel so a guide lands on a pixel boundary.
pub fn canvas_at(axis: GuideAxis, screen: f32, canvas: Rect, size: [u32; 2]) -> Option<f32> {
    let [sx, sy] = scale(canvas, size)?;
    Some(match axis {
        GuideAxis::Vertical => ((screen - canvas.x) / sx).round(),
        GuideAxis::Horizontal => ((screen - canvas.y) / sy).round(),
    })
}

/// The canvas-pixel step a ruler numbers at `screen_per_px` screen pixels per
/// canvas pixel.
pub fn ruler_step(screen_per_px: f32) -> u32 {
    if !screen_per_px.is_finite() || screen_per_px <= 0.0 {
        return RULER_STEPS[RULER_STEPS.len() - 1];
    }
    RULER_STEPS
        .into_iter()
        .find(|step| *step as f32 * screen_per_px >= LABEL_SPACING_PX)
        .unwrap_or(RULER_STEPS[RULER_STEPS.len() - 1])
}

/// The numbered ticks of a ruler spanning `from..to` in screen pixels along an
/// axis whose canvas starts at `origin` and runs at `screen_per_px`: each
/// tick's screen coordinate and the canvas pixel it numbers. Ticks run past
/// both canvas edges, so a zoomed or panned frame keeps its numbers.
pub fn ruler_ticks(from: f32, to: f32, origin: f32, screen_per_px: f32) -> Vec<(f32, i64)> {
    if !(screen_per_px.is_finite() && screen_per_px > 0.0 && to > from) {
        return Vec::new();
    }
    let step = i64::from(ruler_step(screen_per_px));
    let first = ((from - origin) / screen_per_px / step as f32).floor() as i64 * step;
    let last = ((to - origin) / screen_per_px).ceil() as i64;
    (0..)
        .map(|n| first + n * step)
        .take_while(|value| *value <= last)
        .map(|value| (origin + value as f32 * screen_per_px, value))
        .filter(|(screen, _)| *screen >= from && *screen <= to)
        .collect()
}

/// The screen-pixel shift that snaps `layer` onto the nearest guide, per axis:
/// its left edge, center, or right edge onto a vertical guide, and its top,
/// middle, or bottom onto a horizontal one, within [`SNAP_PX`]. Zero on an axis
/// with no guide in reach.
pub fn snap_shift(layer: Rect, canvas: Rect, size: [u32; 2], guides: &[Guide]) -> [f32; 2] {
    let mut best: [Option<f32>; 2] = [None, None];
    for guide in guides {
        let Some(line) = guide_screen(*guide, canvas, size) else {
            continue;
        };
        let (slot, edges) = match guide.axis {
            GuideAxis::Vertical => (0, [layer.x, layer.x + layer.w * 0.5, layer.x + layer.w]),
            GuideAxis::Horizontal => (1, [layer.y, layer.y + layer.h * 0.5, layer.y + layer.h]),
        };
        for edge in edges {
            let shift = line - edge;
            if shift.abs() <= SNAP_PX && best[slot].is_none_or(|held| shift.abs() < held.abs()) {
                best[slot] = Some(shift);
            }
        }
    }
    [best[0].unwrap_or(0.0), best[1].unwrap_or(0.0)]
}

impl ViewerGuides {
    /// Start a new guide on `axis` and drag it.
    pub fn pull(&mut self, axis: GuideAxis, at: f32) {
        self.guides.push(Guide { axis, at });
        self.dragging = Some(self.guides.len() - 1);
    }

    /// Drag guide `index`.
    pub fn grab(&mut self, index: usize) {
        if index < self.guides.len() {
            self.dragging = Some(index);
        }
    }

    /// Move the dragged guide to canvas pixel `at`.
    pub fn drag_to(&mut self, at: f32) {
        if let Some(guide) = self.dragging.and_then(|index| self.guides.get_mut(index)) {
            guide.at = at;
        }
    }

    /// Drop the dragged guide, deleting it when `keep` is false.
    pub fn release(&mut self, keep: bool) {
        if let Some(index) = self.dragging.take()
            && !keep
            && index < self.guides.len()
        {
            self.guides.remove(index);
        }
    }
}

impl AppState {
    /// Ctrl+Shift+R: rulers along the top and left of the Program view.
    pub fn toggle_rulers(&mut self) {
        self.viewer_guides.rulers = !self.viewer_guides.rulers;
        self.status_message = Some(
            if self.viewer_guides.rulers {
                "Rulers on: drag from a ruler for a guide"
            } else {
                "Rulers off"
            }
            .into(),
        );
    }

    /// View > Snap to Guides.
    pub fn toggle_guide_snap(&mut self) {
        self.viewer_guides.snap = !self.viewer_guides.snap;
        self.status_message = Some(
            if self.viewer_guides.snap {
                "Snap to guides on"
            } else {
                "Snap to guides off"
            }
            .into(),
        );
    }

    /// View > Show Guides: draw the guides and snap to them, or keep them
    /// out of sight and out of the way.
    pub fn toggle_guides_shown(&mut self) {
        let guides = &mut self.viewer_guides;
        guides.hidden ^= true;
        if guides.hidden {
            guides.dragging = None;
        }
        self.status_message = Some(match (guides.hidden, guides.guides.len()) {
            (true, 0) => "Guides hidden".into(),
            (true, n) => format!("Guides hidden: {n} kept"),
            (false, 0) => "Guides shown: drag from a ruler for a guide".into(),
            (false, _) => "Guides shown".into(),
        });
    }

    /// View > Clear Guides.
    pub fn clear_guides(&mut self) {
        let count = self.viewer_guides.guides.len();
        self.viewer_guides.guides.clear();
        self.viewer_guides.dragging = None;
        self.status_message = Some(match count {
            0 => "No guides to clear".into(),
            n => format!("Cleared {}", counted(n, "guide", "guides")),
        });
    }

    /// Press on a ruler at window point `at`: a new guide on that ruler's axis
    /// follows the pointer.
    pub fn pull_guide(&mut self, axis: GuideAxis, at: [f32; 2]) {
        let size = self.canvas_size();
        let Some(canvas) = self.overlay_frame.canvas else {
            return;
        };
        let screen = match axis {
            GuideAxis::Vertical => at[0],
            GuideAxis::Horizontal => at[1],
        };
        if let Some(value) = canvas_at(axis, screen, canvas, size) {
            // A guide pulled while the others are hidden would vanish on
            // release, so pulling one shows them all.
            self.viewer_guides.hidden = false;
            self.viewer_guides.pull(axis, value);
        }
    }

    /// Move the dragged guide under window point `at`.
    pub fn drag_guide(&mut self, at: [f32; 2]) {
        let size = self.canvas_size();
        let Some(canvas) = self.overlay_frame.canvas else {
            return;
        };
        let Some(guide) = self
            .viewer_guides
            .dragging
            .and_then(|index| self.viewer_guides.guides.get(index))
        else {
            return;
        };
        let screen = match guide.axis {
            GuideAxis::Vertical => at[0],
            GuideAxis::Horizontal => at[1],
        };
        if let Some(value) = canvas_at(guide.axis, screen, canvas, size) {
            self.viewer_guides.drag_to(value);
        }
    }

    /// Release the dragged guide at window point `at`. Over its own ruler, or
    /// off the canvas on its axis, the guide is deleted.
    pub fn release_guide(&mut self, at: [f32; 2]) {
        let Some(guide) = self
            .viewer_guides
            .dragging
            .and_then(|index| self.viewer_guides.guides.get(index))
            .copied()
        else {
            return;
        };
        let Some(container) = self.overlay_frame.container else {
            self.viewer_guides.release(false);
            return;
        };
        let size = self.canvas_size();
        let (on_ruler, extent) = match guide.axis {
            GuideAxis::Vertical => (at[0] < container.x + RULER_PX, size[0] as f32),
            GuideAxis::Horizontal => (at[1] < container.y + RULER_PX, size[1] as f32),
        };
        let keep = !on_ruler && guide.at >= 0.0 && guide.at <= extent;
        self.viewer_guides.release(keep);
    }

    /// The screen shift that snaps the dragged layer `layer` to a guide, or
    /// zero with snapping off.
    pub(crate) fn guide_snap_shift(&self, layer: Rect) -> [f32; 2] {
        if !self.viewer_guides.snap
            || self.viewer_guides.hidden
            || self.viewer_guides.guides.is_empty()
        {
            return [0.0, 0.0];
        }
        let Some(canvas) = self.overlay_frame.canvas else {
            return [0.0, 0.0];
        };
        snap_shift(
            layer,
            canvas,
            self.canvas_size(),
            &self.viewer_guides.guides,
        )
    }
}

#[cfg(test)]
#[path = "viewer_guides_tests.rs"]
mod tests;
