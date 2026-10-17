use aria_compositor::{
    BlendMode, CoverageMix, LayerBackground, LayerChromaKey, LayerEffects, LayerGlow, LayerMask,
    LayerOutline, LayerShadow, LayerStyles, mask_kind,
};
use aria_models::MaskKind;

use crate::scene::{SceneLayer, SceneMask, SceneStyles};

/// Floor for mask size axes (shader divides by size; avoid zeros).
const MASK_SIZE_EPS: f32 = 1e-3;

/// The layer's look the fx pipeline draws with its pixels: its chroma key,
/// and its mask when the stack is one shape mask. A path mask, or a stack of
/// more than one, cuts through a stencil instead.
pub(super) fn layer_effects(layer: &SceneLayer) -> LayerEffects {
    let mask = match layer.masks.as_slice() {
        [m] if m.kind != MaskKind::Path => Some(LayerMask {
            mix: CoverageMix::single(m.op, m.opacity, m.invert),
            ..shape_mask(m)
        }),
        _ => None,
    };
    let chroma_key = layer.chroma_key.map(|c| LayerChromaKey {
        rgb: [
            f32::from(c.rgb[0]) / 255.0,
            f32::from(c.rgb[1]) / 255.0,
            f32::from(c.rgb[2]) / 255.0,
        ],
        strength: c.strength,
        shadow: c.shadow,
    });
    LayerEffects { mask, chroma_key }
}

/// `m`'s shape for the compositor, before its inversion and its mix with the
/// masks above it. Only a parametric kind reads it: a path mask draws from
/// its raster.
pub(super) fn shape_mask(m: &SceneMask) -> LayerMask {
    LayerMask {
        kind: mask_kind_id(m.kind),
        feather: m.feather,
        invert: 0,
        center: m.center,
        size: [m.size[0].max(MASK_SIZE_EPS), m.size[1].max(MASK_SIZE_EPS)],
        rotation_rad: m.rotation_rad,
        roundness: m.roundness,
        expansion: m.expansion,
        mix: CoverageMix::KEEP,
    }
}

fn mask_kind_id(kind: MaskKind) -> u32 {
    match kind {
        MaskKind::Linear => mask_kind::LINEAR,
        MaskKind::Mirror => mask_kind::MIRROR,
        MaskKind::Circle => mask_kind::CIRCLE,
        MaskKind::Rectangle => mask_kind::RECTANGLE,
        MaskKind::Heart => mask_kind::HEART,
        MaskKind::Star => mask_kind::STAR,
        MaskKind::Path => 0,
    }
}

/// Map a model blend mode onto the compositor's GPU blend id.
pub(super) fn blend_mode(mode: aria_models::BlendMode) -> BlendMode {
    match mode {
        aria_models::BlendMode::Normal => BlendMode::Normal,
        aria_models::BlendMode::Darken => BlendMode::Darken,
        aria_models::BlendMode::Multiply => BlendMode::Multiply,
        aria_models::BlendMode::ColorBurn => BlendMode::ColorBurn,
        aria_models::BlendMode::Lighten => BlendMode::Lighten,
        aria_models::BlendMode::Screen => BlendMode::Screen,
        aria_models::BlendMode::ColorDodge => BlendMode::ColorDodge,
        aria_models::BlendMode::Add => BlendMode::Add,
        aria_models::BlendMode::Overlay => BlendMode::Overlay,
        aria_models::BlendMode::SoftLight => BlendMode::SoftLight,
        aria_models::BlendMode::HardLight => BlendMode::HardLight,
        aria_models::BlendMode::Difference => BlendMode::Difference,
        aria_models::BlendMode::Exclusion => BlendMode::Exclusion,
    }
}

/// Map resolved scene styles onto the compositor's layer-style uniforms.
///
/// `None` (or an empty styles block) keeps the compositor fast path.
pub(super) fn layer_styles(styles: Option<&SceneStyles>) -> LayerStyles {
    let Some(styles) = styles else {
        return LayerStyles::default();
    };
    LayerStyles {
        shadow: styles.shadow.map(|shadow| LayerShadow {
            rgba: shadow.rgba,
            offset: shadow.offset,
            blur: shadow.blur,
        }),
        glow: styles.glow.map(|glow| LayerGlow {
            rgba: glow.rgba,
            radius: glow.radius,
            intensity: glow.intensity,
        }),
        outline: styles.outline.map(|outline| LayerOutline {
            rgba: outline.rgba,
            width: outline.width,
        }),
        background: styles.background.map(|background| LayerBackground {
            rgba: background.rgba,
            padding: background.padding,
            radius: background.radius,
        }),
    }
}
