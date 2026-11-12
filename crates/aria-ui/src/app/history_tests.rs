//! Contracts for the history surface.
//!
//! The class closed here is "undo works but nothing can describe it": a history
//! list that shows the wrong name for a step, or that jumps to a state the
//! engine is not actually in, is worse than no list. Every assertion pairs a
//! label with an observable project state, so a label can never drift from the
//! step it names.
//!
//! A step taken during playback also changes what is heard: the mix the audio
//! side plays was built from the project before the step, so every history
//! mover hands the transport to the audio side again.
//!
//! Not covered: the drawn rows, which need a render surface, and the device
//! stream the audio pump starts from the mix, which these tests never open.

use std::sync::Arc;

use aria_models::ClipId;

use crate::audio::ScrubGrain;

use super::AppState;
use super::premiere_tests::with_clip;

fn labels(state: &AppState) -> Vec<(String, bool)> {
    state
        .history_entries()
        .iter()
        .map(|entry| (entry.label.clone(), entry.applied))
        .collect()
}

/// A clip, then a split, then a marker: three named steps in order.
///
/// A marker rather than a second clip edit, because a duplicate at the split
/// point would be refused for overlap and the fixture would be asserting that
/// instead of history.
fn three_steps() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    let end = state
        .selected_ui_clip()
        .expect("the fixture clip")
        .timeline_end_ticks;
    state.set_playhead(end / 2);
    state.split_at_playhead().expect("split");
    state.add_marker_at_playhead().expect("marker");
    (state, clip)
}

#[test]
fn every_applied_command_appears_in_order_with_its_own_name() {
    let (state, _) = three_steps();
    let names: Vec<String> = labels(&state).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names.iter().rev().take(2).cloned().collect::<Vec<_>>(),
        vec!["Add marker".to_string(), "Split clip".to_string()],
        "the newest steps are the ones just applied: {names:?}"
    );
    assert!(
        labels(&state).iter().all(|(_, applied)| *applied),
        "nothing is undone yet"
    );
    assert_eq!(state.history_depth(), labels(&state).len());
}

#[test]
fn jumping_back_leaves_the_steps_listed_as_undone() {
    let (mut state, _) = three_steps();
    let full = state.history_depth();
    let clips_before = state.store.sequence.clip_count();

    assert!(state.history_jump(full - 2), "two steps back is reachable");
    assert_eq!(state.history_depth(), full - 2);
    assert!(
        state.store.sequence.clip_count() < clips_before,
        "the split is gone from the timeline"
    );
    let applied: Vec<bool> = labels(&state).into_iter().map(|(_, a)| a).collect();
    assert_eq!(
        applied,
        (0..full).map(|i| i < full - 2).collect::<Vec<_>>(),
        "the undone steps stay listed, marked unapplied"
    );

    assert!(state.history_jump(full), "and forward again");
    assert_eq!(state.store.sequence.clip_count(), clips_before);
    assert!(labels(&state).iter().all(|(_, applied)| *applied));
}

#[test]
fn jumping_to_the_current_depth_does_nothing() {
    let (mut state, _) = three_steps();
    let depth = state.history_depth();
    let before = labels(&state);
    assert!(!state.history_jump(depth), "already there");
    assert_eq!(labels(&state), before);
    assert!(state.status_message.is_none() || state.history_depth() == depth);
}

#[test]
fn a_new_edit_after_a_jump_drops_the_steps_that_were_undone() {
    let (mut state, _) = three_steps();
    let full = state.history_depth();
    state.history_jump(full - 1);
    assert_eq!(
        labels(&state).len(),
        full,
        "the undone step is still listed"
    );

    state.set_playhead(0);
    state.add_marker_at_playhead().expect("a fresh edit");
    let names: Vec<String> = labels(&state).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names.len(),
        full,
        "the redo branch was replaced, not appended"
    );
    assert_eq!(names.last().map(String::as_str), Some("Add marker"));
    assert!(labels(&state).iter().all(|(_, applied)| *applied));
}

#[test]
fn a_gesture_is_one_named_step() {
    let (mut state, _) = with_clip();
    let track = state
        .store
        .sequence
        .tracks
        .first()
        .map(|t| aria_models::TrackId::from_raw(t.id))
        .expect("a lane");
    let before = state.history_depth();
    // Shifting a whole lane is one gesture; the engine groups it.
    state.shift_track(track, 4).expect("shift the lane");
    assert_eq!(
        state.history_depth(),
        before + 1,
        "one gesture, one undo step"
    );
    let (name, applied) = labels(&state).pop().expect("a step");
    assert!(applied);
    assert!(
        name.starts_with("Shift clips"),
        "named for the command it ran: {name}"
    );
}

