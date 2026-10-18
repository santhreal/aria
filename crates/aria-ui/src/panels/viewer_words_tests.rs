//! At rest the Viewer's tab row and control row draw icons, and words only
//! where the word is the control.
//!
//! The class closed here: a toggle or a label drawn as a word in a row of
//! icon controls, as the tab row's Split, Rulers, Safe, Guides and Alpha chips
//! and the control row's Handles label and Transform, Crop, Mask, Scopes, Loop
//! and Proxy words were. The worded controls are the timecode field, the
//! Program and Source tabs, the zoom label that fits the frame, the resolution
//! menu, and the In and Out marks. Every text run either row draws must stand
//! inside one of them, and every control in `CONTROL_TIPS` drawn in either row
//! that is not one of them must draw no text. A Source tab is open and Program
//! is in front, so a Source tab and the marks are both drawn.
//!
//! Not covered: the rows while shuttling, which add the speed badge, or with
//! an error on the row, which adds its message; a control with no debug
//! selector, which part two cannot find and part one still checks by its text.

use std::path::PathBuf;

use gpui::{Bounds, Pixels, TestAppContext};

use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::tooltip::CONTROL_TIPS;

/// The controls a word names: the timecode field, the tabs, the zoom label,
/// the resolution menu, and the marks.
const WORDED: [&str; 7] = [
    "playhead-tc",
    "viewer-tab-program",
    "viewer-tab-source-0",
    "preview-zoom-label",
    "preview-resolution",
    "program-mark-in",
    "program-mark-out",
];

/// The rows checked: the tab row over the picture and the control row under
/// it.
const ROWS: [&str; 2] = ["viewer-tab-row", "viewer-row"];

/// Icon controls the scan must reach, so a scan that finds nothing fails.
const ICONS: [&str; 7] = [
    "viewer-display-settings",
    "go-start",
    "preview-loop",
    "preview-proxies-toggle",
    "preview-handles-transform",
    "preview-scopes-toggle",
    "next-frame",
];

#[gpui::test]
fn at_rest_the_viewer_draws_words_only_where_a_word_is_the_control(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "viewer-words");
    let take = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(take).expect("import the take");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the take");
            assert!(state.open_source(media.raw()), "the Source tab opens");
            state.show_program();
            cx.notify();
        })
    });
    // The boot cover's wordmark lies over the Viewer's rows.
    crate::test_support::wait_out_splash(visual);

    let bounds = |visual: &mut gpui::VisualTestContext, selector: &str| {
        visual
            .debug_bounds(intern(selector))
            .unwrap_or_else(|| panic!("the Viewer draws no {selector}"))
    };
    let rows: Vec<Bounds<Pixels>> = ROWS.iter().map(|row| bounds(visual, row)).collect();
    let worded: Vec<(&str, Bounds<Pixels>)> =
        WORDED.iter().map(|&id| (id, bounds(visual, id))).collect();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let in_rows = |at: gpui::Point<Pixels>| rows.iter().any(|row| row.contains(&at));

    for run in runs
        .iter()
        .filter(|run| !run.text.trim().is_empty() && in_rows(run.bounds.center()))
    {
        assert!(
            worded
                .iter()
                .any(|(_, control)| control.contains(&run.bounds.center())),
            "the Viewer draws `{}` at {:?}, a word that is none of {WORDED:?}",
            run.text,
            run.bounds
        );
    }

    let mut icons = Vec::new();
    for tip in CONTROL_TIPS {
        if WORDED.contains(&tip.control) {
            continue;
        }
        let Some(control) = visual.debug_bounds(intern(tip.control)) else {
            continue;
        };
        if !in_rows(control.center()) {
            continue;
        }
        let words: Vec<&str> = runs
            .iter()
            .filter(|run| control.contains(&run.bounds.center()))
            .map(|run| run.text.as_ref())
            .collect();
        assert!(
            words.iter().all(|word| word.trim().is_empty()),
            "the Viewer control {} draws {words:?}; it is not one of {WORDED:?}",
            tip.control
        );
        icons.push(tip.control);
    }
    for id in ICONS {
        assert!(
            icons.contains(&id),
            "the scan did not reach the icon control {id}: it found {icons:?}"
        );
    }
}
