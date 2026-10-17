//! Property drivers in the model: attaching, persisting, and keeping a driver
//! on the property it was written for while the clip around it changes.
//!
//! Closes the class where a driver silently moves to another property: an
//! effect chain or mask stack edit that shifts indices, a chain replacement,
//! or a bake that folds the driven value into a file and leaves the driver to
//! apply twice.
//! Does not cover compiling or evaluating drivers; that is the renderer's.

use super::*;
use crate::clip::{DriverCode, MAX_DRIVER_SOURCE};
use crate::look::{Mask, MaskKind};

fn code(source: &str) -> DriverCode {
    DriverCode::new("aria.expressions", source)
}

fn effect(effect: u32) -> ClipParam {
    ClipParam::Effect { effect, param: 0 }
}

fn mask_feather(mask: u32) -> ClipParam {
    ClipParam::Mask {
        mask,
        param: crate::MaskParam::Feather,
    }
}

fn media_clip() -> (Project, ClipId) {
    let (mut project, media, track) = project_with_media(500);
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    (project, clip)
}

fn driven_params(project: &Project, clip: ClipId) -> Vec<(ClipParam, String)> {
    project
        .clip(clip)
        .unwrap()
        .drivers
        .iter()
        .map(|driver| (driver.param, driver.code.source.clone()))
        .collect()
}

#[test]
fn attaching_replacing_and_detaching_return_the_code_that_was_there() {
    let (mut project, clip) = media_clip();
    let first = project
        .set_param_driver(clip, ClipParam::Opacity, Some(code("value")))
        .unwrap();
    assert_eq!(first, None);
    let second = project
        .set_param_driver(clip, ClipParam::Opacity, Some(code("0.5")))
        .unwrap();
    assert_eq!(second, Some(code("value")));
    assert_eq!(
        project.clip(clip).unwrap().driver(ClipParam::Opacity),
        Some(&code("0.5"))
    );
    let removed = project
        .set_param_driver(clip, ClipParam::Opacity, None)
        .unwrap();
    assert_eq!(removed, Some(code("0.5")));
    assert!(project.clip(clip).unwrap().drivers.is_empty());
    assert_eq!(
        project
            .set_param_driver(clip, ClipParam::Opacity, None)
            .unwrap(),
        None
    );
}

#[test]
fn a_driver_is_refused_where_the_property_cannot_take_one() {
    let (mut project, clip) = media_clip();
    let refusals = [
        (ClipParam::Speed, code("1"), "speed curve"),
        (mask_feather(0), code("1"), "not a property of that clip"),
        (effect(0), code("1"), "not a property of that clip"),
        (
            ClipParam::Opacity,
            DriverCode::new("", "1"),
            "needs a language",
        ),
        (
            ClipParam::Opacity,
            DriverCode::new("aria expressions", "1"),
            "not an id",
        ),
        (
            ClipParam::Opacity,
            code(&"1".repeat(MAX_DRIVER_SOURCE + 1)),
            "the limit is",
        ),
    ];
    for (param, code, reason) in refusals {
        let err = project
            .set_param_driver(clip, param, Some(code))
            .unwrap_err()
            .to_string();
        assert!(err.contains(reason), "{param:?}: {err}");
    }
    let track = project.add_track(TrackKind::Text, "T");
    let text = project
        .add_generated(track, Generator::text("Hi"), tr(0, 24))
        .unwrap();
    let err = project
        .set_param_driver(text, ClipParam::Volume, Some(code("1")))
        .unwrap_err()
        .to_string();
    assert!(err.contains("media-backed"), "{err}");
    assert!(project.clip(clip).unwrap().drivers.is_empty());
}

#[test]
fn a_driver_on_an_enabled_block_is_accepted() {
    let (mut project, clip) = media_clip();
    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    let feather = mask_feather(0);
    project
        .set_param_driver(clip, feather, Some(code("wiggle(1, 2)")))
        .unwrap();
    assert_eq!(
        driven_params(&project, clip),
        vec![(feather, "wiggle(1, 2)".into())]
    );
}

