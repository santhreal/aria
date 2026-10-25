//! Subtitle files read into cues and cues written back, cues placed on new
//! text tracks and read back from the timeline, and every subtitle format
//! driven through the plugin's importer and exporter.
//!
//! Not caught here: how a text clip renders, and the styling a subtitle
//! file holds, which is dropped and only counted.

use aria_commands::EditCommand;
use aria_models::{ClipSource, Generator, Project, Rational, TrackId, TrackKind};
use aria_plugin::{ApplyOutcome, Command, InterchangeError, Origin};

use super::{
    Cue, Cues, SUBTITLE_FORMATS, SubtitleFormat, cues, frames, millis, place, read_srt, read_vtt,
    timestamp, write_srt, write_vtt,
};
use crate::InterchangePlugin;
use crate::test_support::{Session, session, track};

fn cue(start_ms: i64, end_ms: i64, text: &str) -> Cue {
    Cue {
        start_ms,
        end_ms,
        text: text.to_owned(),
    }
}

fn read(cues: &[Cue], skipped: &[&str]) -> Result<Cues, String> {
    Ok(Cues {
        cues: cues.to_vec(),
        skipped: skipped.iter().map(|line| (*line).to_owned()).collect(),
    })
}

/// What `format` writes for `cues`, as text, with what it reported.
fn written(format: &SubtitleFormat, cues: &[Cue]) -> (String, Vec<String>) {
    let mut out = Vec::new();
    let report = (format.write)(cues, &mut out).expect("writes to memory");
    (String::from_utf8(out).expect("UTF-8"), report)
}

/// A session at 24 fps, where every multiple of 125 ms is a whole frame.
fn at_24() -> Session {
    let session = session();
    session.apply(Command::Edit(EditCommand::SetSequenceRate {
        frame_rate: Rational::FPS_24,
    }));
    session
}

fn placed(session: &Session, cues: &[Cue], name: &str) {
    session
        .host()
        .transact_with(Origin::Editor, "Import", |tx| place(cues, name, tx))
        .expect("places the cues");
}

/// Each track of `project`, bottom to top, with its name and clip count.
fn layout(project: &Project) -> Vec<(TrackId, String, usize)> {
    project
        .timeline()
        .tracks_ordered()
        .map(|track| (track.id, track.name.clone(), track.clips_ordered().count()))
        .collect()
}

#[test]
fn srt_reads_numbered_cues_with_lines_and_crlf_endings() {
    let text = "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\nthere\r\n\r\n\
                2\r\n00:00:03,000 --> 00:00:04,000\r\nBye\r\n";
    assert_eq!(
        read_srt(text),
        read(
            &[cue(1000, 2500, "Hello\nthere"), cue(3000, 4000, "Bye")],
            &[]
        )
    );
}

#[test]
fn srt_text_after_a_blank_line_continues_the_cue_and_text_before_any_cue_is_skipped() {
    let text = "Stray words\n\n\
                1\n00:00:01,000 --> 00:00:02,000\nFirst\n\nstill first\n\n\
                2\n00:00:03,000 --> 00:00:04,000\nSecond\n";
    assert_eq!(
        read_srt(text),
        read(
            &[
                cue(1000, 2000, "First\nstill first"),
                cue(3000, 4000, "Second")
            ],
            &["line 1: no cue timing, text skipped"],
        )
    );
}

#[test]
fn srt_markup_is_removed_and_counted_and_text_that_opens_nothing_is_kept() {
    let text = "1\n00:00:01,000 --> 00:00:02,000\n<i>Hi</i> {\\an8}there\n\n\
                2\n00:00:02,000 --> 00:00:03,000\na < b & {curly} &amp; <i\n";
    assert_eq!(
        read_srt(text),
        read(
            &[
                cue(1000, 2000, "Hi there"),
                cue(2000, 3000, "a < b & {curly} &amp; <i"),
            ],
            &["1 cue: styling tags dropped"],
        )
    );
}

