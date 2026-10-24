//! The splitter of the small-window view resizes the Timeline, and nothing
//! else.
//!
//! The class closed here: a gesture in the small-window view that wrote to
//! the workspace's dock tree, which the view does not draw. The splitter's
//! drag and its double-click set the weights of whichever split sat second
//! under the tree's root: in Editing, the row that holds the Timeline and
//! Audio Meters, so a drag widened Audio Meters to as much as three quarters
//! of the window, and the release recorded that as an edit to the workspace.
//! The view drew its two frames at fixed shares, so the drag moved nothing on
//! screen. Every gesture the splitter takes is run in a window under both
//! minimums: a drag by a distance, an Escape in the middle of a drag, a drag
//! to each edge of the window, a double-click, and a drag after the window is
//! made shorter than the saved height allows. Each must draw the Timeline at the
//! height it asks for, save the height it draws, keep the Viewer at its
//! minimum height or taller, draw the Viewer, the splitter and the Timeline
//! without overlap, and leave the dock tree and the recorded workspace edits
//! as they were.
//!
//! Not covered: a window under one minimum only, which draws the same view,
//! and a window too short for both frames' minimums.

use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, Pixels, TestAppContext, VisualTestContext,
    point, px, size,
};

use super::tab_overflow_tests::{frame_bounds, redraw};
use crate::app::AppState;
use crate::dock::{DockTree, Frame, FrameId, Node, PANEL_VIEWER};
use crate::settings::{
    MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, TIMELINE_DEFAULT_HEIGHT, TIMELINE_MIN_HEIGHT,
};
use crate::test_support::drawn_shell_at;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// How the pointer leaves the splitter once it has moved.
#[derive(Clone, Copy)]
enum End {
    Release,
    Escape,
}

/// What every gesture must leave as it was, and the Viewer's floor.
struct Baseline {
    tree: DockTree,
    edits: std::collections::BTreeMap<String, DockTree>,
    viewer_min: f32,
}

impl Baseline {
    fn read(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Self {
        visual.read(|app| {
            let state = state.read(app);
            let viewer = Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER]));
            Self {
                tree: state.dock_tree.clone(),
                edits: state.ui_settings.workspace_trees.clone(),
                viewer_min: state.node_min_size(&viewer).1,
            }
        })
    }

    /// Check the view after `gesture`, which asked for a Timeline `wanted`
    /// px tall, and return the Timeline's and the Viewer's drawn heights.
    /// `saves_drawn` is false after a window resize, which keeps the saved
    /// height when it no longer fits.
    fn check(
        &self,
        state: &gpui::Entity<AppState>,
        visual: &mut VisualTestContext,
        gesture: &str,
        wanted: Option<f32>,
        saves_drawn: bool,
    ) -> (f32, f32) {
        let timeline = frame_bounds(state, visual, FrameId::BOTTOM);
        let viewer = frame_bounds(state, visual, FrameId::CENTER);
        let split = splitter(visual);
        let (timeline_h, viewer_h) = (
            f32::from(timeline.size.height),
            f32::from(viewer.size.height),
        );
        if let Some(wanted) = wanted {
            assert!(
                (timeline_h - wanted).abs() <= EPS,
                "after {gesture} the Timeline is drawn {timeline_h} px tall, not {wanted}"
            );
        }
        let saved = visual.read(|app| state.read(app).ui_settings.timeline_height);
        assert!(
            !saves_drawn || (saved - timeline_h).abs() <= EPS,
            "after {gesture} the Timeline is drawn {timeline_h} px tall and saved as {saved}"
        );
        assert!(
            viewer_h >= self.viewer_min - EPS,
            "after {gesture} the Viewer is drawn {viewer_h} px tall, under its {} px minimum",
            self.viewer_min
        );
        let edge = |at: Pixels| f32::from(at);
        assert!(
            edge(viewer.bottom()) <= edge(split.top()) + EPS
                && edge(split.bottom()) <= edge(timeline.top()) + EPS,
            "after {gesture} the Viewer {viewer:?}, the splitter {split:?} and the Timeline \
             {timeline:?} overlap"
        );
        let (tree, edits) = visual.read(|app| {
            let state = state.read(app);
            (
                state.dock_tree.clone(),
                state.ui_settings.workspace_trees.clone(),
            )
        });
        assert!(
            tree == self.tree,
            "{gesture} changed the workspace's dock tree, which the small-window view does \
             not draw"
        );
        assert!(
            edits == self.edits,
            "{gesture} recorded an edit to the workspace"
        );
        (timeline_h, viewer_h)
    }
}

