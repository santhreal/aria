//! The work area bar under the timeline ruler.
//!
//! The class closed here: a pointer gesture on the bar that moves the wrong
//! point, moves by a travel read at the wrong zoom, lets the range leave the
//! sequence or collapse to nothing, keeps moving after Escape, or stops at the
//! bar's 8 px edge. Every grip is driven through the same press, drag and
//! release, at two zooms, and the drawn test drives the bar the shell draws.
//!
//! Not covered: the bar's colours, which a snapshot owns.

use gpui::{Modifiers, MouseButton, MouseDownEvent, VisualTestContext, point, px};

use super::WorkAreaGrip;
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;
use crate::transport::TIMELINE_BASE_PPS;

/// The window pixels `frames` span on the timeline at the state's zoom.
fn travel(state: &AppState, frames: i64) -> f32 {
    frames as f32 * TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second()
}

fn marks(state: &AppState) -> (Option<i64>, Option<i64>) {
    (state.mark_in, state.mark_out)
}

/// A state with In at 10 and Out at 30 and a sequence well past both.
fn marked() -> (AppState, i64) {
    let (mut state, _) = with_clip();
    let duration = state.store.sequence.duration_ticks;
    assert!(duration > 60, "the fixture sequence is {duration} ticks");
    (state.mark_in, state.mark_out) = (Some(10), Some(30));
    (state, duration)
}

#[test]
fn each_grip_moves_its_points_by_the_pointer_travel_and_stops_at_the_sequence() {
    // (grip, frames dragged, In and Out after)
    let duration = marked().1;
    let cases = [
        (WorkAreaGrip::Bar, 5, (15, 35)),
        (WorkAreaGrip::Bar, -4, (6, 26)),
        // The bar keeps its length against either end of the sequence.
        (WorkAreaGrip::Bar, -50, (0, 20)),
        (WorkAreaGrip::Bar, duration * 3, (duration - 20, duration)),
        (WorkAreaGrip::Start, 8, (18, 30)),
        (WorkAreaGrip::Start, -50, (0, 30)),
        // An end stops one frame short of the other.
        (WorkAreaGrip::Start, 50, (29, 30)),
        (WorkAreaGrip::End, -8, (10, 22)),
        (WorkAreaGrip::End, -50, (10, 11)),
        (WorkAreaGrip::End, duration * 3, (10, duration)),
    ];
    for zoom in [1.0, 2.5] {
        for (grip, frames, (start, end)) in cases {
            let (mut state, _) = marked();
            state.zoom = zoom;
            state.press_work_area(grip, 400.0);
            assert!(state.work_area_held(), "{grip:?} held after the press");
            let moved = state.drag_work_area(400.0 + travel(&state, frames));
            assert!(
                moved,
                "{grip:?} by {frames} frames at zoom {zoom} moved nothing"
            );
            assert_eq!(
                marks(&state),
                (Some(start), Some(end)),
                "{grip:?} by {frames} frames at zoom {zoom}"
            );
            assert!(state.release_work_area(), "{grip:?} let go");
            assert!(!state.work_area_held());
            assert_eq!(
                marks(&state),
                (Some(start), Some(end)),
                "letting go of {grip:?} keeps the range"
            );
        }
    }
}

#[test]
fn a_drag_reads_its_travel_from_the_press_not_from_the_last_move() {
    let (mut state, _) = marked();
    state.press_work_area(WorkAreaGrip::Bar, 400.0);
    for frames in [3, 7, 2, 7] {
        state.drag_work_area(400.0 + travel(&state, frames));
        assert_eq!(marks(&state), (Some(10 + frames), Some(30 + frames)));
    }
    assert!(
        !state.drag_work_area(400.0 + travel(&state, 7)),
        "a move that lands on the same frames reports no change"
    );
}

#[test]
fn an_unmarked_range_starts_from_the_whole_sequence() {
    let (mut state, _) = with_clip();
    let duration = state.store.sequence.duration_ticks;
    state.press_work_area(WorkAreaGrip::End, 400.0);
    state.drag_work_area(400.0 - travel(&state, 10));
    assert_eq!(marks(&state), (Some(0), Some(duration - 10)));
}

