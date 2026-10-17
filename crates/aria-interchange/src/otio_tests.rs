//! OpenTimelineIO: what a reader takes from a timeline as other editors
//! write one, what it reports and leaves out, the JSON the writer produces,
//! and that what the writer produces reads back as the cut it came from.
//!
//! Not caught here: OTIO's other serializations (`.otiod`, `.otioz`) and
//! adapters; only the JSON form is read and written.

use aria_models::{MarkerColor, Rational};
use serde_json::{Value, json};

use super::{read, write};
use crate::model::{Event, LaneKind, Read, Source};
use crate::test_support::{R24, at, cut, event, lane, mark, written};

/// A `RationalTime` as JSON.
fn rt(value: f64, rate: f64) -> Value {
    json!({ "OTIO_SCHEMA": "RationalTime.1", "value": value, "rate": rate })
}

/// A `TimeRange` as JSON.
fn range(start: f64, duration: f64, rate: f64) -> Value {
    json!({ "OTIO_SCHEMA": "TimeRange.1", "start_time": rt(start, rate), "duration": rt(duration, rate) })
}

fn timeline(children: Value) -> Value {
    json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": "Cut",
        "global_start_time": rt(0.0, 24.0),
        "tracks": { "OTIO_SCHEMA": "Stack.1", "children": children },
    })
}

/// A cut with every part a cut holds: a picture with its sound, a shot at
/// double speed dissolving in, a second picture lane, music counted in
/// milliseconds, and markers with notes.
fn promo(rate: Rational) -> crate::model::Cut {
    let interview = Event {
        sound: true,
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, rate))
    };
    let broll = Event {
        speed: Rational::new(2, 1),
        dissolve: Some(12),
        ..event("Broll", 48, 24, "/media/B002.mov", at(0, rate))
    };
    let logo = event("Logo", 12, 24, "/media/logo.mov", at(0, rate));
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            72,
            "/media/music.wav",
            at(0, Rational::new(1000, 1)),
        )
    };
    cut(
        "Promo",
        rate,
        vec![
            lane(LaneKind::Video, "V1", vec![interview, broll]),
            lane(LaneKind::Video, "V2", vec![logo]),
            lane(LaneKind::Audio, "A1", vec![music]),
        ],
        vec![
            mark(24, "Check", "Focus on the left", Some(MarkerColor::Green)),
            mark(60, "Out", "", Some(MarkerColor::Purple)),
        ],
    )
}

#[test]
fn what_the_writer_produces_reads_back_as_the_cut() {
    for rate in [
        R24,
        Rational::FPS_23_976,
        Rational::FPS_25,
        Rational::FPS_29_97,
        Rational::FPS_60,
    ] {
        let original = promo(rate);
        let (text, skipped) = written(write, &original);
        assert!(skipped.is_empty(), "{rate:?}: {skipped:?}");

        let read = read(&text, Rational::new(7, 1), "fallback").expect("reads");
        assert_eq!(
            read,
            Read {
                cut: original,
                skipped: Vec::new()
            },
            "{rate:?}"
        );
    }
}

#[test]
fn the_writer_produces_opentimelineio_json() {
    let back = Event {
        speed: Rational::new(2, 1),
        reversed: true,
        dissolve: Some(13),
        ..event("Broll", 48, 24, "/media/B002.mov", at(0, R24))
    };
    let original = cut(
        "Promo",
        R24,
        vec![lane(
            LaneKind::Video,
            "V1",
            vec![
                event("Interview", 0, 48, "/media/A001.mov", at(24, R24)),
                back,
            ],
        )],
        vec![mark(24, "Check", "Focus", Some(MarkerColor::Green))],
    );
    let (text, _) = written(write, &original);
    let value: Value = serde_json::from_str(&text).expect("JSON");

    let reference = |reel: &str, url: &str| {
        json!({ "DEFAULT_MEDIA": {
            "OTIO_SCHEMA": "ExternalReference.1",
            "metadata": {},
            "name": reel,
            "available_range": null,
            "available_image_bounds": null,
            "target_url": url,
        }})
    };
    let expected = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "metadata": {},
        "name": "Promo",
        "global_start_time": rt(0.0, 24.0),
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "metadata": {},
            "name": "tracks",
            "source_range": null,
            "effects": [],
            "markers": [{
                "OTIO_SCHEMA": "Marker.2",
                "metadata": {},
                "name": "Check",
                "color": "GREEN",
                "comment": "Focus",
                "marked_range": range(24.0, 0.0, 24.0),
            }],
            "enabled": true,
            "children": [{
                "OTIO_SCHEMA": "Track.1",
                "metadata": {},
                "name": "V1",
                "source_range": null,
                "effects": [],
                "markers": [],
                "enabled": true,
                "kind": "Video",
                "children": [
                    {
                        "OTIO_SCHEMA": "Clip.2",
                        "metadata": {},
                        "name": "Interview",
                        "source_range": range(24.0, 48.0, 24.0),
                        "effects": [],
                        "markers": [],
                        "enabled": true,
                        "media_references": reference("A001", "file:///media/A001.mov"),
                        "active_media_reference_key": "DEFAULT_MEDIA",
                    },
                    {
                        "OTIO_SCHEMA": "Transition.1",
                        "metadata": {},
                        "name": "",
                        "transition_type": "SMPTE_Dissolve",
                        "in_offset": rt(6.0, 24.0),
                        "out_offset": rt(7.0, 24.0),
                    },
                    {
                        "OTIO_SCHEMA": "Clip.2",
                        "metadata": {},
                        "name": "Broll",
                        "source_range": range(0.0, 24.0, 24.0),
                        "effects": [{
                            "OTIO_SCHEMA": "LinearTimeWarp.1",
                            "metadata": {},
                            "name": "",
                            "effect_name": "LinearTimeWarp",
                            "time_scalar": -2.0,
                        }],
                        "markers": [],
                        "enabled": true,
                        "media_references": reference("B002", "file:///media/B002.mov"),
                        "active_media_reference_key": "DEFAULT_MEDIA",
                    },
                ],
            }],
        },
    });
    assert_eq!(value, expected);

    // A thirteen-frame dissolve splits unevenly around its cut and reads
    // back whole, unreported.
    let read = read(&text, R24, "fallback").expect("reads");
    assert_eq!(read.skipped, Vec::<String>::new());
    assert_eq!(read.cut, original);
}