/// Where the splitter is drawn.
fn splitter(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    visual
        .debug_bounds("split-timeline")
        .expect("a window under the minimum size draws the splitter")
}

/// Press the splitter, move the pointer to `y` in the window, and end the
/// drag by `end`.
fn drag_splitter_to(visual: &mut VisualTestContext, y: f32, end: End) {
    let from = splitter(visual).center();
    let to = point(from.x, px(y));
    visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    if let End::Escape = end {
        visual.simulate_keystrokes("escape");
    }
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn the_small_window_splitter_resizes_the_timeline_and_nothing_else(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (width, height) = (MIN_WINDOW_WIDTH - 24.0, MIN_WINDOW_HEIGHT - 20.0);
    let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
    let (state, mut visual) = drawn_shell_at(cx, "small-window-splitter", at);
    let visual = &mut visual;
    visual.update(|_, app| app.set_reduce_motion(true));
    redraw(&state, visual);
    let mut baseline = Baseline::read(&state, visual);

    let (opened, _) = baseline.check(
        &state,
        visual,
        "opening",
        Some(TIMELINE_DEFAULT_HEIGHT),
        true,
    );

    let y = f32::from(splitter(visual).center().y);
    drag_splitter_to(visual, y - 40.0, End::Release);
    let (taller, _) = baseline.check(&state, visual, "a drag 40 px up", Some(opened + 40.0), true);

    let y = f32::from(splitter(visual).center().y);
    drag_splitter_to(visual, y + 30.0, End::Escape);
    baseline.check(&state, visual, "Escape during a drag", Some(taller), true);

    drag_splitter_to(visual, height - 1.0, End::Release);
    baseline.check(
        &state,
        visual,
        "a drag to the bottom of the window",
        Some(TIMELINE_MIN_HEIGHT),
        true,
    );

    let at = splitter(visual).center();
    visual.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    baseline.check(
        &state,
        visual,
        "a double-click",
        Some(TIMELINE_DEFAULT_HEIGHT),
        true,
    );

    drag_splitter_to(visual, 1.0, End::Release);
    let (tallest, viewer) = baseline.check(
        &state,
        visual,
        "a drag to the top of the window",
        None,
        true,
    );
    assert!(
        (viewer - baseline.viewer_min).abs() <= EPS,
        "a drag to the top of the window left the Viewer {viewer} px tall, not its {} px \
         minimum",
        baseline.viewer_min
    );

    // A resize is not a gesture on the splitter, and may lay the workspace
    // out again for the new size.
    visual.simulate_resize(size(px(width), px(height - 60.0)));
    visual.run_until_parked();
    redraw(&state, visual);
    baseline = Baseline::read(&state, visual);
    let (shorter, _) = baseline.check(
        &state,
        visual,
        "a window 60 px shorter",
        Some(tallest - 60.0),
        false,
    );

    // The drag starts from the Timeline as drawn, not from the taller height
    // saved before the window shrank.
    let y = f32::from(splitter(visual).center().y);
    drag_splitter_to(visual, y + 20.0, End::Release);
    baseline.check(
        &state,
        visual,
        "a drag 20 px down in the shorter window",
        Some(shorter - 20.0),
        true,
    );
}