#[test]
fn each_malformed_srt_cue_is_skipped_with_its_line() {
    let text = [
        "1",
        "00:00:01,000 -> 00:00:02,000",
        "One arrow",
        "",
        "2",
        "00:00:05,000 --> 00:00:04,000",
        "Backwards",
        "",
        "3",
        "00:00:06,000 --> 00:00:06,000",
        "No span",
        "",
        "4",
        "00:00:07,000 --> 00:00:08,000",
        "<b></b>",
        "",
        "5",
        "00:00:61,000 --> 00:00:62,000",
        "Sixty-one seconds",
        "",
        "6",
        "00:00:09,000 --> 00:00:10,000",
        "Kept",
    ]
    .join("\n");
    assert_eq!(
        read_srt(&text),
        read(
            &[cue(9000, 10_000, "Kept")],
            &[
                "line 1: no cue timing, text skipped",
                "line 6: the cue ends before it starts",
                "line 10: the cue ends before it starts",
                "line 14: the cue has no text",
                "line 18: \"00:00:61,000 --> 00:00:62,000\" is not a cue timing",
            ],
        )
    );
}

#[test]
fn a_file_with_no_readable_cue_is_refused_with_the_first_reason() {
    let refused: [(fn(&str) -> Result<Cues, String>, &str, &str); 8] = [
        (read_srt, "", "the file holds no cues"),
        (read_srt, "\n \n\t\n", "the file holds no cues"),
        (
            read_srt,
            "1\n00:00:02,000 --> 00:00:01,000\nx\n",
            "no cue reads; line 2: the cue ends before it starts",
        ),
        (read_vtt, "", "the file is empty"),
        (read_vtt, "WEBVTT\n", "the file holds no cues"),
        (
            read_vtt,
            "WEBVTTX\n\n00:01.000 --> 00:02.000\nx\n",
            "the file does not start with WEBVTT",
        ),
        (
            read_vtt,
            "1\n00:00:01,000 --> 00:00:02,000\nx\n",
            "the file does not start with WEBVTT",
        ),
        (
            read_vtt,
            "WEBVTT\n\nNOTE only a comment\n",
            "the file holds no cues",
        ),
    ];
    for (read, text, reason) in refused {
        assert_eq!(read(text), Err(reason.to_owned()), "{text:?}");
    }
}

#[test]
fn vtt_reads_ids_settings_entities_and_short_times_and_passes_over_other_blocks() {
    let text = "WEBVTT - Title\nKind: captions\n\n\
                NOTE a comment\nspanning lines\n\n\
                STYLE\n::cue { color: red }\n\n\
                intro\n00:01.000 --> 00:02.500 align:start position:10%\n\
                <v Roger>Tom &amp; Jerry &lt;3&gt;\n\n\
                REGION\nid:fred\n\n\
                00:00:03.000 --> 00:00:04.000\nPlain &unknown; text &#65;&#x42;\n";
    assert_eq!(
        read_vtt(text),
        read(
            &[
                cue(1000, 2500, "Tom & Jerry <3>"),
                cue(3000, 4000, "Plain &unknown; text AB"),
            ],
            &[
                "2 style or region blocks dropped",
                "1 cue: styling tags dropped",
                "1 cue: position and alignment settings dropped",
            ],
        )
    );
}

#[test]
fn a_vtt_cue_inside_the_header_is_reported_and_the_rest_read() {
    let text = "WEBVTT\n00:01.000 --> 00:02.000\nToo early\n\n00:03.000 --> 00:04.000\nOn time\n";
    assert_eq!(
        read_vtt(text),
        read(
            &[cue(3000, 4000, "On time")],
            &["line 2: a cue before the blank line that ends the header, skipped"],
        )
    );
}

