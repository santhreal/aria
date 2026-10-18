//! Text / glyph realize arms — bitmap (static) and per-character GPU glyphs.

use std::collections::HashMap;

use aria_compositor::{LayerPlacement, RgbaImage};
use aria_text::{ClusterBox, TextRenderer, TextStyle};

use crate::scene::{SceneLayer, SizeSpec, TextAnimation};

use super::super::text_anim::{
    atlas_key, cluster_deltas, extent_origin, place_clusters, static_raster_key,
};
use super::layers::Pixels;

/// The pixels and placement of a text layer: instanced glyphs under an
/// `animation`, one bitmap otherwise.
///
/// Returns `None` when the run has no ink (empty text, or no fonts).
pub(super) fn realize_text(
    text: &mut TextRenderer,
    text_memo: &mut HashMap<u64, RgbaImage>,
    layer: &SceneLayer,
    content: &str,
    style: &TextStyle,
    animation: Option<&TextAnimation>,
    canvas: [f32; 2],
) -> Option<(Pixels, LayerPlacement)> {
    let scale = match layer.size {
        SizeSpec::BitmapScaled(s) => s,
        SizeSpec::Fixed(_) => [1.0, 1.0],
    };
    let Some(anim) = animation else {
        let key = static_raster_key(content, style);
        let image = text_memo
            .entry(key)
            .or_insert_with(|| text.rasterize(content, style));
        if image.width == 0 || image.height == 0 {
            return None;
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
        return Some((Pixels::CachedText(key), placement));
    };
    let shaped = text.shape(content, style);
    if !shaped.has_ink() {
        return None;
    }
    let painted = aria_text::paint_animated(&shaped, style);
    // Deltas use the ink-tight shaped clusters (logical order / baselines);
    // placement uses painted images.
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
    let pixels = Pixels::Glyphs {
        glyphs,
        instances,
        atlas_key: atlas_key(content, style),
        background,
    };
    // Each instance carries its glyph's transform and the layer's opacity;
    // the placement spans the run.
    let placement = LayerPlacement {
        center: aligned,
        size: extent_size,
        rotation: 0.0,
        opacity: 1.0,
    };
    Some((pixels, placement))
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
