//! Every drag on a clip, and every key that trims an edge, sent far past where
//! its edit stops.
//!
//! # Why
//!
//! A trim, a ripple, a roll, a slide and a rate stretch were checked only on
//! release. A drag one tick past the end of the take, the clip beside it or
//! the sequence start sent an edit the model refused, and the clip stayed
//! where it was: the further the pointer went, the less the drag did. A key
//! that trims an edge had the same fault, and a trim past the tail was
//! reshaped into a one-tick clip after it rather than refused.
//!
//! Each drag here goes far both ways and must land. A drag with a room must
//! land at its edge: the edit the drag sends one tick further, with no room
//! holding it, is refused or changes nothing. The readout during the drag
//! states the travel the release sends.
//!
//! The drags are the census rows, each edge drag on both edges, so a new drag
//! is sent here too. Every row is sent on the census junction, and each row a
//! room holds is sent again on a lane between two clips, in open lane and at
//! the sequence start, so each bound a room has is the one that stops some
//! drag. The keys are every tool's edge drag under each edit mode.
//!
//! A drag turned into another mid-drag, as a modifier turns a trim into a
//! ripple, is held by the new drag's room. That no move of a drag reads the
//! session is in `drag_session_tests`.
//!
//! This does not reach a limit the fixture lacks: a linked companion on
//! another lane, a ramped speed, a reversed take. The model's reach tests
//! hold those.

use aria_models::{ClipId, MediaId, RationalTime, TimeRange, TrackId};

use super::{Junction, junction, rows, span_of};
use crate::app::AppState;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture, Span};
use crate::types::{EditMode, EditorTool, HeldEdit, TrimSide};

#[path = "drag_session_tests.rs"]
mod session;

/// Further than any limit the fixture has, both ways.
pub(super) const FAR: i64 = 1 << 20;

/// Where each clip on the timeline sits and which take it plays, lane by
/// lane in timeline order. No ids: two dressings of one lane compare equal.
fn placed(state: &AppState) -> Vec<(i64, i64, i64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| {
            (
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
                clip.source_start_ticks,
                clip.source_end_ticks,
            )
        })
        .collect()
}

/// Each lane's clip ids in timeline order.
fn order(state: &AppState) -> Vec<Vec<u64>> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.clips.iter().map(|clip| clip.id).collect())
        .collect()
}

/// Assert a drag a room holds left every clip where a room may put it: at or
/// after the sequence start, and in the order `was` on its lane, since no
/// room lets a clip pass a neighbor.
fn assert_in_room(state: &AppState, was: &[Vec<u64>], name: &str) {
    assert!(
        placed(state).iter().all(|&(start, ..)| start >= 0),
        "{name} placed a clip before the sequence start"
    );
    assert_eq!(order(state), was, "{name} carried a clip past a neighbor");
}

/// Every census drag, and each edge drag on the other edge as well.
pub(super) fn drags(j: &Junction) -> Vec<(Gesture, ClipId)> {
    rows(j)
        .into_iter()
        .flat_map(|row| {
            let other = match row.gesture {
                Gesture::Edge { drag, left, span } => Some(Gesture::Edge {
                    drag,
                    left: !left,
                    span,
                }),
                _ => None,
            };
            std::iter::once(row.gesture)
                .chain(other)
                .map(move |gesture| (gesture, row.clip))
        })
        .collect()
}

/// How far the release took what `gesture` holds, read off where the clip
/// sits: an edge by the change in length, a body by the change in start.
fn travel(gesture: Gesture, was: (i64, i64), now: (i64, i64)) -> i64 {
    let grew = (now.1 - now.0) - (was.1 - was.0);
    match gesture {
        Gesture::Edge { left: true, .. } => -grew,
        Gesture::Edge { left: false, .. } => grew,
        Gesture::Body { .. }
        | Gesture::RippleBody { .. }
        | Gesture::Transition { .. }
        | Gesture::Fade { .. } => now.0 - was.0,
    }
}

/// Assert a drag held short of where the pointer went stopped at its limit:
/// the edits `gesture` sends on `clip` for `past` ticks of travel, one tick
/// beyond the held travel, sent from a fresh dressing of the same lane with
/// no room, magnet or sequence start holding them, are refused or leave the
/// timeline where the held drag `landed`.
fn assert_at_limit(
    state: &mut AppState,
    (gesture, clip): (Gesture, ClipId),
    past: i64,
    landed: &[(i64, i64, i64, i64)],
    name: &str,
) {
    let steps = timeline_gesture::steps(state, gesture, clip, past);
    let sent = state.run_steps(&steps);
    state.sync_from_engine();
    assert!(
        sent.is_err() || placed(state) == landed,
        "{name} stopped short of its limit: the edits of a travel of {past}, unheld, landed"
    );
}

/// Every edit mode. A new one fails to compile here until it is sent.
fn modes() -> [EditMode; 2] {
    match EditMode::default() {
        EditMode::Ripple | EditMode::Overwrite => [EditMode::Ripple, EditMode::Overwrite],
    }
}

