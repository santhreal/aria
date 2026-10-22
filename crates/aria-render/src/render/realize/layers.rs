//! The owned realized-layer model, and how one scene layer becomes one: its
//! pixels, its placement, and its look. The top-level walk in `pass_one.rs` and
//! the sides of a transition or a matte realize through the same
//! [`Renderer::realize_leaf`], so a side draws with every part of the look a
//! layer drawn alone has. A nest realizes as a leaf whose pixels are the
//! picture its own scene draws; [`Nests`] collects those scenes for the walk to
//! realize after the level that shows them. The conversion to a borrowed
//! [`aria_compositor::CompositeLayer`] is in the sibling `composite.rs`.

use std::sync::Arc;

use aria_compositor::{
    BlendMode, ColorGrade, FULL_UV, GlyphInstance, LayerEffects, LayerPlacement, LayerStencil,
    LayerStyles, RgbaImage, SdfLayer, StencilCoverage, StencilMask,
};
use aria_models::{MaskKind, MatteSource, MediaId, Project};

use crate::error::RenderError;
use crate::motion_blur::blur_placements;
use crate::scene::{LayerSource, Scene, SceneLayer, SceneLut, SizeSpec};

use super::super::effects::{blend_mode, layer_effects, layer_styles, shape_mask};
use super::super::look_ahead::{DecoderKey, Shown};
use super::super::{Renderer, SeekPolicy};
use super::text;

/// An owned, decoded or rasterized scene layer kept alive while the compositor
/// borrows it.
pub(super) enum Realized {
    /// A layer drawn alone.
    Layer(Leaf),
    Transition {
        outgoing: Box<Leaf>,
        incoming: Box<Leaf>,
        transition_id: String,
        progress: f32,
    },
    Matte {
        content: Box<Leaf>,
        stencil: Box<Leaf>,
        read: MatteSource,
        invert: bool,
        softness: f32,
    },
    CanvasPass {
        grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        /// The pass's mask over the canvas.
        stencil: Option<Stencil>,
    },
}

/// One layer's pixels, placement, and look.
pub(super) struct Leaf {
    pub(super) pixels: Pixels,
    pub(super) placement: LayerPlacement,
    pub(super) uv: [f32; 4],
    pub(super) fx: LayerEffects,
    pub(super) color_grade: Option<ColorGrade>,
    pub(super) lut: Option<SceneLut>,
    pub(super) blend_mode: BlendMode,
    pub(super) styles: LayerStyles,
    /// Motion-blur sample placements of the quad; empty draws it once.
    pub(super) blur: Vec<LayerPlacement>,
    /// A mask the fx pipeline does not draw for these pixels.
    pub(super) stencil: Option<Stencil>,
}

/// What a [`Leaf`] draws: owned pixels, or a key into a renderer cache.
pub(super) enum Pixels {
    /// A decoded frame, returned to its decoder after the composite.
    Frame(Shown),
    Still(MediaId),
    Sticker {
        asset: String,
        frame_index: usize,
    },
    Lottie {
        path: String,
        frame_index: usize,
    },
    /// A path raster held in `Renderer::paths`.
    Bitmap(Arc<RgbaImage>),
    /// Static text bitmap cached in `Renderer::text_memo`.
    CachedText(u64),
    Solid([u8; 4]),
    Sdf(SdfLayer),
    /// Per-character text: cluster bitmaps and instanced placements.
    Glyphs {
        glyphs: Vec<RgbaImage>,
        instances: Vec<GlyphInstance>,
        atlas_key: u64,
        /// The whole-run background card drawn behind the glyphs.
        background: Option<(RgbaImage, LayerPlacement)>,
    },
    /// The picture the scene at this index of [`Nests::scenes`] draws.
    Nest(u64),
}

