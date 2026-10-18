//! Closing a floating panel window from the window manager.
//!
//! The class closed here: a floating frame whose window the window manager
//! closed (its title bar close, Alt+F4) stayed in `dock_tree.floating` with no
//! window. Its panels were neither docked nor drawn, the Window menu listed
//! them as open, and the first toggle closed what could not be seen. These pin
//! that the window manager's close takes the frame out of the tree and that
//! one toggle then opens the panel in its default frame.
//!
//! Not covered: the main window's close, which goes through the unsaved
//! prompt (`unsaved_tests.rs`), and the in-window Close Window button, which
//! calls the same `AppState::close_floating_frame`.

use gpui::{AnyWindowHandle, Entity, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::dock::{FrameId, PANEL_MIXER};
use crate::test_support::drawn_shell;

/// Float the Mixer and return its frame and the window the shell opened for it.
fn floated(state: &Entity<AppState>, visual: &mut VisualTestContext) -> (FrameId, AnyWindowHandle) {
    let frame = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .float_dock_panel(PANEL_MIXER, (100.0, 100.0, 480.0, 320.0), cx)
                .expect("the Mixer floats")
        })
    });
    visual.run_until_parked();
    let main = visual.update(|window, _| window.window_handle());
    let others: Vec<AnyWindowHandle> = visual
        .cx
        .windows()
        .into_iter()
        .filter(|window| *window != main)
        .collect();
    assert_eq!(
        others.len(),
        1,
        "the shell opens one window for the floated frame"
    );
    (frame, others[0])
}

/// Close `window` the way the window manager does, through its should-close
/// handler.
///
/// `VisualTestContext::simulate_close` runs the handler, then installs it on
/// the window again. Under test support every dirty window draws before the
/// handler returns, and a floating shell whose frame has gone removes its own
/// window on that draw, so the reinstall fails with "window not found" after
/// the handler has run. That failure is the one caught; any other panic
/// propagates.
fn close_from_window_manager(window: AnyWindowHandle, visual: &VisualTestContext) {
    let mut floating = VisualTestContext::from_window(window, &visual.cx);
    let closed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| floating.simulate_close()));
    match closed {
        Ok(accepted) => assert!(accepted, "the window manager's close is not handled"),
        Err(payload) => {
            let text = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or_default();
            if !text.contains("window not found") {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

#[gpui::test]
fn the_window_managers_close_takes_the_frame_out_of_the_tree(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "float-wm-close");
    let (frame, window) = floated(&state, visual);

    close_from_window_manager(window, visual);
    visual.run_until_parked();

    visual.read(|app| {
        let tree = &state.read(app).dock_tree;
        assert!(
            tree.floating.iter().all(|fl| fl.frame.id != frame),
            "the closed window's frame is still floating"
        );
        assert!(
            !tree.is_open(PANEL_MIXER),
            "the Mixer is listed as open with no window to draw it"
        );
    });
}

#[gpui::test]
fn after_the_window_managers_close_one_toggle_opens_the_panel_docked(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "float-wm-reopen");
    let (_, window) = floated(&state, visual);

    close_from_window_manager(window, visual);
    visual.run_until_parked();

    visual.update(|_, app| {
        state.update(app, |state, _| state.toggle_dock_panel(PANEL_MIXER));
    });
    visual.run_until_parked();
    visual.read(|app| {
        let tree = &state.read(app).dock_tree;
        assert_eq!(
            tree.find(PANEL_MIXER),
            Some(FrameId::CENTER),
            "one Window menu toggle opens the Mixer in its default frame"
        );
        assert!(tree.floating.is_empty(), "nothing is left floating");
    });
}
