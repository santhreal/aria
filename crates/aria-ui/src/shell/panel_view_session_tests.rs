//! A frame draws while a preview decode holds the session.
//!
//! The preview worker holds the engine for the length of every frame it
//! decodes: a few milliseconds warm, and over a second for the first decode of
//! a session, which waits for the hardware decoder to load. A draw that reads
//! the session waits for that decode. The timeline ruler asked for its render
//! bar by reading the engine's revision and the sequence's length, so the
//! window's first frame waited for the first picture before it came up, and an
//! In or an Out set while a picture decoded held the next frame until the
//! decode ended.
//!
//! The session is held on another thread while the window draws its first
//! frame, and again while it draws after each mark verb, which moves what the
//! ruler's render bar is keyed on without an edit. A draw that reads the
//! session waits for the holder, whose own wait runs out first. The first
//! frame is read as the window opens, before the tasks it started run: on the
//! test platform those run on the test's own thread, and the wait for the
//! picture that frame asked for is a wait for the session.
//!
//! A menu's rows are read on every frame the menu is open, so the session is
//! held again while the window draws with the Clip menu open over a selected
//! clip, and with a clip's badge menu open: the Clip menu's render and
//! restore rows read the selection's projection, and the badge menu reads the
//! facts it stored when it opened.
//!
//! Not covered: a frame after an edit, since the edit takes the session
//! itself; a panel the Editing layout does not show; a menu row no dressing
//! here reaches; and a read on another thread that the window waits for some
//! other way.

use std::sync::Arc;

use gpui::{Bounds, TestAppContext, point, px, size};

use crate::app::AppState;
use crate::dock::DockTree;
use crate::test_support::{
    drawn_shell, select_a_still, shell_state, shell_window_at, wait_out_splash, while_held,
};
use crate::types::{AppMenu, ContextTarget};

const WINDOW_W: f32 = 1920.0;

#[gpui::test]
fn the_first_frame_draws_while_the_session_is_held(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let state = shell_state(cx, "session-first-frame");
    let engine = cx.update(|app| Arc::clone(state.read(app).engine()));
    let screen = Bounds::new(point(px(0.0), px(0.0)), size(px(WINDOW_W), px(1080.0)));
    let mut ruler = None;
    let mut opened = None;
    let waited = while_held(&engine, || {
        let mut visual = shell_window_at(cx, &state, screen);
        ruler = visual.debug_bounds("timeline-ruler");
        opened = Some(visual);
    });
    if let Some(visual) = opened {
        visual.run_until_parked();
    }
    assert!(!waited, "the first frame waited for the session");
    assert!(ruler.is_some(), "the first frame drew no timeline ruler");
}

#[gpui::test]
fn a_frame_after_a_mark_draws_while_the_session_is_held(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "session-mark");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(WINDOW_W), px(1080.0)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            select_a_still(state);
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            cx.notify();
        })
    });
    wait_out_splash(visual);
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("timeline-ruler").is_some(),
        "the Editing layout drew no timeline ruler"
    );
    let engine = visual.read(|app| Arc::clone(state.read(app).engine()));
    let verbs: [(&str, fn(&mut AppState)); 4] = [
        ("an In set", AppState::set_mark_in),
        ("an Out set", AppState::set_mark_out),
        ("the In cleared", AppState::clear_mark_in),
        ("the Out cleared", AppState::clear_mark_out),
    ];
    for (what, verb) in verbs {
        let waited = while_held(&engine, || {
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    verb(state);
                    cx.notify();
                })
            });
            visual.run_until_parked();
        });
        assert!(!waited, "the frame after {what} waited for the session");
    }
}

#[gpui::test]
fn an_open_menu_draws_while_the_session_is_held(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "session-menu");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(WINDOW_W), px(1080.0)));
    let clip = visual.update(|_, app| {
        state.update(app, |state, cx| {
            select_a_still(state);
            // A saved project, so Render and Replace's row reads the clip.
            state.project_path = Some(std::path::PathBuf::from("session-menu.aria"));
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            cx.notify();
            state.selected_clip().expect("the still is selected")
        })
    });
    wait_out_splash(visual);
    visual.run_until_parked();
    let engine = visual.read(|app| Arc::clone(state.read(app).engine()));
    let menus: [(&str, fn(&mut AppState, aria_models::ClipId)); 2] = [
        ("the Clip menu", |state, _| {
            state.open_menu = Some(AppMenu::Clip)
        }),
        ("a badge menu", |state, clip| {
            let track = state
                .ui_clip_ref(clip)
                .expect("the still is projected")
                .track_id;
            let target = ContextTarget::ClipBadge {
                clip: clip.raw(),
                track,
            };
            state.open_context(target, 400.0, 400.0);
        }),
    ];
    for (what, open) in menus {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.open_menu = None;
                state.close_context();
                open(state, clip);
                cx.notify();
            })
        });
        visual.run_until_parked();
        let waited = while_held(&engine, || {
            visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
            visual.run_until_parked();
        });
        assert!(!waited, "a frame with {what} open waited for the session");
    }
}
