//! The media support report: every value an output mode row can read starts in
//! sentence case, and opening the Media Support dialog builds the report it
//! draws.
//!
//! The first class closed here: a value on a branch the build under test never
//! takes. The decoder backend a build has selects which branch the drawn report
//! shows, so the dialog walk in `chrome/dialog_tests.rs` reads one branch per
//! platform. This walks every branch for every mode in `OutputMode::ALL`, read
//! when the test runs, so a mode added later is checked on the run that adds
//! it. Not covered: the other rows of the report, which the dialog walk reads
//! as the test build draws them.
//!
//! The second: a dialog opened on a report nothing built, which draws a card
//! with no facts in it. Not covered: what the built report holds.
//!
//! The third: a clip that plays none of its file printed the two spans as raw
//! ticks in Rust range syntax, `ticks 120..168` and `0..48`, while the rows
//! beside it read in seconds.
//!
//! The fourth: a count or a rate in a unit the reader has no use for. The
//! selected source read `0×0 at 1000/1, 2000 ticks` for a sound file, a
//! moving picture `at 25/1, 48 ticks`, and an overhang `(24 ticks)`. Each row
//! states a moving picture's rate in frames a second and its counts in frames,
//! and states neither for a sound file or a still, whose ticks are not frames.
//! Not covered: the length a row prints, which is the pool's own label.

use aria_core::{Rational, TimeRange};
use aria_decoder::OutputMode;
use aria_plugin::PluginContext;

use super::{mode_reach, overhang, rate_mismatch, source_line};
use crate::app::status::lowercase_start;
use crate::app::{AppState, DialogKind};
use crate::types::{UiClipKind, UiMediaItem};

#[test]
fn every_output_mode_value_reads_in_sentence_case() {
    let fallbacks = std::iter::once(None).chain(OutputMode::ALL.map(Some));
    for fallback in fallbacks {
        for supported in [true, false] {
            let value = mode_reach(supported, fallback);
            assert_eq!(
                lowercase_start(&value),
                None,
                "supported {supported}, fallback {fallback:?}: {value:?}"
            );
        }
    }
}

#[test]
fn a_clip_beside_its_file_reads_in_seconds() {
    let at = |start, length| TimeRange::at_rate(start, length, Rational::FPS_24);
    for (source, full, frames, line) in [
        (
            at(120, 48),
            at(0, 48),
            true,
            Some(
                "The clip plays 5.00s to 7.00s and the file holds 0.00s to 2.00s, with no overlap",
            ),
        ),
        (
            at(0, 72),
            at(0, 48),
            true,
            Some("Reaches 1.00s past the end of the file (24 frames)"),
        ),
        (
            at(0, 48),
            at(24, 48),
            true,
            Some("Starts 1.00s before the file does (24 frames)"),
        ),
        (
            at(0, 49),
            at(0, 48),
            true,
            Some("Reaches 0.04s past the end of the file (1 frame)"),
        ),
        (
            at(0, 72),
            at(0, 48),
            false,
            Some("Reaches 1.00s past the end of the file"),
        ),
        (
            at(0, 48),
            at(24, 48),
            false,
            Some("Starts 1.00s before the file does"),
        ),
        (at(12, 24), at(0, 48), true, None),
    ] {
        assert_eq!(
            overhang(source, full, frames).as_deref(),
            line,
            "{source:?} in {full:?}, frames {frames}"
        );
    }
}

#[test]
fn a_rate_mismatch_states_rates_only_in_frames_a_second() {
    assert_eq!(
        rate_mismatch(Rational::FPS_23_976, Rational::FPS_25, true),
        "The clip's range is authored at 23.976 fps and the file reports 25 fps; compared by \
         resampling"
    );
    assert_eq!(
        rate_mismatch(Rational::FPS_24, Rational::new(1000, 1), false),
        "The clip's range is authored at a rate other than the file's; compared by resampling"
    );
}

/// A pool item of `kind`, 2 seconds long at 25 fps with sound.
fn pool_item(kind: UiClipKind, still: bool) -> UiMediaItem {
    UiMediaItem {
        id: 1,
        name: "source".into(),
        path: "/media/source.mov".into(),
        width: if kind == UiClipKind::Audio { 0 } else { 1920 },
        height: if kind == UiClipKind::Audio { 0 } else { 1080 },
        frame_rate: Rational::new(25, 1),
        duration_ticks: 50,
        duration_label: "00:00:02:00".into(),
        has_audio: true,
        kind,
        still,
        missing: false,
        label: None,
        imported_at: None,
        video_codec: None,
        audio_codec: None,
        bin: None,
        proxied: false,
        position: None,
    }
}

#[test]
fn the_selected_source_reads_in_frames_a_second_and_the_pool_length() {
    let mut silent = pool_item(UiClipKind::Media, false);
    silent.has_audio = false;
    let mut sound = pool_item(UiClipKind::Audio, false);
    sound.frame_rate = Rational::new(1000, 1);
    sound.duration_ticks = 2000;
    // A rate whose numerator is not its frames a second.
    let mut ntsc = pool_item(UiClipKind::Media, false);
    ntsc.frame_rate = Rational::FPS_29_97;
    for (item, line) in [
        (
            pool_item(UiClipKind::Media, false),
            "1920×1080 at 25 fps, 00:00:02:00, with audio",
        ),
        (ntsc, "1920×1080 at 29.97 fps, 00:00:02:00, with audio"),
        (silent, "1920×1080 at 25 fps, 00:00:02:00"),
        (pool_item(UiClipKind::Media, true), "1920×1080 still image"),
        (sound, "Sound, 00:00:02:00"),
    ] {
        assert_eq!(source_line(&item), line, "{item:?}");
    }
}

#[test]
fn opening_the_media_support_dialog_builds_its_report() {
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .unwrap_or_else(|err| panic!("the ui plugin needs nothing: {err:?}"));
    let mut state = AppState::new(ctx, crate::scratch_pool::scratch("media-support-open"))
        .unwrap_or_else(|err| panic!("editor state: {err:?}"));
    state
        .new_project()
        .unwrap_or_else(|err| panic!("a project to edit: {err:?}"));
    assert!(
        state.media_reach.report.is_none(),
        "a report was built before it was asked for"
    );

    state.open_dialog(DialogKind::MediaSupport);
    assert_eq!(state.dialog, DialogKind::MediaSupport);
    let Some(report) = &state.media_reach.report else {
        panic!("the Media Support dialog opened on no report");
    };
    assert!(
        !report.lines.is_empty(),
        "the report lists no fact about this build"
    );
}
