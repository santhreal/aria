//! A drag ends on the release of the button that pressed it, and on the first
//! move without that button; Ctrl and the wheel zoom the Viewer about the
//! pointer.
//!
//! The class closed here: a pan pressed with a button other than the left
//! one. The middle button's release reached no handler, so a middle-button pan
//! of the timeline stayed held after the button came up and every later move
//! with no button down scrolled the timeline. Each pan, by each button that
//! presses one (the middle button on the timeline and the Viewer, the left
//! with the Hand tool on the timeline and on a zoomed Viewer), is pressed,
//! carried, and ended by its own button's release and, in a second pass, by a
//! move without the button when the window missed the release. A stray
//! release of the other button mid-drag leaves the pan going; after the end,
//! Escape, a drag with the same button on the other view pressed with no move
//! between, and a move with no button down each leave the view where the pan
//! put it.
//!
//! Not covered: the splitters', a lane border's and a tab's drags, which only
//! the left button presses (`DragKind::button` must name a button for each
//! kind) and whose releases are in `held_drag_tests` and
//! `held_drag_panel_tests`.

use gpui::{
    Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase,
    VisualTestContext, point, px,
};

use super::held_drag_panel_tests::{hover, settle};
use super::held_drag_tests::bounds;
use crate::app::AppState;
use crate::app::canvas_overlay::Rect;
use crate::app::preview_view::ZOOM_STEPS;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::types::EditorTool;

/// Where a pan starts the timeline's scroll: far enough from the start that
/// a pan either way moves it.
const START_SCROLL: f32 = 300.0;

/// The view a pan moves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    Timeline,
    Viewer,
}

impl Surface {
    fn other(self) -> Self {
        match self {
            Surface::Timeline => Surface::Viewer,
            Surface::Viewer => Surface::Timeline,
        }
    }
}

/// Each view, with each button that pans it.
const PANS: [(Surface, MouseButton); 4] = [
    (Surface::Timeline, MouseButton::Middle),
    (Surface::Timeline, MouseButton::Left),
    (Surface::Viewer, MouseButton::Middle),
    (Surface::Viewer, MouseButton::Left),
];

/// What `surface` shows, as two numbers: the timeline's scroll and zero, or
/// the Viewer's pan.
fn view(visual: &mut VisualTestContext, state: &Entity<AppState>, surface: Surface) -> [f32; 2] {
    visual.read(|app| {
        let state = state.read(app);
        match surface {
            Surface::Timeline => [state.scroll_x, 0.0],
            Surface::Viewer => [state.preview_view.pan.0, state.preview_view.pan.1],
        }
    })
}

/// Both views where a pan moves them either way: the timeline at its widest
/// zoom and scrolled from its start, the Viewer at its top level and
/// centered, and the Hand tool, so the left button pans the timeline.
fn ready(visual: &mut VisualTestContext, state: &Entity<AppState>) {
    change(visual, state, |state| {
        state.clear_selection();
        state.tool = EditorTool::Hand;
        state.set_zoom(crate::transport::ZOOM_MAX, false);
        state.scroll_timeline_to(START_SCROLL, false);
        state.preview_view.zoom = ZOOM_STEPS.last().copied();
        state.preview_view.pan = (0.0, 0.0);
    });
    settle(visual);
}

/// The Viewer's picture area, in window pixels.
fn picture(visual: &mut VisualTestContext, state: &Entity<AppState>) -> Rect {
    visual.read(|app| {
        state
            .read(app)
            .overlay_frame
            .container
            .expect("the Viewer records its picture area")
    })
}

/// A pan's path across `surface`: where it is pressed, where it passes and
/// where it ends, and what the view reads at the last two.
fn path(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    surface: Surface,
) -> ([Point<Pixels>; 3], [[f32; 2]; 2]) {
    match surface {
        Surface::Timeline => {
            let from = bounds(visual, "timeline-video-canvas").center();
            let at = |dx: f32| point(from.x + px(dx), from.y);
            (
                [from, at(50.0), at(100.0)],
                [[START_SCROLL - 50.0, 0.0], [START_SCROLL - 100.0, 0.0]],
            )
        }
        Surface::Viewer => {
            let pane = picture(visual, state);
            let [x, y] = [pane.x + pane.w / 2.0, pane.y + pane.h / 2.0];
            let at = |dx: f32, dy: f32| point(px(x + dx), px(y + dy));
            (
                [at(0.0, 0.0), at(15.0, 10.0), at(30.0, 20.0)],
                [[15.0, 10.0], [30.0, 20.0]],
            )
        }
    }
}

fn press(visual: &mut VisualTestContext, at: Point<Pixels>, button: MouseButton) {
    hover(visual, at);
    visual.simulate_mouse_down(at, button, Modifiers::none());
    visual.run_until_parked();
}

fn carry(visual: &mut VisualTestContext, to: Point<Pixels>, button: MouseButton) {
    visual.simulate_mouse_move(to, button, Modifiers::none());
    visual.run_until_parked();
}

