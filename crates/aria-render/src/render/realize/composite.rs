//! One realized layer → one borrowed [`CompositeLayer`].
//!
//! Moved out of `layers.rs` to stay under the file ceiling; nothing changed.
//! The three non-layer variants are named rather than swept by a wildcard, so a
//! new `Realized` variant stops compiling here until it says which side it is
//! on.

use std::collections::HashMap;

use aria_compositor::{CompositeLayer, GlyphsLayer, PassInstance, RgbaImage};
use aria_models::MediaId;

use super::super::media_cache::{CubeLutState, LottieState, StickerSequence, layer_lut};
use super::layers::Realized;

pub(super) fn composite_from_realized<'a>(
    r: &'a Realized,
    stills: &'a HashMap<MediaId, RgbaImage>,
    stickers: &'a HashMap<String, StickerSequence>,
    lottie: &'a HashMap<String, LottieState>,
    luts: &'a HashMap<String, CubeLutState>,
    effects: &'a [PassInstance],
) -> CompositeLayer<'a> {
    match r {
        Realized::Solid {
            rgba,
            placement,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::solid(*rgba, *placement)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Bitmap {
            image,
            placement,
            uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::rgba(image, *placement)
            .with_uv(*uv)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Frame {
            frame,
            placement,
            uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::frame(frame, *placement)
            .with_uv(*uv)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Still {
            media,
            placement,
            uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::rgba(&stills[media], *placement)
            .with_uv(*uv)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Sticker {
            asset,
            frame_index,
            placement,
            uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::rgba(&stickers[asset].frames[*frame_index], *placement)
            .with_uv(*uv)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Lottie {
            path,
            frame_index,
            placement,
            uv,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => {
            // Realize only emits `Realized::Lottie` after `ensure_lottie_frame`
            // cached this exact frame, and the LRU never evicts frames stamped
            // by the scene being composed.
            let LottieState::Loaded(player) = &lottie[path] else {
                unreachable!("realized lottie layer without a loaded player")
            };
            CompositeLayer::rgba(&player.frames[frame_index].0, *placement)
                .with_uv(*uv)
                .with_fx(*fx)
                .with_effects(effects)
                .with_color_grade(*color_grade)
                .with_lut(layer_lut(lut, luts))
                .with_blend_mode(*blend_mode)
                .with_styles(*styles)
        }
        Realized::Sdf {
            shape,
            placement,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => CompositeLayer::sdf(*shape, *placement)
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles),
        Realized::Glyphs {
            glyphs,
            instances,
            atlas_key,
            background,
            placement,
            fx,
            color_grade,
            lut,
            blend_mode,
            styles,
            ..
        } => {
            // Background is composited as a separate preceding layer in the
            // job walk; this arm only builds the glyph instances.
            let _ = background;
            CompositeLayer::glyphs(
                GlyphsLayer {
                    atlas_key: *atlas_key,
                    glyphs,
                    instances,
                },
                *placement,
            )
            .with_fx(*fx)
            .with_effects(effects)
            .with_color_grade(*color_grade)
            .with_lut(layer_lut(lut, luts))
            .with_blend_mode(*blend_mode)
            .with_styles(*styles)
        }
        Realized::Transition { .. } | Realized::Matte { .. } | Realized::CanvasPass { .. } => {
            unreachable!("non-layer realized items handled separately")
        }
    }
}
