//! A project file that lists a lane, clip, media item, bin, multicam group or
//! sequence under a key other than the id it holds is refused on load, with
//! the entry's path and both numbers, on both parse paths.
//!
//! The class: a map whose entries hold their own id, read from a file instead
//! of built by edits. Edits file each entry under the id it holds, and the
//! model reaches an entry by either one: a clip listed under key 1 that held
//! id 5 loaded, drew, and could not be selected or moved. Each case edits one
//! key or one id of a saved project holding every kind of keyed entry, at the
//! root and inside a sequence. The census holds every `serde_map` field in the
//! crate to a recorded decision, so a new map of entries fails until it is
//! read through `serde_map::keyed` or recorded as holding no id. What this
//! does not catch: an entry map a project file holds without `serde_map`.

use std::path::Path;

use serde_json::{Value, json};

use super::tests_layout::{both_paths, load};
use super::*;
use crate::clip::Clip;
use crate::media::MediaSource;
use crate::multicam::MulticamGroup;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

const R24: Rational = Rational::FPS_24;

/// A saved project holding one media item, a bin, a multicam group, a
/// sequence whose lane holds one clip, and a root lane holding one clip.
fn saved() -> Value {
    let mut project = Project::new("keys", R24);
    let media = project.add_media(MediaSource::new("one.mp4", 1920, 1080, R24, 1000, true));
    project.add_bin("Rushes", None).expect("bin");
    project
        .add_multicam_group(MulticamGroup::from_media("G", [(media, 0)]).expect("group"))
        .expect("group added");
    let sequence = project.create_sequence("A", R24);
    project.set_active_sequence(Some(sequence)).expect("opened");
    let lane = project.add_track(TrackKind::Video, "VA");
    let span = TimeRange::at_rate(0, 24, R24);
    let clip = Clip::from_media(media, span, span);
    project.timeline_mut().add_clip(lane, clip).expect("placed");
    project.set_active_sequence(None).expect("root opened");
    let lane = project.add_track(TrackKind::Video, "V1");
    project
        .timeline_mut()
        .add_clip(lane, Clip::from_media(media, span, span))
        .expect("placed");
    serde_json::from_str(&project.save_to_string().expect("saved")).expect("saved JSON")
}

/// Each keyed map as the path of its first entry in [`saved`].
const ENTRIES: &[&str] = &[
    "timeline.tracks[0]",
    "timeline.tracks[0][1].clips[0]",
    "media[0]",
    "bins[0]",
    "multicam_groups[0]",
    "sequences[0]",
    "sequences[0][1].timeline.tracks[0]",
    "sequences[0][1].timeline.tracks[0][1].clips[0]",
];

/// The JSON value at `path`, a dotted path with `[index]` steps.
fn at<'d>(doc: &'d mut Value, path: &str) -> &'d mut Value {
    let mut node = doc;
    for step in path.split('.') {
        let mut parts = step.split('[');
        let name = parts.next().expect("step");
        node = &mut node[name];
        for index in parts {
            let index: usize = index.trim_end_matches(']').parse().expect("index");
            node = &mut node[index];
        }
    }
    node
}

#[test]
fn an_entry_listed_under_a_key_other_than_its_id_is_refused() {
    for &entry in ENTRIES {
        for (edited, slot) in [("key", 0), ("id", 1)] {
            let mut doc = saved();
            let pair = at(&mut doc, entry);
            assert!(pair[1]["id"].is_u64(), "{entry}: no id in {pair}");
            let want = if slot == 0 {
                pair[0] = json!(999_999);
                "listed under key 999999 and holds id "
            } else {
                pair[1]["id"] = json!(999_999);
                " and holds id 999999, and each entry is listed under its own id"
            };
            // A typed parse that fails rereads the text as a `Value` to state
            // the path, so both texts end on the value path.
            for (route, text) in both_paths(doc) {
                let case = format!("{entry} {edited} ({route})");
                let err = Project::load_from_string(&text).expect_err(&case);
                let ModelError::ProjectLoad(ProjectLoadError::Damaged {
                    json_path, reason, ..
                }) = &err
                else {
                    panic!("{case}: not a damaged-file refusal: {err:?}");
                };
                assert_eq!(json_path, entry, "{case}");
                assert!(reason.contains(want), "{case}: {reason:?} lacks {want:?}");
            }
        }
    }
}