/// One clip of a dressed lane, in frames: where it starts, how long it is,
/// and where in the take it starts.
struct Cut {
    at: i64,
    len: i64,
    take: i64,
}

/// A lane of clips cut from the video fixture's take, and nothing else on the
/// timeline, dressed as a junction whose subject is `clips[subject]`. No clip
/// carries a blend.
fn lane(clips: &[Cut], subject: usize) -> Junction {
    let (mut state, _) = crate::app::fixtures::with_video();
    let track = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let media = MediaId::from_raw(state.store.media[0].id);
    let placed: Vec<ClipId> = {
        let mut engine = state.engine.lock();
        let mut project = (*engine.project()).clone();
        let rate = project.timeline().frame_rate;
        assert_eq!(
            project.media(media).map(|take| take.frame_rate),
            Some(rate),
            "a dressed lane counts frames of the take as frames of the sequence"
        );
        for clip in state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
        {
            project.remove_clip(ClipId::from_raw(clip.id));
        }
        let placed = clips
            .iter()
            .map(|cut| {
                project
                    .add_clip(
                        track,
                        media,
                        TimeRange::at_rate(cut.take, cut.len, rate),
                        RationalTime::new(cut.at, rate),
                    )
                    .expect("the dressed cut fits the take and the lane")
            })
            .collect();
        engine.reset_project(project);
        placed
    };
    state.sync_from_engine();
    let (start, end) = span_of(&state, placed[subject]);
    Junction {
        state,
        left: placed[0],
        subject: placed[subject],
        track,
        span: Span {
            start,
            end,
            threshold: super::THRESHOLD,
            magnet: false,
        },
        blend: 0,
    }
}

/// Three clips abutting, the subject between a three-frame clip whose take
/// runs on past its tail and a three-frame clip one frame into its take. A
/// roll or a slide stops where a neighbor keeps one frame or runs out of take,
/// and a trim stops at the clip beside it.
pub(super) fn between_two() -> Junction {
    let a = Cut {
        at: 27,
        len: 3,
        take: 40,
    };
    let subject = Cut {
        at: 30,
        len: 20,
        take: 6,
    };
    let c = Cut {
        at: 50,
        len: 3,
        take: 1,
    };
    lane(&[a, subject, c], 1)
}

/// The subject with open lane behind it back to a clip and open lane ahead of
/// it to the next clip, its take running on past both ends. A trim stops at
/// the take or at the next clip, and a slide or a rate stretch at the clip
/// behind or the clip ahead.
pub(super) fn in_open_lane() -> Junction {
    let behind = Cut {
        at: 2,
        len: 5,
        take: 30,
    };
    let subject = Cut {
        at: 20,
        len: 20,
        take: 6,
    };
    let next = Cut {
        at: 52,
        len: 10,
        take: 30,
    };
    lane(&[behind, subject, next], 1)
}

/// The subject at the sequence start, its take running on before its head. A
/// trim of the head stops at the sequence start, and a ripple trim of the head
/// passes it.
pub(super) fn at_the_start() -> Junction {
    lane(
        &[Cut {
            at: 0,
            len: 20,
            take: 20,
        }],
        0,
    )
}

/// Send every drag `dress` offers far both ways: each lands, and one a room
/// holds lands at the room's edge with the readout stating its travel. With
/// `held` only the drags a room holds are sent.
fn far_both_ways(dress: fn() -> Junction, held: bool) {
    // The rows come off a state that is dropped before the loop starts: two
    // projects open at once hold two compositor devices.
    let sent: Vec<usize> = drags(&dress())
        .iter()
        .enumerate()
        .filter(|(_, (gesture, _))| !held || gesture.room().is_some())
        .map(|(index, _)| index)
        .collect();
    for index in sent {
        for sign in [-1, 1] {
            let mut dressed = dress();
            let (gesture, clip) = drags(&dressed).swap_remove(index);
            let name = format!("{gesture:?} by {}", sign * FAR);
            let state = &mut dressed.state;
            let drag = Drag {
                clip,
                delta: sign * FAR,
            };
            let readout = state
                .compute_trim_feedback(gesture, drag)
                .map(|shown| shown.delta_ticks);
            let was = span_of(state, clip);
            let before = order(state);
            timeline_gesture::apply(state, gesture, drag)
                .unwrap_or_else(|err| panic!("{name} was refused: {err}"));
            state.sync_from_engine();
            if gesture.room().is_none() {
                assert!(
                    placed(state).iter().all(|&(start, ..)| start >= 0),
                    "{name} placed a clip before the sequence start"
                );
                continue;
            }
            assert_in_room(state, &before, &name);
            let went = travel(gesture, was, span_of(state, clip));
            assert_eq!(
                readout,
                Some(went),
                "{name}: the readout states a travel the release did not send"
            );
            if went == sign * FAR {
                continue;
            }
            let landed = placed(state);
            drop(dressed);
            let mut again = dress();
            let (gesture, clip) = drags(&again).swap_remove(index);
            assert_at_limit(
                &mut again.state,
                (gesture, clip),
                went + sign,
                &landed,
                &name,
            );
        }
    }
}