#[test]
fn picture_sound_is_written_as_an_audio_track_after_the_video_tracks() {
    let interview = Event {
        sound: true,
        dissolve: Some(4),
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, R24))
    };
    let original = cut(
        "Sound",
        R24,
        vec![
            lane(LaneKind::Video, "V1", vec![interview]),
            lane(
                LaneKind::Audio,
                "A1",
                vec![event("music", 0, 24, "/m.wav", at(0, R24))],
            ),
        ],
        Vec::new(),
    );
    let (text, _) = written(write, &original);
    let value: Value = serde_json::from_str(&text).expect("JSON");
    let tracks: Vec<(&str, &str, usize)> = value["tracks"]["children"]
        .as_array()
        .expect("tracks")
        .iter()
        .map(|track| {
            (
                track["name"].as_str().expect("name"),
                track["kind"].as_str().expect("kind"),
                track["children"].as_array().expect("items").len(),
            )
        })
        .collect();
    assert_eq!(
        tracks,
        [
            ("V1", "Video", 1),
            ("V1 sound", "Audio", 1),
            ("A1", "Audio", 1)
        ]
    );
}

#[test]
fn every_marker_color_reads_back() {
    let marks: Vec<_> = MarkerColor::ALL
        .iter()
        .enumerate()
        .map(|(index, color)| mark(index as i64, &format!("{color:?}"), "", Some(*color)))
        .collect();
    let original = cut("Colors", R24, Vec::new(), marks);
    let (text, _) = written(write, &original);
    assert_eq!(
        read(&text, R24, "x").expect("reads").cut.marks,
        original.marks
    );
}

