//! Schema v3 replaced a clip's single `mask` with the `masks` stack and moved
//! drivers on mask properties from the look axis to the mask axis.
//!
//! These tests load documents written in the v2 shape, the shape a v2 build
//! saved, and check that they reach the project a v3 build holds: the one mask
//! as mask 0, its drivers on mask 0, and everything else untouched. They do not
//! cover v1 documents, which reach this step through the v1 step unchanged.

use serde_json::{Value, json};

use super::*;
use crate::clip::Generator;
use crate::look::{Mask, MaskKind};
use crate::param::Param;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;
use crate::{ClipId, ClipParam, DriverCode, LookParam, MaskParam};

fn solid() -> Generator {
    Generator::SolidColor {
        rgba: [10, 20, 30, 255],
    }
}

/// A project with two bare solid clips, saved as a v3 document.
fn two_clip_document() -> (Value, ClipId, ClipId) {
    let mut project = Project::new("masks", Rational::FPS_24);
    let kind = TrackKind::for_generator(&solid()).expect("a lane for a solid");
    let track = project.add_track(kind, "V1");
    let first = project
        .add_generated(track, solid(), TimeRange::at_rate(0, 48, Rational::FPS_24))
        .expect("first clip");
    let second = project
        .add_generated(track, solid(), TimeRange::at_rate(48, 48, Rational::FPS_24))
        .expect("second clip");
    let doc = serde_json::from_str(&project.save_to_string().expect("save")).expect("json");
    (doc, first, second)
}

/// The clip object with `id` in a saved document, wherever it sits.
fn clip_object(doc: &mut Value, id: ClipId) -> &mut serde_json::Map<String, Value> {
    fn find(value: &mut Value, id: u64) -> Option<&mut serde_json::Map<String, Value>> {
        match value {
            Value::Object(fields) => {
                if fields.contains_key("content") && fields.get("id") == Some(&json!(id)) {
                    return Some(fields);
                }
                fields.values_mut().find_map(|value| find(value, id))
            }
            Value::Array(items) => items.iter_mut().find_map(|value| find(value, id)),
            _ => None,
        }
    }
    find(doc, id.raw()).expect("the clip is in the document")
}

fn load_v2(mut doc: Value) -> Project {
    doc["schema"]["version"] = json!(2);
    Project::load_from_string(&doc.to_string()).expect("a v2 document loads")
}

fn driver(source: &str) -> DriverCode {
    DriverCode::new("aria.expressions", source)
}

#[test]
fn a_v2_mask_loads_as_a_one_mask_stack() {
    let (mut doc, masked, bare) = two_clip_document();
    clip_object(&mut doc, masked).insert(
        "mask".into(),
        json!({ "kind": "rectangle", "feather": 0.25, "invert": true }),
    );

    let project = load_v2(doc);

    let mut expected = Mask::new(MaskKind::Rectangle);
    expected.feather = Param::Constant(0.25);
    expected.invert = true;
    assert_eq!(project.clip(masked).expect("masked").masks, vec![expected]);
    assert!(project.clip(bare).expect("bare").masks.is_empty());
}

#[test]
fn a_v2_mask_driver_moves_to_mask_zero_and_other_drivers_stay() {
    let (mut doc, masked, _) = two_clip_document();
    let brightness = ClipParam::Look {
        param: LookParam::AdjustBrightness,
    };
    let clip = clip_object(&mut doc, masked);
    clip.insert("mask".into(), json!({ "kind": "circle" }));
    clip.insert(
        "drivers".into(),
        json!([
            { "param": { "look": { "param": "mask_feather" } },
              "language": "aria.expressions", "source": "0.5" },
            { "param": { "look": { "param": "mask_center" } },
              "language": "aria.expressions", "source": "[0.1, 0.2]" },
            { "param": serde_json::to_value(brightness).expect("param"),
              "language": "aria.expressions", "source": "0.1" },
        ]),
    );

    let project = load_v2(doc);
    let clip = project.clip(masked).expect("masked");

    let feather = ClipParam::Mask {
        mask: 0,
        param: MaskParam::Feather,
    };
    let center = ClipParam::Mask {
        mask: 0,
        param: MaskParam::Center,
    };
    assert_eq!(clip.driver(feather), Some(&driver("0.5")));
    assert_eq!(clip.driver(center), Some(&driver("[0.1, 0.2]")));
    assert_eq!(clip.driver(brightness), Some(&driver("0.1")));
    assert_eq!(clip.drivers.len(), 3);
}

#[test]
fn a_v2_mask_inside_a_bake_copy_migrates_too() {
    let (mut doc, masked, bare) = two_clip_document();
    let mut inner = Value::Object(clip_object(&mut doc, bare).clone());
    inner["mask"] = json!({ "kind": "circle" });
    clip_object(&mut doc, masked).insert("bake".into(), inner);

    let project = load_v2(doc);
    let bake = project
        .clip(masked)
        .expect("masked")
        .bake
        .as_ref()
        .expect("the bake copy loads");
    assert_eq!(bake.masks, vec![Mask::new(MaskKind::Circle)]);
}

#[test]
fn a_saved_mask_stack_reloads_unchanged() {
    let (doc, masked, _) = two_clip_document();
    let mut project = Project::load_from_string(&doc.to_string()).expect("load");
    project
        .add_clip_mask(masked, Mask::new(MaskKind::Circle))
        .expect("first mask");
    project
        .add_clip_mask(masked, Mask::new(MaskKind::Rectangle))
        .expect("second mask");
    let saved = project.save_to_string().expect("save");
    let reloaded = Project::load_from_string(&saved).expect("reload");
    assert_eq!(
        reloaded.clip(masked).expect("masked").masks,
        project.clip(masked).expect("masked").masks
    );
    assert_eq!(reloaded.save_to_string().expect("resave"), saved);
}
