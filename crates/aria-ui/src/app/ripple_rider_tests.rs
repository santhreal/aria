//! A ripple trim moves every sync-locked lane with the trimmed one.
//!
//! # Why
//!
//! A ripple trim moved the trimmed lane and the lanes of clips linked to it,
//! and no other. A sync-locked lane of titles, sound cuts or a second picture
//! stayed where it was while the lane beside it slid, so the two fell out of
//! step by the length of the trim, though a ripple insert, delete and move
//! each moved that lane. The class closed here is a ripple edit whose riding
//! lanes are selected by something other than the sync lock.
//!
//! Each case drags the subject's head or tail both ways, as the timeline
//! sends the drag, and reads every lane: the held frame, then the release. A
//! sync-locked lane moves what starts at or after the edge as it was by the
//! same amount as the clip after the subject, a lane with its sync lock
//! cleared stays, and one undo puts all three back. A shortening drag far
//! past every bound stops where the riding lane runs out of open lane, the
//! edits one tick further are refused, and with that lane's sync lock cleared
//! the same drag reaches the end of the take.
//!
//! Not covered: a riding lane that is also locked, and a riding clip that
//! lies across the edge, which stays where it is.

use aria_models::{ClipId, MediaId, RationalTime, TimeRange, TrackId, TrackKind};

use crate::app::AppState;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture, Span};
use crate::tool_catalog::EdgeDrag;

/// Further than any bound the dressing has, both ways.
const FAR: i64 = 1 << 20;

/// One clip, in frames: where it starts, how long it is, and where in the
/// take it starts.
type Cut = (i64, i64, i64);

/// The trimmed lane: the subject over 10..30 with six frames of take before
/// its head and the rest after its tail, and a clip after it at 30.
const EDITED: [Cut; 2] = [(10, 20, 6), (30, 10, 0)];

/// A riding lane with a clip before the subject, one under it and one after
/// it: four frames of open lane before the clip at 12, sixteen before the
/// clip at 34.
const UNDER: [Cut; 3] = [(0, 8, 0), (12, 6, 0), (34, 10, 0)];

/// A dressed timeline: the trimmed lane, a sync-locked lane and a lane with
/// its sync lock cleared, the last two holding the same cuts.
struct Lanes {
    state: AppState,
    edited: TrackId,
    riding: TrackId,
    pinned: TrackId,
    subject: ClipId,
}

/// Add a picture lane named `name` and return it.
fn added(state: &mut AppState, name: &str) -> TrackId {
    let had: Vec<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.id)
        .collect();
    state.add_track(TrackKind::Video, name).expect("add a lane");
    state.sync_from_engine();
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.id)
        .find(|id| !had.contains(id))
        .map(TrackId::from_raw)
        .expect("the new lane is projected")
}

/// [`EDITED`] on the first lane, and `under` on a sync-locked lane and on a
/// lane with its sync lock cleared, cut from the video fixture's take.
fn dressed(under: &[Cut]) -> Lanes {
    let (mut state, _) = crate::app::fixtures::with_video();
    let edited = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let riding = added(&mut state, "V2");
    let pinned = added(&mut state, "V3");
    state
        .toggle_track_sync_lock(pinned)
        .expect("clear V3's sync lock");
    let media = MediaId::from_raw(state.store.media[0].id);
    let subject = {
        let mut engine = state.engine().lock();
        let mut project = (*engine.project()).clone();
        let rate = project.timeline().frame_rate;
        for clip in state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
        {
            project.remove_clip(ClipId::from_raw(clip.id));
        }
        let mut place = |track: TrackId, (at, len, take): Cut| {
            project
                .add_clip(
                    track,
                    media,
                    TimeRange::at_rate(take, len, rate),
                    RationalTime::new(at, rate),
                )
                .expect("the cut fits the take and the lane")
        };
        let subject = place(edited, EDITED[0]);
        place(edited, EDITED[1]);
        for &cut in under {
            place(riding, cut);
            place(pinned, cut);
        }
        engine.reset_project(project);
        subject
    };
    state.sync_from_engine();
    Lanes {
        state,
        edited,
        riding,
        pinned,
        subject,
    }
}

/// A ripple drag on the subject's head (`left`) or tail.
fn ripple(left: bool) -> Gesture {
    Gesture::Edge {
        drag: Some(EdgeDrag::Ripple),
        left,
        span: Span {
            start: EDITED[0].0,
            end: EDITED[0].0 + EDITED[0].1,
            threshold: 0,
            magnet: false,
        },
    }
}

/// Where each clip on `lane` starts, as the store holds it.
fn starts(state: &AppState, lane: TrackId) -> Vec<i64> {
    held_starts(state, lane, false)
}

