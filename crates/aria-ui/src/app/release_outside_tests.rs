//! A gesture the shell holds ends when the button comes up, and so does
//! everything it drew.
//!
//! The press began in the window, so the platform sends the release to it
//! wherever the pointer is by then. The class closed here: a release that
//! lands on nothing the shell drew and so reaches no handler, which leaves the
//! gesture held, and a release that ends the gesture but leaves the dock's
//! drop zones drawn, because the hovered zone is shared with every floating
//! panel window and outlived the drag. The wheel's case is in
//! `grade_controls_tests`; this file drives a dock tab, the gesture whose
//! release outside the window has a result of its own: the panel floats where
//! it was let go, on the screen.
//!
//! Not covered: a release the platform never delivers, such as one a window
//! manager's grab takes, and a tab dragged from a floating panel window, whose
//! shell installs the same pair of release handlers.

use gpui::{Bounds, Modifiers, MouseButton, Pixels, Point, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::dock::{FrameId, PanelId};
use crate::theme::Theme;

/// The frames the shell painted that hold a tab, left to right and then top
/// to bottom, by their painted bounds.
fn drawn_frames(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(FrameId, [f32; 4])> {
    visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        let mut drawn: Vec<(FrameId, [f32; 4])> = rects
            .iter()
            .filter(|(id, _)| {
                state
                    .dock_tree
                    .frame(**id)
                    .is_some_and(|f| !f.tabs.is_empty())
            })
            .map(|(id, rect)| (*id, *rect))
            .collect();
        drawn.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]).then(a.1[1].total_cmp(&b.1[1])));
        drawn
    })
}

/// Press the first tab of `frame`, carry it to `over`, and let go at
/// `release`. Returns the panel the tab showed. Fails when the drop zones
/// are not drawn while the tab is carried, or are still drawn once it is let
/// go.
fn carry_first_tab(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    (frame, [fx, fy, _, _]): (FrameId, [f32; 4]),
    over: Point<Pixels>,
    release: Point<Pixels>,
) -> PanelId {
    // The first tab, inside the frame's border and its tab row.
    let tab = point(px(fx + 16.), px(fy + 1. + Theme::TAB_ROW_HEIGHT / 2.));
    visual.simulate_mouse_down(tab, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let panel = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frame(frame)
            .map(|f| f.active_panel())
            .expect("the pressed frame is still docked")
    });
    visual.simulate_mouse_move(over, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("dock-drop-zones").is_some(),
        "{panel} carried over a frame draws no drop zones"
    );
    visual.simulate_mouse_up(release, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("dock-drop-zones").is_none(),
        "{panel} let go at {release:?}: the drop zones are still drawn"
    );
    panel
}

#[gpui::test]
fn a_tab_let_go_outside_the_window_floats_where_it_was_let_go(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    // Off the screen's corner, so a window position and a screen position
    // differ on both axes.
    let at = Bounds::new(point(px(200.), px(120.)), size(px(1440.), px(900.)));
    let (state, mut visual) = crate::test_support::drawn_shell_at(cx, "release-outside", at);
    let visual = &mut visual;
    let pressed = drawn_frames(&state, visual)
        .first()
        .copied()
        .expect("the shell draws a docked frame with a tab");
    let origin = visual.update(|window, _| window.bounds().origin);
    assert_eq!(origin, at.origin, "the window opened where it was asked to");
    let [fx, fy, _, _] = pressed.1;
    let inside = point(px(fx + 76.), px(fy + 1. + Theme::TAB_ROW_HEIGHT / 2. + 60.));
    // Left of the window.
    let release = point(px(-10.), inside.y);
    let panel = carry_first_tab(&state, visual, pressed, inside, release);
    let floated = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .floating
            .iter()
            .find(|fl| fl.frame.tabs.contains(&panel))
            .map(|fl| (fl.x, fl.y))
    });
    let floated = floated.unwrap_or_else(|| {
        panic!("{panel} let go outside the window did not float; the drag outlived the release")
    });
    assert_eq!(
        floated,
        (
            f32::from(origin.x + release.x),
            f32::from(origin.y + release.y)
        ),
        "{panel} floated at a window position, not the screen position it was let go at"
    );
}

/// A tab let go on another frame's drop zone docks there, and the zones go
/// with the drag.
#[gpui::test]
fn a_tab_docked_on_another_frame_leaves_no_drop_zones_drawn(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "release-docked");
    let frames = drawn_frames(&state, visual);
    let pressed = *frames
        .first()
        .expect("the shell draws a docked frame with a tab");
    // The rightmost frame, away from the frame the tab leaves.
    let (target, [tx, ty, tw, th]) = *frames
        .last()
        .filter(|(id, _)| *id != pressed.0)
        .expect("the shell draws a second docked frame with a tab");
    // The target's right drop zone: past 78 per cent of its width, below
    // its tab row.
    let right = point(px(tx + tw * 0.9), px(ty + th * 0.5));
    let panel = carry_first_tab(&state, visual, pressed, right, right);
    let (home, floating) = visual.read(|app| {
        let tree = &state.read(app).dock_tree;
        (
            tree.find(panel),
            tree.floating
                .iter()
                .any(|fl| fl.frame.tabs.contains(&panel)),
        )
    });
    assert!(!floating, "{panel} let go on {target:?} floated");
    assert!(
        home.is_some_and(|frame| frame != pressed.0 && frame != target),
        "{panel} let go on the right zone of {target:?} is in {home:?}, not a frame of its own"
    );
}
