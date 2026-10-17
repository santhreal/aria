//! CMX 3600: what a reader takes from an edit list as editors write them,
//! what it reports and leaves out, the exact text the writer produces, and
//! that what the writer produces reads back as the cut it came from.
//!
//! Not caught here: an EDL at a rate other than the timeline's. The format
//! states no rate, so the reader takes the caller's.

use aria_models::{MarkerColor, Rational};

use super::{Reels, read, write};
use crate::model::{Event, LaneKind, Read, Source};
use crate::test_support::{R24, at, cut, event, lane, mark, written};

/// An edit list as Premiere writes one: a cut, a dissolve into a shot at
/// double speed, a separate sound event and a marker.
const PREMIERE: &str = "\
TITLE: Sequence 01
FCM: NON-DROP FRAME

001  A001C003 V     C        00:00:10:00 00:00:12:00 01:00:00:00 01:00:02:00
* FROM CLIP NAME: Interview
* SOURCE FILE: /media/A001C003.mov

002  A001C003 V     C        00:00:12:00 00:00:12:00 01:00:02:00 01:00:02:00
002  B002     V     D    012 00:00:05:00 00:00:06:00 01:00:02:00 01:00:03:00
M2   B002          048.0                00:00:05:00
* FROM CLIP NAME: Interview
* TO CLIP NAME: Broll
* SOURCE FILE: B002.mov

003  B002     AA    C        00:00:05:00 00:00:08:00 01:00:00:00 01:00:03:00
* FROM CLIP NAME: Broll sound

* LOC: 01:00:01:00 GREEN   Check focus
";

#[test]
fn an_edit_list_reads_as_its_events_dissolves_speeds_and_markers() {
    let read = read(PREMIERE, R24, "fallback").expect("reads");

    let broll = Event {
        speed: Rational::new(2, 1),
        dissolve: Some(12),
        ..event("Broll", 48, 24, "B002.mov", at(120, R24))
    };
    let sound = Event {
        sound: true,
        ..event("Broll sound", 0, 72, "B002.mov", at(120, R24))
    };
    let expected = cut(
        "Sequence 01",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "V1",
                vec![
                    event("Interview", 0, 48, "/media/A001C003.mov", at(240, R24)),
                    broll,
                ],
            ),
            lane(LaneKind::Audio, "A1", vec![sound]),
        ],
        vec![mark(24, "Check focus", "", Some(MarkerColor::Green))],
    );
    assert_eq!(
        read,
        Read {
            cut: expected,
            skipped: Vec::new()
        }
    );
}

#[test]
fn a_title_line_names_the_cut_and_its_absence_takes_the_callers_name() {
    let body = "001  A001     V     C        00:00:00:00 00:00:01:00 01:00:00:00 01:00:01:00\n";
    let titled = read(&format!("TITLE: Promo\n{body}"), R24, "fallback").expect("reads");
    assert_eq!(titled.cut.name, "Promo");
    let untitled = read(body, R24, "fallback").expect("reads");
    assert_eq!(untitled.cut.name, "fallback");
}

#[test]
fn sound_of_the_same_take_folds_into_the_picture() {
    let text = "\
001  A001     V     C        00:00:00:00 00:00:02:00 01:00:00:00 01:00:02:00
002  A001     A     C        00:00:00:00 00:00:02:00 01:00:00:00 01:00:02:00
003  A001     A2    C        00:00:00:00 00:00:02:00 01:00:00:00 01:00:02:00
004  B002     B     C        00:00:00:00 00:00:01:00 01:00:02:00 01:00:03:00
005  C003     A3    C        00:00:00:00 00:00:01:00 01:00:02:00 01:00:03:00
";
    let read = read(text, R24, "Cut").expect("reads");

    let reel = |reel: &str, start, duration, sound| Event {
        name: reel.to_owned(),
        start,
        duration,
        source: Source {
            reel: reel.to_owned(),
            path: None,
            start: at(0, R24),
        },
        speed: Rational::new(1, 1),
        reversed: false,
        dissolve: None,
        sound,
    };
    assert_eq!(
        read.cut.lanes,
        [
            lane(
                LaneKind::Video,
                "V1",
                vec![reel("A001", 0, 48, true), reel("B002", 48, 24, true)]
            ),
            lane(LaneKind::Audio, "A3", vec![reel("C003", 48, 24, true)]),
        ]
    );
    assert!(read.skipped.is_empty(), "{:?}", read.skipped);
}