#[test]
fn escape_puts_the_range_back_and_ends_the_press() {
    for grip in [WorkAreaGrip::Bar, WorkAreaGrip::Start, WorkAreaGrip::End] {
        let (mut state, _) = marked();
        state.press_work_area(grip, 400.0);
        state.drag_work_area(400.0 + travel(&state, 6));
        assert_ne!(marks(&state), (Some(10), Some(30)), "{grip:?} moved");
        assert!(state.cancel_work_area_drag(), "{grip:?} cancelled");
        assert_eq!(marks(&state), (Some(10), Some(30)), "{grip:?} put back");
        assert!(!state.work_area_held());
        assert!(
            !state.drag_work_area(400.0 + travel(&state, 9)),
            "{grip:?} still moved after Escape"
        );
        assert!(
            !state.cancel_work_area_drag(),
            "a second Escape has no press"
        );
        assert_eq!(marks(&state), (Some(10), Some(30)));
    }
}

#[test]
fn a_double_click_sets_the_work_area_to_the_sequence() {
    let (mut state, duration) = marked();
    state.work_area_to_sequence();
    assert_eq!(marks(&state), (Some(0), Some(duration)));
    assert_eq!(state.work_area(), (0, duration));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Work area set to the sequence")
    );
}

/// The centre of the element drawn under `selector`.
fn centre(visual: &mut VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the timeline draws {selector}"))
        .center()
}

#[gpui::test]
fn the_drawn_bar_moves_its_ends_and_the_whole_range_and_fills_the_sequence_on_a_double_click(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "work-area-bar");
    change(visual, &state, |state| {
        (state.mark_in, state.mark_out) = (Some(12), Some(36));
    });
    let read = |visual: &mut VisualTestContext| visual.read(|app| marks(state.read(app)));
    // One frame's travel at the timeline's zoom, read once: the drag below
    // does not zoom.
    let frame = visual.read(|app| travel(state.read(app), 1));
    let frames = |n: i64| point(px(frame * n as f32), px(0.0));

    // The end grip moves Out, and keeps moving it once the pointer has left
    // the bar.
    let end = centre(visual, "work-area-end");
    visual.simulate_mouse_down(end, MouseButton::Left, Modifiers::none());
    let left = end - frames(6);
    visual.simulate_mouse_move(left, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(read(visual), (Some(12), Some(30)), "the end grip moved Out");
    let below = end - frames(9) + point(px(0.0), px(60.0));
    visual.simulate_mouse_move(below, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        read(visual),
        (Some(12), Some(27)),
        "the drag stopped at the bar's edge"
    );
    visual.simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert!(
        !visual.read(|app| state.read(app).work_area_held()),
        "let go"
    );

    // The start grip moves In.
    let start = centre(visual, "work-area-start");
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(start + frames(3), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(start + frames(3), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        read(visual),
        (Some(15), Some(27)),
        "the start grip moved In"
    );

    // The bar moves both, and Escape puts them back with the button down.
    let playhead = visual.read(|app| state.read(app).playhead.value);
    let bar = centre(visual, "timeline-work-area");
    visual.simulate_mouse_down(bar, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(bar + frames(4), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        read(visual),
        (Some(19), Some(31)),
        "the bar moved both points"
    );
    visual.simulate_keystrokes("escape");
    assert_eq!(
        read(visual),
        (Some(15), Some(27)),
        "Escape put the range back"
    );
    visual.simulate_mouse_move(bar + frames(8), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(bar + frames(8), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        read(visual),
        (Some(15), Some(27)),
        "the drag ended at Escape"
    );
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        playhead,
        "a press on the bar is not a press on the ruler"
    );

    // A double-click fills the sequence.
    let bar = centre(visual, "timeline-work-area");
    visual.simulate_event(MouseDownEvent {
        position: bar,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    visual.simulate_mouse_up(bar, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let duration = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    assert_eq!(
        read(visual),
        (Some(0), Some(duration)),
        "the double-click filled the sequence"
    );
}
