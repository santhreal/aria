//! The words a property reaches a reader in.
//!
//! A refusal used to state a property in its Rust debug form: "no Look {
//! param: AdjustBrightness } keyframe at tick 12", "Effect { effect: 0, param:
//! 2 } is not a property of that clip". Closes that class over every property:
//! the space is read off `ClipParam::CENSUS`, each axis's members, and masks
//! and effect slots at two indexes, so a new property is held to the copy
//! without an entry here. Each property has an id of its own, a clip states
//! each offered row as that row, and every refusal the parameter routes return
//! states no debug form. Does not cover a message outside those routes that
//! formats a `ClipParam` itself.

use std::collections::HashMap;

use super::*;
use crate::clip::{LookParam, MaskParam, ParamValue, ShapeParam};
use crate::param::PiecewiseEasingPreset;

/// Every property a command can address, one entry each.
fn space() -> Vec<ClipParam> {
    let mut all: Vec<ClipParam> = ClipParam::CENSUS
        .iter()
        .copied()
        .filter(|param| !matches!(param.class(), ClipParamClass::Delegated(_)))
        .collect();
    for axis in ParamAxis::ALL {
        all.extend(axis.params());
    }
    for index in [0, 1] {
        all.extend(
            MaskParam::ALL
                .iter()
                .map(|&param| ClipParam::Mask { mask: index, param }),
        );
        all.push(ClipParam::EffectOpacity { effect: index });
        all.extend((0..3).map(|slot| ClipParam::Effect {
            effect: index,
            param: slot,
        }));
    }
    all.sort();
    all.dedup();
    all
}

/// Every dressed clip shape, and a media clip with an effect so effect rows
/// exist.
fn clips() -> Vec<Clip> {
    let (mut media, id) = dressed_media_project();
    media.add_effect(id, "gaussian_blur").expect("effect");
    let mut clips = all_dressed();
    clips.push(media.require_clip(id).expect("clip").clone());
    clips
}

/// `message` states no debug form of `param`: no brace, and none of the
/// CamelCase names the debug form spells.
fn assert_plain(param: ClipParam, message: &str) {
    let debug = format!("{param:?}");
    assert!(!message.contains('{'), "{debug}: {message}");
    for word in debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.chars().filter(char::is_ascii_uppercase).count() > 1)
    {
        assert!(!message.contains(word), "{debug}: {message}");
    }
}

#[test]
fn every_property_reads_as_its_own_id() {
    let mut seen = HashMap::new();
    for param in space() {
        let (id, label) = (param.id(), param.label());
        assert!(
            !id.is_empty() && !id.contains(['{', ' ']) && id.to_lowercase() == id,
            "{param:?} reads as {id:?}"
        );
        assert!(
            !label.is_empty() && !label.contains(['{', '_']),
            "{param:?} is labeled {label:?}"
        );
        if let Some(row) = param_spec(param) {
            assert_eq!((&id, &label), (&row.id, &row.label), "{param:?}");
        }
        if let Some(other) = seen.insert(id.clone(), param) {
            panic!("{param:?} and {other:?} both read as {id}");
        }
    }
    for row in clip_param_catalog().iter() {
        assert_eq!(seen.get(&row.id), Some(&row.param), "row {}", row.id);
    }
}

#[test]
fn a_property_off_a_clip_reads_by_position() {
    let cases = [
        (ClipParam::Speed, "speed", "Speed"),
        (
            ClipParam::Effect {
                effect: 2,
                param: 1,
            },
            "effect-2-1",
            "Effect 3 Parameter 2",
        ),
        (
            ClipParam::EffectOpacity { effect: 0 },
            "effect-0-opacity",
            "Effect 1 Opacity",
        ),
        (
            ClipParam::Mask {
                mask: 3,
                param: MaskParam::Feather,
            },
            "mask-3-feather",
            "Mask 4 Feather",
        ),
        (
            ClipParam::Look {
                param: LookParam::AdjustBrightness,
            },
            "adjust-brightness",
            "Brightness",
        ),
    ];
    let bare = dressed_media_clip();
    for (param, id, label) in cases {
        assert_eq!((param.id(), param.label()), (id.into(), label.into()));
        assert_eq!(
            (param.id_on(&bare), param.label_on(&bare)),
            (id.into(), label.into()),
            "{param:?} on a clip without it"
        );
    }
}

#[test]
fn a_clip_states_each_offered_property_as_its_row() {
    let mut effects = 0;
    for clip in clips() {
        for row in clip_params_for(&clip) {
            assert_eq!(row.param.id_on(&clip), row.id, "{:?}", row.param);
            assert_eq!(row.param.label_on(&clip), row.label, "{:?}", row.param);
            effects += usize::from(matches!(row.param, ClipParam::Effect { .. }));
        }
    }
    assert!(effects > 0, "the census reached no effect row");
}

