//! `position_tangents` cases.

use super::*;
use crate::wire;

#[test]
fn extended_wire_clip_params_lower_to_model_params() {
    let (mut project, _, _, _, clip, title) = fixture();
    project
        .add_effect(ClipId::from_raw(clip), "gaussian_blur")
        .unwrap();
    let sticker_track = project.add_track(TrackKind::Sticker, "Shapes");
    let shape = project
        .add_generated(
            sticker_track,
            Generator::shape(aria_models::Shape::Rectangle, [0, 0, 0, 255]),
            TimeRange::at_rate(0, 72, R24),
        )
        .unwrap()
        .raw();

    let cases = [
        (
            clip,
            wire::WireClipParam::AnchorPoint,
            Some([0.25, -0.25]),
            None,
            None,
            ClipParam::AnchorPoint,
            ParamValue::Vec2([0.25, -0.25]),
        ),
        (
            clip,
            wire::WireClipParam::Speed,
            None,
            Some(1.5),
            None,
            ClipParam::Speed,
            ParamValue::Scalar(1.5),
        ),
        (
            clip,
            wire::WireClipParam::Effect {
                index: 0,
                param: "radius".into(),
            },
            None,
            Some(8.0),
            None,
            ClipParam::Effect {
                effect: 0,
                param: 0,
            },
            ParamValue::Scalar(8.0),
        ),
        (
            shape,
            wire::WireClipParam::Shape {
                param: wire::WireShapeParam::Fill,
            },
            None,
            None,
            Some([12, 34, 56, 255]),
            ClipParam::Shape {
                param: aria_models::ShapeParam::Fill,
            },
            ParamValue::Color([12, 34, 56, 255]),
        ),
        (
            title,
            wire::WireClipParam::Text {
                param: wire::WireTextParam::Size,
            },
            None,
            Some(72.0),
            None,
            ClipParam::Text {
                param: aria_models::TextParam::Size,
            },
            ParamValue::Scalar(72.0),
        ),
        (
            title,
            wire::WireClipParam::Text {
                param: wire::WireTextParam::BackgroundRadius,
            },
            None,
            Some(0.5),
            None,
            ClipParam::Text {
                param: aria_models::TextParam::BackgroundRadius,
            },
            ParamValue::Scalar(0.5),
        ),
        (
            title,
            wire::WireClipParam::Text {
                param: wire::WireTextParam::BackgroundColor,
            },
            None,
            None,
            Some([10, 20, 30, 200]),
            ClipParam::Text {
                param: aria_models::TextParam::BackgroundColor,
            },
            ParamValue::Color([10, 20, 30, 200]),
        ),
        (
            clip,
            wire::WireClipParam::Look {
                param: wire::WireLookParam::AdjustContrast,
            },
            None,
            Some(0.5),
            None,
            ClipParam::Look {
                param: aria_models::LookParam::AdjustContrast,
            },
            ParamValue::Scalar(0.5),
        ),
    ];

    for (target, param, position, value, rgba, expected_param, expected_value) in cases {
        let edit = lower(
            &project,
            WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
                clip: target,
                param,
                at: 2.0,
                value,
                position,
                rgba,
                rect: None,
                easing: Some(wire::WireEasing::EaseOut),

                tangent_out: None,
                tangent_in: None,
            }),
        );
        assert_eq!(
            edit,
            EditCommand::SetParamKeyframe {
                clip: ClipId::from_raw(target),
                param: expected_param,
                at: RationalTime::new(48, R24),
                value: expected_value,
                easing: Easing::EaseOut,
                tangents: None,
            }
        );
    }
}

#[test]
fn named_and_bezier_easings_lower_on_set_param_keyframe() {
    let (project, _, _, _, clip, _) = fixture();

    let cases = [
        (
            wire::WireEasing::Snappy,
            Easing::from_preset_id("snappy").unwrap(),
        ),
        (
            wire::WireEasing::Overshoot,
            Easing::from_preset_id("overshoot").unwrap(),
        ),
        (
            wire::WireEasing::Anticipate,
            Easing::from_preset_id("anticipate").unwrap(),
        ),
        (wire::WireEasing::Hold, Easing::Hold),
        (
            wire::WireEasing::Bezier {
                points: [0.42, 0.0, 0.58, 1.0],
            },
            Easing::Bezier {
                points: [0.42, 0.0, 0.58, 1.0],
            },
        ),
    ];

    for (wire_easing, expected) in cases {
        let edit = lower(
            &project,
            WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
                clip,
                param: wire::WireClipParam::Opacity,
                at: 1.0,
                value: Some(0.5),
                position: None,
                rgba: None,
                rect: None,
                easing: Some(wire_easing),

                tangent_out: None,
                tangent_in: None,
            }),
        );
        assert_eq!(
            edit,
            EditCommand::SetParamKeyframe {
                clip: ClipId::from_raw(clip),
                param: ClipParam::Opacity,
                at: RationalTime::new(24, R24),
                value: ParamValue::Scalar(0.5),
                easing: expected,
                tangents: None,
            }
        );
    }

    let err = validate(
        &WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Opacity,
            at: 1.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: Some(wire::WireEasing::Bezier {
                points: [1.5, 0.0, 0.5, 1.0],
            }),

            tangent_out: None,
            tangent_in: None,
        }),
        &project,
    )
    .expect_err("x control point outside 0..=1 must reject");
    assert!(
        err.to_string().contains("invalid easing"),
        "unexpected rejection: {err}"
    );
}

