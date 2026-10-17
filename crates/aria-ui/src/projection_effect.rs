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
    let (values, visible) = spec
        .as_ref()
        .map(|spec| {
            let values = spec
                .params
                .iter()
                .map(|param| match param.kind {
                    EffectParamKind::Scalar | EffectParamKind::Angle => UiEffectValue::Scalar(
                        effect
                            .sample_param(&param.name, 0.0)
                            .unwrap_or(param.default),
                    ),
                    EffectParamKind::Int => UiEffectValue::Int(
                        effect
                            .sample_int_param(&param.name, 0.0)
                            .unwrap_or(param.default_int) as i32,
                    ),
                    EffectParamKind::Bool => UiEffectValue::Bool(
                        effect
                            .sample_bool_param(&param.name)
                            .unwrap_or(param.default_bool),
                    ),
                    EffectParamKind::Choice => UiEffectValue::Choice(
                        effect
                            .sample_choice_param(&param.name)
                            .or_else(|| param.default_choice_id().map(str::to_owned))
                            .unwrap_or_default()
                            .into(),
                    ),
                    EffectParamKind::Color => UiEffectValue::Color(
                        effect
                            .sample_color_param(&param.name, 0.0)
                            .unwrap_or_else(|| param.default_color_u8()),
                    ),
                    EffectParamKind::Vec2 | EffectParamKind::Size => UiEffectValue::Vec2(
                        effect
                            .sample_vec2_param(&param.name, 0.0)
                            .unwrap_or(param.default_vec2),
                    ),
                    EffectParamKind::Point3 => UiEffectValue::Point3(
                        effect
                            .sample_vec3_param(&param.name, 0.0)
                            .unwrap_or(param.default_vec3),
                    ),
                    EffectParamKind::Curve => UiEffectValue::Curve(
                        effect
                            .sample_curve_param(&param.name)
                            .unwrap_or_else(|| param.default_curve_points()),
                    ),
                    EffectParamKind::Gradient => UiEffectValue::Gradient(
                        effect
                            .sample_gradient_param(&param.name)
                            .unwrap_or_else(|| param.default_gradient_stops()),
                    ),
                    EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {
                        UiEffectValue::Text(
                            effect
                                .sample_text_param(&param.name)
                                .unwrap_or_else(|| param.default_text.clone())
                                .into(),
                        )
                    }
                    EffectParamKind::Layer => UiEffectValue::Layer(
                        effect
                            .sample_layer_param(&param.name)
                            .flatten()
                            .map(aria_models::ClipId::raw),
                    ),
                })
                .collect();
            let visible = spec
                .params
                .iter()
                .map(|param| effect.is_param_visible(param, spec, 0.0))
                .collect();
            (values, visible)
        })
        .unwrap_or_default();
    UiEffectInstance {
        id: effect.effect_id.clone().into(),
        name: match (&effect.name, spec) {
            (Some(name), _) => SharedString::from(name.clone()),
            (None, Some(spec)) => SharedString::from(spec.label.clone()),
            (None, None) => effect.effect_id.clone().into(),
        },
        bypass: effect.bypass,
        values,
        visible,
    }
}

pub(super) fn transition_to_ui(transition: &aria_models::Transition) -> UiTransition {
    let spec = aria_models::transition_spec(&transition.transition_id);
    let (values, visible) = spec
        .as_ref()
        .map(|spec| {
            let values: Vec<_> = spec
                .params
                .iter()
                .map(|param| match param.kind {
                    EffectParamKind::Scalar | EffectParamKind::Angle => UiEffectValue::Scalar(
                        transition
                            .sample_param(&param.name)
                            .unwrap_or(param.default),
                    ),
                    EffectParamKind::Int => UiEffectValue::Int(
                        transition
                            .sample_int_param(&param.name)
                            .unwrap_or(param.default_int) as i32,
                    ),
                    EffectParamKind::Bool => UiEffectValue::Bool(
                        transition
                            .sample_bool_param(&param.name)
                            .unwrap_or(param.default_bool),
                    ),
                    EffectParamKind::Choice => UiEffectValue::Choice(
                        transition
                            .sample_choice_param(&param.name)
                            .or_else(|| param.default_choice_id().map(str::to_owned))
                            .unwrap_or_default()
                            .into(),
                    ),
                    EffectParamKind::Color => UiEffectValue::Color(
                        transition
                            .sample_color_param(&param.name)
                            .unwrap_or_else(|| param.default_color_u8()),
                    ),
                    EffectParamKind::Vec2 | EffectParamKind::Size => UiEffectValue::Vec2(
                        transition
                            .sample_vec2_param(&param.name)
                            .unwrap_or(param.default_vec2),
                    ),
                    EffectParamKind::Point3 => UiEffectValue::Point3(
                        transition
                            .sample_vec3_param(&param.name)
                            .unwrap_or(param.default_vec3),
                    ),
                    EffectParamKind::Curve => UiEffectValue::Curve(
                        transition
                            .sample_curve_param(&param.name)
                            .unwrap_or_else(|| param.default_curve_points()),
                    ),
                    EffectParamKind::Gradient => UiEffectValue::Gradient(
                        transition
                            .sample_gradient_param(&param.name)
                            .unwrap_or_else(|| param.default_gradient_stops()),
                    ),
                    EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {
                        UiEffectValue::Text(
                            transition
                                .sample_text_param(&param.name)
                                .unwrap_or_else(|| param.default_text.clone())
                                .into(),
                        )
                    }
                    EffectParamKind::Layer => UiEffectValue::Layer(
                        transition
                            .sample_layer_param(&param.name)
                            .flatten()
                            .map(aria_models::ClipId::raw),
                    ),
                })
                .collect();
            let visible: Vec<_> = spec
                .params
                .iter()
                .map(|param| transition.is_param_visible(param, spec))
                .collect();
            (std::sync::Arc::from(values), std::sync::Arc::from(visible))
        })
        .unwrap_or_else(|| (std::sync::Arc::from([]), std::sync::Arc::from([])));
    UiTransition {
        id: transition.transition_id.clone().into(),
        label: spec
            .map(|spec| SharedString::from(spec.label.clone()))
            .or_else(|| {
                aria_models::AudioTransition::from_id(&transition.transition_id)
                    .map(|audio| SharedString::new_static(audio.label()))
            })
            .unwrap_or_else(|| "Transition".into()),
        duration_ticks: transition.duration,
        alignment: transition.alignment,
        values,
        visible,
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
