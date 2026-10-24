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

use aria_decoder::OutputMode;
use aria_plugin::PluginContext;

use super::mode_reach;
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
