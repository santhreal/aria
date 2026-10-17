//! `mask_stack` cases: the four mask-stack tools and the `mask` keyframe param.
//!
//! WHY: a clip's masks are a stack addressed by position, so every call that
//! names a position has to be checked against the stack the clip holds, and
//! every field the wire offers has to reach the model. These cases pin both:
//! each wire field lowers to its model field, each range is refused with the
//! field's name, and each index past the stack is refused with the stack's
//! length.
//!
//! What this does not catch: whether the engine applies the lowered command.
//! `aria-engine`'s `inverse_edits_mask_stack` drives the same commands.

use super::*;
use crate::wire;

/// The fixture's video clip with a circle mask over a rectangle mask.
fn masked() -> (Project, u64) {
    let (mut project, _, _, _, clip, _) = fixture();
    for kind in [MaskKind::Circle, MaskKind::Rectangle] {
        project
            .add_clip_mask(ClipId::from_raw(clip), Mask::new(kind))
            .expect("a video clip takes masks");
    }
    (project, clip)
}

fn keyframe(clip: u64, param: wire::WireClipParam) -> wire::SetParamKeyframe {
    wire::SetParamKeyframe {
        clip,
        param,
        at: 1.0,
        value: None,
        position: None,
        rgba: None,
        rect: None,
        easing: None,
        tangent_out: None,
        tangent_in: None,
    }
}

#[test]
fn every_mask_field_lowers_to_its_model_field() {
    let (project, _, _, _, clip, title) = fixture();

    let edit = lower(
        &project,
        WireCommand::AddClipMask(wire::AddClipMask {
            clip,
            mask: wire::WireMask {
                feather: Some(0.25),
                invert: Some(true),
                center: Some([0.1, -0.2]),
                size: Some([0.8, 0.6]),
                rotation: Some(15.0),
                roundness: Some(0.3),
                mode: Some(wire::WireMaskMode::Intersect),
                opacity: Some(0.75),
                expansion: Some(-12.0),
                ..wire_mask("rectangle")
            },
        }),
    );
    let mut expected = Mask::new(MaskKind::Rectangle);
    expected.feather = Param::Constant(0.25);
    expected.invert = true;
    expected.center = Param::Constant([0.1, -0.2]);
    expected.size = Param::Constant([0.8, 0.6]);
    expected.rotation = Param::Constant(15.0);
    expected.roundness = Param::Constant(0.3);
    expected.mode = MaskMode::Intersect;
    expected.opacity = Param::Constant(0.75);
    expected.expansion = Param::Constant(-12.0);
    assert_eq!(
        edit,
        EditCommand::AddClipMask {
            clip: ClipId::from_raw(clip),
            mask: expected,
        }
    );

    // Omitted fields are the model's defaults, and a generated clip takes a
    // mask as footage does.
    let edit = lower(
        &project,
        WireCommand::AddClipMask(wire::AddClipMask {
            clip: title,
            mask: wire_mask("circle"),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::AddClipMask {
            clip: ClipId::from_raw(title),
            mask: Mask::new(MaskKind::Circle),
        }
    );
}

/// Every wire mode lowers to the model mode of the same name.
#[test]
fn every_mask_mode_lowers_to_the_mode_of_its_name() {
    for mode in MaskMode::ALL {
        let wire: wire::WireMaskMode = serde_json::from_value(serde_json::json!(mode.id()))
            .unwrap_or_else(|_| {
                panic!("MaskMode::{mode:?} has no WireMaskMode member");
            });
        assert_eq!(lower_mask_mode(wire), mode);
    }
}

#[test]
fn mask_values_outside_their_range_are_refused_by_name() {
    let (project, clip) = masked();
    let cases = [
        (
            wire::WireMask {
                opacity: Some(1.5),
                ..wire_mask("circle")
            },
            "mask opacity must be between 0 and 1",
        ),
        (
            wire::WireMask {
                feather: Some(-0.1),
                ..wire_mask("circle")
            },
            "mask feather must be between 0 and 1",
        ),
        (
            wire::WireMask {
                expansion: Some(5000.0),
                ..wire_mask("circle")
            },
            "mask expansion",
        ),
        (
            wire::WireMask {
                size: Some([0.0, 1.0]),
                ..wire_mask("circle")
            },
            "mask size",
        ),
    ];
    for (mask, refusal) in cases {
        let add = reject(
            &project,
            WireCommand::AddClipMask(wire::AddClipMask {
                clip,
                mask: mask.clone(),
            }),
        );
        assert!(add.contains(refusal), "add: {add}");
        let set = reject(
            &project,
            WireCommand::SetClipMask(wire::SetClipMask {
                clip,
                index: 1,
                mask,
            }),
        );
        assert!(set.contains(refusal), "set: {set}");
    }
}

#[test]
fn a_position_past_the_stack_is_refused_with_the_stack_length() {
    let (project, clip) = masked();
    let past = "has no mask at index 2 (stack length 2)";
    let calls = [
        WireCommand::SetClipMask(wire::SetClipMask {
            clip,
            index: 2,
            mask: wire_mask("star"),
        }),
        WireCommand::RemoveClipMask(wire::RemoveClipMask { clip, index: 2 }),
        WireCommand::MoveClipMask(wire::MoveClipMask {
            clip,
            from_index: 2,
            to_index: 0,
        }),
        WireCommand::MoveClipMask(wire::MoveClipMask {
            clip,
            from_index: 0,
            to_index: 2,
        }),
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            value: Some(0.5),
            ..keyframe(
                clip,
                wire::WireClipParam::Mask {
                    index: 2,
                    param: wire::WireMaskParam::Opacity,
                },
            )
        }),
    ];
    for call in calls {
        let msg = reject(&project, call);
        assert!(msg.contains(past), "{msg}");
    }

    // The last position in the stack is in range.
    assert_eq!(
        lower(
            &project,
            WireCommand::RemoveClipMask(wire::RemoveClipMask { clip, index: 1 }),
        ),
        EditCommand::RemoveClipMask {
            clip: ClipId::from_raw(clip),
            index: 1,
        }
    );
    assert_eq!(
        lower(
            &project,
            WireCommand::MoveClipMask(wire::MoveClipMask {
                clip,
                from_index: 1,
                to_index: 0,
            }),
        ),
        EditCommand::MoveClipMask {
            clip: ClipId::from_raw(clip),
            from_index: 1,
            to_index: 0,
        }
    );
    let msg = reject(
        &project,
        WireCommand::MoveClipMask(wire::MoveClipMask {
            clip,
            from_index: 1,
            to_index: 1,
        }),
    );
    assert!(msg.contains("would make no change"), "{msg}");
}

