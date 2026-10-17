//! An edit made while paused asks for the edited frame, whichever control or
//! job made it.
//!
//! Closes the class "an edit that leaves the frame from before it in the
//! Viewer": the Effect Controls Add list committed an effect and asked for no
//! frame, so the Viewer kept the old picture until the playhead moved. Every edit
//! reaches `picture_changed`, and the shell's next draw asks for the frame, so
//! the request is read here through `preview_generation`, which every request
//! advances. The edits below are made on the state directly, the way a control
//! that asks for nothing makes them. Does not cover playback, whose
//! render-ahead restarts on the same edit, nor whether the decode succeeds.

use gpui::{TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::test_support::drawn_shell;

/// How many frames the Viewer has asked for.
fn requests(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> u64 {
    visual.read(|app| state.read(app).preview_generation)
}

/// Place the generator `id` at the playhead without asking for a frame, then
/// draw.
fn place(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext, id: &str) {
    visual.update(|window, app| {
        // A state change made outside the shell notifies nothing, so the window
        // is asked to draw again.
        window.refresh();
        state.update(app, |state, _| {
            state
                .add_effect_generator(id)
                .unwrap_or_else(|err| panic!("place {id}: {err:?}"));
        });
    });
    visual.run_until_parked();
}

#[gpui::test]
async fn the_first_draw_after_a_paused_edit_asks_for_the_frame_and_an_idle_draw_for_none(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "still-frame");
    visual.run_until_parked();
    let opened = requests(&state, visual);

    place(&state, visual, "color_matte");
    let placed = requests(&state, visual);
    assert!(
        placed > opened,
        "placing a clip while paused asked for no frame"
    );
    assert!(
        !visual.read(|app| state.read(app).still_frame_stale()),
        "the frame asked for left the mark set"
    );

    // A draw with nothing edited asks for nothing, so an idle editor renders
    // no frames.
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert_eq!(
        requests(&state, visual),
        placed,
        "a draw with nothing edited asked for a frame"
    );

    place(&state, visual, "checkerboard");
    assert!(
        requests(&state, visual) > placed,
        "an edit after the first asked for no frame"
    );
}