/// **The undo list is one list per projection, not one per read.**
///
/// The dialog cannot hold a borrow of the state across the frame, so it used to
/// copy every step of the stack on every frame it was open, and the copy grew
/// with the session rather than with what was on screen. It is a handle now: two
/// reads with nothing in between are the same list, and an edit replaces it.
#[test]
fn the_undo_list_is_one_list_per_projection() {
    let (mut state, _) = three_steps();
    let first = state.history_entries();
    assert!(
        std::sync::Arc::ptr_eq(&first, &state.history_entries()),
        "a second read copied the stack"
    );
    assert!(!first.is_empty(), "the fixture wrote no history");
    assert!(state.undo(), "the marker comes off");
    let after = state.history_entries();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &after),
        "an undo left the dialog drawing the stack from before it"
    );
    assert_eq!(
        after.len(),
        first.len(),
        "an undo keeps the step and unapplies it"
    );
    assert!(
        after.last().is_some_and(|entry| !entry.applied),
        "the step the undo took is still listed, unapplied"
    );
}

/// One way of moving through the history, as the controls call it.
type Step = fn(&mut AppState) -> bool;

/// Every way of moving through the history: the undo and redo commands and a
/// jump from the history list, in an order that leaves the depth where it
/// started.
fn steps() -> [(&'static str, Step); 4] {
    [
        ("undo", AppState::undo),
        ("redo", AppState::redo),
        ("jump back", |state: &mut AppState| {
            state.history_jump(state.history_depth() - 1)
        }),
        ("jump forward", |state: &mut AppState| {
            state.history_jump(state.history_depth() + 1)
        }),
    ]
}

/// A pending scrub grain marks the audio handle: a play or a stop handed to
/// the audio side clears it, and no pump runs in these tests to take it.
fn mark_the_audio_side(state: &AppState) {
    state.audio.queue_scrub(ScrubGrain {
        mixer: None,
        revision: 0,
        tick: 0,
        fps_num: 24,
        fps_den: 1,
    });
}

/// **A history step during playback hands the audio side the new mix.**
///
/// The class is a history mover that refreshes the picture and leaves the
/// sound: undo, redo, or a jump from the history list while playing, and the
/// mixer keeps playing the edit that was just taken back until the next seek.
/// Each mover is checked in the three transport states it can meet: playing
/// with the mixer following hands a play (the lane meters the play armed
/// stay), playing above the audible ceiling hands a stop, and paused hands
/// nothing, because an unasked play would start the sound while the picture
/// stands still.
#[test]
fn every_history_step_hands_the_audio_side_the_transport_it_steps_under() {
    let (mut state, _) = three_steps();

    state.toggle_playback();
    assert!(
        state.playing && state.audio_follows(),
        "the fixture plays at 1x"
    );
    for (name, step) in steps() {
        let armed = state.audio.arm_lane_meters(std::iter::empty());
        mark_the_audio_side(&state);
        assert!(step(&mut state), "{name} moved the history");
        assert!(state.playing, "{name} left playback running");
        assert_eq!(
            state.audio.pending_scrub_grains(),
            0,
            "{name} during playback handed the audio side nothing"
        );
        assert!(
            Arc::ptr_eq(&state.audio.lane_meters(), &armed),
            "{name} during playback stopped the sound instead of handing it the mix"
        );
    }

    state.toggle_playback();
    assert!(!state.playing, "the fixture paused");
    for (name, step) in steps() {
        let armed = state.audio.arm_lane_meters(std::iter::empty());
        mark_the_audio_side(&state);
        assert!(step(&mut state), "{name} moved the history");
        assert_eq!(
            state.audio.pending_scrub_grains(),
            1,
            "{name} while paused handed the audio side a play or a stop"
        );
        assert!(Arc::ptr_eq(&state.audio.lane_meters(), &armed));
    }

    for _ in 0..4 {
        state.set_shuttle(1);
    }
    assert!(
        state.playing && !state.audio_follows(),
        "the fixture plays at {}, above the audible ceiling",
        state.shuttle
    );
    for (name, step) in steps() {
        let armed = state.audio.arm_lane_meters(std::iter::empty());
        mark_the_audio_side(&state);
        assert!(step(&mut state), "{name} moved the history");
        assert_eq!(
            state.audio.pending_scrub_grains(),
            0,
            "{name} above the audible ceiling handed the audio side nothing"
        );
        assert!(
            !Arc::ptr_eq(&state.audio.lane_meters(), &armed),
            "{name} above the audible ceiling handed the audio side a play"
        );
    }
}
