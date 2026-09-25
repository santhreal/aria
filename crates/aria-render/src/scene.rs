//! The intermediate scene description a render pass consumes.
//!
//! [`resolve`](crate::resolve) turns a [`Project`](aria_models::Project) at a
//! timeline instant into a [`Scene`]: the canvas plus an ordered, bottom-to-top
//! stack of placed layers. A `Scene` is a pure value — it names *what* to draw
//! (which media frame, which text, which fill) and *where*, but holds no decoded
//! pixels and touches no GPU. That split keeps the geometry (canvas sizing,
//! z-order, transforms, crop) deterministic and unit-testable without a device,
//! while [`Renderer`](crate::Renderer) does the decode + rasterize + composite.

use aria_compositor::ColorGrade;
use aria_core::Rational;
use aria_models::{AnimationSlot, BlendMode, ClipId, MaskKind};
use aria_text::{TextAlign, TextStyle, TextVerticalAlign};

pub use crate::scene_layers::{CpuRaster, layer_source_spec};

/// The rest of the topology registry, read by the conformance suite rather than
/// by the renderer.
///
/// The realize dispatch asks a layer for its own row (`source.spec()`) and the
/// raster column above; nothing on the render path enumerates the table or looks
/// a row up by name. Behind `cfg(test)` the facade stays the crate's one path to
/// the registry without claiming a production reader it does not have — a row
/// that gains one moves up a line.
#[cfg(test)]
pub use crate::scene_layers::{Nested, layer_source_specs};

/// Sampled per-character text animation attached to a [`LayerSource::Text`].
///
/// Pure resolve-time data: the preset id, which slot it came from, a
/// normalized progress / phase in `0…1`, and the clip's tunable knobs.
/// Realize expands this into per-cluster placement deltas — the Scene stays
/// GPU-free and unit-testable.
#[derive(Debug, Clone, PartialEq)]
pub struct TextAnimation {
    pub id: String,
    pub slot: AnimationSlot,
    /// Eased entrance/exit progress, or combo phase (both in `0…1`).
    pub t: f32,
    /// Magnitude multiplier (`1` = catalog feel).
    pub intensity: f32,
    /// Per-character stagger stretch (`1` = catalog feel).
    pub stagger: f32,
}

pub use aria_core::RationalTime;

/// One sampled GPU effect pass attached to a clip at resolve time.
///
/// The compositor defines this owned row. Scene resolution interns the
/// descriptor id and stores the flattened parameters once, so frame rendering
/// borrows the scene slice directly.
pub use aria_compositor::PassInstance as ResolvedPass;

/// A canvas plus the ordered layer stack to composite for one timeline instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Canvas clear color before layers composite. Alpha 0 is supported for
    /// gesture sprite/foreground passes that stack over an opaque backdrop.
    pub background: [u8; 4],
    /// Timeline instant this scene was resolved at (timeline rate). Used by
    /// motion-blur supersampling to re-place animated transforms at sub-ticks
    /// without re-resolving content.
    pub tick: RationalTime,
    /// Layers in bottom-to-top stacking order (index 0 draws first).
    pub layers: Vec<SceneLayer>,
}

impl Scene {
    /// An empty canvas of `width`×`height` over `background` (no layers).
    pub fn empty(width: u32, height: u32, background: [u8; 4]) -> Self {
        Self {
            width,
            height,
            background,
            tick: RationalTime::new(0, Rational::FPS_30),
            layers: Vec::new(),
        }
    }

    /// Uniformly scale the whole scene — canvas and every layer's geometry —
    /// by `factor`. Content keeps its composition exactly (same relative
    /// placement, crop, rotation); only the pixel density changes. This is
    /// how preview renders at fit-to-view size and export renders at a
    /// non-native resolution without touching the resolver.
    ///
    /// Degenerate factors (non-finite or ≤ 0) are ignored.
    pub fn scale(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.width = scaled_dim(self.width, factor);
        self.height = scaled_dim(self.height, factor);
        for layer in &mut self.layers {
            scale_layer(layer, factor);
        }
    }

