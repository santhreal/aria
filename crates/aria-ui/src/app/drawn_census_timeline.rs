//! Driving every drag the timeline understands: the three body drags, the four
//! edge drags, and the two edges of a transition's window.
//!
//! # Why
//!
//! Each of these was a `match` arm inside a mouse-up listener, so nothing could
//! enumerate them, and a tool wired to the wrong verb read exactly like a tool
//! wired to the right one. Two commands had no measurable surface at all: `Slip
//! clip` and `Set transition length` were reachable only by dragging.
//!
//! The registry is [`timeline_gesture`](crate::surfaces::timeline_gesture) and
//! this presses every row of it. A drag is asserted in ticks rather than pixels,
//! because a gesture asserted in pixels is asserted against a zoom level.
//!
//! Every row is driven against its own dressed state: a clip id is read off the
//! state it is pressed against, and a gesture that trims a neighbour must not
//! find the neighbour already trimmed by the row before it.

use std::collections::BTreeSet;

use aria_models::{ClipId, MediaId, TrackId};

use crate::app::AppState;
use crate::app::sent_census::Watch;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture, Span};
use crate::tool_catalog::{BodyDrag, EdgeDrag};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "timeline_gesture.rs";

/// How far a drag goes, in ticks. Six frames at the fixture's rate: far enough
/// that every verb has something to do, near enough that the magnet's threshold
/// can reach across it in the test that wants it to.
const STEP: i64 = 6;

/// How much source each clip keeps in hand, so a gesture that grows an edge has
/// somewhere to grow into. A clip placed from the pool spans its whole media, so
/// without this a roll or a slide is refused for reaching past the end of the
/// take rather than for anything the gesture got wrong.
const SLACK: i64 = 12;

/// A magnet threshold wide enough to reach the cut from a [`STEP`] drag.
const THRESHOLD: i64 = 12;

/// Two clips abutting on one lane, with a blend at the cut between them.
///
/// The second clip is the one dragged: it has a neighbour behind it, so a roll
/// and a slide have a cut to work with, and open space ahead of it, so a move
/// lands somewhere. Its tail is trimmed in by [`SLACK`], which is what gives a
/// slip room to move the take without the model clamping the gesture to nothing.
struct Junction {
    state: AppState,
    /// The clip on the left of the cut, which owns the blend.
    left: ClipId,
    /// The clip every clip gesture below is performed on.
    subject: ClipId,
    /// The lane both clips sit on, which is where a body drag lands.
    track: TrackId,
    /// Where the subject sits, as the drag sees it.
    span: Span,
    /// The blend window's current length.
    blend: i64,
}

fn junction() -> Junction {
    let (mut state, left) = crate::app::fixtures::with_video();
    let track = TrackId::from_raw(
        state
            .store
            .sequence
            .tracks
            .first()
            .expect("the fixture projects a lane")
            .id,
    );
    let (left_start, left_end) = span_of(&state, left);
    state
        .trim_clip(left, left_start, left_end - SLACK)
        .expect("leave the left clip's take room to grow at the cut");
    state.sync_from_engine();
    let cut = span_of(&state, left).1;
    let media = MediaId::from_raw(
        state
            .store
            .media
            .first()
            .expect("the fixture imports one pool item")
            .id,
    );
    state
        .drop_media_on_timeline(media, Some(track), cut)
        .expect("place a second clip at the cut");
    state.sync_from_engine();
    let subject = clips_on(&state, track)
        .into_iter()
        .find(|clip| *clip != left)
        .expect("a second clip on the lane");
    let (_, end) = span_of(&state, subject);
    state
        .trim_clip(subject, cut, end - SLACK)
        .expect("leave the take some slack to slip into");
    let blend = crate::app::clip_transitions::transition_choices()
        .first()
        .expect("the catalog offers a blend")
        .id
        .clone();
    state
        .set_clip_transition(left, &blend)
        .expect("blend the cut");
    state.sync_from_engine();
    let blend = state
        .clip_transition_ticks(left)
        .expect("the cut carries a blend to drag");
    let (start, end) = span_of(&state, subject);
    assert_eq!(
        start, cut,
        "the dressed clip must still abut its neighbour, or a roll has no cut"
    );
    Junction {
        state,
        left,
        subject,
        track,
        span: Span {
            start,
            end,
            threshold: THRESHOLD,
            // Off by default: a magnet that pulls a drag back onto the cut it
            // started from turns most of these rows into no-ops. One test below
            // turns it on, which is the only place it is under test.
            magnet: false,
        },
        blend,
    }
}

