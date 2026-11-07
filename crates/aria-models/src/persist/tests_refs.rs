//! A project file whose clip plays something the project does not hold, whose
//! sequence contains itself, or whose multicam group is invalid is refused on
//! load, with the field and the clip or group in the refusal, on both parse
//! paths.
//!
//! The class: a reference the edit path keeps ([`Project::add_clip`],
//! [`Project::remove_media`], the multicam group constructors, the nested
//! sequence cycle check) that reaches the model through the deserializer
//! instead. A clip playing media the project does not hold rendered black
//! without a word, and a sequence containing itself loaded and failed the
//! first frame. Each case edits one reference of a saved project holding
//! every kind of reference, and loads the result through the
//! current-schema path and the migrating path. The accepted cases pin the
//! boundary: a sequence nested in two places without a cycle loads. What
//! this does not catch: a reference kind a clip gains later that
//! [`Project::reference_fault`] does not read.

use serde_json::{Value, json};

use super::tests_layout::{both_paths, load};
use super::*;
use crate::clip::Clip;
use crate::media::MediaSource;
use crate::multicam::MulticamGroup;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

const R24: Rational = Rational::FPS_24;

fn span(start: i64) -> TimeRange {
    TimeRange::at_rate(start, 24, R24)
}

/// A saved project whose root lane V1 holds a media clip, a clip playing
/// sequence A and a clip playing multicam group G (two angles), and whose
/// sequences A and B each hold one media clip, on lanes VA and VB.
fn saved() -> Value {
    let mut project = Project::new("refs", R24);
    let media = project.add_media(MediaSource::new("one.mp4", 1920, 1080, R24, 1000, true));
    let other = project.add_media(MediaSource::new("two.mp4", 1920, 1080, R24, 1000, true));
    let group = project
        .add_multicam_group(
            MulticamGroup::from_media("G", [(media, 0), (other, 0)]).expect("group"),
        )
        .expect("group added");
    let mut nested = Vec::new();
    for (name, lane) in [("A", "VA"), ("B", "VB")] {
        let sequence = project.create_sequence(name, R24);
        project.set_active_sequence(Some(sequence)).expect("opened");
        let lane = project.add_track(TrackKind::Video, lane);
        let clip = Clip::from_media(media, span(0), span(0));
        project.timeline_mut().add_clip(lane, clip).expect("placed");
        nested.push(sequence);
    }
    project.set_active_sequence(None).expect("root opened");
    let lane = project.add_track(TrackKind::Video, "V1");
    let clips = [
        Clip::from_media(media, span(0), span(0)),
        Clip::from_sequence(nested[0], span(0), span(24)),
        Clip::from_multicam(group, span(0), span(48)),
    ];
    for clip in clips {
        project.timeline_mut().add_clip(lane, clip).expect("placed");
    }
    serde_json::from_str(&project.save_to_string().expect("saved")).expect("saved JSON")
}

/// The content of the root clip whose content is a `kind`.
fn root_content<'d>(doc: &'d mut Value, kind: &str) -> &'d mut Value {
    doc["timeline"]["tracks"][0][1]["clips"]
        .as_array_mut()
        .expect("clips")
        .iter_mut()
        .find_map(|entry| entry[1]["content"].get_mut(kind))
        .expect("a root clip of this kind")
}

/// The content of the clip in sequence `index` (A is 0, B is 1).
fn nested_content(doc: &mut Value, index: usize) -> &mut Value {
    &mut doc["sequences"][index][1]["timeline"]["tracks"][0][1]["clips"][0][1]["content"]
}

/// `doc`'s clip in sequence `index` set to play sequence `plays`.
fn nest(doc: &mut Value, index: usize, plays: usize) {
    let sequence = doc["sequences"][plays][0].clone();
    let source = nested_content(doc, index)["Media"]["source"].clone();
    *nested_content(doc, index) = json!({ "Sequence": { "sequence": sequence, "source": source } });
}

type Edit = fn(&mut Value);

/// One edit per reference, the JSON path the refusal states, and parts of
/// its reason.
const REFUSED: &[(&str, Edit, &str, &[&str])] = &[
    (
        "media the project does not hold",
        |d: &mut Value| root_content(d, "Media")["media"] = json!(999_999),
        "timeline.tracks",
        &["on lane 'V1' plays media 999999, and the project holds no media 999999"],
    ),
    (
        "sequence the project does not hold",
        |d: &mut Value| root_content(d, "Sequence")["sequence"] = json!(999_999),
        "timeline.tracks",
        &["on lane 'V1' plays sequence 999999, and the project holds no sequence 999999"],
    ),
    (
        "multicam group the project does not hold",
        |d: &mut Value| root_content(d, "Multicam")["group"] = json!(999_999),
        "timeline.tracks",
        &["plays multicam group 999999, and the project holds no multicam group 999999"],
    ),
    (
        "media a nested clip plays and the project does not hold",
        |d: &mut Value| nested_content(d, 1)["Media"]["media"] = json!(999_999),
        "sequences.timeline.tracks",
        &["in sequence 'B', clip ", "on lane 'VB' plays media 999999"],
    ),
    (
        "sequence containing itself",
        |d: &mut Value| nest(d, 0, 0),
        "sequences.timeline.tracks",
        &[
            "in sequence 'A', clip ",
            "on lane 'VA' plays sequence 'A', and sequence 'A' contains this sequence",
        ],
    ),
    (
        "two sequences containing each other",
        |d: &mut Value| {
            nest(d, 0, 1);
            nest(d, 1, 0);
        },
        "sequences.timeline.tracks",
        &[
            "in sequence 'A', clip ",
            "plays sequence 'B', and sequence 'B' contains this sequence",
        ],
    ),
    (
        "multicam angle playing media the project does not hold",
        |d: &mut Value| d["multicam_groups"][0][1]["angles"][1]["media"] = json!(999_999),
        "multicam_groups",
        &[
            "multicam group ",
            " 'G' angle 2 plays media 999999, and the project holds no media 999999",
        ],
    ),
    (
        "multicam cut to an angle the group does not hold",
        |d: &mut Value| d["multicam_groups"][0][1]["cuts"] = json!([{ "at": 0, "angle": 5 }]),
        "multicam_groups",
        &["multicam group ", " 'G' is invalid: "],
    ),
];

#[test]
fn a_reference_the_project_does_not_hold_is_refused_naming_the_field() {
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
            for part in want_reason {
                assert!(
                    reason.contains(part),
                    "{case} ({route}): {reason:?} lacks {part:?}"
                );
            }
        }
    }
}

#[test]
fn a_sequence_nested_in_two_places_without_a_cycle_loads() {
    let edits: [(&str, Edit); 3] = [
        ("unedited", |_: &mut Value| {}),
        ("B plays A", |d: &mut Value| nest(d, 1, 0)),
        ("A plays B", |d: &mut Value| nest(d, 0, 1)),
    ];
    for (case, edit) in edits {
        let mut doc = saved();
        edit(&mut doc);
        for (route, text) in both_paths(doc) {
            let project = load(route, &text)
                .unwrap_or_else(|err| panic!("{case} ({route}) was refused: {err}"));
            assert_eq!(project.root_timeline().clip_count(), 3, "{case} ({route})");
        }
    }
}
