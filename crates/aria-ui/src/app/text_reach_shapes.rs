//! Picking a clip by clicking its picture on the canvas.
//!
//! [`aria_shapes::SdfShape::hit_test`] is the crate's editor-facing query — the
//! same signed-distance evaluation the fragment shader runs, answered on the CPU
//! for a point. It shipped with no caller: selection was reachable only from the
//! timeline, so a title over a shape over a video was three indistinguishable
//! bars, and the star the operator was looking at could not be clicked.
//!
//! What this walks is the resolved scene, not the project. The resolver has
//! already folded in the transform, the fit, the anchor and the rotation, so the
//! quad tested here is the quad drawn. Two questions are answered per layer and
//! each comes from a table rather than from a match here:
//!
//! - Whether the layer places pixels at all, from the topology's own
//!   `paints` column in `aria_render`'s layer-source catalog. A canvas pass
//!   covers the frame and belongs to no clip.
//! - Whether the point is inside the ink. A parametric shape answers exactly,
//!   through its SDF; every other topology fills its quad, so the quad is the
//!   answer.

use aria_render::{LayerSource, SizeSpec};
use aria_shapes::SdfShape;

use crate::app::AppState;
use crate::app::canvas_overlay::Rect;

impl AppState {
    /// Select the topmost clip whose picture covers the window point.
    ///
    /// Returns whether the selection changed. `false` means the click landed on
    /// the letterbox, on empty canvas, or on the clip already selected, and the
    /// caller should leave the selection alone rather than clearing it — a
    /// click that deselects on every miss makes a drag on the frame edge lose
    /// the operator's work.
    pub fn select_layer_at(&mut self, point: [f32; 2]) -> bool {
        let Some(clip) = self.clip_at_canvas_point(point) else {
            return false;
        };
        if self.selected_clip() == Some(clip) {
            return false;
        }
        self.select_clip(clip);
        true
    }

    /// The topmost clip drawing at a window point, or `None`.
    ///
    /// Separate from the selection so a caller can ask without changing
    /// anything, and so the geometry is testable without a window.
    pub fn clip_at_canvas_point(&self, point: [f32; 2]) -> Option<aria_models::ClipId> {
        let canvas = self.overlay_frame.canvas?;
        let engine = self.engine.lock();
        let project = engine.project();
        let rate = project.timeline().frame_rate;
        let at = aria_models::RationalTime::new(self.store.playhead_ticks, rate);
        // The previewed scene, not the committed one: mid-gesture the picture on
        // screen is the override, and a click means the layer the operator sees.
        let scene = engine.preview_scene(at).ok()?;
        let in_canvas = canvas_pixel(canvas, point, [scene.width, scene.height])?;
        // Top of the stack first: index 0 draws first, so the last layer is the
        // one an operator sees and therefore the one they mean.
        scene
            .layers
            .iter()
            .rev()
            .find(|layer| layer.clip.is_some() && layer_covers(layer, in_canvas))
            .and_then(|layer| layer.clip)
    }
}

/// A window point in canvas pixels, or `None` when it is outside the picture.
///
/// The letterbox is not the canvas: a click on the black bars beside a 4:3
/// sequence in a 16:9 panel is a click on nothing, and mapping it to a clamped
/// edge pixel would select whatever happens to touch the frame border.
pub(super) fn canvas_pixel(canvas: Rect, [x, y]: [f32; 2], size: [u32; 2]) -> Option<[f32; 2]> {
    if canvas.w <= 0.0 || canvas.h <= 0.0 {
        return None;
    }
    let fx = (x - canvas.x) / canvas.w;
    let fy = (y - canvas.y) / canvas.h;
    if !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
        return None;
    }
    Some([fx * size[0] as f32, fy * size[1] as f32])
}

/// Whether one resolved layer puts ink under a canvas-pixel point.
pub(super) fn layer_covers(layer: &aria_render::SceneLayer, point: [f32; 2]) -> bool {
    // The topology's own column, not a match here: a canvas pass reprocesses
    // what is below it and places no picture, and a topology added to
    // `aria-render` answers this without touching the pick.
    if !layer.source.spec().paints {
        return false;
    }
    // Only a fixed size is a quad this can test. A bitmap-scaled layer's extent
    // is known after rasterizing, which is the same reason the on-canvas
    // handles refuse it.
    let SizeSpec::Fixed(size) = layer.size else {
        return false;
    };
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return false;
    }
    // `center` is where the anchor sits; the quad's own centre is offset by how
    // far the anchor is from the middle of the content.
    let quad = [
        layer.center[0] + (0.5 - layer.anchor_point[0]) * size[0],
        layer.center[1] + (0.5 - layer.anchor_point[1]) * size[1],
    ];
    // Rotation spins the quad about the anchor, so the point is unrotated about
    // the anchor before it is compared to an axis-aligned box.
    let local = unrotate(point, layer.center, layer.rotation);
    let offset = [local[0] - quad[0], local[1] - quad[1]];
    if offset[0].abs() > size[0] * 0.5 || offset[1].abs() > size[1] * 0.5 {
        return false;
    }
    // A parametric shape can answer exactly. Extracting its payload is not a
    // decision about the other topologies: they have no SDF to consult, and
    // their quad is their ink.
    // The fill is named and ignored: a transparent shape is still a hitbox, so
    // colour cannot decide a hit, and a variant that grows a field has to come
    // back through here.
    if let LayerSource::Shape {
        params,
        fill: _,
        stroke,
        pad,
    } = &layer.source
    {
        let half = [
            (size[0] * 0.5 - pad).max(0.0),
            (size[1] * 0.5 - pad).max(0.0),
        ];
        if half[0] <= 0.0 || half[1] <= 0.0 {
            return false;
        }
        let width = stroke.as_ref().map_or(0.0, |stroke| stroke.width);
        return SdfShape {
            half,
            params: *params,
        }
        .hit_test(offset, width);
    }
    true
}

/// `point` rotated by `-angle` about `pivot`, both in canvas pixels.
fn unrotate([x, y]: [f32; 2], [px, py]: [f32; 2], angle: f32) -> [f32; 2] {
    if angle == 0.0 {
        return [x, y];
    }
    let (sin, cos) = (-angle).sin_cos();
    let (dx, dy) = (x - px, y - py);
    [px + dx * cos - dy * sin, py + dx * sin + dy * cos]
}