#[test]
fn timestamps_read_to_the_millisecond_within_their_bounds() {
    let cases: [(&str, Option<i64>); 18] = [
        ("00:00:01,000", Some(1000)),
        ("00:00:01.5", Some(1500)),
        ("00:00:01.05", Some(1050)),
        ("01:02:03.004", Some(3_723_004)),
        ("1:00:00.000", Some(3_600_000)),
        ("02:03.004", Some(123_004)),
        ("99999:59:59.999", Some(359_999_999_999)),
        ("100000:00:00.000", None),
        ("00:60:00.000", None),
        ("00:00:60.000", None),
        ("00:000:01.000", None),
        ("00:00:01.0000", None),
        ("00:00:01", None),
        ("00:00:01.", None),
        ("1:02:03:04.000", None),
        ("00:00:0a.000", None),
        ("-1:00:00.000", None),
        ("", None),
    ];
    for (text, ms) in cases {
        assert_eq!(timestamp(text), ms, "{text:?}");
    }
}

#[test]
fn writers_produce_the_exact_text_of_each_format() {
    let cues = [
        cue(0, 1500, "One"),
        cue(3_723_004, 3_724_000, "Two\nlines & <tags>"),
    ];
    let mut srt = Vec::new();
    assert_eq!(
        write_srt(&cues, &mut srt).expect("writes"),
        Vec::<String>::new()
    );
    assert_eq!(
        String::from_utf8(srt).expect("UTF-8"),
        "1\n00:00:00,000 --> 00:00:01,500\nOne\n\n\
         2\n01:02:03,004 --> 01:02:04,000\nTwo\nlines & <tags>\n\n"
    );
    let mut vtt = Vec::new();
    assert_eq!(
        write_vtt(&cues, &mut vtt).expect("writes"),
        Vec::<String>::new()
    );
    assert_eq!(
        String::from_utf8(vtt).expect("UTF-8"),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nOne\n\n\
         01:02:03.004 --> 01:02:04.000\nTwo\nlines &amp; &lt;tags&gt;\n\n"
    );
}

#[test]
fn every_format_leaves_out_and_counts_blank_lines_inside_cue_text() {
    let cues = [
        cue(0, 1000, "A\n\nB"),
        cue(1000, 2000, "C\n \nD"),
        cue(2000, 3000, "E"),
    ];
    for format in &SUBTITLE_FORMATS {
        let (text, report) = written(format, &cues);
        assert_eq!(
            report,
            ["2 cues: blank lines inside the text are not written"],
            "{}",
            format.id
        );
        let back = (format.read)(&text).expect("reads back");
        assert_eq!(
            back.cues,
            [
                cue(0, 1000, "A\nB"),
                cue(1000, 2000, "C\nD"),
                cue(2000, 3000, "E")
            ],
            "{}",
            format.id
        );
    }
}

#[test]
fn every_format_reads_back_what_it_writes() {
    let cues = [
        cue(0, 1, "From the first millisecond"),
        cue(999, 61_001, "a < b > c & d &amp; e"),
        cue(3_599_999, 3_600_000, "Two\nlines --> here"),
        cue(360_000_000, 360_000_500, "A hundred hours in: é 字"),
        cue(
            359_999_999_998,
            359_999_999_999,
            "The last time a field holds",
        ),
    ];
    for format in &SUBTITLE_FORMATS {
        let (text, report) = written(format, &cues);
        assert!(report.is_empty(), "{}: {report:?}", format.id);
        assert_eq!((format.read)(&text), read(&cues, &[]), "{}", format.id);
    }
}

#[test]
fn frames_and_milliseconds_round_to_the_nearest_and_a_frame_survives_both() {
    assert_eq!(frames(1000, Rational::FPS_24), 24);
    assert_eq!(frames(20, Rational::FPS_24), 0);
    assert_eq!(frames(21, Rational::FPS_24), 1);
    assert_eq!(frames(1001, Rational::FPS_23_976), 24);
    assert_eq!(millis(24, Rational::FPS_23_976), 1001);
    assert_eq!(millis(1, Rational::FPS_30), 33);
    assert_eq!(millis(2, Rational::FPS_30), 67);
    assert_eq!(millis(i64::MAX, Rational::FPS_24), i64::MAX);

    let rates = [
        Rational::FPS_23_976,
        Rational::FPS_24,
        Rational::FPS_25,
        Rational::FPS_29_97,
        Rational::FPS_30,
        Rational::FPS_50,
        Rational::FPS_59_94,
        Rational::FPS_60,
        Rational::new(240, 1),
    ];
    for rate in rates {
        for frame in 0..3000 {
            assert_eq!(
                frames(millis(frame, rate), rate),
                frame,
                "frame {frame} at {rate:?}"
            );
        }
        for ms in (0..10_000).step_by(7) {
            // Within half a frame of `ms`, plus the half millisecond of the
            // second rounding.
            let off = (millis(frames(ms, rate), rate) - ms).abs();
            assert!(
                2 * off * i64::from(rate.num) <= 1000 * i64::from(rate.den) + i64::from(rate.num),
                "{ms} ms at {rate:?} comes back {off} ms off"
            );
        }
    }
}

