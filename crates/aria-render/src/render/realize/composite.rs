//! One realized [`Leaf`] → the borrowed [`CompositeLayer`]s it draws as.

use std::collections::HashMap;

use aria_compositor::{
    CompositeLayer, FULL_UV, GlyphsLayer, LayerContent, LayerStyles, PassInstance, RgbaImage,
};
use aria_models::MediaId;

use super::super::media_cache::{CubeLutState, LottieState, StickerSequence, layer_lut};
use super::layers::{Leaf, Pixels};

/// The renderer caches the pixels of a realized layer are read from.
pub(super) struct Caches<'a> {
    pub(super) stills: &'a HashMap<MediaId, RgbaImage>,
    pub(super) stickers: &'a HashMap<String, StickerSequence>,
    pub(super) lottie: &'a HashMap<String, LottieState>,
    pub(super) luts: &'a HashMap<String, CubeLutState>,
}

fn content<'a>(pixels: &'a Pixels, caches: &Caches<'a>) -> LayerContent<'a> {
    match pixels {
        Pixels::Frame(frame) => LayerContent::Frame(frame),
        Pixels::Still(media) => LayerContent::Rgba(&caches.stills[media]),
        Pixels::Sticker { asset, frame_index } => {
            LayerContent::Rgba(&caches.stickers[asset].frames[*frame_index])
        }
        Pixels::Lottie { path, frame_index } => {
            // Realize only emits Lottie pixels after `ensure_lottie_frame`
            // cached this exact frame, and the LRU never evicts frames stamped
            // by the scene being composed.
            let LottieState::Loaded(player) = &caches.lottie[path] else {
                unreachable!("realized lottie layer without a loaded player")
            };
            LayerContent::Rgba(&player.frames[frame_index].0)
        }
        Pixels::Bitmap(image) => LayerContent::Rgba(image),
        Pixels::Solid(rgba) => LayerContent::Solid(*rgba),
        Pixels::Sdf(shape) => LayerContent::Sdf(*shape),
        Pixels::Glyphs {
            glyphs,
            instances,
            atlas_key,
            ..
        } => LayerContent::Glyphs(GlyphsLayer {
            atlas_key: *atlas_key,
            glyphs,
            instances,
        }),
        Pixels::Nest(nest) => LayerContent::Nest(*nest),
    }
}

/// `leaf` as the compositor draws it, under `effects`.
pub(super) fn composite_leaf<'a>(
    leaf: &'a Leaf,
    caches: &Caches<'a>,
    effects: &'a [PassInstance],
) -> CompositeLayer<'a> {
    CompositeLayer {
        content: content(&leaf.pixels, caches),
        placement: leaf.placement,
        uv: leaf.uv,
        effects,
        fx: leaf.fx,
        color_grade: leaf.color_grade,
        lut: layer_lut(&leaf.lut, caches.luts),
        blend_mode: leaf.blend_mode,
        styles: leaf.styles,
        blur_passes: &leaf.blur,
        stencil: leaf
            .stencil
            .as_ref()
            .map(|stencil| stencil.borrow(&leaf.blur)),
    }
}

/// The background card of a glyph run, drawn behind it with the run's look
/// and mask and none of its styles; `None` for any other leaf.
pub(super) fn composite_card<'a>(
    leaf: &'a Leaf,
    caches: &Caches<'a>,
    effects: &'a [PassInstance],
) -> Option<CompositeLayer<'a>> {
    let Pixels::Glyphs {
        background: Some((image, placement)),
        ..
    } = &leaf.pixels
    else {
        return None;
    };
    Some(CompositeLayer {
        content: LayerContent::Rgba(image),
        placement: *placement,
        uv: FULL_UV,
        styles: LayerStyles::default(),
        ..composite_leaf(leaf, caches, effects)
    })
}
