//! A seek shows its frame in the Viewer whole, on the first frame drawn with
//! it.
//!
//! The class closed here: a frame that fades in after a seek, so the picture
//! the Viewer lands on is drawn dim, or not at all, until the fade ends. Every
//! transport button that moves the tab in front of the Viewer is pressed in
//! turn, after a pause longer than any fade, first with Program in front and
//! then with a Source tab in front, once the clock has run past every
//! transition the setup started. Once the frame for the new position is on
//! screen the window is drawn with the test clock where the press left it, and
//! the one picture in the Viewer must be painted at full opacity, its own and
//! every ancestor's.
//!
//! Not covered: Program's GPU surface, which a test window has no device for.
//! It is the same `picture` as the image, over a texture.

use std::time::{Duration, Instant};

use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};

use super::viewer_row_height_tests::{press, viewer_with_take};
use crate::app::AppState;
use crate::components::drawn;

/// A pause between two presses, longer than any fade in the catalog, so each
/// press is a seek of its own rather than one step of a run.
const PAUSE: Duration = Duration::from_millis(400);

/// How long the frame for a new position may take to reach the screen.
const DECODE: Duration = Duration::from_secs(20);

/// The buttons pressed in turn. From the middle of the take each lands
/// somewhere new.
const MOVES: [&str; 4] = ["next-frame", "prev-frame", "go-end", "go-start"];

/// Change `state` and draw the result.
fn change(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
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

/// Move the test clock past every transition and draw again.
fn settle(visual: &mut VisualTestContext) {
    visual.executor().advance_clock(Duration::from_secs(2));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

/// Draw on the wall clock until `shown` holds, leaving the test clock where it
/// is. The frames decode off the test's executor.
fn until(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    what: &str,
    shown: impl Fn(&AppState) -> bool,
) {
    let deadline = Instant::now() + DECODE;
    loop {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        if visual.read(|app| shown(state.read(app))) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: no frame for the new position reached the Viewer in {DECODE:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

/// The pictures the last frame painted in the Viewer, above its control row,
/// with the opacity of each.
fn in_viewer(visual: &mut VisualTestContext) -> Vec<(Bounds<Pixels>, f32)> {
    let row = visual
        .debug_bounds("viewer-row")
        .expect("the Viewer draws its control row");
    let slack = px(0.5);
    visual
        .update(|window, _| drawn::painted_pictures(window))
        .into_iter()
        .filter(|(bounds, _)| {
            bounds.left() >= row.left() - slack
                && bounds.right() <= row.right() + slack
                && bounds.bottom() <= row.top() + slack
        })
        .collect()
}

#[gpui::test]
fn each_seek_shows_its_frame_whole_on_the_first_frame_drawn_with_it(cx: &mut TestAppContext) {
    let (state, visual) = viewer_with_take(cx, "viewer-seek");
    // On before any picture is painted, so a body the shell replays holds the
    // markers its paint added.
    drawn::start();
    for front in ["Program", "a Source tab"] {
        change(visual, &state, |state| {
            if front == "Program" {
                state.set_playhead(state.store.sequence.duration_ticks / 2);
            } else {
                state.focus_source_tab(0);
                let tab = state.sources.focused_mut().expect("the Source tab");
                tab.tick = tab.duration_ticks / 2;
            }
        });
        settle(visual);
        for button in MOVES {
            let what = format!("{button} with {front} in front");
            std::thread::sleep(PAUSE);
            let before = visual.read(|app| position(state.read(app)));
            press(visual, button);
            let after = visual.read(|app| position(state.read(app)));
            assert_ne!(after, before, "{what} left the position where it was");
            until(visual, &state, &what, |state| shown(state, after));
            let pictures = in_viewer(visual);
            assert!(
                matches!(pictures.as_slice(), [(_, opacity)] if (opacity - 1.0).abs() < 1e-3),
                "{what}: the Viewer painted {pictures:?} on the first frame drawn with the new \
                 picture, not one picture at full opacity"
            );
        }
    }
    drawn::take();
}

/// Where the tab in front of the Viewer stands: the Source tab's tick, or the
/// playhead with Program in front.
fn position(state: &AppState) -> i64 {
    match state.sources.focused() {
        Some(tab) => tab.tick,
        None => state.playhead.value,
    }
}

/// Whether the frame at `at` is on screen for the tab in front.
fn shown(state: &AppState, at: i64) -> bool {
    match state.sources.focused() {
        Some(tab) => state.pool_preview.source(tab.media, at).is_some(),
        None => state.shown_tick == Some(at),
    }
}