#[test]
fn placed_cues_land_on_frames_and_overlapping_cues_take_further_tracks() {
    let session = at_24();
    placed(
        &session,
        &[
            cue(1250, 1375, "D overlaps B and C"),
            cue(0, 1000, "A"),
            cue(500, 1500, "B overlaps A"),
            cue(1000, 2000, "C starts as A ends"),
            cue(3000, 3010, "E under a frame"),
        ],
        "Captions",
    );
    let project = session.project();
    let names = |name: &str| -> Vec<String> {
        track(&project, name)
            .clips_ordered()
            .map(|clip| match &clip.content {
                ClipSource::Generated(Generator::Text { content, .. }) => content.clone(),
                other => panic!("{name} holds {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        names("Captions"),
        ["A", "C starts as A ends", "E under a frame"]
    );
    assert_eq!(names("Captions (2)"), ["B overlaps A"]);
    assert_eq!(names("Captions (3)"), ["D overlaps B and C"]);
    for name in ["Captions", "Captions (2)", "Captions (3)"] {
        assert_eq!(track(&project, name).kind, TrackKind::Text, "{name}");
    }
    // 3010 ms is frame 72, so the cue lasts its one frame: 73 frames end at
    // 3041.67 ms.
    assert_eq!(
        cues(&project),
        Cues {
            cues: vec![
                cue(0, 1000, "A"),
                cue(500, 1500, "B overlaps A"),
                cue(1000, 2000, "C starts as A ends"),
                cue(1250, 1375, "D overlaps B and C"),
                cue(3000, 3042, "E under a frame"),
            ],
            skipped: Vec::new(),
        }
    );

    assert!(session.host().undo().expect("undoes"));
    let project = session.project();
    assert!(
        project
            .timeline()
            .tracks_ordered()
            .all(|track| !track.name.starts_with("Captions")),
        "one undo removes every track the cues made"
    );
}

#[test]
fn only_shown_text_tracks_are_written_and_what_a_cue_cannot_hold_is_reported() {
    let session = at_24();
    placed(
        &session,
        &[
            cue(0, 1000, "Styled"),
            cue(1000, 2000, "Blank soon"),
            cue(3000, 4000, "Plain"),
        ],
        "Shown",
    );
    placed(&session, &[cue(5000, 6000, "Hidden")], "Hidden");
    let ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(empty)) =
        session.apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Text,
            name: "Empty".into(),
            index: None,
            pinned: true,
        }))
    else {
        panic!("adds the empty track");
    };
    let project = session.project();
    let hidden = track(&project, "Hidden").id;
    let shown: Vec<_> = track(&project, "Shown")
        .clips_ordered()
        .map(|clip| (clip.id, clip.content.clone()))
        .collect();
    let disable = |track: TrackId| {
        session.apply(Command::Edit(EditCommand::SetTrackEnabled {
            track,
            enabled: false,
        }));
    };
    disable(hidden);
    disable(empty);
    let ClipSource::Generated(Generator::Text { content, mut style }) = shown[0].1.clone() else {
        panic!("a text clip");
    };
    style.bold = !style.bold;
    session.apply(Command::Edit(EditCommand::SetGenerator {
        clip: shown[0].0,
        generator: Generator::Text { content, style },
    }));
    session.apply(Command::Edit(EditCommand::SetGenerator {
        clip: shown[1].0,
        generator: Generator::text(" \n "),
    }));

    let Cues { cues, mut skipped } = cues(&session.project());
    assert_eq!(cues, [cue(0, 1000, "Styled"), cue(3000, 4000, "Plain")]);
    skipped.sort();
    assert_eq!(
        skipped,
        [
            "1 text clip: font, color, size and position are not written",
            "Hidden: a hidden text track, not written",
            "Shown at 00:00:01.000: a text clip with no text",
        ]
    );
}