#[test]
fn drop_frame_counts_the_minute_boundary_as_one_frame() {
    let text = "\
TITLE: Drop
FCM: DROP FRAME
001  A001     V     C        00:00:00;00 00:00:00;03 01:00:59;29 01:01:00;02
";
    let read = read(text, Rational::new(30, 1), "Cut").expect("reads");

    assert_eq!(read.cut.rate, Rational::FPS_29_97);
    let event = &read.cut.lanes[0].events[0];
    // 01:01:00;00 and ;01 do not exist, so ;02 follows ;29 of the minute before.
    assert_eq!((event.start, event.duration), (1799, 1));
    assert_eq!(event.source.start, at(0, Rational::FPS_29_97));
}

#[test]
fn non_drop_frame_keeps_the_callers_rate() {
    let text = "\
FCM: NON-DROP FRAME
001  A001     V     C        00:00:00:00 00:00:00:03 01:00:59:29 01:01:00:02
";
    let read = read(text, Rational::new(30, 1), "Cut").expect("reads");
    assert_eq!(read.cut.rate, Rational::new(30, 1));
    let event = &read.cut.lanes[0].events[0];
    assert_eq!((event.start, event.duration), (1799, 3));
}

#[test]
fn lines_and_events_it_cannot_read_are_reported_by_line_and_number() {
    let text = "\
TITLE: Reports
001  A001     V     C        00:00:00:00 00:00:01:00 01:00:00:00 01:00:01:00
002  A001     V     K B      00:00:01:00 00:00:02:00 01:00:01:00 01:00:02:00
garbage line
003  A001     X     C        00:00:00:00 00:00:01:00 01:00:01:00 01:00:02:00
004  BL       V     C        00:00:00:00 00:00:01:00 01:00:02:00 01:00:03:00
005  A001     V     C        00:00:03:00 00:00:04:00 01:00:03:00 01:00:04:00
M2   ZZZ      048.0   00:00:00:00
006  C003     V     C        00:00:00:00 00:00:01:00 01:00:04:00 01:00:05:00
M2   C003     000.0   00:00:00:00
007  BL       V     C        00:00:00:00 00:00:00:00 01:00:05:00 01:00:05:00
007  D004     V     D    010 00:00:00:00 00:00:01:00 01:00:05:00 01:00:06:00
008  D004     V     C        00:00:01:00 00:00:01:00 01:00:06:00 01:00:06:00
008  E005     V     W001 010 00:00:00:00 00:00:01:00 01:00:06:00 01:00:07:00
009  A001     V     C        00:00:00:00 00:00:01:00 01:00:07:00
010  A001     V     C        00:00:61:00 00:00:62:00 01:00:07:00 01:00:08:00
011  A001     V     D        00:00:00:00 00:00:01:00 01:00:08:00 01:00:09:00
";
    let read = read(text, R24, "Cut").expect("reads");

    assert_eq!(
        read.skipped,
        [
            "line 3: a key, which is not read",
            "line 4: \"garbage line\" is not read",
            "line 5: channels \"X\" are not read",
            "line 8: M2 names reel \"ZZZ\", which no event before it reads",
            "line 15: an event line holds eight or nine fields, not 7",
            "line 16: \"00:00:61:00\" is out of range at 24 frames a second",
            "line 17: transition \"D\" has no length",
            "event 006: a freeze frame, which is not read",
            "event 007: a dissolve from black, read as a cut",
            "event 008: a wipe, read as a dissolve",
        ]
    );
    let placed: Vec<(&str, i64, i64, Option<i64>)> = read.cut.lanes[0]
        .events
        .iter()
        .map(|event| {
            (
                event.source.reel.as_str(),
                event.start,
                event.duration,
                event.dissolve,
            )
        })
        .collect();
    // Black and the unreadable events leave gaps.
    assert_eq!(
        placed,
        [
            ("A001", 0, 24, None),
            ("A001", 72, 24, None),
            ("D004", 120, 24, None),
            ("E005", 144, 24, Some(10)),
        ]
    );
}

