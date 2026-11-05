//! Dragging a marker along the timeline ruler.
//!
//! The class closed here: a marker drag that moves by a travel read at the
//! wrong zoom, lets a marker leave the sequence or its clip, lands as more than
//! one undo step or under another name, keeps what it wrote after Escape,
//! turns a click into a move, or turns a move into a click. A point marker, a
//! range marker and a clip marker are driven through the same press, drag and
//! release at two zooms, and the drawn test drives the flag the ruler draws.
//!
//! Not covered: snapping, which a marker drag does not do.

use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};

use super::MOVE_MARKER;
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::app::work_area::tests::{center, travel};
use crate::interned::intern;
use crate::transport::TIMELINE_BASE_PPS;

/// The marker a case drags.
#[derive(Clone, Copy, Debug)]
enum Held {
    /// A sequence marker with no duration.
    Point,
    /// A sequence marker [`RANGE`] frames long.
    Range,
    /// A marker on a clip that starts [`CLIP_AT`] frames into the sequence.
    OnClip,
}

const HELD: [Held; 3] = [Held::Point, Held::Range, Held::OnClip];

/// Where a sequence marker stands before the drag.
const AT: i64 = 20;

/// The length of the range marker.
const RANGE: i64 = 10;

/// Where the clip holding a clip marker starts.
const CLIP_AT: i64 = 30;

/// A marker of the kind `held` names, after the drag's press.
struct Marked {
    state: AppState,
    marker: u64,
    /// Where the marker stands.
    at: i64,
    /// The first and last tick the marker may reach.
    span: (i64, i64),
}

fn marked(held: Held) -> Marked {
    let (mut state, clip) = with_clip();
    let at = match held {
        Held::Point | Held::Range => {
            state.clear_selection();
            AT
        }
        Held::OnClip => {
            let lane = state.selected_track().expect("the clip's lane");
            state
                .shift_track(lane, CLIP_AT)
                .expect("move the clip off 0");
            state.select_clip(clip);
            CLIP_AT + AT
        }
    };
    state.set_playhead(at);
    state.add_marker_at_playhead().expect("add the marker");
    let row = &state.store.sequence.markers[0];
    assert_eq!(
        (row.tick, row.clip_id.is_some()),
        (at, matches!(held, Held::OnClip)),
        "{held:?}: the fixture marker"
    );
    let (marker, name, comment, color, kind) = (
        row.id,
        row.name.to_string(),
        row.comment.to_string(),
        row.color,
        row.kind,
    );
    if let Held::Range = held {
        state
            .edit_marker_full(marker, name, comment, color, RANGE, kind)
            .expect("give the marker a length");
    }
    let duration = state.store.sequence.duration_ticks;
    let span = match held {
        Held::Point => (0, duration),
        Held::Range => (0, duration - RANGE),
        Held::OnClip => {
            let clip = state.selected_ui_clip().expect("the clip");
            assert_eq!(clip.timeline_start_ticks, CLIP_AT, "the clip moved");
            (CLIP_AT, clip.timeline_end_ticks - 1)
        }
    };
    assert!(
        span.0 < at - 4 && at + 12 < span.1,
        "{held:?}: the fixture has room either side of the marker"
    );
    Marked {
        state,
        marker,
        at,
        span,
    }
}

/// The marker's tick and length, or `None` once it is gone.
fn placed(state: &AppState, marker: u64) -> Option<(i64, i64)> {
    state
        .store
        .sequence
        .markers
        .iter()
        .find(|row| row.id == marker)
        .map(|row| (row.tick, row.duration_ticks))
}

fn steps(state: &AppState) -> usize {
    state.history_entries().len()
}

