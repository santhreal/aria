//! Where a resolved layer lands on the canvas, without drawing it.
//!
//! A text or pen-path layer has no size until its content is rasterized, so
//! the resolver leaves it relative ([`SizeSpec::BitmapScaled`]) and the realize
//! pass sizes it from the bitmap. Selection handles and the canvas pick need
//! that quad on frames nothing composited, so the measure asks the rasterizers
//! the realize pass asks, through the same memos: a size taken here is a raster
//! the next composite reuses, and a size taken after a composite is one lookup.

use aria_models::EffectBounds;
use aria_shapes::{ShapeStyle, Stroke};

use super::Renderer;
use crate::scene::{CpuRaster, LayerSource, SceneLayer, SizeSpec, layer_source_spec};

/// The style a path-shape layer rasterizes with. A zero-alpha fill draws
/// nothing, so it is no fill: a path with neither fill nor stroke has no
/// raster and no quad.
pub fn path_style(fill: [u8; 4], stroke: Option<Stroke>) -> ShapeStyle {
    ShapeStyle {
        fill: Some(fill).filter(|rgba| rgba[3] > 0),
        stroke,
    }
}

/// One caret of a title as the canvas draws it: a segment from the top of
/// its line to the bottom, in canvas pixels with the layer's scale and
/// rotation applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerCaret {
    /// Byte offset in the layer's content the caret sits before.
    pub byte: usize,
    /// Zero-based visual line.
    pub line: usize,
    /// The end of the segment at the top of the line.
    pub top: [f32; 2],
    /// The end of the segment at the bottom of the line.
    pub bottom: [f32; 2],
}

impl Renderer {
    /// The quad `layer` is drawn on in a `canvas`-sized scene, `(center,
    /// size)` in canvas pixels before rotation, placed as the realize pass
    /// places it: the anchor offset is folded into the center, a bitmap layer
    /// is its raster times the layer scale, and a title sits at its alignment.
    ///
    /// `None` for a bitmap layer with no ink. A layer that draws no picture of
    /// its own, such as a canvas pass, still has the quad its size names.
    pub fn layer_quad(
        &mut self,
        layer: &SceneLayer,
        canvas: [f32; 2],
    ) -> Option<([f32; 2], [f32; 2])> {
        if !layer_source_spec(&layer.source).cpu_raster.is_cpu() {
            let size = match layer.size {
                SizeSpec::Fixed(size) => size,
                SizeSpec::BitmapScaled(_) => canvas,
            };
            return Some((layer.quad_center(size), size));
        }
        let scale = match layer.size {
            SizeSpec::BitmapScaled(scale) => scale,
            SizeSpec::Fixed(_) => [1.0, 1.0],
        };
        let (width, height) = self.raster_size(&layer.source)?;
        let size = [width as f32 * scale[0], height as f32 * scale[1]];
        let center = match &layer.source {
            LayerSource::Text { style, .. } => layer.text_quad_center(style, size, canvas),
            _ => layer.quad_center(size),
        };
        Some((center, size))
    }

