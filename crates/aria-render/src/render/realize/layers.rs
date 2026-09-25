//! The owned realized-layer model: what a decoded/rasterized layer is and how a
//! single scene layer becomes one. The conversion to a borrowed
//! [`aria_compositor::CompositeLayer`] lives in the sibling `composite.rs`.

use aria_compositor::{
    BlendMode, ColorGrade, GlyphInstance, LayerEffects, LayerPlacement, LayerStyles, RgbaImage,
    SdfLayer,
};
use aria_core::VideoFrame;
use aria_models::{MatteSource, MediaId, Project};
use aria_shapes::ShapeStyle;

use crate::error::RenderError;
use crate::scene::{LayerSource, Scene, SceneLut, SizeSpec};

use super::super::effects::{blend_mode, layer_effects, layer_styles};
use super::super::{Renderer, SeekPolicy};
use super::text;

/// An owned, decoded/rasterized layer kept alive while the compositor borrows it.
pub(super) enum Realized {
    Transition {
        outgoing: Box<Realized>,
        incoming: Box<Realized>,
        transition_id: String,
        progress: f32,
    },
    Matte {
        content: Box<Realized>,
        stencil: Box<Realized>,
        read: MatteSource,
        invert: bool,
        softness: f32,
    },
    CanvasPass {
        grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
    },
    Frame {
        frame: VideoFrame,
        placement: LayerPlacement,
        uv: [f32; 4],
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Still {
        media: MediaId,
        placement: LayerPlacement,
        uv: [f32; 4],
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Sticker {
        asset: String,
        frame_index: usize,
        placement: LayerPlacement,
        uv: [f32; 4],
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Lottie {
        path: String,
        frame_index: usize,
        placement: LayerPlacement,
        uv: [f32; 4],
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Bitmap {
        image: RgbaImage,
        placement: LayerPlacement,
        uv: [f32; 4],
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Solid {
        rgba: [u8; 4],
        placement: LayerPlacement,
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    Sdf {
        shape: SdfLayer,
        placement: LayerPlacement,
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
    /// Per-character text: cluster bitmaps + instanced placements.
    Glyphs {
        glyphs: Vec<RgbaImage>,
        instances: Vec<GlyphInstance>,
        atlas_key: u64,
        /// Optional whole-run background card drawn behind the glyphs.
        background: Option<(RgbaImage, LayerPlacement)>,
        /// Layer opacity multiplier (instance opacities are pre-multiplied).
        placement: LayerPlacement,
        fx: LayerEffects,
        color_grade: Option<ColorGrade>,
        lut: Option<SceneLut>,
        blend_mode: BlendMode,
        styles: LayerStyles,
    },
}

/// The on-canvas size for a non-text layer, falling back to the canvas if a
/// bitmap-scaled spec ever reaches here (it shouldn't for media/solid).
pub(super) fn fixed_size(size: SizeSpec, canvas: [f32; 2]) -> [f32; 2] {
    match size {
        SizeSpec::Fixed(s) => s,
        SizeSpec::BitmapScaled(_) => canvas,
    }
}

impl Renderer {
    pub(super) fn realize_subscene_layer(
        &mut self,
        project: &Project,
        scene: &Scene,
        layer: &crate::scene::SceneLayer,
        policy: SeekPolicy,
    ) -> Result<Box<Realized>, RenderError> {
        let place = |size: [f32; 2]| LayerPlacement {
            center: layer.quad_center(size),
            size,
            rotation: layer.rotation,
            opacity: layer.opacity,
        };
        let fx = layer_effects(layer);
        let color_grade = layer.color_grade;
        let mode = blend_mode(layer.blend_mode);
        let styles = layer_styles(layer.styles.as_ref());
        let realized = match &layer.source {
            LayerSource::Solid(rgba) => {
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                Realized::Solid {
                    rgba: *rgba,
                    placement: place(size),
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
            }
            LayerSource::Text {
                content,
                style,
                animation,
            } => {
                // Transition sides keep the bitmap path — per-character
                // animation on a transition edge is not a supported surface.
                let _ = animation;
                text::realize_text_bitmap(
                    &mut self.text,
                    layer,
                    content,
                    style,
                    [scene.width as f32, scene.height as f32],
                    fx,
                    color_grade,
                    mode,
                    styles,
                )?
            }
            LayerSource::Media { media, source_time } => {
                let frame = self.decode(project, *media, 0, *source_time, policy)?;
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                Realized::Frame {
                    frame,
                    placement: place(size),
                    uv: layer.uv,
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
            }
            LayerSource::Still { media } => {
                self.ensure_still(project, *media)?;
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                Realized::Still {
                    media: *media,
                    placement: place(size),
                    uv: layer.uv,
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
            }
            LayerSource::Lottie { path, local_time } => {
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                match self.ensure_lottie_frame(path, *local_time) {
                    Some(frame_index) => Realized::Lottie {
                        path: path.clone(),
                        frame_index,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: None,
                        blend_mode: mode,
                        styles,
                    },
                    // Missing file inside a transition: a transparent side,
                    // matching the draw-nothing policy of the main path.
                    None => Realized::Solid {
                        rgba: [0, 0, 0, 0],
                        placement: place(size),
                        fx,
                        color_grade,
                        lut: None,
                        blend_mode: mode,
                        styles,
                    },
                }
            }
            LayerSource::Sticker { asset, local_time } => {
                let spec = aria_models::sticker_spec(asset)
                    .ok_or_else(|| RenderError::unsupported("unknown sticker asset"))?;
                self.ensure_sticker(spec)?;
                let frame_index = self.stickers[spec.id.as_str()].frame_at(*local_time);
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                Realized::Sticker {
                    asset: asset.clone(),
                    frame_index,
                    placement: place(size),
                    uv: layer.uv,
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
            }
            LayerSource::Shape {
                params,
                fill,
                stroke,
                pad,
            } => {
                let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                let half = [
                    (size[0] * 0.5 - pad).max(0.0),
                    (size[1] * 0.5 - pad).max(0.0),
                ];
                Realized::Sdf {
                    shape: SdfLayer {
                        shape: params.with_half(half),
                        fill: *fill,
                        stroke: *stroke,
                    },
                    placement: place(size),
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
            }
            LayerSource::PathShape {
                path,
                fill,
                stroke,
                raster_scale,
            } => {
                let style = ShapeStyle {
                    fill: Some(*fill).filter(|c| c[3] > 0),
                    stroke: *stroke,
                };
                let image = self.paths.rasterize(path, &style, *raster_scale);
                if image.width == 0 || image.height == 0 {
                    return Err(RenderError::unsupported("degenerate path layer"));
                }
                let scale = match layer.size {
                    SizeSpec::BitmapScaled(s) => s,
                    SizeSpec::Fixed(_) => [1.0, 1.0],
                };
                let size = [
                    image.width as f32 * scale[0],
                    image.height as f32 * scale[1],
                ];
                Realized::Bitmap {
                    image,
                    placement: place(size),
                    uv: layer.uv,
                    fx,
                    color_grade,
                    lut: None,
                    blend_mode: mode,
                    styles,
                }
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
        Ok(Box::new(realized))
    }
}