#[test]
fn a_file_with_no_events_or_markers_is_not_an_edit_list() {
    for text in ["", "TITLE: Empty\nFCM: NON-DROP FRAME\n", "hello\n"] {
        assert_eq!(
            read(text, R24, "Cut"),
            Err("the file holds no EDL events".to_owned()),
            "{text:?}"
        );
    }
}

#[test]
fn markers_alone_read_as_a_cut_with_no_lanes() {
    let read = read("* LOC: 00:00:01:00 Plain name\n", R24, "Cut").expect("reads");
    assert!(read.cut.lanes.is_empty());
    assert_eq!(read.cut.marks, [mark(24, "Plain name", "", None)]);
}

/// The cut the writer suites write: a picture with its sound, a shot at
/// double speed dissolving in, music, and a marker.
fn promo() -> crate::model::Cut {
    let interview = Event {
        sound: true,
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, R24))
    };
    let broll = Event {
        speed: Rational::new(2, 1),
        dissolve: Some(12),
        ..event("Broll", 48, 24, "/media/B002.mov", at(0, R24))
    };
    let music = Event {
        sound: true,
        ..event("music", 0, 72, "/media/music.wav", at(0, R24))
    };
    cut(
        "Promo",
        R24,
        vec![
            lane(LaneKind::Video, "V1", vec![interview, broll]),
            lane(LaneKind::Audio, "A1", vec![music]),
        ],
        vec![mark(24, "Check", "", Some(MarkerColor::Green))],
    )
}

#[test]
fn the_writer_produces_cmx_3600_text() {
    let (text, skipped) = written(write, &promo());
    let expected = "\
TITLE: Promo
FCM: NON-DROP FRAME

001  A001     B     C        00:00:01:00 00:00:03:00 01:00:00:00 01:00:02:00
* FROM CLIP NAME: Interview
* SOURCE FILE: /media/A001.mov
002  A001     V     C        00:00:03:00 00:00:03:00 01:00:02:00 01:00:02:00
002  B002     V     D    012 00:00:00:00 00:00:01:00 01:00:02:00 01:00:03:00
M2   B002           048.0                00:00:00:00
* FROM CLIP NAME: Interview
* TO CLIP NAME: Broll
* SOURCE FILE: /media/B002.mov
003  music    A     C        00:00:00:00 00:00:03:00 01:00:00:00 01:00:03:00
* FROM CLIP NAME: music
* SOURCE FILE: /media/music.wav
* LOC: 01:00:01:00 GREEN   Check
";
    assert_eq!(text, expected);
    assert!(skipped.is_empty(), "{skipped:?}");
}

#[test]
fn what_the_writer_produces_reads_back_as_the_cut() {
    let cases = [
        ("24 fps", R24, R24),
        ("25 fps", Rational::FPS_25, Rational::FPS_25),
        // The file states drop-frame; the reader takes 29.97 from 30.
        ("29.97 drop", Rational::FPS_29_97, Rational::new(30, 1)),
        ("59.94 drop", Rational::FPS_59_94, Rational::new(60, 1)),
    ];
    for (label, rate, timeline) in cases {
        let mut original = promo();
        original.rate = rate;
        for event in original.lanes.iter_mut().flat_map(|lane| &mut lane.events) {
            event.source.start.rate = rate;
        }
        let (text, skipped) = written(write, &original);
        assert!(skipped.is_empty(), "{label}: {skipped:?}");

        let read = read(&text, timeline, "fallback").expect("reads");
        assert_eq!(
            read,
            Read {
                cut: original,
                skipped: Vec::new()
            },
            "{label}"
        );
    }
}

