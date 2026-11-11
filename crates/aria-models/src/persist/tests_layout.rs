//! A project file whose sequence breaks a placement rule is refused on load,
//! with the field and the clip in the refusal, on both parse paths.
//!
//! The class: a value the edit path refuses ([`crate::Timeline::add_clip`],
//! the speed setter, the sequence rate edit, the canvas size edit, the setter
//! of a clip's transform, level, or filter or LUT strength) that reaches the
//! model through the deserializer instead, where a render or an edit then
//! fails on it with an internal error or a panic. Each case edits one value
//! of a saved project and loads the result through the current-schema fast
//! path and the migrating path. The accepted cases pin the boundary: a
//! zero-length clip, a clip at tick zero, a clip value at its bound, and the
//! smallest and largest canvas load. What this does not catch: a rule
//! `add_clip` or a setter gains later that [`crate::Timeline::layout_fault`]
//! does not repeat.
//!
//! The lane order and clip index restate the lanes, and a file can misstate
//! them: a lane left out of the order is drawn and rendered nowhere, and a
//! clip the index places on another lane is found by no edit. The load
//! rebuilds both from the lanes, and refuses one clip id on two lanes.

use serde_json::{Value, json};

use super::*;
use crate::clip::Clip;
use crate::ids::TrackId;
use crate::media::MediaSource;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

const R24: Rational = Rational::FPS_24;

/// A saved project with one media clip at ticks 24..72 on lane V1 and one
/// empty nested sequence named N.
fn saved() -> Value {
    let mut project = Project::new("layout", R24);
    let lane = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(MediaSource::new("layout.mp4", 1920, 1080, R24, 1000, true));
    project
        .timeline_mut()
        .add_clip(
            lane,
            Clip::from_media(
                media,
                TimeRange::at_rate(0, 48, R24),
                TimeRange::at_rate(24, 48, R24),
            ),
        )
        .expect("placed");
    project.create_sequence("N", R24);
    serde_json::from_str(&project.save_to_string().expect("saved")).expect("saved JSON")
}

fn clip(doc: &mut Value) -> &mut Value {
    &mut doc["timeline"]["tracks"][0][1]["clips"][0][1]
}