#[test]
fn a_full_stack_refuses_another_mask() {
    let (mut project, _, _, _, clip, _) = fixture();
    let id = ClipId::from_raw(clip);
    for _ in 1..aria_models::MAX_CLIP_MASKS {
        project
            .add_clip_mask(id, Mask::new(MaskKind::Circle))
            .unwrap();
    }
    // One place left: the call that fills it is accepted.
    lower(
        &project,
        WireCommand::AddClipMask(wire::AddClipMask {
            clip,
            mask: wire_mask("circle"),
        }),
    );
    project
        .add_clip_mask(id, Mask::new(MaskKind::Circle))
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::AddClipMask(wire::AddClipMask {
            clip,
            mask: wire_mask("circle"),
        }),
    );
    assert!(msg.contains("already holds 32 masks"), "{msg}");
}

#[test]
fn mask_center_keyframe_takes_a_position() {
    let (project, clip) = masked();
    let center = wire::WireClipParam::Mask {
        index: 1,
        param: wire::WireMaskParam::Center,
    };

    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            value: Some(0.5),
            ..keyframe(clip, center.clone())
        }),
    );
    assert!(msg.contains("position"), "{msg}");

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            position: Some([0.25, -0.1]),
            ..keyframe(clip, center.clone())
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Mask {
                mask: 1,
                param: aria_models::MaskParam::Center,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([0.25, -0.1]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    // Tagged nesting on the wire: {"mask":{"index":1,"param":"center"}}
    let from_json = WireCommand::from_tool_call(
        "set_param_keyframe",
        serde_json::json!({
            "clip": clip,
            "param": { "mask": { "index": 1, "param": "center" } },
            "at": 1.0,
            "position": [0.25, -0.1],
        }),
    )
    .unwrap();
    let WireCommand::SetParamKeyframe(args) = from_json else {
        panic!("expected SetParamKeyframe");
    };
    assert_eq!(args.param, center);
}

#[test]
fn scalar_mask_keyframes_take_a_value() {
    let (project, clip) = masked();
    for (param, model) in [
        (
            wire::WireMaskParam::Feather,
            aria_models::MaskParam::Feather,
        ),
        (
            wire::WireMaskParam::Rotation,
            aria_models::MaskParam::Rotation,
        ),
        (
            wire::WireMaskParam::Roundness,
            aria_models::MaskParam::Roundness,
        ),
        (
            wire::WireMaskParam::Opacity,
            aria_models::MaskParam::Opacity,
        ),
        (
            wire::WireMaskParam::Expansion,
            aria_models::MaskParam::Expansion,
        ),
    ] {
        let selector = wire::WireClipParam::Mask { index: 0, param };
        let msg = reject(
            &project,
            WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
                position: Some([0.5, 0.5]),
                ..keyframe(clip, selector.clone())
            }),
        );
        assert!(msg.contains("'value'"), "{param:?}: {msg}");
        let edit = lower(
            &project,
            WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
                value: Some(0.5),
                ..keyframe(clip, selector)
            }),
        );
        let EditCommand::SetParamKeyframe {
            param: lowered,
            value,
            ..
        } = edit
        else {
            panic!("{param:?} lowered to {edit:?}");
        };
        assert_eq!(
            lowered,
            ClipParam::Mask {
                mask: 0,
                param: model
            }
        );
        assert_eq!(value, ParamValue::Scalar(0.5));
    }
}