#[test]
fn drop_frame_rates_are_written_drop_frame() {
    let mut drop = promo();
    drop.rate = Rational::FPS_29_97;
    let (text, _) = written(write, &drop);
    assert!(
        text.starts_with("TITLE: Promo\nFCM: DROP FRAME\n"),
        "{text}"
    );
    assert!(
        text.contains("01:00:00;00 01:00:01;18"),
        "record timecode counts drop-frame: {text}"
    );
}

#[test]
fn reversed_events_are_written_with_a_negative_speed_and_read_back() {
    let reversed = Event {
        reversed: true,
        ..event("Back", 0, 24, "/media/A001.mov", at(48, R24))
    };
    let original = cut(
        "Reverse",
        R24,
        vec![lane(LaneKind::Video, "V1", vec![reversed])],
        Vec::new(),
    );
    let (text, _) = written(write, &original);
    assert!(text.contains("M2   A001           -24.0"), "{text}");
    assert_eq!(read(&text, R24, "x").expect("reads").cut, original);
}

#[test]
fn source_frames_are_written_at_the_record_rate() {
    // Sound counted in milliseconds: 1500 ms is 36 frames at 24 fps.
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            24,
            "/media/music.wav",
            at(1500, Rational::new(1000, 1)),
        )
    };
    let original = cut(
        "Music",
        R24,
        vec![lane(LaneKind::Audio, "A1", vec![music])],
        Vec::new(),
    );
    let (text, _) = written(write, &original);
    assert!(
        text.contains(
            "001  music    A     C        00:00:01:12 00:00:02:12 01:00:00:00 01:00:01:00"
        ),
        "{text}"
    );
}

#[test]
fn what_an_edl_cannot_hold_is_reported() {
    let clip = |name: &str, start| event(name, start, 24, "/media/A001.mov", at(0, R24));
    let lone = Event {
        dissolve: Some(12),
        ..clip("Lone", 48)
    };
    let mut lanes = vec![
        lane(LaneKind::Video, "V1", vec![clip("First", 0), lone]),
        lane(LaneKind::Video, "V2", vec![clip("Logo", 0)]),
        lane(LaneKind::Video, "V3", Vec::new()),
    ];
    for number in 1..=5 {
        let name = format!("A{number}");
        lanes.push(lane(LaneKind::Audio, &name, vec![clip(&name, 0)]));
    }
    lanes.push(lane(LaneKind::Audio, "A6", Vec::new()));
    let marks = vec![
        mark(0, "One", "a note", None),
        mark(1, "Two", "", None),
        mark(2, "Three", "another", None),
    ];
    let (text, skipped) = written(write, &cut("Too much", R24, lanes, marks));

    assert_eq!(
        skipped,
        [
            "V2: an EDL holds one video track",
            "A5: an EDL holds four audio channels",
            "V1 Lone: a dissolve with no clip before it, written as a cut",
            "2 marker notes: an EDL marker holds a name only",
        ]
    );
    assert!(!text.contains("Logo"), "{text}");
    assert!(text.contains("005  A001     A3    C"), "{text}");
    assert!(
        text.contains("* LOC: 01:00:00:00 RED     One"),
        "unset color: {text}"
    );
}

