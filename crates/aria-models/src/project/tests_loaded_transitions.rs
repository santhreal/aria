//! A transition read from a project file holds the rules every edit keeps:
//! both of its clips are on its lane and abut, and its window is longer than
//! zero and fits the junction. Each edit of a lane drops a transition that
//! breaks one ([`Project::prune_dead_transitions`]). A file reaches the model
//! through the deserializer instead, and a window longer than its junction
//! loaded and blended the clips on both sides of the cut from start to end,
//! until an unrelated edit removed it; a window of zero or fewer ticks loaded
//! and outlived every edit.
//!
//! The cases edit one transition of a saved project: each alignment in
//! [`TransitionAlignment::ALL`] at the lengths around the limit the model
//! computes for it, then a gap at the cut and an end clip the lane lacks.
//! Each file loads on both parse paths, and the loaded project holds nothing
//! an edit drops. What this does not catch: a transition whose id no catalog
//! has. It loads and renders as a cut, since the plugin that supplies it may
//! be missing.

use serde_json::{Value, json};

use super::*;
use crate::transition::{Transition, TransitionAlignment};

/// The outgoing clip's length in ticks.
const LEFT: i64 = 16;
/// The incoming clip's length in ticks.
const RIGHT: i64 = 48;

/// A saved project whose sticker lane cuts a [`LEFT`]-tick clip to a
/// [`RIGHT`]-tick clip under a crossfade, and that lane.
fn saved() -> (Value, TrackId) {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let solid = |rgba| Generator::SolidColor { rgba };
    let left = project
        .add_generated(track, solid([255, 0, 0, 255]), tr(0, LEFT))
        .unwrap();
    project
        .add_generated(track, solid([0, 0, 255, 255]), tr(LEFT, RIGHT))
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    let text = project.save_to_string().unwrap();
    (serde_json::from_str(&text).unwrap(), track)
}

/// The lane entry of `doc` that holds the transition.
fn lane(doc: &mut Value) -> &mut Value {
    doc["timeline"]["tracks"]
        .as_array_mut()
        .expect("a saved timeline lists its lanes")
        .iter_mut()
        .map(|pair| &mut pair[1])
        .find(|lane| lane.get("transitions").is_some())
        .expect("the saved lane holds the transition")
}

/// `doc` as a current file, with `schema` first as a save writes it, and as a
/// file of schema version 3, which the loader migrates.
fn both_texts(doc: &Value) -> [(&'static str, String); 2] {
    let mut rest = doc.clone();
    let schema = rest
        .as_object_mut()
        .and_then(|fields| fields.remove("schema"))
        .expect("a saved project has a schema");
    let current = format!("{{\"schema\":{schema},{}", &rest.to_string()[1..]);
    let mut migrated = doc.clone();
    migrated["schema"]["version"] = json!(3);
    [
        ("current schema", current),
        ("migrated", migrated.to_string()),
    ]
}

/// The transitions `track` holds once `doc` loads, on each parse path. Each
/// loaded project is asserted to hold nothing an edit drops.
fn loaded(doc: &Value, track: TrackId, case: &str) -> Vec<Vec<Transition>> {
    both_texts(doc)
        .into_iter()
        .map(|(route, text)| {
            let mut project = Project::load_from_string(&text)
                .unwrap_or_else(|err| panic!("{case} ({route}): {err}"));
            let held = project
                .timeline()
                .track(track)
                .unwrap()
                .transitions()
                .to_vec();
            assert!(
                !project.prune_dead_transitions(),
                "{case} ({route}): loaded a transition an edit drops: {held:?}"
            );
            held
        })
        .collect()
}

#[test]
fn a_loaded_transition_keeps_only_a_window_an_edit_keeps() {
    for alignment in TransitionAlignment::ALL {
        let limit = alignment.junction_limit(LEFT, RIGHT);
        assert!(limit > 1, "{alignment:?}: a limit of {limit} ticks");
        for duration in [i64::MIN, -1, 0, 1, limit, limit + 1, i64::MAX] {
            let (mut doc, track) = saved();
            let transition = &mut lane(&mut doc)["transitions"][0];
            transition["duration"] = json!(duration);
            transition["alignment"] = json!(alignment.id());
            let case = format!("{alignment:?} {duration}");
            let want = if (1..=limit).contains(&duration) {
                vec![(duration, alignment)]
            } else {
                Vec::new()
            };
            for held in loaded(&doc, track, &case) {
                let kept: Vec<(i64, TransitionAlignment)> =
                    held.iter().map(|t| (t.duration, t.alignment)).collect();
                assert_eq!(kept, want, "{case}");
            }
        }
    }
}

#[test]
fn a_loaded_transition_whose_clips_do_not_meet_is_dropped() {
    let edits: [(&str, fn(&mut Value), usize); 4] = [
        ("unedited", |_| {}, 1),
        (
            "a gap at the cut",
            |doc| {
                let clips = lane(doc)["clips"].as_array_mut().unwrap();
                let start = clips
                    .iter_mut()
                    .map(|pair| &mut pair[1]["timeline"]["start"]["value"])
                    .find(|start| **start == json!(LEFT))
                    .expect("the incoming clip starts at the cut");
                *start = json!(LEFT + 1);
            },
            0,
        ),
        (
            "an outgoing clip the lane lacks",
            |doc| lane(doc)["transitions"][0]["left"] = json!(999),
            0,
        ),
        (
            "an incoming clip the lane lacks",
            |doc| lane(doc)["transitions"][0]["right"] = json!(999),
            0,
        ),
    ];
    for (case, edit, want) in edits {
        let (mut doc, track) = saved();
        lane(&mut doc)["transitions"][0]["duration"] = json!(1);
        edit(&mut doc);
        for held in loaded(&doc, track, case) {
            assert_eq!(held.len(), want, "{case}: {held:?}");
        }
    }
}
