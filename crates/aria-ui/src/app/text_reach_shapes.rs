//! Picking a clip by clicking its picture on the canvas.
//!
//! [`aria_shapes::SdfShape::hit_test`] is the crate's editor-facing query — the
//! same signed-distance evaluation the fragment shader runs, answered on the CPU
//! for a point. It shipped with no caller: selection was reachable only from the
//! timeline, so a title over a shape over a video was three indistinguishable
//! bars, and the star on screen could not be clicked.
//!
//! What this walks is the resolved scene, not the project. The resolver has
//! already folded in the transform, the fit, the anchor and the rotation, so the
//! quad tested here is the quad drawn. Two questions are answered per layer and
//! each comes from a table rather than from a match here:
//!
//! - Whether the layer places pixels at all, from the topology's own
//!   `paints` column in `aria_render`'s layer-source catalog. A canvas pass
//!   covers the frame and belongs to no clip.
//! - Whether the point is inside the ink. A parametric shape answers exactly
//!   through its SDF, and a pen path through its outline; every other topology
//!   fills its quad, so the quad is the answer. The quad is the renderer's
//!   measure, so a title or pen path, whose size is its raster, is picked
//!   where the preview drew it.

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
    /// the edit in progress.
    ///
    /// With the Type tool the press is the caret's: see
    /// [`AppState::press_type_at`], which `extend` (Shift held) is passed to.
    pub fn select_layer_at(&mut self, point: [f32; 2], extend: bool) -> bool {
        if self.tool.types() {
            if let Some(canvas) = self.overlay_frame.canvas {
                let size = self.canvas_size();
                if canvas_pixel(canvas, point, size).is_none() {
                    return false;
                }
            }
            return self.press_type_at(point, extend);
        }
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
        let mut engine = self.engine.lock();
        let rate = engine.project().timeline().frame_rate;
        let at = aria_models::RationalTime::new(self.store.playhead_ticks, rate);
        // The previewed scene, not the committed one: mid-gesture the picture on
        // screen is the override, and a click means the layer on screen.
        let scene = engine.preview_scene(at).ok()?;
        let in_canvas = canvas_pixel(canvas, point, [scene.width, scene.height])?;
        let size = [scene.width as f32, scene.height as f32];
        // Top of the stack first: index 0 draws first, so the last layer is the
        // one on top and therefore the one a click means. Each layer is
        // measured by the renderer, so a title or pen path is sized from the
        // raster the preview drew.
        scene
            .layers
            .iter()
            .rev()
            .filter(|layer| layer.clip.is_some())
            .find(|layer| {
                engine.layer_quad(layer, size).is_some_and(|quad| {
                    layer_covers(layer, quad, in_canvas) || margin_covers(layer, quad, in_canvas)
                })
            })
            .and_then(|layer| layer.clip)
    }
}

/// Whether a registered effect on `layer` draws under `point` past the quad:
/// the point is inside the quad grown by the margins its effects declare
/// ([`aria_render::drawn_bounds`]).
fn margin_covers(
    layer: &aria_render::SceneLayer,
    quad: ([f32; 2], [f32; 2]),
    point: [f32; 2],
) -> bool {
    let (center, size) = aria_render::drawn_bounds(layer, quad);
    if (center, size) == quad || !layer.source.spec().paints {
        return false;
    }
    let local = unrotate(point, center, layer.rotation);
    (local[0] - center[0]).abs() <= size[0] * 0.5 && (local[1] - center[1]).abs() <= size[1] * 0.5
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

/// Whether one resolved layer, drawn on `quad`, puts ink under a canvas-pixel
/// point.
///
/// `quad` is `(center, size)` before rotation, as the renderer measures it:
/// the anchor and a title's alignment are already in the center, and a title's
/// or pen path's size is its raster times the layer scale.
pub(super) fn layer_covers(
    layer: &aria_render::SceneLayer,
    (center, size): ([f32; 2], [f32; 2]),
    point: [f32; 2],
) -> bool {
    // The topology's own column, not a match here: a canvas pass reprocesses
    // what is below it and places no picture, and a topology added to
    // `aria-render` answers this without touching the pick.
    if !layer.source.spec().paints {
        return false;
    }
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return false;
    }
    // The compositor spins the quad about its own centre, so the point is
    // unrotated about it before it is compared to an axis-aligned box.
    let local = unrotate(point, center, layer.rotation);
    let offset = [local[0] - center[0], local[1] - center[1]];
    if offset[0].abs() > size[0] * 0.5 || offset[1].abs() > size[1] * 0.5 {
        return false;
    }
    // A parametric shape and a pen path can answer exactly. Extracting their
    // payloads is not a decision about the other topologies: they have no
    // outline to consult, and their quad is their ink.
    match &layer.source {
        // The fill is named and ignored: a transparent shape is still a
        // hitbox, so colour cannot decide a hit, and a variant that grows a
        // field has to come back through here.
        LayerSource::Shape {
            params,
            fill: _,
            stroke,
            pad,
        } => {
            let half = [
                (size[0] * 0.5 - pad).max(0.0),
                (size[1] * 0.5 - pad).max(0.0),
            ];
            if half[0] <= 0.0 || half[1] <= 0.0 {
                return false;
            }
            let width = stroke.as_ref().map_or(0.0, |stroke| stroke.width);
            SdfShape {
                half,
                params: *params,
            }
            .hit_test(offset, width)
        }
        LayerSource::PathShape {
            path,
            fill,
            stroke,
            raster_scale,
        } => {
            // The quad is the raster times the layer scale, so dividing the
            // scale out lands the point on the raster.
            let scale = match layer.size {
                SizeSpec::BitmapScaled(scale) => scale,
                SizeSpec::Fixed(_) => [1.0, 1.0],
            };
            aria_shapes::path_raster_hit(
                path,
                &aria_render::path_style(*fill, *stroke),
                *raster_scale,
                [offset[0] / scale[0], offset[1] / scale[1]],
            )
        }
        _ => true,
    }
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
