//! Contracts for the Viewer's control row, the display button over the
//! picture, and the timeline's zoom readout.
//!
//! The class closed here: a Viewer control that acts on a tab other than the
//! one in front of the Viewer. Every transport button is clicked with Program
//! in front, where it moves the sequence playhead, and with a Source tab in
//! front, where it moves the take and leaves the playhead where it was. Each
//! click starts with the Timeline active, so a button that relies on the
//! Viewer already having the keys goes red.
//!
//! Also asserted: the In and Out buttons mark the sequence and are drawn only
//! while Program is in front, each switch under the display button flips the
//! state its check mark is read from and flips it back, and a click on the
//! timeline's zoom readout fits the sequence.
//!
//! The row's fit is a second class: a control drawn past the Viewer's edge,
//! where the pointer does not reach it. At the smallest window the row wraps
//! between its three groups, and each group, the row and the Viewer stay
//! inside one another; at a wide window the groups share one line. The
//! smallest window is read from the one constant the platform minimum, the
//! placement clamp and the two-frame fallback all use, and the fallback is
//! asserted on both sides of it.
//!
//! Not covered: the play button, whose click starts the vsync loop a test
//! window does not run, the lit look of a toggle, which
//! `chrome::tool_strip::lit_icon` draws, and widths under real glyph metrics,
//! which the test text system does not have.

use std::path::PathBuf;

use aria_models::MediaId;

use super::AppState;
use crate::chrome::menu_items::VIEWER_DISPLAY;
use crate::dock::{PANEL_INSPECTOR, PANEL_TIMELINE, PANEL_VIEWER};
use crate::interned::intern;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::test_support::{bounds_inside, drawn_shell_at};
use crate::types::{ActivePanel, AppMenu};

/// The three groups of the control row, left to right.
const GROUPS: [&str; 3] = [
    "viewer-row-picture",
    "viewer-row-transport",
    "viewer-row-display",
];

/// A window at the screen's top left corner.
fn window(width: f32, height: f32) -> gpui::Bounds<gpui::Pixels> {
    gpui::Bounds::new(
        gpui::point(gpui::px(0.), gpui::px(0.)),
        gpui::size(gpui::px(width), gpui::px(height)),
    )
}

/// The bounds drawn under `selector`, or a failure naming it.
pub(super) fn bounds_of(
    visual: &mut gpui::VisualTestContext,
    selector: &str,
) -> gpui::Bounds<gpui::Pixels> {
    crate::test_support::panel_bounds(visual, intern(selector))
        .unwrap_or_else(|| panic!("the shell draws no {selector}"))
}

/// The transport buttons in the Viewer's control row, play aside.
const STEPS: [&str; 4] = ["next-frame", "prev-frame", "go-end", "go-start"];

/// Where `button` puts a tab parked on `from` whose last reachable position is
/// `end`.
fn landing(button: &str, from: i64, end: i64) -> i64 {
    match button {
        "next-frame" => from + 1,
        "prev-frame" => from - 1,
        "go-end" => end,
        "go-start" => 0,
        other => panic!("no landing stated for {other}"),
    }
}

/// Click the centre of the element drawn under `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &str) {
    let bounds = bounds_of(visual, selector);
    visual.simulate_click(bounds.center(), gpui::Modifiers::none());
    visual.run_until_parked();
}

/// Whether the shell draws an element under `selector`.
fn drawn(visual: &mut gpui::VisualTestContext, selector: &str) -> bool {
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    visual.debug_bounds(intern(selector)).is_some()
}

/// Change `state` and draw the result.
pub(super) fn change(
    visual: &mut gpui::VisualTestContext,
    state: &gpui::Entity<AppState>,
    edit: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            edit(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
}

/// A drawn shell with a three-second, 24 fps take on the timeline. Shared with
/// the held-step contracts.
pub(super) fn shell_with_take<'a>(
    cx: &'a mut gpui::TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<AppState>,
    &'a mut gpui::VisualTestContext,
    MediaId,
) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, tag);
    let take = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let media = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(take).expect("import the take");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the take");
            cx.notify();
            media
        })
    });
    visual.run_until_parked();
    (state, visual, media)
}