fn release(visual: &mut VisualTestContext, at: Point<Pixels>, button: MouseButton) {
    visual.simulate_mouse_up(at, button, Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn each_pan_ends_on_the_release_of_the_button_that_pressed_it(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "pan-release");
    visual.update(|_, app| app.set_reduce_motion(true));
    for (surface, button) in PANS {
        let stray = match button {
            MouseButton::Left => MouseButton::Middle,
            _ => MouseButton::Left,
        };
        for missed in [false, true] {
            let label = format!(
                "a {surface:?} pan with the {button:?} button, {}",
                if missed {
                    "its release missed"
                } else {
                    "released"
                }
            );
            ready(visual, &state);
            let ([from, mid, to], [at_mid, at_to]) = path(visual, &state, surface);

            press(visual, from, button);
            carry(visual, mid, button);
            assert_eq!(view(visual, &state, surface), at_mid, "{label}: the carry");
            // A button whose press went elsewhere comes up mid-drag.
            release(visual, mid, stray);
            carry(visual, to, button);
            assert_eq!(
                view(visual, &state, surface),
                at_to,
                "{label}: a carry after the {stray:?} button came up"
            );

            // The release, or the move without the button that stands for the
            // release the window missed.
            if missed {
                hover(visual, from);
            } else {
                release(visual, to, button);
            }
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
            assert_eq!(
                view(visual, &state, surface),
                at_to,
                "{label}: Escape after it ended"
            );

            // The same button pans the other view, pressed with no move
            // between, as a pen or a touch does, and this one stays put.
            // Escape gave the Pointer back, and the left button pans the
            // timeline with the Hand tool.
            change(visual, &state, |state| state.tool = EditorTool::Hand);
            let other = surface.other();
            let ([other_from, _, other_at], [_, other_to]) = path(visual, &state, other);
            visual.simulate_mouse_down(other_from, button, Modifiers::none());
            visual.run_until_parked();
            carry(visual, other_at, button);
            assert_eq!(
                view(visual, &state, other),
                other_to,
                "{label}: the {other:?} pan that followed"
            );
            release(visual, other_at, button);
            assert_eq!(
                view(visual, &state, surface),
                at_to,
                "{label}: a {other:?} pan with the same button that followed"
            );
            hover(visual, from);
            assert_eq!(
                view(visual, &state, surface),
                at_to,
                "{label}: a move with no button down after it ended"
            );
        }
    }
}

#[gpui::test]
fn ctrl_and_the_wheel_over_the_viewer_zoom_a_level_a_notch_about_the_pointer(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "viewer-wheel-zoom");
    change(visual, &state, |state| {
        state.preview_view.zoom = Some(4.0);
        state.preview_view.pan = (0.0, 0.0);
    });
    settle(visual);
    let pane = picture(visual, &state);
    // Off the pane's center, so a zoom about the center moves the canvas
    // point under it.
    let at = [pane.x + pane.w * 0.375, pane.y + pane.h * 0.625];
    let read = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            let frame = state.overlay_frame.canvas.expect("the frame is placed");
            (
                state.preview_view.zoom,
                [(at[0] - frame.x) / frame.w, (at[1] - frame.y) / frame.h],
                [frame.w, frame.h],
            )
        })
    };
    let (_, held, _) = read(visual);
    let turn = |visual: &mut VisualTestContext, delta: ScrollDelta, modifiers: Modifiers| {
        visual.simulate_event(ScrollWheelEvent {
            position: point(px(at[0]), px(at[1])),
            delta,
            modifiers,
            touch_phase: TouchPhase::Moved,
        });
        settle(visual);
    };
    let ctrl = Modifiers::secondary_key();
    let notches = |n: f32| ScrollDelta::Lines(point(0.0, n));
    let quarter = ScrollDelta::Pixels(point(px(0.0), px(crate::transport::WHEEL_LINE_PX / 4.0)));

    // Each turn, the level it lands on, and whether the frame there is larger
    // than the pane, so the pan holds the point under the pointer.
    for (delta, modifiers, zoom, holds, label) in [
        (notches(1.0), ctrl, 8.0, true, "a notch up zooms in a level"),
        (notches(1.0), ctrl, 8.0, true, "a notch up at the top level"),
        (
            notches(-1.0),
            ctrl,
            4.0,
            true,
            "a notch down zooms out a level",
        ),
        (notches(1.0), Modifiers::none(), 4.0, true, "a plain wheel"),
        (quarter, ctrl, 4.0, true, "a quarter notch on a trackpad"),
        (quarter, ctrl, 4.0, true, "half a notch on a trackpad"),
        (quarter, ctrl, 4.0, true, "three quarters of a notch"),
        (quarter, ctrl, 8.0, true, "a whole notch on a trackpad"),
        (
            notches(-2.0),
            ctrl,
            2.0,
            false,
            "two notches step two levels",
        ),
    ] {
        turn(visual, delta, modifiers);
        let (now, under, size) = read(visual);
        assert_eq!(now, Some(zoom), "{label}");
        let larger = size[0] > pane.w && size[1] > pane.h;
        assert_eq!(
            larger, holds,
            "{label}: the frame is {size:?} in the {}x{} pane",
            pane.w, pane.h
        );
        if holds {
            assert!(
                (under[0] - held[0]).abs() < 1e-3 && (under[1] - held[1]).abs() < 1e-3,
                "{label}: the pointer is over {under:?} of the frame, not {held:?}"
            );
        } else {
            let pan = visual.read(|app| state.read(app).preview_view.pan);
            assert_eq!(
                pan,
                (0.0, 0.0),
                "{label}: a frame inside the pane stays centered"
            );
        }
    }
}