#[test]
fn every_format_imports_onto_the_timeline_and_exports_the_same_file() {
    let source = [
        cue(0, 1000, "One"),
        cue(500, 1500, "Two & 3 < 4"),
        cue(2000, 2125, "Line one\nLine two"),
    ];
    for format in &SUBTITLE_FORMATS {
        let session = at_24();
        session.context().mount(&InterchangePlugin).expect("mounts");
        let (text, _) = written(format, &source);
        let file = session.dir.path().join(format!("Cut.{}", format.extension));
        std::fs::write(&file, &text).expect("writes the file");

        let (report, _) = session
            .context()
            .import_file(session.host(), &file)
            .expect("imports");
        assert!(
            report.skipped.is_empty(),
            "{}: {:?}",
            format.id,
            report.skipped
        );
        let project = session.project();
        assert_eq!(
            track(&project, "Cut").clips_ordered().count(),
            2,
            "{}",
            format.id
        );
        assert_eq!(
            track(&project, "Cut (2)").clips_ordered().count(),
            1,
            "{}",
            format.id
        );

        let out = session.dir.path().join(format!("Out.{}", format.extension));
        let report = session
            .context()
            .export_file(format.id, &project, &out)
            .expect("exports");
        assert!(
            report.skipped.is_empty(),
            "{}: {:?}",
            format.id,
            report.skipped
        );
        assert_eq!(
            std::fs::read_to_string(&out).expect("reads the export"),
            text,
            "{}",
            format.id
        );

        assert!(session.host().undo().expect("undoes"), "{}", format.id);
        assert!(
            session
                .project()
                .timeline()
                .tracks_ordered()
                .all(|track| !track.name.starts_with("Cut")),
            "{}: the import is one undo step",
            format.id
        );
    }
}

#[test]
fn a_file_another_format_wrote_is_refused_and_leaves_the_project_as_it_was() {
    let session = at_24();
    session.context().mount(&InterchangePlugin).expect("mounts");
    let file = session.dir.path().join("Captions.vtt");
    std::fs::write(&file, "1\n00:00:01,000 --> 00:00:02,000\nSubRip\n").expect("writes");
    let before = layout(&session.project());

    let err = session
        .context()
        .import_file(session.host(), &file)
        .expect_err("refused");
    assert!(matches!(err, InterchangeError::Import { .. }), "{err:?}");
    assert_eq!(
        err.to_string(),
        format!(
            "aria.interchange.vtt could not import {path}: {path} is not a WebVTT subtitles \
             file: the file does not start with WEBVTT",
            path = file.display()
        )
    );
    assert_eq!(layout(&session.project()), before);
}

#[test]
fn a_timeline_with_no_shown_text_is_not_written_and_leaves_no_file() {
    let session = at_24();
    session.context().mount(&InterchangePlugin).expect("mounts");
    let project = session.project();
    for format in &SUBTITLE_FORMATS {
        let out = session
            .dir
            .path()
            .join(format!("Empty.{}", format.extension));
        let err = session
            .context()
            .export_file(format.id, &project, &out)
            .expect_err("nothing to write");
        assert_eq!(
            err.to_string(),
            format!(
                "{id} could not export {path}: {name} has no text clip on a shown text track \
                 to write as subtitles",
                id = format.id,
                path = out.display(),
                name = project.name
            )
        );
        assert!(!out.exists(), "{}", format.id);
        assert_eq!(
            std::fs::read_dir(session.dir.path())
                .expect("lists")
                .count(),
            0,
            "{}: no partial file remains",
            format.id
        );
    }
}