#[gpui::test]
fn each_transport_button_moves_the_tab_in_front_of_the_viewer(cx: &mut gpui::TestAppContext) {
    let (state, visual, media) = shell_with_take(cx, "viewer-row-transport");
    let end = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    assert!(end > 20, "the sequence has frames to step through");

    for button in STEPS {
        change(visual, &state, |state| {
            state.set_playhead(10);
            state.active_panel = ActivePanel::Timeline;
        });
        click(visual, button);
        let (at, panel) = visual.read(|app| {
            let state = state.read(app);
            (state.playhead.value, state.active_panel)
        });
        assert_eq!(
            at,
            landing(button, 10, end),
            "{button} with Program in front"
        );
        assert_eq!(
            panel,
            ActivePanel::Preview,
            "{button} left the keys elsewhere"
        );
    }

    change(visual, &state, |state| {
        assert!(state.open_source(media.raw()), "the Source tab opens");
    });
    let last = visual.read(|app| {
        let tab = state.read(app).sources.focused().cloned();
        tab.expect("a Source tab in front").duration_ticks - 1
    });
    for button in STEPS {
        change(visual, &state, |state| {
            state.set_playhead(10);
            state.sources.focused_mut().expect("the Source tab").tick = 30;
            state.active_panel = ActivePanel::Timeline;
        });
        click(visual, button);
        let (tick, at) = visual.read(|app| {
            let state = state.read(app);
            let tab = state
                .sources
                .focused()
                .expect("the Source tab stays in front");
            (tab.tick, state.playhead.value)
        });
        assert_eq!(
            tick,
            landing(button, 30, last),
            "{button} with a Source tab in front"
        );
        assert_eq!(
            at, 10,
            "{button} on a Source tab moved the sequence playhead"
        );
    }
}

#[gpui::test]
fn the_in_and_out_buttons_mark_the_sequence_while_program_is_in_front(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, media) = shell_with_take(cx, "viewer-row-marks");
    change(visual, &state, |state| state.set_playhead(5));
    click(visual, "program-mark-in");
    change(visual, &state, |state| state.set_playhead(20));
    click(visual, "program-mark-out");
    let marks = visual.read(|app| {
        let state = state.read(app);
        (state.mark_in, state.mark_out)
    });
    assert_eq!(
        marks,
        (Some(5), Some(20)),
        "the buttons marked another frame"
    );

    change(visual, &state, |state| {
        assert!(state.open_source(media.raw()), "the Source tab opens");
    });
    for button in ["program-mark-in", "program-mark-out"] {
        assert!(
            !drawn(visual, button),
            "{button} is drawn while a Source tab, with marks of its own, is in front"
        );
    }
}

#[gpui::test]
fn each_viewer_display_row_flips_the_state_it_is_checked_from(cx: &mut gpui::TestAppContext) {
    let (state, visual, media) = shell_with_take(cx, "viewer-row-display");
    visual.update(|_, app| app.set_reduce_motion(true));
    // Split needs a Source tab to put beside Program.
    change(visual, &state, |state| {
        assert!(state.open_source(media.raw()), "the Source tab opens");
        state.show_program();
    });
    // The rows are the registry's, so a row added to the submenu is pressed
    // here on the run that adds it. A row with no check mark is a command,
    // not a switch.
    let rows: Vec<(usize, &'static str)> = visual.read(|app| {
        state
            .read(app)
            .menu_rows(AppMenu::View)
            .iter()
            .filter(|item| item.under == Some(VIEWER_DISPLAY))
            .enumerate()
            .filter(|(_, item)| item.checked.is_some())
            .map(|(at, item)| (at, item.id))
            .collect()
    });
    assert!(rows.len() >= 8, "the submenu holds {rows:?}");
    let checked = |visual: &mut gpui::VisualTestContext, id: &str| {
        visual.read(|app| {
            let state = state.read(app);
            state
                .menu_rows(AppMenu::View)
                .iter()
                .find(|item| item.id == id)
                .is_some_and(|item| item.is_checked(state))
        })
    };
    for (at, id) in rows {
        let before = checked(visual, id);
        for pass in [!before, before] {
            click(visual, "viewer-display-settings");
            for _ in 0..at {
                visual.simulate_keystrokes("down");
            }
            visual.run_until_parked();
            click(visual, "focus-ring");
            assert_eq!(
                checked(visual, id),
                pass,
                "pressing {id} under the Viewer's display button"
            );
        }
    }
}

