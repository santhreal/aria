//! Every value an output mode row of the media support report can read starts
//! in sentence case.
//!
//! The class closed here: a value on a branch the build under test never
//! takes. The decoder backend a build has selects which branch the drawn report
//! shows, so the panel walk in `panels/panel_casing_tests.rs` reads one branch
//! per platform. This walks every branch for every mode in `OutputMode::ALL`,
//! read when the test runs, so a mode added later is checked on the run that
//! adds it. Not covered: the other rows of the report, which the panel walk
//! reads as the test build draws them.

use aria_decoder::OutputMode;

use super::mode_reach;
use crate::app::status::lowercase_start;

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
