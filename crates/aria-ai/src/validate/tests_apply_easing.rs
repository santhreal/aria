//! `apply_easing` cases.

use super::*;
use crate::wire;

#[test]
fn typed_effect_params_accept_rgba_and_position_on_wire() {
    let (mut project, _, _, _, clip, _) = fixture();
    project
        .add_effect(ClipId::from_raw(clip), "duotone")
        .unwrap();
    project
        .add_effect(ClipId::from_raw(clip), "color_overlay")
        .unwrap();

    let kf = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Effect {
                index: 0,
                param: "shadow_color".into(),
            },
            at: 1.0,
            value: None,
            position: None,
            rgba: Some([20, 16, 60, 255]),
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        kf,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Effect {
                effect: 0,
                param: 0,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Color([20, 16, 60, 255]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Effect {
                index: 0,
                param: "shadow_color".into(),
            },
            at: 1.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(msg.contains("color"), "{msg}");
    assert!(msg.contains("rgba"), "{msg}");

    let offset = lower(
        &project,
        WireCommand::SetEffectParam(wire::SetEffectParam {
            clip,
            index: 1,
            param: "offset".into(),
            value: None,
            position: Some([0.25, -0.5]),
            rgba: None,
        }),
    );
    assert_eq!(
        offset,
        EditCommand::SetParamConstant {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Effect {
                effect: 1,
                param: 1,
            },
            value: ParamValue::Vec2([0.25, -0.5]),
        }
    );

    let color_const = lower(
        &project,
        WireCommand::SetEffectParam(wire::SetEffectParam {
            clip,
            index: 0,
            param: "highlight_color".into(),
            value: None,
            position: None,
            rgba: Some([255, 220, 160, 255]),
        }),
    );
    assert_eq!(
        color_const,
        EditCommand::SetParamConstant {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Effect {
                effect: 0,
                param: 1,
            },
            value: ParamValue::Color([255, 220, 160, 255]),
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetEffectParam(wire::SetEffectParam {
            clip,
            index: 0,
            param: "shadow_color".into(),
            value: Some(0.5),
            position: None,
            rgba: None,
        }),
    );
    assert!(msg.contains("color"), "{msg}");
    assert!(msg.contains("rgba"), "{msg}");
}

#[test]
fn apply_easing_preset_lowers_to_engine_command() {
    let (project, _, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::ApplyEasingPreset(wire::ApplyEasingPreset {
            clip,
            param: wire::WireClipParam::Opacity,
            from_tick: 0.0,
            preset: wire::WireEasingPreset::BounceOut,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::ApplyEasingPreset {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Opacity,
            at: RationalTime::new(0, R24),
            preset: aria_models::PiecewiseEasingPreset::BounceOut,
        }
    );
}

#[test]
fn apply_easing_preset_rejects_crop_and_colors() {
    let (project, _, _, _, clip, _) = fixture();
    let crop_msg = reject(
        &project,
        WireCommand::ApplyEasingPreset(wire::ApplyEasingPreset {
            clip,
            param: wire::WireClipParam::Crop,
            from_tick: 0.0,
            preset: wire::WireEasingPreset::BackOut,
        }),
    );
    assert!(
        crop_msg.contains("scalar/vec2") || crop_msg.contains("not support"),
        "{crop_msg}"
    );
    let color_msg = reject(
        &project,
        WireCommand::ApplyEasingPreset(wire::ApplyEasingPreset {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::ShadowColor,
            },
            from_tick: 0.0,
            preset: wire::WireEasingPreset::ElasticOut,
        }),
    );
    assert!(color_msg.contains("color"), "{color_msg}");
}