#[gpui::test]
fn a_click_on_the_timeline_zoom_readout_fits_the_sequence(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "viewer-row-zoom-fit");
    change(visual, &state, |state| state.zoom_around_playhead(0.25));
    let zoomed = visual.read(|app| state.read(app).zoom);
    click(visual, "timeline-zoom-fit");
    let clicked = visual.read(|app| state.read(app).zoom);

    // The oracle: the same fit run directly from the zoomed view.
    change(visual, &state, |state| state.zoom_around_playhead(0.25));
    let fitted = visual.update(|_, app| {
        state.update(app, |state, _| {
            state.zoom_fit_sequence();
            state.zoom
        })
    });
    assert!(
        (zoomed - fitted).abs() > 1e-3,
        "the zoomed view already sat at the fit, so the click proves nothing"
    );
    assert!(
        (clicked - fitted).abs() < 1e-4,
        "the readout click left the zoom at {clicked}, the fit is {fitted}"
    );
}

/// The window bounds of the frame that holds the Viewer.
fn viewer_frame(
    state: &gpui::Entity<AppState>,
    visual: &mut gpui::VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    visual.read(|app| {
        let state = state.read(app);
        let frame = state
            .dock_tree
            .find(PANEL_VIEWER)
            .expect("the Viewer is docked");
        let [x, y, w, h] = *state
            .dock_frame_rects
            .lock()
            .get(&frame)
            .expect("the Viewer's frame was drawn");
        gpui::Bounds::new(
            gpui::point(gpui::px(x), gpui::px(y)),
            gpui::size(gpui::px(w), gpui::px(h)),
        )
    })
}

#[gpui::test]
fn every_viewer_control_stays_inside_the_viewer_at_the_smallest_window(
    cx: &mut gpui::TestAppContext,
) {
    let at = window(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);
    let (state, mut visual) = drawn_shell_at(cx, "viewer-row-smallest", at);
    let visual = &mut visual;
    let frame = viewer_frame(&state, visual);
    let row = bounds_of(visual, "viewer-row");
    assert!(
        bounds_inside(row, frame),
        "the control row {row:?} runs past the Viewer {frame:?}"
    );
    let mut lines = Vec::new();
    for group in GROUPS {
        let drawn = bounds_of(visual, group);
        assert!(
            bounds_inside(drawn, row),
            "{group} {drawn:?} runs past the control row {row:?}"
        );
        assert!(drawn.size.width > gpui::px(0.), "{group} drew no width");
        lines.push(drawn.origin.y);
    }
    // Each group's top is at or below the one before it: a wrap moves the
    // later groups down, never back up past an earlier one.
    assert!(
        lines
            .windows(2)
            .all(|pair| pair[1] + gpui::px(0.5) >= pair[0]),
        "the groups are out of order: {lines:?}"
    );
}

#[gpui::test]
fn the_control_row_is_one_line_when_the_viewer_has_room(cx: &mut gpui::TestAppContext) {
    let (_state, mut visual) = drawn_shell_at(cx, "viewer-row-wide", window(1920., 1080.));
    let visual = &mut visual;
    let row = bounds_of(visual, "viewer-row");
    assert!(
        (f32::from(row.size.height) - 40.).abs() < 0.5,
        "a Viewer with room drew a {row:?} row, not one 40 px line"
    );
    let first = bounds_of(visual, GROUPS[0]);
    for group in &GROUPS[1..] {
        let drawn = bounds_of(visual, group);
        assert!(
            (drawn.center().y - first.center().y).abs() < gpui::px(0.5),
            "{group} {drawn:?} is not on the line of {} {first:?}",
            GROUPS[0]
        );
    }
}

/// The smallest window draws every docked frame, and one pixel less on either
/// side draws the Viewer and the Timeline only (`docs/spec/layout.md:24-28`).
#[gpui::test]
fn the_smallest_window_is_the_edge_of_the_two_frame_layout(cx: &mut gpui::TestAppContext) {
    let cases = [
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, true),
        (MIN_WINDOW_WIDTH - 1., MIN_WINDOW_HEIGHT, false),
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT - 1., false),
    ];
    for (width, height, every_frame) in cases {
        let tag = format!("viewer-row-edge-{width}x{height}");
        let (_state, mut visual) = drawn_shell_at(cx, &tag, window(width, height));
        let visual = &mut visual;
        for panel in [PANEL_VIEWER, PANEL_TIMELINE] {
            assert!(
                drawn(visual, &format!("dock-tab-{panel}")),
                "{width}x{height} drew no {panel} tab"
            );
        }
        assert_eq!(
            drawn(visual, &format!("dock-tab-{PANEL_INSPECTOR}")),
            every_frame,
            "{width}x{height}: the Inspector's tab drawn is not {every_frame}"
        );
    }
}