impl Pixels {
    /// Whether the pixels draw as a sampled texture: the crop UV applies, and
    /// the fx pipeline draws a shape mask on them.
    fn is_textured(&self) -> bool {
        match self {
            Self::Frame(_)
            | Self::Still(_)
            | Self::Sticker { .. }
            | Self::Lottie { .. }
            | Self::Bitmap(_)
            | Self::CachedText(_)
            | Self::Nest(_) => true,
            Self::Solid(_) | Self::Sdf(_) | Self::Glyphs { .. } => false,
        }
    }
}

/// The scenes of the nests a frame shows, in the order their layers were
/// realized, and the canvas every nest draws onto.
///
/// Every nest draws onto a target the size of the frame's canvas, from its
/// top-left corner, so a nest scene smaller than the canvas fills part of it.
pub(super) struct Nests<'s> {
    canvas: [f32; 2],
    pub(super) scenes: Vec<&'s Scene>,
}

impl<'s> Nests<'s> {
    /// No nests yet, for a frame whose canvas is `scene`'s.
    pub(super) fn new(scene: &Scene) -> Self {
        Self {
            canvas: [scene.width as f32, scene.height as f32],
            scenes: Vec::new(),
        }
    }

    /// Add `scene`, returning the id its picture is drawn under and the
    /// share of the canvas target that picture covers.
    fn push(&mut self, scene: &'s Scene) -> (u64, [f32; 2]) {
        self.scenes.push(scene);
        let covers = [
            scene.width as f32 / self.canvas[0],
            scene.height as f32 / self.canvas[1],
        ];
        ((self.scenes.len() - 1) as u64, covers)
    }
}

/// `uv` over a picture that covers the `covers` share of its target, from the
/// target's top-left corner, as UV over the target.
fn uv_within([u0, v0, u1, v1]: [f32; 4], [sx, sy]: [f32; 2]) -> [f32; 4] {
    [u0 * sx, v0 * sy, u1 * sx, v1 * sy]
}

/// A mask stack placed on the canvas; see [`LayerStencil`].
pub(super) struct Stencil {
    masks: Vec<StencilMask>,
    /// The rasterized path masks, alpha the coverage, in stack order, held in
    /// `Renderer::paths`.
    images: Vec<Arc<RgbaImage>>,
    placement: LayerPlacement,
}

impl Stencil {
    /// The compositor's view of this stack, moving through `samples` with the
    /// layer it cuts.
    pub(super) fn borrow<'a>(&'a self, samples: &'a [LayerPlacement]) -> LayerStencil<'a> {
        LayerStencil {
            masks: &self.masks,
            images: &self.images,
            placement: self.placement,
            blur_passes: samples,
        }
    }
}

/// The on-canvas size for a non-text layer, falling back to the canvas if a
/// bitmap-scaled spec ever reaches here (it shouldn't for media/solid).
fn fixed_size(size: SizeSpec, canvas: [f32; 2]) -> [f32; 2] {
    match size {
        SizeSpec::Fixed(s) => s,
        SizeSpec::BitmapScaled(_) => canvas,
    }
}