#[test]
fn a_drag_far_past_its_limit_lands_at_the_limit() {
    far_both_ways(junction, false);
}

#[test]
fn a_drag_between_two_clips_stops_where_a_neighbor_does() {
    far_both_ways(between_two, true);
}

#[test]
fn a_drag_in_open_lane_stops_at_the_take_or_the_next_clip() {
    far_both_ways(in_open_lane, true);
}

#[test]
fn a_drag_at_the_sequence_start_stops_there_unless_it_ripples() {
    far_both_ways(at_the_start, true);
}

/// The junction with `tool` armed in `mode` and the subject's `side` edge
/// selected, and the subject.
fn edge_selected(tool: EditorTool, mode: EditMode, side: TrimSide) -> (Junction, ClipId) {
    let mut dressed = junction();
    let clip = dressed.subject;
    dressed.state.tool = tool;
    dressed.state.edit_mode = mode;
    dressed.state.select_edge(clip, side);
    (dressed, clip)
}

#[test]
fn a_key_far_past_an_edges_limit_lands_at_the_limit() {
    // One tool per edge drag: two tools that drag an edge the same way send
    // the same key.
    let mut tools: Vec<EditorTool> = Vec::new();
    for tool in EditorTool::all() {
        if tools
            .iter()
            .all(|seen| seen.edge_drag() != tool.edge_drag())
        {
            tools.push(tool);
        }
    }
    for tool in tools {
        for mode in modes() {
            for side in [TrimSide::In, TrimSide::Out] {
                for sign in [-1, 1] {
                    let (mut dressed, clip) = edge_selected(tool, mode, side);
                    let state = &mut dressed.state;
                    let name = format!("{tool:?} in {mode:?} on {side:?} by {}", sign * FAR);
                    let (gesture, _) = state
                        .selected_edge_gesture()
                        .unwrap_or_else(|| panic!("{name}: the edge is not selected"));
                    assert!(gesture.room().is_some(), "{name}: an edge key has no room");
                    let was = span_of(state, clip);
                    let before = order(state);
                    state
                        .trim_selected_edge(sign * FAR)
                        .unwrap_or_else(|err| panic!("{name} was refused: {err}"));
                    state.sync_from_engine();
                    assert_in_room(state, &before, &name);
                    let went = travel(gesture, was, span_of(state, clip));
                    if went == sign * FAR {
                        continue;
                    }
                    let landed = placed(state);
                    drop(dressed);
                    let (mut again, clip) = edge_selected(tool, mode, side);
                    let (gesture, _) = again
                        .state
                        .selected_edge_gesture()
                        .unwrap_or_else(|| panic!("{name}: the edge is not selected"));
                    assert_at_limit(
                        &mut again.state,
                        (gesture, clip),
                        went + sign,
                        &landed,
                        &name,
                    );
                }
            }
        }
    }
}

/// The readout and the clips a held drag draws.
fn shown(state: &AppState) -> (Option<i64>, Option<HeldEdit>) {
    (
        state.trim_feedback.as_ref().map(|shown| shown.delta_ticks),
        state.held_edit.as_deref().cloned(),
    )
}

/// A modifier pressed or let go mid-drag turns a trim into a ripple or a roll
/// under the pointer, and the room the press read is the old gesture's. Each
/// drag a room holds is held after every other, without a release between,
/// and draws what it draws held alone.
#[test]
fn a_drag_whose_gesture_changes_is_held_by_the_new_gesture() {
    for dress in [between_two, in_open_lane, at_the_start] {
        let mut dressed = dress();
        let held: Vec<(Gesture, ClipId)> = drags(&dressed)
            .into_iter()
            .filter(|(gesture, _)| gesture.room().is_some())
            .collect();
        let state = &mut dressed.state;
        for sign in [-1, 1] {
            let far = |clip| Drag {
                clip,
                delta: sign * FAR,
            };
            let alone: Vec<_> = held
                .iter()
                .map(|&(gesture, clip)| {
                    state.hold_drag(gesture, far(clip));
                    let alone = shown(state);
                    state.end_drag();
                    alone
                })
                .collect();
            for &(before, on) in &held {
                for (&(gesture, clip), alone) in held.iter().zip(&alone) {
                    if (before.room(), on) == (gesture.room(), clip) {
                        continue;
                    }
                    state.hold_drag(before, far(on));
                    state.hold_drag(gesture, far(clip));
                    assert_eq!(
                        &shown(state),
                        alone,
                        "{gesture:?} by {} held after {before:?}",
                        sign * FAR
                    );
                    state.end_drag();
                }
            }
        }
    }
}