#[test]
fn a_timeline_from_another_editor_reads_with_what_it_leaves_out_reported() {
    let file = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": "Resolve cut",
        "global_start_time": rt(90000.0, 25.0),
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "markers": [{
                "OTIO_SCHEMA": "Marker.2",
                "name": "Top",
                "comment": "Note",
                "color": "MAGENTA",
                "marked_range": range(50.0, 0.0, 25.0),
            }],
            "children": [
                {
                    "OTIO_SCHEMA": "Track.1",
                    "name": "Video 1",
                    "kind": "Video",
                    "children": [
                        { "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 25.0, 25.0) },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "A001.mov",
                            "source_range": range(90025.0, 50.0, 25.0),
                            "media_reference": {
                                "OTIO_SCHEMA": "ExternalReference.1",
                                "target_url": "file:///media/A001.mov",
                                "available_range": range(90000.0, 1000.0, 25.0),
                            },
                            "markers": [{
                                "OTIO_SCHEMA": "Marker.2",
                                "name": "Clip mark",
                                "color": "PINK",
                                "marked_range": range(90035.0, 0.0, 25.0),
                            }],
                            "effects": [{ "OTIO_SCHEMA": "Effect.1", "effect_name": "Color Corrector" }],
                        },
                        {
                            "OTIO_SCHEMA": "Transition.1",
                            "in_offset": rt(0.0, 25.0),
                            "out_offset": rt(10.0, 25.0),
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "Back",
                            "source_range": range(0.0, 25.0, 25.0),
                            "media_reference": { "OTIO_SCHEMA": "ExternalReference.1", "target_url": "B002.mov" },
                            "effects": [{ "OTIO_SCHEMA": "LinearTimeWarp.1", "time_scalar": -1.0 }],
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "Still",
                            "source_range": range(0.0, 25.0, 25.0),
                            "effects": [{ "OTIO_SCHEMA": "FreezeFrame.1" }],
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "Off",
                            "enabled": false,
                            "source_range": range(0.0, 25.0, 25.0),
                        },
                        {
                            "OTIO_SCHEMA": "Stack.1",
                            "children": [{
                                "OTIO_SCHEMA": "Track.1",
                                "children": [{ "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 25.0, 25.0) }],
                            }],
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "Missing",
                            "source_range": range(0.0, 25.0, 25.0),
                            "media_reference": { "OTIO_SCHEMA": "MissingReference.1" },
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "Gen",
                            "source_range": range(0.0, 25.0, 25.0),
                            "media_reference": { "OTIO_SCHEMA": "GeneratorReference.1" },
                        },
                    ],
                },
                { "OTIO_SCHEMA": "Track.1", "name": "", "kind": "Data", "children": [] },
                { "OTIO_SCHEMA": "Stack.1", "children": [] },
                {
                    "OTIO_SCHEMA": "Track.1",
                    "name": "Audio 1",
                    "kind": "Audio",
                    "children": [
                        { "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 25.0, 25.0) },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "A001.mov",
                            "source_range": range(90025.0, 50.0, 25.0),
                            "media_reference": {
                                "OTIO_SCHEMA": "ExternalReference.1",
                                "target_url": "file:///media/A001.mov",
                                "available_range": range(90000.0, 1000.0, 25.0),
                            },
                        },
                    ],
                },
            ],
        },
    });
    let read = read(&file.to_string(), R24, "fallback").expect("reads");

    let r25 = Rational::FPS_25;
    let a001 = Event {
        sound: true,
        ..event("A001.mov", 25, 50, "/media/A001.mov", at(25, r25))
    };
    let back = Event {
        reversed: true,
        dissolve: Some(10),
        ..event("Back", 75, 25, "B002.mov", at(0, r25))
    };
    let missing = Event {
        source: Source {
            reel: "Missing".to_owned(),
            path: None,
            start: at(0, r25),
        },
        ..event("Missing", 175, 25, "", at(0, r25))
    };
    let expected = cut(
        "Resolve cut",
        r25,
        vec![lane(LaneKind::Video, "Video 1", vec![a001, back, missing])],
        vec![
            mark(50, "Top", "Note", Some(MarkerColor::Purple)),
            mark(35, "Clip mark", "", Some(MarkerColor::Pink)),
        ],
    );
    assert_eq!(read.cut, expected);
    assert_eq!(
        read.skipped,
        [
            "Video 1 A001.mov: effect Color Corrector, not read",
            "Video 1: a transition off its cut, centered on it",
            "Video 1 Still: a freeze frame, not read",
            "Video 1 Off: disabled, not read",
            "Video 1: a nested Stack, which is not read",
            "Video 1 Gen: a GeneratorReference, which is not read",
            "track 2: kind \"Data\" is not read",
            "track 3: a nested Stack, which is not read",
        ]
    );
}

#[test]
fn a_transition_two_frames_off_its_cut_is_reported() {
    let item = |name: &str| {
        json!({
            "OTIO_SCHEMA": "Clip.2",
            "name": name,
            "source_range": range(0.0, 24.0, 24.0),
        })
    };
    for (before, after, reported) in [
        (6.0, 7.0, false),
        (7.0, 6.0, false),
        (6.0, 8.0, true),
        (0.0, 12.0, true),
    ] {
        let file = timeline(json!([{
            "OTIO_SCHEMA": "Track.1",
            "name": "V1",
            "kind": "Video",
            "children": [
                item("A"),
                { "OTIO_SCHEMA": "Transition.1", "in_offset": rt(before, 24.0), "out_offset": rt(after, 24.0) },
                item("B"),
            ],
        }]));
        let read = read(&file.to_string(), R24, "x").expect("reads");
        let off = ["V1: a transition off its cut, centered on it".to_owned()];
        let expected: &[String] = if reported { &off } else { &[] };
        assert_eq!(read.skipped, expected, "{before} + {after}");
        let dissolve = read.cut.lanes[0].events[1].dissolve;
        assert_eq!(
            dissolve,
            Some((before + after) as i64),
            "{before} + {after}"
        );
    }
}