    /// Scale the scene to fit within `max_width`×`max_height`, preserving
    /// aspect and never upscaling. The result has no letterbox: the canvas
    /// itself shrinks to the fitted box.
    pub fn fit_within(&mut self, max_width: u32, max_height: u32) {
        if self.width == 0 || self.height == 0 || max_width == 0 || max_height == 0 {
            return;
        }
        let factor = (max_width as f32 / self.width as f32)
            .min(max_height as f32 / self.height as f32)
            .min(1.0);
        if factor < 1.0 {
            self.scale(factor);
        }
    }

    /// Scale the scene to exactly `width`×`height`: uniform aspect-preserving
    /// scale (up or down), content centered, any aspect mismatch letterboxed
    /// with the scene background. This is the export path for a requested
    /// output resolution.
    pub fn fit_into(&mut self, width: u32, height: u32) {
        if self.width == 0 || self.height == 0 || width == 0 || height == 0 {
            return;
        }
        let (cw, ch) = (self.width as f32, self.height as f32);
        let factor = (width as f32 / cw).min(height as f32 / ch);
        self.scale(factor);
        let dx = (width as f32 - cw * factor) * 0.5;
        let dy = (height as f32 - ch * factor) * 0.5;
        for layer in &mut self.layers {
            layer.center = [layer.center[0] + dx, layer.center[1] + dy];
            for pass in &mut layer.blur_passes {
                pass.center = [pass.center[0] + dx, pass.center[1] + dy];
            }
        }
        self.width = width;
        self.height = height;
    }
}

/// One transform sample for motion-blur supersampling (scene space).
///
/// `center` is the **anchor** position (same convention as [`SceneLayer::center`]);
/// the renderer derives the quad center from `size` + the layer's anchor point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneBlurPass {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub opacity: f32,
}

/// A scaled canvas dimension: rounded, never collapsing to zero.
fn scaled_dim(dim: u32, factor: f32) -> u32 {
    ((dim as f32 * factor).round() as u32).max(1)
}

/// Scale one layer's geometry, and any layer nested inside it.
///
/// A transition wrapper and a matte wrapper both hold whole layers, and those
/// carry their own canvas placement. Scaling only the wrapper left the content
/// inside it at the unscaled size, so a preview at fit-to-view drew a
/// transition or a matte at the export geometry.
fn scale_layer(layer: &mut SceneLayer, factor: f32) {
    layer.center = [layer.center[0] * factor, layer.center[1] * factor];
    layer.size = match layer.size {
        SizeSpec::Fixed([w, h]) => SizeSpec::Fixed([w * factor, h * factor]),
        // Text / path bitmaps rasterize at their reference resolution and ride
        // the quad; scaling the multiplier scales the quad.
        SizeSpec::BitmapScaled([sx, sy]) => SizeSpec::BitmapScaled([sx * factor, sy * factor]),
    };
    for pass in &mut layer.blur_passes {
        pass.center = [pass.center[0] * factor, pass.center[1] * factor];
        pass.size = [pass.size[0] * factor, pass.size[1] * factor];
    }
    match &mut layer.source {
        // SDF stroke width and AA pad are in canvas pixels.
        LayerSource::Shape { stroke, pad, .. } => {
            *pad *= factor;
            if let Some(stroke) = stroke {
                stroke.width *= factor;
            }
        }
        // Path strokes live in path-local pixels folded into the raster, so
        // scaling the raster factor scales them too.
        LayerSource::PathShape { raster_scale, .. } => *raster_scale *= factor,
        LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            scale_layer(outgoing, factor);
            scale_layer(incoming, factor);
        }
        LayerSource::Matte { layers, .. } => {
            scale_layer(&mut layers[0], factor);
            scale_layer(&mut layers[1], factor);
        }
        LayerSource::CanvasPass
        | LayerSource::Media { .. }
        | LayerSource::Still { .. }
        | LayerSource::Sticker { .. }
        | LayerSource::Lottie { .. }
        | LayerSource::Text { .. }
        | LayerSource::Solid(_) => {}
    }
}