#[test]
fn hold_easing_lowers_from_wire_json() {
    let (project, _, _, _, clip, _) = fixture();
    let cmd: WireCommand = serde_json::from_value(serde_json::json!({
        "command": "set_param_keyframe",
        "clip": clip,
        "param": "opacity",
        "at": 1.0,
        "value": 0.5,
        "easing": "hold",
    }))
    .expect("\"hold\" is a valid wire easing tag");
    assert_eq!(
        lower(&project, cmd),
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Opacity,
            at: RationalTime::new(24, R24),
            value: ParamValue::Scalar(0.5),
            easing: Easing::Hold,
            tangents: None,
        }
    );
}

#[test]
fn position_tangents_lower_on_set_param_keyframe() {
    let (project, _, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Position,
            at: 1.0,
            value: None,
            position: Some([0.25, -0.5]),
            rgba: None,
            rect: None,
            easing: Some(wire::WireEasing::Linear),
            tangent_out: Some([0.0, 0.5]),
            tangent_in: Some([-0.25, 0.0]),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Position,
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([0.25, -0.5]),
            easing: Easing::Linear,
            tangents: Some(aria_models::SpatialTangents {
                out_t: [0.0, 0.5],
                in_t: [-0.25, 0.0],
            }),
        }
    );
}

#[test]
fn position_tangents_on_opacity_are_rejected() {
    let (project, _, _, _, clip, _) = fixture();
    let err = validate(
        &WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Opacity,
            at: 1.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: None,
            tangent_out: Some([0.1, 0.0]),
            tangent_in: None,
        }),
        &project,
    )
    .expect_err("tangents on opacity must reject");
    assert!(
        err.to_string()
            .contains("spatial tangents are only supported on position"),
        "unexpected rejection: {err}"
    );
}

#[test]
fn position_tangents_roundtrip_through_wire_json() {
    let (project, _, _, _, clip, _) = fixture();
    let cmd: WireCommand = serde_json::from_value(serde_json::json!({
        "command": "set_param_keyframe",
        "clip": clip,
        "param": "position",
        "at": 1.0,
        "position": [0.25, -0.5],
        "tangent_out": [0.0, 0.5],
        "tangent_in": [-0.25, 0.0],
    }))
    .expect("tangent fields deserialize");
    let WireCommand::SetParamKeyframe(args) = &cmd else {
        panic!("expected SetParamKeyframe");
    };
    assert_eq!(args.tangent_out, Some([0.0, 0.5]));
    assert_eq!(args.tangent_in, Some([-0.25, 0.0]));
    assert_eq!(
        lower(&project, cmd),
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Position,
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([0.25, -0.5]),
            easing: Easing::Linear,
            tangents: Some(aria_models::SpatialTangents {
                out_t: [0.0, 0.5],
                in_t: [-0.25, 0.0],
            }),
        }
    );
}

#[test]
fn extended_wire_params_work_for_constant_and_removal() {
    let (project, _, _, _, clip, _) = fixture();

    assert_eq!(
        lower(
            &project,
            WireCommand::SetParamConstant(wire::SetParamConstant {
                clip,
                param: wire::WireClipParam::AnchorPoint,
                value: None,
                position: Some([0.25, 0.5]),
                rgba: None,
                rect: None,
            }),
        ),
        EditCommand::SetParamConstant {
            clip: ClipId::from_raw(clip),
            param: ClipParam::AnchorPoint,
            value: ParamValue::Vec2([0.25, 0.5]),
        }
    );
    assert_eq!(
        lower(
            &project,
            WireCommand::RemoveParamKeyframe(wire::RemoveParamKeyframe {
                clip,
                param: wire::WireClipParam::Speed,
                at: 1.0,
            }),
        ),
        EditCommand::RemoveParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Speed,
            at: RationalTime::new(24, R24),
        }
    );
}

#[test]
fn add_transition_rejects_canvas_pass_lanes() {
    // Effect/filter/adjustment segments resolve to canvas-wide passes the
    // renderer can't nest inside a transition; the agent gets a rejection
    // instead of a downstream engine error.
    let (mut project, ..) = fixture();
    let lane = project.add_track(TrackKind::Adjustment, "FX");
    let left = project
        .add_generated(lane, Generator::Adjustment, TimeRange::at_rate(0, 24, R24))
        .unwrap();
    project
        .add_generated(lane, Generator::Adjustment, TimeRange::at_rate(24, 24, R24))
        .unwrap();

    let msg = reject(
        &project,
        WireCommand::AddTransition(wire::AddTransition {
            clip: left.raw(),
            transition: "crossfade".into(),
        }),
    );
    assert!(
        msg.contains("Adjustment lane"),
        "message should name the lane kind: {msg}"
    );
}