impl Renderer {
    /// Realize a leaf scene layer: its pixels, placement, and look, with the
    /// layer's mask and motion-blur samples. `None` when the layer draws
    /// nothing: a missing Lottie file, an unknown sticker, a degenerate path,
    /// or text with no ink. A transition or matte side sets `side`, which
    /// draws text as one bitmap. A nest adds its scene to `nests`.
    ///
    /// # Errors
    ///
    /// A wrapper (a transition, a matte, or a canvas pass) is refused: only
    /// the top-level walk realizes wrappers, and a wrapper nested in a side
    /// is not drawn.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn realize_leaf<'s>(
        &mut self,
        project: &Project,
        scene: &Scene,
        layer: &'s SceneLayer,
        policy: SeekPolicy,
        side: bool,
        nests: &mut Nests<'s>,
        decode_ms: &mut f64,
    ) -> Result<Option<Leaf>, RenderError> {
        let canvas = [scene.width as f32, scene.height as f32];
        let place = |size: [f32; 2]| LayerPlacement {
            center: layer.quad_center(size),
            size,
            rotation: layer.rotation,
            opacity: layer.opacity,
        };
        let fixed = || place(fixed_size(layer.size, canvas));
        // The share of its texture a picture covers: all of it, except a nest
        // smaller than the canvas target it draws onto.
        let mut covers = [1.0, 1.0];
        let (pixels, placement) = match &layer.source {
            LayerSource::Solid(rgba) => (Pixels::Solid(*rgba), fixed()),
            LayerSource::Text {
                content,
                style,
                animation,
            } => {
                // A side draws its text as one bitmap.
                let animation = animation.as_ref().filter(|_| !side);
                match text::realize_text(
                    &mut self.text,
                    &mut self.text_memo,
                    layer,
                    content,
                    style,
                    animation,
                    canvas,
                ) {
                    Some(text) => text,
                    None => return Ok(None),
                }
            }
            LayerSource::Media { media, source_time } => {
                let decode_started = std::time::Instant::now();
                let key = DecoderKey::of(*media, layer);
                let shown = self.decode(project, key, *source_time, policy)?;
                *decode_ms += decode_started.elapsed().as_secs_f64() * 1000.0;
                (Pixels::Frame(shown), fixed())
            }
            LayerSource::Still { media } => {
                self.ensure_still(project, *media)?;
                (Pixels::Still(*media), fixed())
            }
            LayerSource::Lottie { path, local_time } => {
                // A missing or unsupported file draws nothing (the media
                // offline story: projects move machines), never an error.
                let Some(frame_index) = self.ensure_lottie_frame(path, *local_time) else {
                    return Ok(None);
                };
                let path = path.clone();
                (Pixels::Lottie { path, frame_index }, fixed())
            }
            LayerSource::Sticker { asset, local_time } => {
                // The resolver only emits catalog ids, but stay graceful: an
                // unknown id draws nothing rather than failing a frame.
                let Some(spec) = aria_models::sticker_spec(asset) else {
                    return Ok(None);
                };
                self.ensure_sticker(spec)?;
                let frame_index = self.stickers[spec.id.as_str()].frame_at(*local_time);
                let asset = asset.clone();
                (Pixels::Sticker { asset, frame_index }, fixed())
            }
            LayerSource::Shape {
                params,
                fill,
                stroke,
                pad,
            } => {
                // The resolver sized the quad as shape + pad per side; recover
                // the shape's own half-extents for the shader.
                let placement = fixed();
                let half = [
                    (placement.size[0] * 0.5 - pad).max(0.0),
                    (placement.size[1] * 0.5 - pad).max(0.0),
                ];
                let shape = SdfLayer {
                    shape: params.with_half(half),
                    fill: *fill,
                    stroke: *stroke,
                };
                (Pixels::Sdf(shape), placement)
            }
            LayerSource::PathShape {
                path,
                fill,
                stroke,
                raster_scale,
            } => {
                let style = crate::render::path_style(*fill, *stroke);
                let image = self.paths.rasterize(path, &style, *raster_scale);
                if image.width == 0 || image.height == 0 {
                    return Ok(None); // nothing inked (degenerate path or style)
                }
                let scale = match layer.size {
                    SizeSpec::BitmapScaled(s) => s,
                    SizeSpec::Fixed(_) => [1.0, 1.0],
                };
                let size = [
                    image.width as f32 * scale[0],
                    image.height as f32 * scale[1],
                ];
                (Pixels::Bitmap(image), place(size))
            }
            LayerSource::Nest { scene: nest, .. } => {
                let (id, share) = nests.push(nest);
                covers = share;
                (Pixels::Nest(id), fixed())
            }
            LayerSource::Transition { .. } => {
                return Err(RenderError::unsupported("nested transitions"));
            }
            LayerSource::Matte { .. } => {
                return Err(RenderError::unsupported("nested mattes"));
            }
            LayerSource::CanvasPass => {
                return Err(RenderError::unsupported("nested canvas pass"));
            }
        };
        let textured = pixels.is_textured();
        // Glyph runs carry their transform on each instance and draw no
        // motion-blur samples.
        let blur = match &pixels {
            Pixels::Glyphs { .. } => Vec::new(),
            _ => blur_placements(layer, placement.size),
        };
        let mut fx = layer_effects(layer);
        // The mask covers the layer's quad, turned with the layer.
        let mask_box = LayerPlacement {
            rotation: layer.rotation,
            opacity: 1.0,
            ..placement
        };
        let stencil = self.layer_stencil(layer, &mut fx, textured, mask_box);
        Ok(Some(Leaf {
            pixels,
            placement,
            uv: if textured {
                uv_within(layer.uv, covers)
            } else {
                FULL_UV
            },
            fx,
            color_grade: layer.color_grade,
            lut: self.resolve_scene_lut(&layer.lut),
            blend_mode: blend_mode(layer.blend_mode),
            styles: layer_styles(layer.styles.as_ref()),
            blur,
            stencil,
        }))
    }

    /// Realize one side of a transition or a matte. A side that draws
    /// nothing is a transparent layer, so the pair still blends.
    pub(super) fn realize_side<'s>(
        &mut self,
        project: &Project,
        scene: &Scene,
        layer: &'s SceneLayer,
        policy: SeekPolicy,
        nests: &mut Nests<'s>,
        decode_ms: &mut f64,
    ) -> Result<Box<Leaf>, RenderError> {
        let leaf = self.realize_leaf(project, scene, layer, policy, true, nests, decode_ms)?;
        Ok(Box::new(leaf.unwrap_or_else(|| Leaf {
            pixels: Pixels::Solid([0, 0, 0, 0]),
            placement: LayerPlacement {
                center: [scene.width as f32 * 0.5, scene.height as f32 * 0.5],
                size: [scene.width as f32, scene.height as f32],
                rotation: 0.0,
                opacity: 1.0,
            },
            uv: FULL_UV,
            fx: LayerEffects::default(),
            color_grade: None,
            lut: None,
            blend_mode: BlendMode::Normal,
            styles: LayerStyles::default(),
            blur: Vec::new(),
            stencil: None,
        })))
    }

    /// The stencil that cuts `layer` inside `mask_box`: its whole mask stack,
    /// with the fx mask taken out of `fx`. `None` when the layer has no mask
    /// that draws, or when the fx pipeline draws its one shape mask over
    /// `textured` pixels. A path of fewer than three points draws nothing and
    /// leaves the stack.
    pub(super) fn layer_stencil(
        &mut self,
        layer: &SceneLayer,
        fx: &mut LayerEffects,
        textured: bool,
        mask_box: LayerPlacement,
    ) -> Option<Stencil> {
        if textured && fx.mask.is_some() {
            return None;
        }
        fx.mask = None;
        let size = [
            mask_box.size[0].round().max(1.0) as u32,
            mask_box.size[1].round().max(1.0) as u32,
        ];
        let mut masks = Vec::with_capacity(layer.masks.len());
        let mut images = Vec::new();
        for mask in &layer.masks {
            let (coverage, softness) = if mask.kind == MaskKind::Path {
                if mask.path.len() < 3 {
                    continue;
                }
                images.push(self.paths.rasterize_mask(
                    size[0],
                    size[1],
                    &mask.path,
                    mask.center,
                    mask.rotation_rad,
                    mask.expansion,
                ));
                (
                    StencilCoverage::Image(images.len() - 1),
                    mask.feather.clamp(0.0, 1.0),
                )
            } else {
                // A shape feathers in the pass that evaluates it.
                (StencilCoverage::Shape(shape_mask(mask)), 0.0)
            };
            masks.push(StencilMask {
                coverage,
                invert: mask.invert,
                softness,
                op: mask.op,
                opacity: mask.opacity,
            });
        }
        if masks.is_empty() {
            return None;
        }
        Some(Stencil {
            masks,
            images,
            placement: mask_box,
        })
    }
}