#[test]
fn a_drag_moves_each_marker_by_the_pointer_travel_and_stops_at_its_span() {
    for held in HELD {
        let Marked {
            mut state,
            marker,
            at,
            span: (first, last),
        } = marked(held);
        let length = placed(&state, marker).expect("the marker").1;
        // (frames dragged, tick after)
        let cases = [
            (5, at + 5),
            (-4, at - 4),
            (-(at + 50), first),
            (last * 3, last),
        ];
        for zoom in [1.0, 2.5] {
            state.zoom = zoom;
            for (frames, landed) in cases {
                assert!(state.press_marker(marker, 400.0), "{held:?} pressed");
                assert!(state.marker_held(), "{held:?} held after the press");
                let moved = state.drag_marker(400.0 + travel(&state, frames));
                assert!(moved, "{held:?} at zoom {zoom}: {frames} frames moved it");
                assert_eq!(
                    placed(&state, marker),
                    Some((landed, length)),
                    "{held:?} at zoom {zoom}: {frames} frames, the length kept"
                );
                assert!(state.release_marker(), "{held:?} let go");
                assert!(!state.marker_held(), "{held:?} released");
                assert_eq!(
                    placed(&state, marker),
                    Some((landed, length)),
                    "{held:?} at zoom {zoom}: the release kept the move"
                );
                assert!(state.undo(), "{held:?}: the move is undoable");
                assert_eq!(
                    placed(&state, marker),
                    Some((at, length)),
                    "{held:?}: undo put the marker back"
                );
            }
        }
    }
}

#[test]
fn a_drag_is_one_move_marker_step_measured_from_the_press() {
    for held in HELD {
        let Marked {
            mut state,
            marker,
            at,
            ..
        } = marked(held);
        let before = steps(&state);
        state.press_marker(marker, 400.0);
        for frames in [3, 12, 7] {
            state.drag_marker(400.0 + travel(&state, frames));
        }
        assert_eq!(
            placed(&state, marker).map(|placed| placed.0),
            Some(at + 7),
            "{held:?}: the last move is read from the press, not added to the moves before it"
        );
        state.release_marker();
        let history = state.history_entries();
        assert_eq!(history.len(), before + 1, "{held:?}: one step for the drag");
        assert_eq!(
            history.last().map(|entry| entry.label.as_str()),
            Some(MOVE_MARKER),
            "{held:?}: the step's name"
        );
        assert!(state.undo(), "{held:?}: the drag is on the undo stack");
        assert_eq!(
            placed(&state, marker).map(|placed| placed.0),
            Some(at),
            "{held:?}: one undo takes back the whole drag"
        );
        assert!(state.redo(), "{held:?}: and redo puts it back");
        assert_eq!(placed(&state, marker).map(|placed| placed.0), Some(at + 7));
    }
}

#[test]
fn escape_puts_the_marker_back_and_writes_no_step() {
    for held in HELD {
        let Marked {
            mut state,
            marker,
            at,
            ..
        } = marked(held);
        let (before, playhead) = (steps(&state), state.playhead.value);
        state.press_marker(marker, 400.0);
        state.drag_marker(400.0 + travel(&state, 9));
        assert_ne!(placed(&state, marker).map(|placed| placed.0), Some(at));
        assert!(state.cancel_marker_drag(), "{held:?}: Escape had a press");
        assert!(!state.marker_held(), "{held:?}: Escape ended the press");
        assert!(!state.cancel_marker_drag(), "a second Escape has no press");
        assert!(
            !state.release_marker(),
            "the release after Escape has no press"
        );
        assert_eq!(placed(&state, marker).map(|placed| placed.0), Some(at));
        assert_eq!(steps(&state), before, "{held:?}: Escape wrote no step");
        assert_eq!(
            state.playhead.value, playhead,
            "{held:?}: nor moved the playhead"
        );
    }
}