/// Every clip on one lane, in timeline order.
fn clips_on(state: &AppState, track: TrackId) -> Vec<ClipId> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.id == track.raw())
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect()
}

/// Where a clip sits on the timeline, in ticks.
fn span_of(state: &AppState, clip: ClipId) -> (i64, i64) {
    let found = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|item| item.id == clip.raw())
        .expect("the clip is on the timeline");
    (found.timeline_start_ticks, found.timeline_end_ticks)
}

/// Where a clip's take begins, in source ticks.
fn take_start(state: &AppState, clip: ClipId) -> i64 {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|item| item.id == clip.raw())
        .expect("the clip is on the timeline")
        .source_start_ticks
}

/// One drag, and the command it must ask for.
struct Row {
    gesture: Gesture,
    /// The clip under the pointer. A clip gesture holds the subject; a
    /// transition edge holds the clip that owns the cut.
    clip: ClipId,
    /// How far the pointer went. Signed per row, because a widening drag on the
    /// left edge of a wedge goes the other way from one on its right edge.
    delta: i64,
    writes: &'static str,
}

/// Every drag the timeline offers, dressed against one state.
///
/// Built from the state rather than declared as a constant: a clip id belongs to
/// the project it was created in, and each row is pressed against its own.
fn rows(j: &Junction) -> Vec<Row> {
    let span = j.span;
    let track = j.track;
    vec![
        Row {
            gesture: Gesture::Body {
                drag: None,
                track,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Move clip",
        },
        Row {
            gesture: Gesture::Body {
                drag: Some(BodyDrag::Slip),
                track,
                span,
            },
            clip: j.subject,
            // The content follows the pointer, so dragging left moves the take
            // right, which is the direction the dressed slack allows.
            delta: -STEP,
            writes: "Slip clip",
        },
        Row {
            gesture: Gesture::Body {
                drag: Some(BodyDrag::Slide),
                track,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Move clip",
        },
        Row {
            gesture: Gesture::Edge {
                drag: None,
                left: true,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Trim clip",
        },
        Row {
            gesture: Gesture::Edge {
                drag: Some(EdgeDrag::Roll),
                left: true,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Trim clip",
        },
        Row {
            gesture: Gesture::Edge {
                drag: Some(EdgeDrag::Rate),
                left: true,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Set speed",
        },
        Row {
            gesture: Gesture::Edge {
                drag: Some(EdgeDrag::Ripple),
                left: true,
                span,
            },
            clip: j.subject,
            delta: STEP,
            writes: "Trim clip",
        },
        Row {
            gesture: Gesture::Transition {
                right: true,
                duration: j.blend,
            },
            clip: j.left,
            delta: STEP,
            writes: "Set transition length",
        },
        Row {
            gesture: Gesture::Transition {
                right: false,
                duration: j.blend,
            },
            clip: j.left,
            // The left edge widens the window by going left.
            delta: -STEP,
            writes: "Set transition length",
        },
        Row {
            gesture: Gesture::Fade {
                left: true,
                current: 0,
            },
            clip: j.subject,
            // A fade in grows rightwards, so the pointer goes the same way.
            delta: STEP,
            writes: "Set audio levels",
        },
        Row {
            gesture: Gesture::Fade {
                left: false,
                current: 0,
            },
            clip: j.subject,
            // A fade out grows leftwards, so the same lengthening drag is the
            // other sign. A row that used the same delta for both would pass
            // while one of the two handles ran backwards.
            delta: -STEP,
            writes: "Set audio levels",
        },
        Row {
            gesture: Gesture::RippleBody { track, span },
            clip: j.subject,
            delta: STEP,
            writes: "Ripple move clip",
        },
    ]
}

/// Run one row against the state it was dressed from.
fn press(state: &mut AppState, row: &Row) {
    timeline_gesture::apply(
        state,
        row.gesture,
        Drag {
            clip: row.clip,
            delta: row.delta,
        },
    )
    .unwrap_or_else(|err| panic!("`{}` was refused: {err}", row.gesture.what()));
}

/// Every command a drag on the timeline asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    // The count comes off a state that is dropped before the loop starts: a
    // census holding two projects open at once holds two compositor devices.
    let count = rows(&junction()).len();
    for index in 0..count {
        let mut dressed = junction();
        let row = rows(&dressed).swap_remove(index);
        let watch = Watch::on(&dressed.state);
        press(&mut dressed.state, &row);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_drag_the_timeline_offers_is_pressed() {
    // Fail by default on a new arm: a new tool axis is a new variant of
    // `BodyDrag` or `EdgeDrag`, `what` is a total match, and this then has a row
    // with no prose behind it.
    let dressed = junction();
    let pressed: BTreeSet<String> = rows(&dressed)
        .iter()
        .map(|row| row.gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these drags are offered and nothing presses them: {missing:?}"
    );
}

#[test]
fn every_drag_asks_for_the_edit_its_row_claims() {
    let count = rows(&junction()).len();
    for index in 0..count {
        let mut dressed = junction();
        let row = rows(&dressed).swap_remove(index);
        let watch = Watch::on(&dressed.state);
        press(&mut dressed.state, &row);
        let sent = watch.take();
        assert!(
            sent.contains(&row.writes),
            "`{}` sent {sent:?} rather than `{}`",
            row.gesture.what(),
            row.writes
        );
    }
}

#[test]
fn a_slip_moves_the_take_and_leaves_the_clip_where_it_was() {
    // The gesture that had no surface. Slip is the one drag whose effect is
    // invisible on the timeline, which is exactly why a wrong wiring survived:
    // the card does not move either way.
    let mut dressed = junction();
    let before = take_start(&dressed.state, dressed.subject);
    let placed = span_of(&dressed.state, dressed.subject);
    let row = rows(&dressed).swap_remove(1);
    press(&mut dressed.state, &row);
    assert!(
        take_start(&dressed.state, dressed.subject) > before,
        "the take did not move inside the clip"
    );
    assert_eq!(
        span_of(&dressed.state, dressed.subject),
        placed,
        "a slip moved the clip as well as the take"
    );
}

#[test]
fn a_roll_with_no_neighbour_asks_for_nothing() {
    // The subject's tail edge has open space behind it. A roll there has no
    // shared cut, and the declared answer is that the gesture does nothing
    // rather than quietly trimming one side.
    let mut dressed = junction();
    let span = dressed.span;
    let watch = Watch::on(&dressed.state);
    press(
        &mut dressed.state,
        &Row {
            gesture: Gesture::Edge {
                drag: Some(EdgeDrag::Roll),
                left: false,
                span,
            },
            clip: dressed.subject,
            delta: STEP,
            writes: "",
        },
    );
    assert!(
        watch.take().is_empty(),
        "a roll with nothing on the far side of the edge edited the timeline"
    );
}

#[test]
fn a_roll_moves_the_cut_and_neither_outer_edge() {
    // What separates a roll from a trim: the sequence keeps its length.
    let mut dressed = junction();
    let outer_left = span_of(&dressed.state, dressed.left).0;
    let outer_right = dressed.span.end;
    let row = rows(&dressed).swap_remove(4);
    press(&mut dressed.state, &row);
    let (moved_start, moved_end) = span_of(&dressed.state, dressed.subject);
    assert_eq!(
        moved_start,
        dressed.span.start + STEP,
        "the cut did not move by the drag"
    );
    assert_eq!(moved_end, outer_right, "the roll moved the outer tail");
    assert_eq!(
        span_of(&dressed.state, dressed.left).0,
        outer_left,
        "the roll moved the outer head"
    );
}

#[test]
fn a_ripple_takes_the_rest_of_the_lane_with_it() {
    // A ripple is two commands in one group: the trim, and the shift that closes
    // the hole it left. One without the other is the defect.
    let mut dressed = junction();
    let row = rows(&dressed).swap_remove(6);
    let watch = Watch::on(&dressed.state);
    press(&mut dressed.state, &row);
    let sent = watch.take();
    for label in ["Trim clip", "Shift clips"] {
        assert!(
            sent.contains(&label),
            "a ripple sent {sent:?}, which is missing `{label}`"
        );
    }
}

#[test]
fn either_edge_of_a_wedge_lengthens_the_window_and_neither_moves_the_cut() {
    // The window is centred on the junction, so both edges are the same edit in
    // opposite pointer directions. A sign error here reads as a wedge that
    // shrinks when it should grow.
    for index in [7, 8] {
        let mut dressed = junction();
        let cut = span_of(&dressed.state, dressed.left).1;
        let row = rows(&dressed).swap_remove(index);
        press(&mut dressed.state, &row);
        let after = dressed
            .state
            .clip_transition_ticks(dressed.left)
            .expect("the blend survived its own drag");
        assert!(
            after > dressed.blend,
            "dragging {} left the window at {after} ticks",
            row.gesture.what()
        );
        assert_eq!(
            span_of(&dressed.state, dressed.left).1,
            cut,
            "a wedge drag moved the cut it is centred on"
        );
    }
}

#[test]
fn each_fade_handle_lengthens_its_own_end_of_the_clip() {
    // The two handles sit at opposite ends and the same pointer direction has to
    // mean the opposite thing on each. A sign error reads as a handle that runs
    // away from the pointer, and it is invisible in a census that only checks
    // which command was sent.
    for (index, left) in [(9, true), (10, false)] {
        let mut dressed = junction();
        let row = rows(&dressed).swap_remove(index);
        press(&mut dressed.state, &row);
        let clip = dressed
            .state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
            .find(|clip| clip.id == dressed.subject.raw())
            .expect("the dragged clip is still on the timeline");
        let (grown, other) = if left {
            (clip.fade_in_ticks, clip.fade_out_ticks)
        } else {
            (clip.fade_out_ticks, clip.fade_in_ticks)
        };
        assert_eq!(
            grown,
            STEP,
            "dragging {} left the fade at {grown} ticks",
            row.gesture.what()
        );
        assert_eq!(
            other,
            0,
            "dragging {} moved the fade at the other end of the clip",
            row.gesture.what()
        );
    }
}

#[test]
fn the_magnet_takes_a_drag_that_lands_near_the_cut() {
    // The magnet belongs to the registry, so it is asserted here rather than in
    // whatever panel happened to call `snap_at`. Both halves are needed: a
    // magnet that never lets go is as wrong as one that never holds.
    let mut held = junction();
    let mut span = held.span;
    span.magnet = true;
    press(
        &mut held.state,
        &Row {
            gesture: Gesture::Body {
                drag: None,
                track: held.track,
                span,
            },
            clip: held.subject,
            delta: STEP,
            writes: "Move clip",
        },
    );
    assert_eq!(
        span_of(&held.state, held.subject).0,
        held.span.start,
        "the magnet let a drag inside its threshold walk away from the cut"
    );

    let mut free = junction();
    let row = rows(&free).swap_remove(0);
    press(&mut free.state, &row);
    assert_eq!(
        span_of(&free.state, free.subject).0,
        free.span.start + STEP,
        "the magnet held a drag that had it suspended"
    );
}

#[test]
fn the_census_reaches_every_command_these_drags_claim() {
    let sent = dispatched();
    for label in [
        "Move clip",
        "Slip clip",
        "Trim clip",
        "Set speed",
        "Shift clips",
        "Set transition length",
        "Set audio levels",
        "Ripple move clip",
    ] {
        assert!(
            sent.contains(label),
            "the timeline census pressed every drag the panel offers and none of \
             them sent `{label}`"
        );
    }
}

#[test]
fn no_timeline_panel_file_calls_a_setter_itself() {
    for panel in [
        "timeline/clip.rs",
        "timeline/clip/finish.rs",
        "timeline/clip/finish_drag.rs",
        "timeline/clip/finish_press.rs",
        "timeline/clip/hit.rs",
        "timeline/ruler.rs",
    ] {
        super::no_setter_escapes(REGISTRY, panel);
    }
}
