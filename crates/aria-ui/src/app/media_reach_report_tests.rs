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

use aria_core::{Rational, TimeRange};
use aria_decoder::OutputMode;
use aria_plugin::PluginContext;

use super::{mode_reach, overhang};
use crate::app::status::lowercase_start;
use crate::app::{AppState, DialogKind};

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
    for (source, full, line) in [
        (
            at(120, 48),
            at(0, 48),
            Some(
                "The clip plays 5.00s to 7.00s and the file holds 0.00s to 2.00s, with no overlap",
            ),
        ),
        (
            at(0, 72),
            at(0, 48),
            Some("Reaches 1.00s past the end of the file (24 ticks)"),
        ),
        (
            at(0, 48),
            at(24, 48),
            Some("Starts 1.00s before the file does (24 ticks)"),
        ),
        (at(12, 24), at(0, 48), None),
    ] {
        assert_eq!(
            overhang(source, full).as_deref(),
            line,
            "{source:?} in {full:?}"
        );
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