    /// The carets of a title `layer` in a `canvas`-sized scene, one per
    /// character boundary, placed where [`layer_quad`](Self::layer_quad)
    /// places the title. `None` for a layer that is not a title and for a
    /// title scaled to nothing.
    ///
    /// A static title is drawn from its ink box with the effect padding on
    /// every side; an animated one from the ink box alone, as
    /// `raster_size` measures them. A title with no ink
    /// still has a caret: its empty line, centred where the title sits.
    pub fn layer_carets(
        &mut self,
        layer: &SceneLayer,
        canvas: [f32; 2],
    ) -> Option<Vec<LayerCaret>> {
        let LayerSource::Text {
            content,
            style,
            animation,
        } = &layer.source
        else {
            return None;
        };
        let scale = match layer.size {
            SizeSpec::BitmapScaled(scale) => scale,
            SizeSpec::Fixed(_) => [1.0, 1.0],
        };
        if scale[0] == 0.0 || scale[1] == 0.0 {
            return None;
        }
        let stops = self.text.carets(content, style);
        let (center, raster, pad) = match self.layer_quad(layer, canvas) {
            Some((center, size)) => {
                let pad = match animation {
                    Some(_) => 0.0,
                    None => aria_text::raster_padding(style) as f32,
                };
                (center, [size[0] / scale[0], size[1] / scale[1]], pad)
            }
            None => {
                let height = stops.iter().map(|stop| stop.bottom).fold(0.0f32, f32::max);
                let size = [0.0, height * scale[1]];
                (
                    layer.text_quad_center(style, size, canvas),
                    [0.0, height],
                    0.0,
                )
            }
        };
        let (sin, cos) = layer.rotation.sin_cos();
        let place = |x: f32, y: f32| {
            let dx = (x + pad - raster[0] * 0.5) * scale[0];
            let dy = (y + pad - raster[1] * 0.5) * scale[1];
            [
                center[0] + dx * cos - dy * sin,
                center[1] + dx * sin + dy * cos,
            ]
        };
        Some(
            stops
                .iter()
                .map(|stop| LayerCaret {
                    byte: stop.byte,
                    line: stop.line,
                    top: place(stop.x, stop.top),
                    bottom: place(stop.x, stop.bottom),
                })
                .collect(),
        )
    }

    /// The pixel size of the bitmap a layer is drawn from, `None` for a
    /// topology with no CPU raster and for a raster with no ink.
    ///
    /// Which rasterizer to ask is the topology's own column, so a topology
    /// that grows a new raster kind turns this match red rather than measuring
    /// as nothing.
    pub(super) fn raster_size(&mut self, source: &LayerSource) -> Option<(u32, u32)> {
        let (width, height) = match layer_source_spec(source).cpu_raster {
            CpuRaster::None => return None,
            CpuRaster::Text => {
                // The column already named this variant; the pattern only
                // lifts its payload out.
                let LayerSource::Text {
                    content,
                    style,
                    animation,
                } = source
                else {
                    return None;
                };
                // Animated text is placed by its ink-tight shaped extent, a
                // static run by its painted bitmap.
                if animation.is_some() {
                    self.text.shaped_extent(content, style)
                } else {
                    self.text.raster_size(content, style)
                }
            }
            CpuRaster::Path => {
                let LayerSource::PathShape {
                    path,
                    fill,
                    stroke,
                    raster_scale,
                } = source
                else {
                    return None;
                };
                aria_shapes::path_raster_size(path, &path_style(*fill, *stroke), *raster_scale)
            }
        };
        (width > 0 && height > 0).then_some((width, height))
    }
}

/// The region `layer` draws on when [`Renderer::layer_quad`] places it on
/// `quad`: the quad grown by the margins its registered effects declare
/// ([`EffectBounds`]), summed in chain order, since each effect draws past the
/// frame the one before it left.
///
/// Effects run in canvas pixels, so the margins are canvas-axis. For a rotated
/// layer each margin is projected onto both of the quad's axes, which covers
/// the margin at any angle and matches it at multiples of a quarter turn. A
/// shrinking margin never takes a side below zero.
pub fn drawn_bounds(
    layer: &SceneLayer,
    (center, size): ([f32; 2], [f32; 2]),
) -> ([f32; 2], [f32; 2]) {
    let mut m = EffectBounds::ZERO;
    for pass in &layer.effects {
        if let aria_compositor::PassId::Registered(registered) = &pass.id {
            let b = registered.bounds();
            m.left += b.left;
            m.top += b.top;
            m.right += b.right;
            m.bottom += b.bottom;
        }
    }
    if m.is_zero() {
        return (center, size);
    }
    let (sin, cos) = layer.rotation.sin_cos();
    let (sin, cos) = (sin.abs(), cos.abs());
    let (across, down) = (m.left + m.right, m.top + m.bottom);
    (
        [
            center[0] + (m.right - m.left) * 0.5,
            center[1] + (m.bottom - m.top) * 0.5,
        ],
        [
            (size[0] + cos * across + sin * down).max(0.0),
            (size[1] + sin * across + cos * down).max(0.0),
        ],
    )
}