#[test]
fn effect_chain_edits_keep_each_driver_on_its_own_effect() {
    let (mut project, clip) = media_clip();
    for _ in 0..3 {
        project.add_effect(clip, "gaussian_blur").unwrap();
    }
    for index in 0..3 {
        project
            .set_param_driver(clip, effect(index), Some(code(&format!("e{index}"))))
            .unwrap();
    }
    project
        .set_param_driver(clip, ClipParam::Rotation, Some(code("time")))
        .unwrap();

    project.move_effect(clip, 0, 2).unwrap();
    let mut moved = driven_params(&project, clip);
    moved.sort_by_key(|(_, source)| source.clone());
    assert_eq!(
        moved,
        vec![
            (effect(2), "e0".into()),
            (effect(0), "e1".into()),
            (effect(1), "e2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    project.move_effect(clip, 2, 0).unwrap();
    let mut back = driven_params(&project, clip);
    back.sort_by_key(|(_, source)| source.clone());
    assert_eq!(
        back,
        vec![
            (effect(0), "e0".into()),
            (effect(1), "e1".into()),
            (effect(2), "e2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    project.remove_effect(clip, 1).unwrap();
    let mut removed = driven_params(&project, clip);
    removed.sort_by_key(|(_, source)| source.clone());
    assert_eq!(
        removed,
        vec![
            (effect(0), "e0".into()),
            (effect(1), "e2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    let chain = project.clip(clip).unwrap().effects.clone();
    project.set_clip_effects(clip, chain).unwrap();
    assert_eq!(
        driven_params(&project, clip),
        vec![(ClipParam::Rotation, "time".into())]
    );
}

#[test]
fn mask_stack_edits_keep_each_driver_on_its_own_mask() {
    let (mut project, clip) = media_clip();
    for _ in 0..3 {
        project
            .add_clip_mask(clip, Mask::new(MaskKind::Circle))
            .unwrap();
    }
    for index in 0..3 {
        project
            .set_param_driver(clip, mask_feather(index), Some(code(&format!("m{index}"))))
            .unwrap();
    }
    project
        .set_param_driver(clip, ClipParam::Rotation, Some(code("time")))
        .unwrap();
    let sorted = |project: &Project| {
        let mut params = driven_params(project, clip);
        params.sort_by_key(|(_, source)| source.clone());
        params
    };

    project.move_clip_mask(clip, 0, 2).unwrap();
    assert_eq!(
        sorted(&project),
        vec![
            (mask_feather(2), "m0".into()),
            (mask_feather(0), "m1".into()),
            (mask_feather(1), "m2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    project.move_clip_mask(clip, 2, 0).unwrap();
    assert_eq!(
        sorted(&project),
        vec![
            (mask_feather(0), "m0".into()),
            (mask_feather(1), "m1".into()),
            (mask_feather(2), "m2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    // Replacing a mask keeps the drivers on its properties.
    project
        .set_clip_mask(clip, 1, Mask::new(MaskKind::Rectangle))
        .unwrap();
    assert_eq!(sorted(&project).len(), 4);

    project.remove_clip_mask(clip, 1).unwrap();
    assert_eq!(
        sorted(&project),
        vec![
            (mask_feather(0), "m0".into()),
            (mask_feather(1), "m2".into()),
            (ClipParam::Rotation, "time".into()),
        ]
    );

    project
        .remove_clip_attributes(clip, &[crate::ClipAttribute::Mask])
        .unwrap();
    assert!(project.clip(clip).unwrap().masks.is_empty());
    assert_eq!(sorted(&project), vec![(ClipParam::Rotation, "time".into())]);
}

/// Every `drivers` value anywhere in a saved document.
fn drivers_in(doc: &serde_json::Value) -> Vec<&serde_json::Value> {
    match doc {
        serde_json::Value::Object(map) => map
            .iter()
            .flat_map(|(key, value)| {
                if key == "drivers" {
                    vec![value]
                } else {
                    drivers_in(value)
                }
            })
            .collect(),
        serde_json::Value::Array(items) => items.iter().flat_map(drivers_in).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn drivers_round_trip_through_a_save_and_are_absent_when_there_are_none() {
    let (mut project, clip) = media_clip();
    let bare = project.save_to_string().unwrap();
    assert!(!bare.contains("\"drivers\""), "{bare}");

    project
        .set_param_driver(clip, ClipParam::Position, Some(code("wiggle(2, 30)")))
        .unwrap();
    let saved = project.save_to_string().unwrap();
    let doc: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        drivers_in(&doc),
        vec![&serde_json::json!([
            {"param": "position", "language": "aria.expressions", "source": "wiggle(2, 30)"}
        ])],
        "{saved}"
    );
    let loaded = Project::load_from_string(&saved).unwrap();
    assert_eq!(
        loaded.clip(clip).unwrap().drivers,
        project.clip(clip).unwrap().drivers
    );
}

#[test]
fn a_driver_dresses_a_clip_and_a_bake_folds_it_in() {
    let (mut project, clip) = media_clip();
    assert!(!project.clip(clip).unwrap().is_dressed());
    project
        .set_param_driver(clip, ClipParam::Opacity, Some(code("0.5")))
        .unwrap();
    let driven = project.clip(clip).unwrap();
    assert!(driven.is_dressed());
    let baked = driven.baked(MediaId::from_raw(77), tr(0, 48)).unwrap();
    assert!(baked.drivers.is_empty());
    assert_eq!(baked.bake.as_deref().unwrap().drivers, driven.drivers);
}

#[test]
fn every_property_samples_the_same_through_its_param_as_through_its_row() {
    let (mut project, clip) = media_clip();
    project.add_effect(clip, "gaussian_blur").unwrap();
    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(0),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(40),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    let clip = project.clip(clip).unwrap();
    let rows = crate::clip_params_for(clip);
    assert!(rows.len() > 10, "{}", rows.len());
    for row in &rows {
        for tick in [0, 10, 40, 47] {
            assert_eq!(
                row.param.sample_at(clip, tick as f64),
                row.sample(clip, tick),
                "{} at {tick}",
                row.id
            );
        }
    }
    let mut ticks = Vec::new();
    ClipParam::Opacity.visit_keyframe_ticks(clip, |tick| ticks.push(tick));
    assert_eq!(ticks, vec![0, 40]);
    assert_eq!(
        ClipParam::Opacity.sample_at(clip, 20.0),
        Some(ParamValue::Scalar(0.5))
    );
}

#[test]
fn a_driven_value_is_held_inside_the_property_bounds() {
    let (mut project, clip) = media_clip();
    project.add_effect(clip, "gaussian_blur").unwrap();
    let clip = project.clip(clip).unwrap();
    let held = [
        (
            ClipParam::Opacity,
            ParamValue::Scalar(1.5),
            Some(ParamValue::Scalar(1.0)),
        ),
        (
            ClipParam::Opacity,
            ParamValue::Scalar(-2.0),
            Some(ParamValue::Scalar(0.0)),
        ),
        (
            effect(0),
            ParamValue::Scalar(100.0),
            Some(ParamValue::Scalar(64.0)),
        ),
        (
            ClipParam::Position,
            ParamValue::Vec2([30.0, -4.0]),
            Some(ParamValue::Vec2([30.0, -4.0])),
        ),
        (ClipParam::Opacity, ParamValue::Scalar(f32::NAN), None),
        (
            ClipParam::Position,
            ParamValue::Vec2([0.0, f32::INFINITY]),
            None,
        ),
    ];
    for (param, value, expected) in held {
        assert_eq!(param.clamp_on(clip, value), expected, "{param:?} {value:?}");
    }
}

#[test]
fn a_clip_is_addressed_by_its_own_name_then_its_source() {
    let (mut project, clip) = media_clip();
    assert_eq!(
        project.clip_title(project.clip(clip).unwrap()),
        "sample.mp4"
    );
    project.set_clip_name(clip, Some("Logo".into())).unwrap();
    assert_eq!(project.clip_title(project.clip(clip).unwrap()), "Logo");
    let track = project.add_track(TrackKind::Text, "T");
    let text = project
        .add_generated(track, Generator::text("Title card"), tr(0, 24))
        .unwrap();
    assert_eq!(
        project.clip_title(project.clip(text).unwrap()),
        "Title card"
    );
}