#[test]
fn the_rate_comes_from_the_start_time_then_the_first_item_then_the_caller() {
    let gap = |rate: f64| {
        json!([{
            "OTIO_SCHEMA": "Track.1",
            "kind": "Video",
            "children": [{ "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 10.0, rate) }],
        }])
    };
    let mut stated = timeline(gap(30.0));
    stated["global_start_time"] = rt(0.0, 23.976_023_976_023_978);
    assert_eq!(
        read(&stated.to_string(), R24, "x").expect("reads").cut.rate,
        Rational::FPS_23_976
    );

    let mut unstated = timeline(gap(30.0));
    unstated["global_start_time"] = Value::Null;
    assert_eq!(
        read(&unstated.to_string(), R24, "x")
            .expect("reads")
            .cut
            .rate,
        Rational::FPS_30
    );

    let mut empty = timeline(json!([]));
    empty
        .as_object_mut()
        .expect("object")
        .remove("global_start_time");
    assert_eq!(
        read(&empty.to_string(), R24, "x").expect("reads").cut.rate,
        R24
    );
}

#[test]
fn an_unnamed_timeline_and_track_take_the_callers_name_and_a_number() {
    let mut file = timeline(json!([
        { "OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [] },
        { "OTIO_SCHEMA": "Track.1", "name": "", "kind": "Audio", "children": [] },
    ]));
    file["name"] = json!("");
    let read = read(&file.to_string(), R24, "Cut file").expect("reads");
    assert_eq!(read.cut.name, "Cut file");
    let names: Vec<&str> = read
        .cut
        .lanes
        .iter()
        .map(|lane| lane.name.as_str())
        .collect();
    // The empty audio lane is dropped; the video lane stays.
    assert_eq!(names, ["Track 1"]);
}

#[test]
fn a_clip_plays_its_active_media_reference() {
    let file = timeline(json!([{
        "OTIO_SCHEMA": "Track.1",
        "kind": "Video",
        "children": [{
            "OTIO_SCHEMA": "Clip.2",
            "name": "Shot",
            "source_range": range(0.0, 24.0, 24.0),
            "media_references": {
                "DEFAULT_MEDIA": { "OTIO_SCHEMA": "ExternalReference.1", "target_url": "file:///proxy.mov" },
                "high": { "OTIO_SCHEMA": "ExternalReference.1", "name": "Hi", "target_url": "file:///full%20res.mov" },
            },
            "active_media_reference_key": "high",
        }],
    }]));
    let read = read(&file.to_string(), R24, "x").expect("reads");
    let source = &read.cut.lanes[0].events[0].source;
    assert_eq!(source.reel, "Hi");
    assert_eq!(
        source.path.as_deref(),
        Some(std::path::Path::new("/full res.mov"))
    );
}

#[test]
fn malformed_files_are_refused_with_the_reason() {
    let cases = [
        ("not JSON", "{".to_owned(), "the file is not JSON"),
        (
            "not a timeline",
            json!({ "OTIO_SCHEMA": "Clip.2" }).to_string(),
            "the file holds no OpenTimelineIO timeline",
        ),
        (
            "no stack",
            json!({ "OTIO_SCHEMA": "Timeline.1" }).to_string(),
            "the timeline holds no track stack",
        ),
        (
            "a zero rate",
            timeline(json!([{
                "OTIO_SCHEMA": "Track.1",
                "kind": "Video",
                "children": [{ "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 10.0, 0.0) }],
            }]))
            .to_string(),
            "a time with no finite value or positive rate",
        ),
        (
            "a negative duration",
            timeline(json!([{
                "OTIO_SCHEMA": "Track.1",
                "kind": "Video",
                "children": [{ "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, -10.0, 24.0) }],
            }]))
            .to_string(),
            "a negative duration",
        ),
        (
            "a clip with no range",
            timeline(json!([{
                "OTIO_SCHEMA": "Track.1",
                "kind": "Video",
                "children": [{ "OTIO_SCHEMA": "Clip.2", "name": "Shot", "source_range": range(0.0, 24.0, 24.0) }, {
                    "OTIO_SCHEMA": "Stack.1",
                    "children": [{ "OTIO_SCHEMA": "Clip.2", "name": "Inner" }],
                }],
            }]))
            .to_string(),
            "a clip with no range",
        ),
    ];
    for (label, text, reason) in cases {
        let err = read(&text, R24, "x").expect_err(label);
        assert!(err.starts_with(reason), "{label}: {err}");
    }
}

#[test]
fn stacks_nested_past_the_limit_are_refused_rather_than_walked() {
    let mut nested = json!({ "OTIO_SCHEMA": "Gap.1", "source_range": range(0.0, 1.0, 24.0) });
    for _ in 0..40 {
        nested = json!({ "OTIO_SCHEMA": "Stack.1", "children": [nested] });
    }
    let file =
        timeline(json!([{ "OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [nested] }]));
    assert_eq!(
        read(&file.to_string(), R24, "x"),
        Err("stacks nest deeper than 32".to_owned())
    );
}
