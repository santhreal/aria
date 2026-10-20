//! The inspector view of a clip's effects and of a transition, and the track
//! tint, for [`super`].

use super::*;
use aria_models::{EffectParamKind, EffectParamSpec, StoredParams};
use gpui::SharedString;

use crate::types::UiEffectValue;

/// The inspector value and visibility of every slot in `params`, read from
/// an effect instance or a transition at `tick`.
fn slots_to_ui(
    stored: &impl StoredParams,
    params: &[EffectParamSpec],
    tick: f64,
) -> (Vec<UiEffectValue>, Vec<bool>) {
    let values = params
        .iter()
        .map(|p| match p.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => {
                UiEffectValue::Scalar(stored.scalar(p, tick).unwrap_or(p.default))
            }
            EffectParamKind::Int => {
                UiEffectValue::Int(stored.int(p, tick).unwrap_or(p.default_int) as i32)
            }
            EffectParamKind::Bool => {
                UiEffectValue::Bool(stored.boolean(p).unwrap_or(p.default_bool))
            }
            EffectParamKind::Choice => {
                UiEffectValue::Choice(stored.choice(p).unwrap_or_default().to_owned().into())
            }
            EffectParamKind::Color => UiEffectValue::Color(
                stored
                    .color(p, tick)
                    .unwrap_or_else(|| p.default_color_u8()),
            ),
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                UiEffectValue::Vec2(stored.vec2(p, tick).unwrap_or(p.default_vec2))
            }
            EffectParamKind::Point3 => {
                UiEffectValue::Point3(stored.vec3(p, tick).unwrap_or(p.default_vec3))
            }
            EffectParamKind::Curve => {
                UiEffectValue::Curve(stored.curve(p).unwrap_or_else(|| p.default_curve_points()))
            }
            EffectParamKind::Gradient => UiEffectValue::Gradient(
                stored
                    .gradient(p)
                    .unwrap_or_else(|| p.default_gradient_stops()),
            ),
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {
                UiEffectValue::Text(
                    stored
                        .text(p)
                        .unwrap_or_else(|| p.default_text.clone())
                        .into(),
                )
            }
            EffectParamKind::Layer => {
                UiEffectValue::Layer(stored.layer(p).flatten().map(aria_models::ClipId::raw))
            }
        })
        .collect();
    let visible = params
        .iter()
        .map(|p| stored.visible(p, params, tick))
        .collect();
    (values, visible)
}

pub(super) fn effect_to_ui(effect: &EffectInstance) -> UiEffectInstance {
    let spec = effect.spec().ok();
    let (values, visible) = spec
        .as_ref()
        .map(|spec| slots_to_ui(effect, &spec.params, 0.0))
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
            let (values, visible) = slots_to_ui(transition, &spec.params, 0.0);
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
