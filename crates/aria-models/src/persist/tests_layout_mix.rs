//! A project file whose lane or master strip holds a mix value an edit
//! rejects is refused on load, with the field and the strip in the refusal,
//! on both parse paths.
//!
//! The class: a fader, pan, envelope or send level outside the range its
//! setter accepts ([`crate::AudioSend::check_level`],
//! [`crate::check_track_pan`] and the envelope checks), or an output or send
//! back into its own lane, reaching the model through the deserializer, where
//! the mix then reads a pan of `9.2e18` or a gain past `+12 dB`. Each case
//! edits one value of a saved project and loads the result. The accepted
//! cases pin the boundary: each value at its bound loads. What this does not
//! catch: a routing loop through other lanes, which the mix reports when it
//! reads the routing, and a mix value a setter gains later that
//! [`crate::Track::check_mix_values`] does not repeat.

use serde_json::{Value, json};

use super::{Edit, both_paths, load, saved};
use crate::{ModelError, ProjectLoadError};

fn lane(doc: &mut Value) -> &mut Value {
    &mut doc["timeline"]["tracks"][0][1]
}

fn lane_id(doc: &Value) -> Value {
    doc["timeline"]["tracks"][0][0].clone()
}

fn curve(value: f32) -> Value {
    json!({ "kf": [{ "t": 0, "v": 0.5 }, { "t": 24, "v": value }] })
}

/// One edit per value, the JSON path the refusal states, and its reason.
const REFUSED: &[(&str, Edit, &str, &str)] = &[
    (
        "lane gain past +12 dB",
        |d: &mut Value| lane(d)["gain"] = json!(5.0),
        "timeline.tracks",
        "lane 'V1' holds a value an edit rejects: invalid parameter: lane gain 5 is outside \
         0 to 4",
    ),
    (
        "negative lane gain",
        |d: &mut Value| lane(d)["gain"] = json!(-0.5),
        "timeline.tracks",
        "lane gain -0.5 is outside 0 to 4",
    ),
    (
        "lane pan past full right",
        |d: &mut Value| lane(d)["pan"] = json!(9.223_372e18),
        "timeline.tracks",
        "lane pan 9223372000000000000 is outside -1 to 1",
    ),
    (
        "lane pan past full left",
        |d: &mut Value| lane(d)["pan"] = json!(-1.5),
        "timeline.tracks",
        "lane pan -1.5 is outside -1 to 1",
    ),
    (
        "constant lane volume envelope past +12 dB",
        |d: &mut Value| lane(d)["volume_envelope"] = json!(5.0),
        "timeline.tracks",
        "lane volume envelope 5 is outside 0 to 4",
    ),
    (
        "lane volume keyframe past +12 dB",
        |d: &mut Value| lane(d)["volume_envelope"] = curve(5.0),
        "timeline.tracks",
        "lane volume envelope 5 is outside 0 to 4",
    ),
    (
        "lane pan keyframe past full right",
        |d: &mut Value| lane(d)["pan_envelope"] = curve(2.0),
        "timeline.tracks",
        "lane pan envelope 2 is outside -1 to 1",
    ),
    (
        "send level past +12 dB",
        |d: &mut Value| lane(d)["sends"] = json!([{ "to": 999, "level": 9.0 }]),
        "timeline.tracks",
        "send level 9 is outside 0 to 4",
    ),
    (
        "output into the lane itself",
        |d: &mut Value| {
            let own = lane_id(d);
            lane(d)["output"] = json!({ "Bus": own });
        },
        "timeline.tracks",
        "would feed it back into itself",
    ),
    (
        "send into the lane itself",
        |d: &mut Value| {
            let own = lane_id(d);
            lane(d)["sends"] = json!([{ "to": own, "level": 1.0 }]);
        },
        "timeline.tracks",
        "would feed it back into itself",
    ),
    (
        "master gain past +12 dB",
        |d: &mut Value| d["timeline"]["master_gain"] = json!(5.0),
        "timeline.master_gain",
        "the master strip holds a value an edit rejects: invalid parameter: master gain 5 is \
         outside 0 to 4",
    ),
    (
        "master pan past full left",
        |d: &mut Value| d["timeline"]["master_pan"] = json!(-2.0),
        "timeline.master_pan",
        "master pan -2 is outside -1 to 1",
    ),
    (
        "master volume keyframe past +12 dB",
        |d: &mut Value| d["timeline"]["master_volume_envelope"] = curve(5.0),
        "timeline.master_volume_envelope",
        "master volume envelope 5 is outside 0 to 4",
    ),
    (
        "master pan keyframe past full right",
        |d: &mut Value| d["timeline"]["master_pan_envelope"] = curve(3.0),
        "timeline.master_pan_envelope",
        "master pan envelope 3 is outside -1 to 1",
    ),
    (
        "nested sequence master pan",
        |d: &mut Value| d["sequences"][0][1]["timeline"]["master_pan"] = json!(2.0),
        "sequences.timeline.master_pan",
        "in sequence 'N', the master strip holds a value an edit rejects",
    ),
];

