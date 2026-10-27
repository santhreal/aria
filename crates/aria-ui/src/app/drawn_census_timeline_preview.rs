//! Every drag a room holds draws, while the button is down, the timeline its
//! release leaves.
//!
//! # Why
//!
//! An edge drag and a slide drew nothing until the button came up: the clip
//! kept its edges while the pointer moved, a ripple left the rest of the lane
//! where it was, and a slide left its neighbors whole. The class closed here
//! is a drag that draws something other than what its release leaves.
//!
//! Each census drag a room holds, each edge drag on both edges, is held at
//! travels short of its limit and far past it, in each dressing of the room
//! census and with linked selection on and off. The lanes the held frame
//! draws, clip for clip and in order, are the lanes the store holds after the
//! release. A clip is compared by where it sits, which part of its take it
//! plays and how fast, and the marks and beats drawn on it.
//!
//! Not covered: the filmstrip cells and waveform bars drawn on a card, the
//! property rows of a twirled-open clip, and a drag whose release is refused.

use aria_models::ClipId;

use super::room::{FAR, at_the_start, between_two, drags, in_open_lane};
use super::{Junction, junction};
use crate::app::AppState;
use crate::projection::Placement;
use crate::surfaces::timeline_gesture::{self, Drag};

/// Each lane as the frame draws it, in timeline order: each clip's id and
/// where it plays its take.
fn drawn(state: &AppState) -> Vec<Vec<(u64, Placement)>> {
    let rate = state.store.sequence.frame_rate;
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| {
            let clips = match state
                .held_edit
                .as_deref()
                .filter(|edit| edit.touches(lane.id))
            {
                Some(edit) => edit.lane(lane.id, &lane.clips, (i64::MIN, i64::MAX), rate),
                None => lane.clips.to_vec(),
            };
            clips
                .iter()
                .map(|clip| (clip.id, Placement::of_card(clip)))
                .collect()
        })
        .collect()
}

/// Hold every drag a room holds in `dress` at each travel, with linked
/// selection on and off, and let it go: the held frame draws what the
/// release leaves.
fn held_as_released(dress: fn() -> Junction) {
    let held: Vec<usize> = drags(&dress())
        .iter()
        .enumerate()
        .filter(|(_, (gesture, _))| gesture.room().is_some())
        .map(|(index, _)| index)
        .collect();
    for index in held {
        for delta in [-FAR, -5, -1, 1, 5, FAR] {
            for linked in [true, false] {
                let mut dressed = dress();
                let (gesture, clip) = drags(&dressed).swap_remove(index);
                let name = format!("{gesture:?} by {delta}, linked selection {linked}");
                let state = &mut dressed.state;
                state.linked_selection = linked;
                let drag = Drag { clip, delta };
                state.hold_drag(gesture, drag);
                let shown = drawn(state);
                state.end_drag();
                timeline_gesture::apply(state, gesture, drag)
                    .unwrap_or_else(|err| panic!("{name} was refused: {err}"));
                state.sync_from_engine();
                assert_eq!(
                    shown,
                    drawn(state),
                    "{name}: the held frame drew a timeline its release did not leave"
                );
                assert!(
                    state.held_edit.is_none() && state.drag_project.is_none(),
                    "{name}: the release kept what the drag held"
                );
            }
        }
    }
}

#[test]
fn a_held_drag_on_the_junction_draws_what_its_release_leaves() {
    held_as_released(junction);
}

#[test]
fn a_held_drag_between_two_clips_draws_what_its_release_leaves() {
    held_as_released(between_two);
}

#[test]
fn a_held_drag_in_open_lane_draws_what_its_release_leaves() {
    held_as_released(in_open_lane);
}

#[test]
fn a_held_drag_at_the_sequence_start_draws_what_its_release_leaves() {
    held_as_released(at_the_start);
}

/// The census's plain trim of the subject's head in `dressed`.
fn head_trim(dressed: &Junction) -> (timeline_gesture::Gesture, ClipId) {
    drags(dressed)
        .into_iter()
        .find(|(gesture, _)| {
            matches!(
                gesture,
                timeline_gesture::Gesture::Edge {
                    drag: None,
                    left: true,
                    ..
                }
            )
        })
        .expect("the census trims the head")
}

/// An edge drag held short of its limit draws its edge as a held edge, and
/// one held at the limit draws it stopped.
#[test]
fn an_edge_held_at_its_limit_is_drawn_stopped() {
    for (delta, stopped) in [(-1, false), (-FAR, true)] {
        let mut dressed = in_open_lane();
        let (gesture, clip) = head_trim(&dressed);
        let state = &mut dressed.state;
        state.hold_drag(gesture, Drag { clip, delta });
        let edge = state.held_edit.as_ref().and_then(|edit| edit.edge);
        assert_eq!(
            edge.map(|edge| (ClipId::from_raw(edge.clip), edge.left, edge.stopped)),
            Some((clip, true, stopped)),
            "a head trim by {delta}"
        );
    }
}

/// A press on an edge, before the pointer moves, holds the edge where it is
/// and draws no magnet, with a magnet in reach of the edge.
#[test]
fn a_press_that_has_not_moved_holds_the_edge_in_place() {
    let mut dressed = in_open_lane();
    let (gesture, clip) = head_trim(&dressed);
    let timeline_gesture::Gesture::Edge { drag, left, span } = gesture else {
        unreachable!("a head trim is an edge drag");
    };
    let span = timeline_gesture::Span {
        magnet: true,
        ..span
    };
    let state = &mut dressed.state;
    state.snap_enabled = true;
    state.mark_in = Some(span.start + 1);
    assert_eq!(
        state
            .snap_at(span.start, Some(clip.raw()), span.threshold, false)
            .0,
        span.start + 1,
        "the mark in pulls the head"
    );
    state.hold_drag(
        timeline_gesture::Gesture::Edge { drag, left, span },
        Drag { clip, delta: 0 },
    );
    assert_eq!(state.snap_guide, None, "a press drew a magnet");
    assert_eq!(
        state.held_edit.as_ref().map(|edit| edit.cards.len()),
        Some(0),
        "a press drew the head moved"
    );
}
