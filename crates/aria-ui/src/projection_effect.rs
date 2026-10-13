//! Effect: 2 helpers from [`super`].
//!
//! Split out of `projection.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;
use aria_models::EffectParamKind;
use gpui::SharedString;

use crate::types::UiEffectValue;

pub(super) fn effect_to_ui(effect: &EffectInstance) -> UiEffectInstance {
    let spec = effect.spec().ok();
    let values = spec
        .as_ref()
        .map(|spec| {
            spec.params
                .iter()
                .map(|param| match param.kind {
                    EffectParamKind::Scalar => UiEffectValue::Scalar(
                        effect
                            .params
                            .get(&param.name)
                            .map(|value| value.sample(0))
                            .unwrap_or(param.default),
                    ),
                    EffectParamKind::Color => UiEffectValue::Color(
                        effect
                            .sample_color_param(&param.name, 0.0)
                            .unwrap_or(param.default_color),
                    ),
                    EffectParamKind::Vec2 => UiEffectValue::Vec2(
                        effect
                            .sample_vec2_param(&param.name, 0.0)
                            .unwrap_or(param.default_vec2),
                    ),
                })
                .collect()
        })
        .unwrap_or_default();
    UiEffectInstance {
        id: effect.effect_id.clone().into(),
        name: spec
            .map(|s| SharedString::from(s.label.clone()))
            .unwrap_or_else(|| effect.effect_id.clone().into()),
        enabled: true,
        values,
    }
}

pub(super) fn track_tint(kind: TrackKind) -> u32 {
    match kind {
        TrackKind::Video => 0x8B8BA3,
        TrackKind::Audio => 0x7A9A86,
        TrackKind::Text => 0xA39A7A,
        TrackKind::Sticker => 0x7A92A3,
        // A bus reads as the sound family it sums, darker: it holds nothing of
        // its own.
        TrackKind::Submix => 0x5E7A69,
        TrackKind::Effect | TrackKind::Filter | TrackKind::Adjustment => 0xA37A7A,
    }
}