/// A timeline keeps empty lanes, and an empty first lane took the picture or
/// a channel: the picture was dropped as a second video track, and a fourth
/// populated sound lane as a fifth channel.
#[test]
fn an_empty_lane_takes_no_slot() {
    let clip = |name: &str| event(name, 0, 24, "/media/A001.mov", at(0, R24));
    let mut lanes = vec![
        lane(LaneKind::Video, "V1", Vec::new()),
        lane(LaneKind::Video, "V2", vec![clip("Shot")]),
        lane(LaneKind::Audio, "A1", Vec::new()),
    ];
    for number in 2..=5 {
        let name = format!("A{number}");
        lanes.push(lane(LaneKind::Audio, &name, vec![clip(&name)]));
    }
    let (text, skipped) = written(write, &cut("Sparse", R24, lanes, Vec::new()));

    assert_eq!(skipped, Vec::<String>::new());
    for line in [
        "001  A001     V     C        00:00:00:00 00:00:01:00 01:00:00:00 01:00:01:00",
        "002  A001     A     C",
        "005  A001     A4    C",
    ] {
        assert!(text.contains(line), "{line:?} missing from\n{text}");
    }
}

#[test]
fn one_marker_note_is_reported_in_the_singular() {
    let marks = vec![mark(0, "One", "a note", None)];
    let (_, skipped) = written(write, &cut("Note", R24, Vec::new(), marks));
    assert_eq!(skipped, ["1 marker note: an EDL marker holds a name only"]);
}

#[test]
fn a_thousand_events_are_reported_past_three_digits() {
    let events = (0..1000)
        .map(|index| event("Shot", index, 1, "/media/A001.mov", at(0, R24)))
        .collect();
    let (text, skipped) = written(
        write,
        &cut(
            "Long",
            R24,
            vec![lane(LaneKind::Video, "V1", events)],
            Vec::new(),
        ),
    );
    assert_eq!(
        skipped,
        ["1000 events: numbers past 999 have four digits, which strict CMX 3600 readers refuse"]
    );
    assert!(text.contains("\n999  A001 "), "{text}");
    assert!(text.contains("\n1000  A001 "), "{text}");
}

#[test]
fn names_and_markers_are_written_on_one_line() {
    let named = event("Two\nlines\tand tab", 0, 24, "/media/A001.mov", at(0, R24));
    let original = cut(
        "Title\r\nbreak",
        R24,
        vec![lane(LaneKind::Video, "V1", vec![named])],
        vec![mark(0, "Mark\nname", "", Some(MarkerColor::Blue))],
    );
    let (text, _) = written(write, &original);
    assert!(text.starts_with("TITLE: Title  break\n"), "{text}");
    assert!(
        text.contains("* FROM CLIP NAME: Two lines and tab\n"),
        "{text}"
    );
    assert!(
        text.contains("* LOC: 01:00:00:00 BLUE    Mark name\n"),
        "{text}"
    );
}

fn source(reel: &str, path: Option<&str>) -> Source {
    Source {
        reel: reel.to_owned(),
        path: path.map(Into::into),
        start: at(0, R24),
    }
}

#[test]
fn reels_are_eight_characters_one_per_file() {
    let mut reels = Reels::default();
    let names: Vec<String> = [
        source("x", Some("/a/Interview_A.mov")),
        source("x", Some("/a/Interview_B.mov")),
        source("x", Some("/a/Interview_A.mov")),
        source("x", Some("/a/clip-1 (copy).mov")),
        source("x", Some("/a/BL.mov")),
        source("x", Some("/a/.mov")),
        source("Tape 7", None),
        source("Tape 7", None),
    ]
    .iter()
    .map(|source| reels.reel(source))
    .collect();
    assert_eq!(
        names,
        [
            "Intervie", "Intervi2", "Intervie", "clip_1__", "BLAX", "_mov", "Tape_7", "Tape_7"
        ]
    );
}

#[test]
fn colliding_reels_are_numbered_within_eight_characters() {
    let mut reels = Reels::default();
    let names: Vec<String> = (0..11)
        .map(|index| {
            reels.reel(&source(
                "x",
                Some(format!("/a/Interview_{index}.mov").as_str()),
            ))
        })
        .collect();
    assert_eq!(
        names,
        [
            "Intervie", "Intervi2", "Intervi3", "Intervi4", "Intervi5", "Intervi6", "Intervi7",
            "Intervi8", "Intervi9", "Interv10", "Interv11",
        ]
    );
}