#[test]
fn an_entry_whose_key_and_id_agree_loads() {
    let edits: [(&str, fn(&mut Value)); 2] = [
        ("unedited", |_: &mut Value| {}),
        ("bin renumbered", |d: &mut Value| {
            d["bins"][0][0] = json!(999_999);
            d["bins"][0][1]["id"] = json!(999_999);
        }),
    ];
    for (case, edit) in edits {
        let mut doc = saved();
        edit(&mut doc);
        for (route, text) in both_paths(doc) {
            let project = load(route, &text)
                .unwrap_or_else(|err| panic!("{case} ({route}) was refused: {err}"));
            assert_eq!(project.root_timeline().clip_count(), 1, "{case} ({route})");
            assert_eq!(project.sequences().count(), 1, "{case} ({route})");
        }
    }
}

/// Entry maps read through `serde_map::keyed`: the key and the value type.
const KEYED: &[(&str, &str)] = &[
    ("TrackId", "Track"),
    ("ClipId", "Clip"),
    ("MediaId", "MediaSource"),
    ("BinId", "Bin"),
    ("MulticamGroupId", "MulticamGroup"),
    ("SequenceId", "Sequence"),
];

/// Maps read through plain `serde_map`, because the value holds no id.
const PLAIN: &[(&str, &str, &str)] = &[
    ("String", "*", "a parameter map, keyed by parameter name"),
    (
        "ClipId",
        "TrackId",
        "the clip index, rederived from the lanes on load",
    ),
    (
        "MediaId",
        "PoolPosition",
        "a pool position, which holds no id",
    ),
];

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The last path segment of a type: `crate::bin::Bin` is `Bin`.
fn last_segment(ty: &str) -> &str {
    ty.trim().rsplit("::").next().expect("segment")
}

#[test]
fn every_serde_map_field_of_entries_is_read_keyed() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    let mut seen_keyed = Vec::new();
    let mut fields = 0;
    for file in files.iter().filter(|f| !f.ends_with("serde_map.rs")) {
        let text = std::fs::read_to_string(file).expect("source");
        let lines: Vec<&str> = text.lines().collect();
        for (at_line, line) in lines.iter().enumerate() {
            let keyed = line.contains("\"crate::serde_map::keyed\"");
            if !keyed && !line.contains("\"crate::serde_map\"") {
                continue;
            }
            let field = lines[at_line..]
                .iter()
                .take(6)
                .find_map(|l| l.split_once(": Map<"))
                .map(|(_, rest)| rest.trim_end().trim_end_matches(','))
                .and_then(|rest| rest.strip_suffix('>'))
                .unwrap_or_else(|| panic!("{}:{}: no Map field", file.display(), at_line + 1));
            let (key, value) = field.split_once(", ").expect("key and value");
            let (key, value) = (last_segment(key), last_segment(value));
            let place = format!("{}:{} Map<{key}, {value}>", file.display(), at_line + 1);
            fields += 1;
            if let Some(&pair) = KEYED.iter().find(|&&pair| pair == (key, value)) {
                assert!(
                    keyed,
                    "{place} holds entries with ids; read it through serde_map::keyed"
                );
                seen_keyed.push(pair);
            } else if PLAIN
                .iter()
                .any(|&(k, v, _)| k == key && (v == "*" || v == value))
            {
                assert!(
                    !keyed,
                    "{place} holds no ids; read it through plain serde_map"
                );
            } else {
                panic!("{place} has no decision; add it to KEYED or PLAIN");
            }
        }
    }
    assert!(fields > 20, "the census read {fields} fields");
    for pair in KEYED {
        assert!(
            seen_keyed.contains(pair),
            "no field reads {pair:?}; drop it from KEYED"
        );
    }
}