/// One placed layer: a pixel source plus where it lands on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLayer {
    /// Originating timeline clip, when this layer maps 1:1 to one clip.
    /// `None` for transition composites and other multi-source layers.
    pub clip: Option<ClipId>,
    /// What to draw.
    pub source: LayerSource,
    /// Canvas position of the content's anchor point (+x right, +y down) —
    /// the pivot `rotation` spins about, and what the clip transform's
    /// `position` places. Equals the placed quad's center for the default
    /// centered `anchor_point`.
    pub center: [f32; 2],
    /// Pivot within the content bounds, normalized to the placed size
    /// (`[0.5, 0.5]` = content center). The renderer derives the quad center
    /// from `center` once the final pixel size is known — deferred because
    /// text/path bitmaps only get a size after rasterization.
    pub anchor_point: [f32; 2],
    /// On-canvas extent of the content.
    pub size: SizeSpec,
    /// Clockwise rotation about the anchor (`center`), in radians.
    pub rotation: f32,
    /// Layer opacity in `0.0..=1.0`; multiplies the content's alpha.
    pub opacity: f32,
    /// Sampled UV rect `[u0, v0, u1, v1]` across the visible picture. A sub-rect
    /// crops; a reversed axis mirrors. Ignored by solid fills.
    pub uv: [f32; 4],
    /// GPU effect chain sampled at clip-local tick (empty when none).
    pub effects: Vec<ResolvedPass>,
    /// Shaped alpha mask (media clips only).
    pub mask: Option<SceneMask>,
    /// Green-screen keying (media clips only).
    pub chroma_key: Option<SceneChromaKey>,
    /// Resolved color grade (filter preset + manual adjustments); `None` when
    /// the clip's look is identity.
    pub color_grade: Option<ColorGrade>,
    /// `.cube` 3D LUT applied after the grade; `None` when the clip has none.
    /// File-backed: the renderer parses and uploads the table on first use
    /// and skips missing/unparseable files gracefully.
    pub lut: Option<SceneLut>,
    /// How this layer composites over the stack below (`Normal` = source-over).
    /// Canvas-wide passes and transition wrappers stay `Normal`.
    pub blend_mode: BlendMode,
    /// Layer-quad styles (shadow/glow/outline/background) sampled at the
    /// clip-local tick into canvas pixels. `None` when the clip has no style
    /// blocks — the compositor fast path. Canvas-wide passes and transition
    /// wrappers stay `None`.
    pub styles: Option<SceneStyles>,
    /// Alternate transform samples for motion-blur supersampling. Empty when
    /// unused. Content (texture / source time) stays at the primary tick —
    /// transform-only blur. Filled by `motion_blur::attach_motion_blur_passes`
    /// before fit/scale; ignored by interactive preview when motion blur is off.
    pub blur_passes: Vec<SceneBlurPass>,
}

/// Mask values sampled at a clip-local tick.
///
/// Geometry (`center`, `size`, `rotation_rad`, `roundness`) stays in the same
/// fraction-of-layer units as the model — the compositor consumes those
/// fractions directly; no canvas-pixel conversion happens here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneMask {
    pub kind: MaskKind,
    pub feather: f32,
    /// Mask center offset from the layer center, as a fraction of layer size.
    pub center: [f32; 2],
    /// Mask size as a fraction of layer size (`[1,1]` = cover the layer).
    pub size: [f32; 2],
    /// Mask rotation in radians (converted from model degrees at resolve).
    pub rotation_rad: f32,
    /// Rectangle corner rounding, `0` … `1`.
    pub roundness: f32,
    pub invert: bool,
}

/// Chroma-key values sampled at a clip-local tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneChromaKey {
    pub rgb: [u8; 3],
    pub strength: f32,
    pub shadow: f32,
}