/// A keyframe lookup that finds none states the row's id, on every row every
/// dressed clip offers.
#[test]
fn a_missing_keyframe_states_the_row_id() {
    let mut reached = 0;
    for clip in clips() {
        for row in clip_params_for(&clip) {
            let refusal = row
                .param
                .set_keyframe_roving(&mut clip.clone(), 987_654, true)
                .expect_err("no keyframe sits there");
            assert_eq!(
                refusal.to_string(),
                format!(
                    "invalid parameter: {} has no keyframe at tick 987654",
                    row.id
                )
            );
            if !matches!(row.param, ClipParam::Effect { .. }) {
                let refusal = row
                    .param
                    .remove_keyframe(&mut clip.clone(), 987_654)
                    .expect_err("no keyframe sits there");
                assert_eq!(
                    refusal.to_string(),
                    format!(
                        "invalid parameter: {} has no keyframe at tick 987654",
                        row.id
                    )
                );
            }
            reached += 1;
        }
    }
    assert!(reached > 50, "the census reached {reached} rows");
}

/// Every write, erase and ease a property rejects, on every clip shape,
/// states no debug form.
#[test]
fn no_refusal_states_a_property_in_its_debug_form() {
    let values = [
        ParamValue::Scalar(1e9),
        ParamValue::Scalar(-1e9),
        ParamValue::Scalar(f32::NAN),
        ParamValue::Vec2([f32::NAN, 1e9]),
        ParamValue::Vec3([0.0; 3]),
        ParamValue::Color([1, 2, 3, 4]),
        ParamValue::Rect([0.0, 0.0, -1.0, -1.0]),
    ];
    let mut refusals = 0;
    for clip in clips() {
        for param in space() {
            let mut outcomes = vec![
                param.remove_keyframe(&mut clip.clone(), 987_654),
                param
                    .set_keyframe_roving(&mut clip.clone(), 987_654, false)
                    .map(|_| ()),
                param.apply_easing_preset(
                    &mut clip.clone(),
                    987_654,
                    PiecewiseEasingPreset::BounceOut,
                ),
            ];
            for value in values {
                outcomes.push(param.set_constant(&mut clip.clone(), value));
                outcomes.push(param.set_keyframe(&mut clip.clone(), 0, value, Easing::Linear));
            }
            for refusal in outcomes.into_iter().filter_map(Result::err) {
                assert_plain(param, &refusal.to_string());
                refusals += 1;
            }
        }
    }
    assert!(refusals > 1000, "the census met {refusals} refusals");
}

#[test]
fn a_refusal_states_the_bound_and_the_kind_it_holds_to() {
    let mut clip = fully_dressed_clip();
    let brightness = ClipParam::Look {
        param: LookParam::AdjustBrightness,
    };
    let cases = [
        (
            brightness.set_constant(&mut clip, ParamValue::Scalar(5.0)),
            "adjust-brightness 5 is outside -1 to 1",
        ),
        (
            brightness.set_constant(&mut clip, ParamValue::Color([0; 4])),
            "adjust-brightness takes a scalar value, got a color value",
        ),
        (
            ClipParam::Shape {
                param: ShapeParam::Fill,
            }
            .apply_easing_preset(&mut clip, 0, PiecewiseEasingPreset::BounceOut),
            "easing presets are not supported on shape-fill: a color does not extrapolate",
        ),
        (
            ClipParam::Speed.remove_keyframe(&mut clip, 0),
            "speed is not written through a param command: a speed ramp is keyframed \
             through set_clip_speed_curve, which re-derives the clip's timeline duration",
        ),
    ];
    for (outcome, message) in cases {
        assert_eq!(
            outcome.expect_err(message).to_string(),
            format!("invalid parameter: {message}")
        );
    }
}

#[test]
fn a_driver_or_a_bar_on_a_missing_property_states_its_id() {
    let (mut project, id) = dressed_media_project();
    let fill = ClipParam::Shape {
        param: ShapeParam::Fill,
    };
    let refusal = project
        .set_param_driver(
            id,
            fill,
            Some(crate::DriverCode::new("aria.expressions", "1")),
        )
        .expect_err("a media clip has no shape fill");
    assert_eq!(
        refusal.to_string(),
        "invalid parameter: shape-fill is not a property of that clip, so it cannot take a driver"
    );
    let refusal = project
        .set_clip_line_param(
            id,
            Some(ClipParam::Mask {
                mask: 3,
                param: MaskParam::Feather,
            }),
        )
        .expect_err("the clip has no fourth mask");
    assert_eq!(
        refusal.to_string(),
        "invalid parameter: the clip has no property mask-3-feather to draw on its bar; \
         choose one from its fx badge menu"
    );
}