#[test]
fn a_mix_value_an_edit_rejects_is_refused_naming_the_field() {
    for &(case, edit, want_path, want_reason) in REFUSED {
        let mut doc = saved();
        edit(&mut doc);
        for (route, text) in both_paths(doc) {
            let err = load(route, &text).expect_err(case);
            let ModelError::ProjectLoad(ProjectLoadError::Damaged {
                json_path, reason, ..
            }) = &err
            else {
                panic!("{case} ({route}): not a damaged-file refusal: {err:?}");
            };
            assert_eq!(json_path, want_path, "{case} ({route})");
            assert!(
                reason.contains(want_reason),
                "{case} ({route}): {reason:?} lacks {want_reason:?}"
            );
        }
    }
}

/// A value at the bound an edit accepts loads: the check rejects past the
/// bound, not at it.
#[test]
fn a_mix_value_at_its_bound_loads() {
    let edits: [(&str, Edit); 12] = [
        ("lane gain 0", |d: &mut Value| lane(d)["gain"] = json!(0.0)),
        ("lane gain 4", |d: &mut Value| lane(d)["gain"] = json!(4.0)),
        ("lane pan -1", |d: &mut Value| lane(d)["pan"] = json!(-1.0)),
        ("lane pan 1", |d: &mut Value| lane(d)["pan"] = json!(1.0)),
        ("lane volume keyframe 4", |d: &mut Value| {
            lane(d)["volume_envelope"] = curve(4.0)
        }),
        ("lane pan keyframe -1", |d: &mut Value| {
            lane(d)["pan_envelope"] = curve(-1.0)
        }),
        ("send level 0 and 4", |d: &mut Value| {
            lane(d)["sends"] = json!([
                { "to": 998, "level": 0.0 },
                { "to": 999, "level": 4.0 },
            ]);
        }),
        ("output into another lane", |d: &mut Value| {
            lane(d)["output"] = json!({ "Bus": 999 })
        }),
        ("master gain 4", |d: &mut Value| {
            d["timeline"]["master_gain"] = json!(4.0)
        }),
        ("master pan -1", |d: &mut Value| {
            d["timeline"]["master_pan"] = json!(-1.0)
        }),
        ("master volume keyframe 0", |d: &mut Value| {
            d["timeline"]["master_volume_envelope"] = curve(0.0)
        }),
        ("master pan keyframe 1", |d: &mut Value| {
            d["timeline"]["master_pan_envelope"] = curve(1.0)
        }),
    ];
    for (case, edit) in edits {
        let mut doc = saved();
        edit(&mut doc);
        for (route, text) in both_paths(doc) {
            if let Err(err) = load(route, &text) {
                panic!("{case} ({route}) was refused: {err}");
            }
        }
    }
}