/// A file-backed `.cube` LUT reference on a [`SceneLayer`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLut {
    /// Absolute path to the `.cube` file.
    pub path: String,
    /// Blend of the looked-up result over the original, `0` … `1`.
    pub intensity: f32,
}

/// Drop shadow drawn from the layer's alpha (offset + blur), in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneShadow {
    pub rgba: [u8; 4],
    pub offset: [f32; 2],
    pub blur: f32,
}

/// Soft glow bloom drawn from the layer's alpha, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneGlow {
    pub rgba: [u8; 4],
    pub radius: f32,
    pub intensity: f32,
}

/// Hard outline / stroke around the layer's alpha silhouette, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneOutline {
    pub rgba: [u8; 4],
    pub width: f32,
}

/// Solid plate behind the layer (padded AABB of the alpha bounds), in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneBackground {
    pub rgba: [u8; 4],
    pub padding: f32,
    pub radius: f32,
}

/// Layer-quad styles sampled at a clip-local tick into canvas pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneStyles {
    pub shadow: Option<SceneShadow>,
    pub glow: Option<SceneGlow>,
    pub outline: Option<SceneOutline>,
    pub background: Option<SceneBackground>,
}

impl SceneStyles {
    /// True iff no style block is present.
    pub fn is_empty(&self) -> bool {
        self.shadow.is_none()
            && self.glow.is_none()
            && self.outline.is_none()
            && self.background.is_none()
    }
}

impl SceneLayer {
    /// The placed quad's center for a final pixel `size`: offset the anchor
    /// by the (rotated) anchor→center vector. Identity for center anchors.
    pub fn quad_center(&self, size: [f32; 2]) -> [f32; 2] {
        let to_center = [
            (0.5 - self.anchor_point[0]) * size[0],
            (0.5 - self.anchor_point[1]) * size[1],
        ];
        if to_center == [0.0, 0.0] {
            return self.center;
        }
        let (sin, cos) = self.rotation.sin_cos();
        [
            self.center[0] + to_center[0] * cos - to_center[1] * sin,
            self.center[1] + to_center[0] * sin + to_center[1] * cos,
        ]
    }

    /// Place an ink-tight text bitmap at its requested canvas alignment while
    /// retaining the clip transform's position and anchor offsets.
    pub fn text_quad_center(
        &self,
        style: &TextStyle,
        size: [f32; 2],
        canvas: [f32; 2],
    ) -> [f32; 2] {
        let mut center = self.quad_center(size);
        center[0] += match style.align {
            TextAlign::Left => (size[0] - canvas[0]) * 0.5,
            TextAlign::Center => 0.0,
            TextAlign::Right => (canvas[0] - size[0]) * 0.5,
        };
        center[1] += match style.vertical_align {
            TextVerticalAlign::Top => (size[1] - canvas[1]) * 0.5,
            TextVerticalAlign::Middle => 0.0,
            TextVerticalAlign::Bottom => (canvas[1] - size[1]) * 0.5,
        };
        center
    }
}

/// How a layer's on-canvas size is determined.
///
/// Most content has a size the resolver can compute up front (media aspect-fit,
/// shapes, solids). Text is the exception: its pixel extent isn't known until
/// it is shaped and rasterized, so the resolver defers it to the renderer as a
/// multiplier on the rasterized bitmap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeSpec {
    /// A known on-canvas size in pixels (scale already folded in).
    Fixed([f32; 2]),
    /// Multiply the rasterized content's pixel size by these per-axis factors
    /// (text / path shapes). Uniform scales use equal components.
    BitmapScaled([f32; 2]),
}

/// The pixel source for a [`SceneLayer`].
///
/// Declared beside its capability table in `scene_layers`: the two are
/// one axis, and the table's exhaustive match is what a new topology has to
/// satisfy. Re-exported here because a `Scene` is what callers name.
pub use crate::scene_layers::LayerSource;

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