/// `doc` as the fast path reads it, then as the migrating path reads it. The
/// fast path reads a file whose first key is a current `schema`, and a
/// [`Value`] writes its keys sorted, so the first text puts `schema` first.
pub(super) fn both_paths(mut doc: Value) -> [(&'static str, String); 2] {
    let mut rest = doc.clone();
    let schema = rest
        .as_object_mut()
        .and_then(|fields| fields.remove("schema"))
        .expect("a saved project has a schema");
    let current = format!("{{\"schema\":{schema},{}", &rest.to_string()[1..]);
    doc["schema"]["version"] = json!(3);
    [("current schema", current), ("migrated", doc.to_string())]
}

/// `text` loaded, asserting it took the parse path `route` names.
pub(super) fn load(route: &str, text: &str) -> Result<Project, ModelError> {
    reset_value_fallback_count();
    let loaded = Project::load_from_string(text);
    assert_eq!(
        value_fallback_count(),
        usize::from(route == "migrated"),
        "{route}: the parse path read"
    );
    loaded
}

type Edit = fn(&mut Value);

/// One edit per rule, the JSON path the refusal states, and a part of its
/// reason.
const REFUSED: &[(&str, Edit, &str, &str)] = &[
    (
        "zero sequence rate",
        |d: &mut Value| d["timeline"]["frame_rate"]["num"] = json!(0),
        "timeline.frame_rate",
        "the sequence runs at 0/1, and a rate needs",
    ),
    (
        "negative sequence rate",
        |d: &mut Value| d["timeline"]["frame_rate"]["den"] = json!(-1),
        "timeline.frame_rate",
        "the sequence runs at 24/-1",
    ),
    (
        "clip start at another rate",
        |d: &mut Value| clip(d)["timeline"]["start"]["rate"]["num"] = json!(25),
        "timeline.tracks",
        "on lane 'V1' is timed at 25/1, and the sequence runs at 24/1",
    ),
    (
        "clip length at another rate",
        |d: &mut Value| clip(d)["timeline"]["duration"]["rate"]["num"] = json!(30),
        "timeline.tracks",
        "on lane 'V1' is timed at 30/1, and the sequence runs at 24/1",
    ),
    (
        "negative start",
        |d: &mut Value| clip(d)["timeline"]["start"]["value"] = json!(-1),
        "timeline.tracks",
        "has a negative start or length on the timeline",
    ),
    (
        "negative length",
        |d: &mut Value| clip(d)["timeline"]["duration"]["value"] = json!(-1),
        "timeline.tracks",
        "has a negative start or length on the timeline",
    ),
    (
        "end past the last tick",
        |d: &mut Value| clip(d)["timeline"]["start"]["value"] = json!(i64::MAX),
        "timeline.tracks",
        "ends past the last tick a time holds on the timeline",
    ),
    (
        "negative source start",
        |d: &mut Value| clip(d)["content"]["Media"]["source"]["start"]["value"] = json!(-1),
        "timeline.tracks",
        "has a negative start or length in its source",
    ),
    (
        "source end past the last tick",
        |d: &mut Value| clip(d)["content"]["Media"]["source"]["start"]["value"] = json!(i64::MAX),
        "timeline.tracks",
        "ends past the last tick a time holds in its source",
    ),
    (
        "zero speed",
        |d: &mut Value| clip(d)["speed"] = json!({ "num": 0, "den": 1 }),
        "timeline.tracks",
        "plays at speed 0/1, and a speed needs",
    ),
    (
        "opacity below zero",
        |d: &mut Value| clip(d)["transform"]["opacity"] = json!(-1.0),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "opacity above one",
        |d: &mut Value| clip(d)["transform"]["opacity"] = json!(2.0),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "volume above the ceiling",
        |d: &mut Value| clip(d)["volume"] = json!(1000.0),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "pan past full right",
        |d: &mut Value| clip(d)["pan"] = json!(2.0),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "filter intensity above one",
        |d: &mut Value| clip(d)["filter"] = json!({ "id": "no-such-filter", "intensity": 2.0 }),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "LUT intensity below zero",
        |d: &mut Value| clip(d)["lut"] = json!({ "path": "look.cube", "intensity": -1.0 }),
        "timeline.tracks",
        "on lane 'V1' holds a value an edit rejects",
    ),
    (
        "nested sequence rate",
        |d: &mut Value| d["sequences"][0][1]["timeline"]["frame_rate"]["num"] = json!(0),
        "sequences.timeline.frame_rate",
        "in sequence 'N', the sequence runs at 0/1",
    ),
    (
        "zero canvas",
        |d: &mut Value| d["timeline"]["canvas"]["resolution"] = json!([0, 0]),
        "timeline.canvas",
        "the canvas is 0x0 pixels, and each edge needs to be even and from 16 to 8192",
    ),
    (
        "canvas edge one under the bound",
        |d: &mut Value| d["timeline"]["canvas"]["resolution"] = json!([14, 1080]),
        "timeline.canvas",
        "the canvas is 14x1080 pixels",
    ),
    (
        "canvas edge one past the bound",
        |d: &mut Value| d["timeline"]["canvas"]["resolution"] = json!([1920, 8194]),
        "timeline.canvas",
        "the canvas is 1920x8194 pixels",
    ),
    (
        "odd canvas edge",
        |d: &mut Value| d["timeline"]["canvas"]["resolution"] = json!([1921, 1080]),
        "timeline.canvas",
        "the canvas is 1921x1080 pixels",
    ),
    (
        "largest canvas a file holds",
        |d: &mut Value| d["timeline"]["canvas"]["resolution"] = json!([u32::MAX, u32::MAX]),
        "timeline.canvas",
        "the canvas is 4294967295x4294967295 pixels",
    ),
    (
        "nested sequence canvas",
        |d: &mut Value| d["sequences"][0][1]["timeline"]["canvas"]["resolution"] = json!([0, 16]),
        "sequences.timeline.canvas",
        "in sequence 'N', the canvas is 0x16 pixels",
    ),
];

#[test]
fn a_sequence_that_breaks_a_placement_rule_is_refused_naming_the_field() {
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

#[test]
fn the_refusal_is_the_sentence_a_caller_prints() {
    let mut doc = saved();
    doc["timeline"]["frame_rate"]["num"] = json!(0);
    let err = Project::load_from_string(&doc.to_string()).expect_err("zero rate");
    assert_eq!(
        err.to_string(),
        "Couldn't open this project: the project file is damaged or corrupted at field \
         'frame_rate' in 'timeline.frame_rate' (the sequence runs at 0/1, and a rate needs a \
         numerator and denominator above zero). Pick another .aria file."
    );
}

#[test]
fn a_zero_length_clip_and_a_clip_at_tick_zero_load() {
    let edits: [(&str, Edit); 3] = [
        ("unedited", |_: &mut Value| {}),
        ("zero length", |d: &mut Value| {
            clip(d)["timeline"]["duration"]["value"] = json!(0);
        }),
        ("at tick zero", |d: &mut Value| {
            clip(d)["timeline"]["start"]["value"] = json!(0);
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

/// A value at the bound an edit accepts loads: the check refuses past the
/// bound, not at it.
#[test]
fn a_value_at_its_bound_loads() {
    let edits: [(&str, Edit); 9] = [
        ("opacity 0", |d: &mut Value| {
            clip(d)["transform"]["opacity"] = json!(0.0)
        }),
        ("opacity 1", |d: &mut Value| {
            clip(d)["transform"]["opacity"] = json!(1.0)
        }),
        ("volume 0", |d: &mut Value| clip(d)["volume"] = json!(0.0)),
        ("pan -1", |d: &mut Value| clip(d)["pan"] = json!(-1.0)),
        ("pan 1", |d: &mut Value| clip(d)["pan"] = json!(1.0)),
        ("filter intensity 1", |d: &mut Value| {
            clip(d)["filter"] = json!({ "id": "no-such-filter", "intensity": 1.0 })
        }),
        ("LUT intensity 0", |d: &mut Value| {
            clip(d)["lut"] = json!({ "path": "look.cube", "intensity": 0.0 })
        }),
        ("smallest canvas", |d: &mut Value| {
            d["timeline"]["canvas"]["resolution"] = json!([16, 16])
        }),
        ("largest canvas", |d: &mut Value| {
            d["timeline"]["canvas"]["resolution"] = json!([8192, 8192])
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

/// A saved project with lanes V1 and V2, each holding one media clip at ticks
/// 24..72, and the lane ids in id order.
fn saved_two() -> (Value, [TrackId; 2]) {
    let mut project = Project::new("lanes", R24);
    let lanes = [
        project.add_track(TrackKind::Video, "V1"),
        project.add_track(TrackKind::Video, "V2"),
    ];
    let media = project.add_media(MediaSource::new("lanes.mp4", 1920, 1080, R24, 1000, true));
    for lane in lanes {
        project
            .timeline_mut()
            .add_clip(
                lane,
                Clip::from_media(
                    media,
                    TimeRange::at_rate(0, 48, R24),
                    TimeRange::at_rate(24, 48, R24),
                ),
            )
            .expect("placed");
    }
    let doc = serde_json::from_str(&project.save_to_string().expect("saved")).expect("JSON");
    let mut sorted = lanes;
    sorted.sort_unstable();
    (doc, sorted)
}

fn raw(lane: TrackId) -> Value {
    json!(lane.raw())
}

/// The lane order and clip index restate what the lanes hold. Each case
/// misstates one of them and expects the load to show every lane once, in
/// the order the case names, and to find every clip on the lane holding it.
#[test]
fn a_lane_order_and_clip_index_the_file_misstates_are_rederived_from_the_lanes() {
    type Case = (
        &'static str,
        fn(&mut Value, [TrackId; 2]),
        fn([TrackId; 2]) -> Vec<TrackId>,
    );
    let cases: [Case; 8] = [
        ("unedited order", |_, _| {}, |[a, b]| vec![a, b]),
        (
            "empty order",
            |d, _| d["timeline"]["order"] = json!([]),
            |[a, b]| vec![a, b],
        ),
        (
            "one lane left out",
            |d, [_, b]| d["timeline"]["order"] = json!([raw(b)]),
            |[a, b]| vec![b, a],
        ),
        (
            "a lane twice and a lane no file holds",
            |d, [a, b]| d["timeline"]["order"] = json!([raw(b), 999_999, raw(b), raw(a)]),
            |[a, b]| vec![b, a],
        ),
        (
            "empty clip index",
            |d, _| d["timeline"]["clip_index"] = json!([]),
            |[a, b]| vec![a, b],
        ),
        (
            "clip index naming a lane no file holds",
            |d, _| {
                for entry in d["timeline"]["clip_index"].as_array_mut().expect("index") {
                    entry[1] = json!(999_999);
                }
            },
            |[a, b]| vec![a, b],
        ),
        (
            "clip index naming the other lane",
            |d, [a, b]| {
                for entry in d["timeline"]["clip_index"].as_array_mut().expect("index") {
                    entry[1] = if entry[1] == raw(a) { raw(b) } else { raw(a) };
                }
            },
            |[a, b]| vec![a, b],
        ),
        (
            "clip index naming a clip no lane holds",
            |d, [a, _]| {
                let index = d["timeline"]["clip_index"].as_array_mut().expect("index");
                index.push(json!([999_999, raw(a)]));
            },
            |[a, b]| vec![a, b],
        ),
    ];
    for (case, edit, want_order) in cases {
        let (mut doc, lanes) = saved_two();
        // With no lane flagged main, the load makes the bottom lane of the
        // order main, and the zones keep the order the case names.
        for entry in doc["timeline"]["tracks"].as_array_mut().expect("lanes") {
            entry[1]["main"] = json!(false);
        }
        doc["timeline"]["order"] = json!([raw(lanes[0]), raw(lanes[1])]);
        edit(&mut doc, lanes);
        for (route, text) in both_paths(doc) {
            let project = load(route, &text)
                .unwrap_or_else(|err| panic!("{case} ({route}) was refused: {err}"));
            let timeline = project.timeline();
            assert_eq!(
                timeline.order(),
                want_order(lanes).as_slice(),
                "{case} ({route}): lane order"
            );
            let mut held = 0;
            for lane in timeline.tracks_ordered() {
                for clip in lane.clips() {
                    held += 1;
                    assert_eq!(
                        timeline.track_of(clip.id),
                        Some(lane.id),
                        "{case} ({route}): clip {} is found on another lane",
                        clip.id.raw()
                    );
                }
            }
            assert_eq!(
                (held, timeline.clip_count()),
                (2, 2),
                "{case} ({route}): clips held, clips indexed"
            );
        }
    }
}

/// One clip id on two lanes leaves an edit of either clip reaching the other:
/// the index holds one lane per id. The load refuses it and states both lanes.
#[test]
fn a_clip_id_on_two_lanes_is_refused_naming_both() {
    let (mut doc, [a, b]) = saved_two();
    doc["timeline"]["order"] = json!([raw(a), raw(b)]);
    let lanes = doc["timeline"]["tracks"].as_array().expect("lanes").clone();
    let first = |lane: TrackId| {
        let entry = lanes.iter().find(|e| e[0] == raw(lane)).expect("lane");
        (entry[1]["name"].clone(), entry[1]["clips"][0][0].clone())
    };
    let ((a_name, a_clip), (b_name, _)) = (first(a), first(b));
    for entry in doc["timeline"]["tracks"].as_array_mut().expect("lanes") {
        if entry[0] == raw(b) {
            entry[1]["clips"][0][0] = a_clip.clone();
            entry[1]["clips"][0][1]["id"] = a_clip.clone();
        }
    }
    let want = format!("clip {a_clip} is on lane {a_name} and on lane {b_name}").replace('"', "'");
    for (route, text) in both_paths(doc) {
        let err = load(route, &text).expect_err(route);
        let ModelError::ProjectLoad(ProjectLoadError::Damaged {
            json_path, reason, ..
        }) = &err
        else {
            panic!("{route}: not a damaged-file refusal: {err:?}");
        };
        assert_eq!(json_path, "timeline.tracks", "{route}");
        assert_eq!(reason, &want, "{route}");
    }
}

#[path = "tests_layout_mix.rs"]
mod mix;
