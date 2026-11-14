//! Every registered overlay closed by Escape plays its exit to the end and
//! leaves the screen.
//!
//! The class this closes: an exit that stops after its first frame. GPUI keeps a
//! one-shot animation's clock by element id, and an overlay whose exit drew
//! under its entrance's id started the exit from the finished entrance: the
//! exit drew one frame at its start, requested no other, and the overlay stayed
//! on screen with its scrim taking the pointer until some other input redrew
//! the window. The overlays are read from the registry, so one registered later
//! is checked on the run that adds it, and a new [`OverlayWhen`] does not
//! compile here until it says how it opens.
//!
//! Not caught: an exit that animates but draws the wrong picture, and an
//! overlay whose exit something else in the window keeps redrawing.

use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};

use super::{OverlayWhen, host_of};
use crate::app::AppState;
use crate::types::{ContextTarget, RenameTarget};

/// One frame at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);

/// Frames an overlay may keep requesting after it opened or closed: about a
/// second, far past the end of every entrance and exit.
const SETTLE_FRAMES: usize = 64;

/// Delivers the frames the window requests until it requests none, one 60 Hz
/// interval of clock per frame, as the platform's frame loop does. Returns how
/// many were delivered.
///
/// # Panics
///
/// Panics when the window still requests frames after [`SETTLE_FRAMES`]: an
/// animation that never ends is a defect of its own.
fn settle(visual: &mut VisualTestContext, id: &str) -> usize {
    visual.run_until_parked();
    for frame in 0..SETTLE_FRAMES {
        visual.executor().advance_clock(FRAME);
        let ran = visual.update(|window, app| window.simulate_next_frame(app));
        visual.run_until_parked();
        if ran == 0 {
            return frame;
        }
    }
    panic!("{id}: the window still requests frames {SETTLE_FRAMES} frames later");
}

/// Puts the overlay for `when` on screen.
fn open(when: OverlayWhen, state: &mut AppState) {
    match when {
        OverlayWhen::Dialog(kind) => state.open_dialog(kind),
        OverlayWhen::ContextMenu => state.open_context(ContextTarget::Preview, 400.0, 300.0),
        OverlayWhen::Rename => state.begin_rename(RenameTarget::Project),
    }
}

/// Whether the condition `when` names holds on `state`.
fn holds(when: OverlayWhen, state: &AppState) -> bool {
    match when {
        OverlayWhen::Dialog(kind) => state.dialog == kind,
        OverlayWhen::ContextMenu => state.context_menu.is_some(),
        OverlayWhen::Rename => state.rename.is_some(),
    }
}

/// The debug selector of the card the overlay for `when` draws, where it has
/// one.
fn card(when: OverlayWhen) -> Option<&'static str> {
    match when {
        OverlayWhen::Dialog(_) => Some("dialog-card"),
        OverlayWhen::ContextMenu => Some("context-menu-card"),
        OverlayWhen::Rename => None,
    }
}

#[gpui::test]
fn every_overlay_closed_by_escape_plays_its_exit_and_leaves(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "overlay-exit");
    // The boot cover fades on its own frames, which would draw a stopped exit
    // forward.
    crate::test_support::wait_out_splash(visual);
    let host = visual
        .read(|app| host_of(&state.read(app).ctx).expect("the ui plugin mounts the overlay host"));
    let overlays = host.entries_meta();
    assert!(
        overlays
            .iter()
            .any(|(_, when)| matches!(when, OverlayWhen::Dialog(_)))
            && overlays
                .iter()
                .any(|(_, when)| *when == OverlayWhen::ContextMenu)
            && overlays
                .iter()
                .any(|(_, when)| *when == OverlayWhen::Rename),
        "the shipped overlays did not register: {overlays:?}"
    );

    let mut stuck = Vec::new();
    for (id, when) in overlays {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                open(when, state);
                cx.notify();
            })
        });
        settle(visual, id);
        if !visual.read(|app| holds(when, state.read(app))) {
            stuck.push(format!("{id}: did not open"));
            continue;
        }

        visual.simulate_keystrokes("escape");
        assert!(
            !visual.read(|app| holds(when, state.read(app))),
            "{id}: Escape left it open"
        );
        let frames = settle(visual, id);
        let leaving = host.exit_t(when);
        let drawn = card(when).and_then(|selector| visual.debug_bounds(selector));
        // A 100 ms exit at 60 Hz is six frames; one is an exit that stopped.
        if frames < 2 || leaving.is_some() || drawn.is_some() {
            stuck.push(format!(
                "{id}: the exit drew {frames} frames after Escape, then stood at {leaving:?} \
                 with the card at {drawn:?}"
            ));
        }
    }
    assert!(
        stuck.is_empty(),
        "an overlay closed by Escape did not leave the screen:\n  {}",
        stuck.join("\n  ")
    );
}
