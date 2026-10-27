//! The Viewer's control row holds one height in every state its controls
//! put it in.
//!
//! The class closed here: a control that changes the row's height when it is
//! drawn, lit, or swapped, so the picture above jumps during editing. A
//! Viewer with room draws the row as one 40 px line, and every control in it is
//! `Theme::ICON_HIT` high on the centre of the line under the row's top border.
//! The row is measured at rest and after each toggle in it is pressed, and
//! with a Source tab in front, which drops the marks. The controls are every
//! `CONTROL_TIPS` control the row draws, so a control added to the row is
//! held to the line without an edit here.
//!
//! Not covered: a Viewer narrow enough to wrap the row, which is
//! `every_viewer_control_stays_inside_the_viewer_at_the_smallest_window`; the
//! shuttle badge and an error message, which are not controls.

use std::path::PathBuf;

use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, px};

use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::tooltip::CONTROL_TIPS;

/// The row's one-line height.
const ROW_H: f32 = 40.0;

const EPS: f32 = 0.5;

/// The rule drawn along the row's top edge, above the line.
const BORDER_T: f32 = 1.0;

/// Controls the row draws at rest with Program in front, so a scan that finds
/// fewer fails.
const FLOOR: [&str; 11] = [
    "playhead-tc",
    "preview-zoom",
    "preview-resolution",
    "preview-proxies-toggle",
    "go-start",
    "preview-loop",
    "program-mark-in",
    "program-mark-out",
    "preview-handles-transform",
    "preview-scopes-toggle",
    "viewer-display-settings",
];

/// The toggles pressed in turn, each measured after its press.
const PRESSED: [&str; 5] = [
    "preview-loop",
    "preview-proxies-toggle",
    "preview-handles-crop",
    "preview-handles-mask",
    "preview-scopes-toggle",
];

/// The row's bounds and the bounds of every tipped control drawn in it.
pub(super) fn measure(
    visual: &mut VisualTestContext,
) -> (Bounds<Pixels>, Vec<(&'static str, Bounds<Pixels>)>) {
    let row = visual
        .debug_bounds("viewer-row")
        .expect("the Viewer draws its control row");
    let controls = CONTROL_TIPS
        .iter()
        .filter_map(|tip| {
            let drawn = visual.debug_bounds(intern(tip.control))?;
            row.contains(&drawn.center())
                .then_some((tip.control, drawn))
        })
        .collect();
    (row, controls)
}

/// The row is one `ROW_H` line and every control in it is `ICON_HIT` high on
/// the line's centre.
fn assert_line(visual: &mut VisualTestContext, what: &str) -> Vec<&'static str> {
    let (row, controls) = measure(visual);
    let line_centre = (row.top() + px(BORDER_T) + row.bottom()) / 2.0;
    assert!(
        (f32::from(row.size.height) - ROW_H).abs() < EPS,
        "{what}: the control row is {:?} high, not {ROW_H} px",
        row.size.height
    );
    for (control, drawn) in &controls {
        assert!(
            (f32::from(drawn.size.height) - Theme::ICON_HIT).abs() < EPS,
            "{what}: {control} is {:?} high, not {} px",
            drawn.size.height,
            Theme::ICON_HIT
        );
        assert!(
            (drawn.center().y - line_centre).abs() < px(EPS),
            "{what}: {control} {drawn:?} is off the line's center {line_centre:?}"
        );
    }
    controls.into_iter().map(|(control, _)| control).collect()
}

pub(super) fn press(visual: &mut VisualTestContext, control: &str) {
    let at = visual
        .debug_bounds(intern(control))
        .unwrap_or_else(|| panic!("the Viewer draws no {control}"))
        .center();
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
}

/// A drawn shell with a take on the timeline and in a Source tab, Program in
/// front, past the boot cover.
pub(super) fn viewer_with_take<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<crate::app::AppState>,
    &'a mut VisualTestContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
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
    // The boot cover takes the presses until it is gone.
    crate::test_support::wait_out_splash(visual);
    (state, visual)
}

#[gpui::test]
fn the_control_row_holds_one_height_in_every_state(cx: &mut TestAppContext) {
    let (_state, visual) = viewer_with_take(cx, "viewer-row-height");

    let at_rest = assert_line(visual, "at rest");
    for control in FLOOR {
        assert!(
            at_rest.contains(&control),
            "the scan did not reach {control} at rest: it found {at_rest:?}"
        );
    }
    for control in PRESSED {
        press(visual, control);
        assert_line(visual, &format!("after {control}"));
    }
    press(visual, "viewer-tab-source-0");
    assert_line(visual, "with the Source tab in front");
}
