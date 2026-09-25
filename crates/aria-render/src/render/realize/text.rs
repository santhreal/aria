//! Text / glyph realize arms — bitmap (static) and per-character GPU glyphs.

use aria_compositor::{
    BlendMode, ColorGrade, LayerEffects, LayerPlacement, LayerStyles, RgbaImage,
};
use aria_text::{ClusterBox, TextRenderer, TextStyle};

use crate::error::RenderError;
use crate::scene::{SceneLayer, SizeSpec, TextAnimation};

use super::super::text_anim::{atlas_key, cluster_deltas, extent_origin, place_clusters};
use super::Realized;

/// Realize a text layer for the main scene walk.
///
/// Returns `None` when the run has no ink (empty / no fonts) — the caller
/// skips the layer, matching the previous inline `continue`.
#[allow(clippy::too_many_arguments)] // mirrors former inline match-arm locals
pub(super) fn realize_text_layer(
    text: &mut TextRenderer,
    layer: &SceneLayer,
    content: &str,
    style: &TextStyle,
    animation: &Option<TextAnimation>,
    canvas: [f32; 2],
    fx: LayerEffects,
    color_grade: Option<ColorGrade>,
    lut: Option<crate::scene::SceneLut>,
    blend_mode: BlendMode,
    styles: LayerStyles,
) -> Option<Realized> {
    let scale = match layer.size {
        SizeSpec::BitmapScaled(s) => s,
        SizeSpec::Fixed(_) => [1.0, 1.0],
    };
    if let Some(anim) = animation {
        let shaped = text.shape(content, style);
        if !shaped.has_ink() {
            return None;
        }
        let painted = aria_text::paint_animated(&shaped, style);
        // Deltas use the ink-tight shaped clusters (logical
        // order / baselines); placement uses painted images.
        let deltas = cluster_deltas(&shaped, anim);
        let extent_size = [
            painted.extent.0 as f32 * scale[0],
            painted.extent.1 as f32 * scale[1],
        ];
        let aligned = layer.text_quad_center(style, extent_size, canvas);
        let origin = extent_origin(aligned, painted.extent, scale);
        let (glyphs, instances) = move_painted_clusters(
            painted.extent,
            painted.clusters,
            &deltas,
            origin,
            scale,
            layer.rotation,
            layer.opacity,
        );
        if instances.is_empty() {
            return None;
        }
        let background = painted.background.map(|bg| {
            let size = [bg.width as f32 * scale[0], bg.height as f32 * scale[1]];
            let center = [
                origin[0] + painted.background_offset[0] * scale[0] + size[0] * 0.5,
                origin[1] + painted.background_offset[1] * scale[1] + size[1] * 0.5,
            ];
            (
                bg,
                LayerPlacement {
                    center,
                    size,
                    rotation: layer.rotation,
                    opacity: layer.opacity,
                },
            )
        });
        Some(Realized::Glyphs {
            glyphs,
            instances,
            atlas_key: atlas_key(content, style),
            background,
            placement: LayerPlacement {
                center: aligned,
                size: extent_size,
                rotation: 0.0,
                opacity: 1.0,
            },
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
        })
    } else {
        let image = text.rasterize(content, style);
        if image.width == 0 || image.height == 0 {
            return None; // nothing rasterized (no fonts / empty run)
        }
        let size = [
            image.width as f32 * scale[0],
            image.height as f32 * scale[1],
        ];
        let placement = LayerPlacement {
            center: layer.text_quad_center(style, size, canvas),
            size,
            rotation: layer.rotation,
            opacity: layer.opacity,
        };
        Some(Realized::Bitmap {
            image,
            placement,
            uv: layer.uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
        })
    }
}

/// Realize text for a transition side — bitmap path only.
///
/// Per-character animation on a transition edge is not a supported surface.
#[allow(clippy::too_many_arguments)] // mirrors former inline match-arm locals
pub(super) fn realize_text_bitmap(
    text: &mut TextRenderer,
    layer: &SceneLayer,
    content: &str,
    style: &TextStyle,
    canvas: [f32; 2],
    fx: LayerEffects,
    color_grade: Option<ColorGrade>,
    blend_mode: BlendMode,
    styles: LayerStyles,
) -> Result<Realized, RenderError> {
    let image = text.rasterize(content, style);
    if image.width == 0 || image.height == 0 {
        return Err(RenderError::unsupported("empty text layer"));
    }
    let scale = match layer.size {
        SizeSpec::BitmapScaled(s) => s,
        SizeSpec::Fixed(_) => [1.0, 1.0],
    };
    let size = [
        image.width as f32 * scale[0],
        image.height as f32 * scale[1],
    ];
    let placement = LayerPlacement {
        center: layer.text_quad_center(style, size, canvas),
        size,
        rotation: layer.rotation,
        opacity: layer.opacity,
    };
    Ok(Realized::Bitmap {
        image,
        placement,
        uv: layer.uv,
        fx,
        color_grade,
        lut: None,
        blend_mode,
        styles,
    })
}

#[allow(clippy::too_many_arguments)]
fn move_painted_clusters(
    extent: (u32, u32),
    clusters: Vec<ClusterBox>,
    deltas: &[super::super::text_anim::ClusterDelta],
    origin: [f32; 2],
    scale: [f32; 2],
    layer_rotation: f32,
    layer_opacity: f32,
) -> (Vec<RgbaImage>, Vec<aria_compositor::GlyphInstance>) {
    let painted = aria_text::ShapedText { extent, clusters };
    let instances = place_clusters(
        &painted,
        deltas,
        origin,
        scale,
        layer_rotation,
        layer_opacity,
    );
    let glyphs = painted
        .clusters
        .into_iter()
        .map(|cluster| cluster.image)
        .collect();
    (glyphs, instances)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_cluster_pixels_move_into_realized_glyphs() {
        let pixels = vec![255; 4 * 3 * 4];
        let allocation = pixels.as_ptr();
        let clusters = vec![ClusterBox {
            text_range: 0..1,
            line: 0,
            offset: [0.0, 0.0],
            baseline: 2.0,
            image: RgbaImage::new(4, 3, pixels),
        }];

        let (glyphs, instances) = move_painted_clusters(
            (4, 3),
            clusters,
            &[super::super::super::text_anim::ClusterDelta::IDENTITY],
            [0.0, 0.0],
            [1.0, 1.0],
            0.0,
            1.0,
        );

        assert_eq!(glyphs.len(), 1);
        assert_eq!(instances.len(), 1);
        assert_eq!(
            glyphs[0].pixels.as_ptr(),
            allocation,
            "realization must move rather than clone uncompressed glyph pixels"
        );
    }
}