#[test]
fn a_press_without_travel_is_a_click_and_a_drag_back_is_not() {
    let Marked {
        mut state,
        marker,
        at,
        ..
    } = marked(Held::Point);
    let before = steps(&state);
    // A frame to a pixel, so the travel under the slop spans whole frames:
    // only the slop keeps the press a click.
    state.zoom = state.store.sequence.ticks_per_second() / TIMELINE_BASE_PPS;
    assert!((travel(&state, 1) - 1.0).abs() < 1e-3, "a frame to a pixel");

    // Under the click slop: no move, and the release moves the playhead.
    state.set_playhead(0);
    state.press_marker(marker, 400.0);
    assert!(
        !state.drag_marker(402.0),
        "travel under the slop moves nothing"
    );
    assert_eq!(placed(&state, marker).map(|placed| placed.0), Some(at));
    state.release_marker();
    assert_eq!(state.playhead.value, at, "the click moved the playhead");
    assert_eq!(placed(&state, marker).map(|placed| placed.0), Some(at));

    // Past the slop and back: a drag that lands where it started.
    state.set_playhead(0);
    state.press_marker(marker, 400.0);
    assert!(
        state.drag_marker(400.0 + travel(&state, 6)),
        "the drag moved"
    );
    assert!(state.drag_marker(400.0), "and came back");
    state.release_marker();
    assert_eq!(state.playhead.value, 0, "a drag is not a click");
    assert_eq!(placed(&state, marker).map(|placed| placed.0), Some(at));
    assert_eq!(
        steps(&state),
        before,
        "a drag back to the press wrote no step"
    );
}

#[gpui::test]
fn the_drawn_flag_follows_the_drag_and_a_click_on_it_moves_the_playhead(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "marker-drag");
    change(visual, &state, |state| {
        state.clear_selection();
        state.set_playhead(24);
        state.add_marker_at_playhead().expect("add the marker");
    });
    let marker = visual.read(|app| state.read(app).store.sequence.markers[0].id);
    let flag = intern(&format!("ruler-marker-{marker}"));
    let at = |visual: &mut VisualTestContext| {
        visual.read(|app| placed(state.read(app), marker).map(|placed| placed.0))
    };
    let frame = visual.read(|app| travel(state.read(app), 1));
    let frames = |n: i64| point(px(frame * n as f32), px(0.0));

    // The flag moves with the pointer, and keeps moving once the pointer has
    // left the ruler.
    let press = center(visual, flag);
    let before = visual.read(|app| steps(state.read(app)));
    visual.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(press + frames(6), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(at(visual), Some(30), "the drag moved the marker");
    let drawn = f32::from(center(visual, flag).x - press.x);
    assert!(
        (drawn - frame * 6.0).abs() < 1.0,
        "the flag is drawn under the pointer: {drawn} px for {} px",
        frame * 6.0
    );
    let below = press + frames(9) + point(px(0.0), px(80.0));
    visual.simulate_mouse_move(below, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(at(visual), Some(33), "the drag kept going off the ruler");
    let (count, label) = visual.read(|app| {
        let history = state.read(app).history_entries();
        (
            history.len(),
            history.last().map(|entry| entry.label.clone()),
        )
    });
    assert_eq!(count, before + 1, "the drag is one step");
    assert_eq!(label.as_deref(), Some(MOVE_MARKER));

    // Escape puts the flag back with the button down.
    let press = center(visual, flag);
    visual.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(press - frames(4), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(at(visual), Some(29));
    visual.simulate_keystrokes("escape");
    assert_eq!(at(visual), Some(33), "Escape put the marker back");
    visual.simulate_mouse_move(press - frames(8), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(press - frames(8), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(at(visual), Some(33), "the drag ended at Escape");

    // A click moves the playhead to the marker on the release.
    change(visual, &state, |state| state.set_playhead(0));
    let press = center(visual, flag);
    visual.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        0,
        "the press alone leaves the playhead"
    );
    visual.simulate_mouse_up(press, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).playhead.value),
        33,
        "the click moved the playhead to the marker"
    );
    assert_eq!(at(visual), Some(33), "the click left the marker");
}