/// Where each clip on `lane` starts, as the frame draws it: with `held`, the
/// drag in flight drawn over the store.
fn held_starts(state: &AppState, lane: TrackId, held: bool) -> Vec<i64> {
    let rate = state.store.sequence.frame_rate;
    let row = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|row| row.id == lane.raw())
        .expect("the lane is projected");
    let clips = match state
        .held_edit
        .as_deref()
        .filter(|edit| held && edit.touches(row.id))
    {
        Some(edit) => edit.lane(row.id, &row.clips, (i64::MIN, i64::MAX), rate),
        None => row.clips.to_vec(),
    };
    let mut starts: Vec<i64> = clips.iter().map(|clip| clip.timeline_start_ticks).collect();
    starts.sort_unstable();
    starts
}

#[test]
fn a_sync_locked_lane_rides_a_ripple_trim_and_a_cleared_one_stays() {
    // (head, travel, the trimmed lane after, the riding lane after). The
    // riding lane moves what starts at or after the edge as it was: the head
    // at 10, the tail at 30.
    let cases: [(bool, i64, [i64; 2], [i64; 3]); 4] = [
        (false, -4, [10, 26], [0, 12, 30]),
        (false, 4, [10, 34], [0, 12, 38]),
        (true, 4, [10, 26], [0, 8, 30]),
        (true, -4, [10, 34], [0, 16, 38]),
    ];
    for (left, delta, edited_after, riding_after) in cases {
        let Lanes {
            mut state,
            edited,
            riding,
            pinned,
            subject,
        } = dressed(&UNDER);
        let name = format!(
            "a ripple of the {} by {delta}",
            if left { "head" } else { "tail" }
        );
        let was = [edited, riding, pinned].map(|lane| starts(&state, lane));
        let gesture = ripple(left);
        let drag = Drag {
            clip: subject,
            delta,
        };
        state.hold_drag(gesture, drag);
        let drawn = [edited, riding, pinned].map(|lane| held_starts(&state, lane, true));
        state.end_drag();
        timeline_gesture::apply(&mut state, gesture, drag)
            .unwrap_or_else(|err| panic!("{name} was refused: {err}"));
        state.sync_from_engine();
        let landed = [edited, riding, pinned].map(|lane| starts(&state, lane));
        assert_eq!(
            landed[0], edited_after,
            "{name} left the trimmed lane wrong"
        );
        assert_eq!(
            landed[1], riding_after,
            "{name} left the sync-locked lane out of step"
        );
        assert_eq!(
            landed[2], was[2],
            "{name} moved a lane with its sync lock cleared"
        );
        assert_eq!(drawn, landed, "{name} drew lanes its release did not leave");
        assert!(state.undo(), "{name} left no undo step");
        assert_eq!(
            [edited, riding, pinned].map(|lane| starts(&state, lane)),
            was,
            "one undo of {name} did not put every lane back"
        );
    }
}

#[test]
fn a_shortening_ripple_stops_where_a_sync_locked_lane_runs_out_of_open_lane() {
    // (head, the riding lane, the travel the drag lands, the travel with that
    // lane's sync lock cleared: the subject down to one frame).
    let cases: [(bool, [Cut; 2], i64, i64); 2] = [
        (false, [(0, 29, 0), (31, 10, 0)], -2, -19),
        (true, [(0, 9, 0), (12, 10, 0)], 3, 19),
    ];
    for (left, under, stops, unpinned) in cases {
        let edge = if left { "head" } else { "tail" };
        let far = if left { FAR } else { -FAR };
        let lanes = dressed(&under);
        let (mut state, subject) = (lanes.state, lanes.subject);
        let gesture = ripple(left);
        let drag = Drag {
            clip: subject,
            delta: far,
        };
        let held = timeline_gesture::held(&state, gesture, &drag).expect("a ripple has a room");
        assert_eq!(
            (held.travel, held.stopped),
            (stops, true),
            "a far ripple of the {edge} stopped at the wrong tick"
        );
        let past = timeline_gesture::steps(&state, gesture, subject, stops + stops.signum());
        assert!(
            state.run_steps(gesture.step_name(), &past).is_err(),
            "a ripple of the {edge} one tick past where it stopped was not refused"
        );
        timeline_gesture::apply(&mut state, gesture, drag)
            .unwrap_or_else(|err| panic!("a far ripple of the {edge} was refused: {err}"));
        assert!(
            state.undo(),
            "the far ripple of the {edge} left no undo step"
        );
        state
            .toggle_track_sync_lock(lanes.riding)
            .expect("clear the riding lane's sync lock");
        let free = timeline_gesture::held(&state, gesture, &drag).expect("a ripple has a room");
        assert_eq!(
            free.travel, unpinned,
            "a far ripple of the {edge} stopped at a lane with its sync lock cleared"
        );
    }
}
