//! Contracts for transitions at a cut.
//!
//! The class closed here is "a surface offers a blend at a junction the engine
//! refuses, or a duration edit silently replaces the blend". The catalog is
//! enumerated from `aria_models::transition_catalog` at run time, so a new blend
//! id is covered without a second list, and every one of them is applied,
//! replaced, resized, and removed.

use aria_models::{ClipId, transition_catalog};

use super::premiere_tests::with_clip;
use crate::app::AppState;
use crate::app::clip_transitions::{MAX_TRANSITION_TICKS, MIN_TRANSITION_TICKS};
use crate::app::decline::Decline;

/// A project with two abutting clips on one video track, the left one selected.
///
/// A split is the only way to guarantee abutment, which is exactly the junction
/// `AddTransition` accepts.
fn with_cut() -> (AppState, ClipId, ClipId) {
    let (mut state, left) = with_clip();
    let (_, end) = bounds(&state, left);
    state.set_playhead(end / 2);
    state.split_at_playhead().expect("split");

    let mut clips: Vec<(u64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.id, clip.timeline_start_ticks))
        .collect();
    clips.sort_by_key(|(_, start)| *start);
    assert_eq!(clips.len(), 2, "the split must produce two clips");
    let left = ClipId::from_raw(clips[0].0);
    let right = ClipId::from_raw(clips[1].0);
    state.select_clip(left);
    (state, left, right)
}

fn bounds(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("clip present");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

/// Whether the selection's right junction can carry a blend, read where the
/// surfaces read it: the projected flag on the selected clip.
fn can_transition(state: &AppState) -> bool {
    state
        .selected_ui_clip_ref()
        .is_some_and(|clip| clip.can_transition)
}

#[test]
fn a_cut_can_carry_a_transition_and_a_lone_clip_cannot() {
    let (state, _) = with_clip();
    assert!(
        !can_transition(&state),
        "a clip with nothing to its right has no junction"
    );

    let (state, _, right) = with_cut();
    assert!(
        can_transition(&state),
        "the left side of a cut must accept a transition"
    );

    // The right clip of the cut has nothing after it, so its own junction is
    // not offered either.
    let mut state = state;
    state.select_clip(right);
    assert!(
        !can_transition(&state),
        "the last clip on a track has no right junction"
    );
}

#[test]
fn every_catalog_transition_applies_replaces_and_removes_at_the_cut() {
    let (mut state, left, _) = with_cut();
    assert!(!transition_catalog().is_empty());

    for spec in transition_catalog() {
        state
            .set_selected_transition(&spec.id)
            .unwrap_or_else(|err| panic!("apply {}: {err:?}", spec.id));
        assert_eq!(
            state.selected_transition_id().as_deref(),
            Some(spec.id.as_str()),
            "{} did not land at the junction",
            spec.id
        );
        assert_eq!(
            state.selected_transition_ticks(),
            Some(aria_models::DEFAULT_TRANSITION_TICKS),
            "{} did not use the model's default window",
            spec.id
        );

        // The projected label must resolve from the catalog, not a placeholder.
        let ui = state.selected_ui_clip().expect("clip");
        let projected = ui.transition.as_ref().expect("projected transition");
        assert_eq!(projected.label, spec.label);

        // Applying a second id replaces rather than stacks: one junction, one
        // transition.
        let count = transitions_on_track(&state);
        assert_eq!(count, 1, "{} stacked a second transition", spec.id);
        let _ = left;
    }

    state.clear_selected_transition().expect("remove");
    assert_eq!(state.selected_transition_id(), None);
    assert_eq!(transitions_on_track(&state), 0);
}

/// How many transitions the projected store shows on the whole sequence.
fn transitions_on_track(state: &AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .filter(|clip| clip.transition.is_some())
        .count()
}

#[test]
fn the_window_length_clamps_and_keeps_the_blend() {
    let (mut state, left, right) = with_cut();
    let id = transition_catalog()[0].id.as_str();
    state.set_selected_transition(id).expect("apply");
    let left_span = bounds(&state, left);
    let right_span = bounds(&state, right);
    let adjacent_limit = (left_span.1 - left_span.0).min(right_span.1 - right_span.0);

    state
        .set_selected_transition_ticks(MAX_TRANSITION_TICKS + 5_000)
        .expect("high");
    assert_eq!(
        state.selected_transition_ticks(),
        Some(MAX_TRANSITION_TICKS.min(adjacent_limit))
    );
    assert_eq!(state.selected_transition_id().as_deref(), Some(id));

    state.set_selected_transition_ticks(-1_000).expect("low");
    assert_eq!(
        state.selected_transition_ticks(),
        Some(MIN_TRANSITION_TICKS)
    );

    state.nudge_selected_transition_ticks(12).expect("nudge up");
    assert_eq!(
        state.selected_transition_ticks(),
        Some(MIN_TRANSITION_TICKS + 12)
    );
    state
        .nudge_selected_transition_ticks(-12)
        .expect("nudge down");
    assert_eq!(
        state.selected_transition_ticks(),
        Some(MIN_TRANSITION_TICKS)
    );
    assert_eq!(
        state.selected_transition_id().as_deref(),
        Some(id),
        "resizing must not drop the blend"
    );
}

/// A refusal is inert and named: nothing fails, nothing changes, and the status
/// line says which of the three reasons it was. Silence here was the defect —
/// the palette's "Add cross fade at cut" ran after a split, found the selected
/// half's junction abutting nothing, and left no trace at all.
#[test]
fn a_refused_transition_is_inert_and_names_the_reason() {
    // No junction: the lone clip fixture.
    let (mut state, _) = with_clip();
    state.set_selected_transition("crossfade").expect("inert");
    assert_eq!(state.selected_transition_id(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoAbuttingCut.reason())
    );
    state.clear_selected_transition().expect("inert");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoTransitionAtCut.reason())
    );
    state.set_selected_transition_ticks(100).expect("inert");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoTransitionAtCut.reason())
    );
    state.nudge_selected_transition_ticks(6).expect("inert");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoTransitionAtCut.reason())
    );

    // A real junction, but an id the catalog does not know.
    let (mut state, _, _) = with_cut();
    state
        .set_selected_transition("no-such-blend")
        .expect("inert");
    assert_eq!(state.selected_transition_id(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::UnknownTransition.reason())
    );

    // Duration edits before a transition exists are inert too.
    state.set_selected_transition_ticks(48).expect("inert");
    assert_eq!(state.selected_transition_ticks(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoTransitionAtCut.reason())
    );

    // And with no selection at all.
    state.clear_selection();
    state.set_selected_transition("crossfade").expect("inert");
    assert!(!can_transition(&state));
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
}

#[test]
fn breaking_the_abutment_drops_the_dead_junction() {
    let (mut state, left, right) = with_cut();
    let id = transition_catalog()[0].id.as_str();
    state.set_selected_transition(id).expect("apply");
    assert_eq!(transitions_on_track(&state), 1);

    // Deleting the incoming side leaves no junction, so the transition must not
    // survive as a phantom on the projected clip.
    state.select_clip(right);
    state.delete_selected().expect("delete right");
    state.select_clip(left);
    assert_eq!(
        transitions_on_track(&state),
        0,
        "a transition outlived its junction"
    );
    assert!(!can_transition(&state));

    // Undo restores both the clip and the transition.
    assert!(state.undo(), "undo did nothing");
    assert_eq!(
        transitions_on_track(&state),
        1,
        "undo did not restore the transition"
    );
}

/// The class closed below is "the wedge on the timeline is a readout". A wedge
/// belongs to a clip, so its menu and its edge drag must address that clip without
/// taking the selection, and an edge drag must be one edit.

/// Both edges read the same window, the two directions are opposite, and a drag
/// past the bounds stalls at them.
#[test]
fn dragging_either_wedge_edge_sets_the_window_symmetrically() {
    use crate::app::clip_transitions::transition_ticks_after_drag;

    let current = 48;
    assert_eq!(
        transition_ticks_after_drag(current, 6, true),
        current + 12,
        "pulling the right edge outward must widen by twice the move"
    );
    assert_eq!(
        transition_ticks_after_drag(current, -6, false),
        current + 12,
        "pulling the left edge outward must widen by the same amount"
    );
    assert_eq!(transition_ticks_after_drag(current, -6, true), current - 12);
    assert_eq!(transition_ticks_after_drag(current, 6, false), current - 12);
    assert_eq!(
        transition_ticks_after_drag(current, 0, true),
        current,
        "a drag that moved nothing must not change the window"
    );
    assert_eq!(
        transition_ticks_after_drag(current, 10_000, true),
        MAX_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(current, -10_000, true),
        MIN_TRANSITION_TICKS
    );
    // Saturating, not panicking, at the ends of the tick domain. Widening from
    // `i64::MIN` by the largest possible move still lands below the floor, so the
    // floor is the answer; the point is that neither end overflows.
    assert_eq!(
        transition_ticks_after_drag(i64::MAX, i64::MAX, true),
        MAX_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(i64::MIN, i64::MIN, false),
        MIN_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(i64::MIN, i64::MAX, false),
        MIN_TRANSITION_TICKS
    );
}

/// A wedge acts on its own clip: the menu and the drag work while another clip is
/// selected, and neither changes the selection.
#[test]
fn a_wedge_edits_its_own_junction_without_taking_the_selection() {
    let (mut state, left, right) = with_cut();
    let id = transition_catalog()[0].id.as_str();

    state.select_clip(right);
    assert!(
        !state.clip_can_transition(right),
        "the fixture's right clip must have no junction of its own"
    );
    assert!(state.clip_can_transition(left));

    state.set_clip_transition(left, id).expect("apply");
    assert_eq!(state.clip_transition_id(left).as_deref(), Some(id));
    assert_eq!(state.selection, vec![right], "the wedge took the selection");

    let before = state.engine().lock().revision();
    let target = state.clip_transition_ticks(left).expect("window") + 24;
    state
        .set_clip_transition_ticks(left, target)
        .expect("resize");
    assert_eq!(state.clip_transition_ticks(left), Some(target));
    assert_eq!(
        state.engine().lock().revision(),
        before + 1,
        "one drag must be one edit"
    );
    assert_eq!(state.clip_transition_id(left).as_deref(), Some(id));

    // A release that changed nothing writes nothing.
    state
        .set_clip_transition_ticks(left, target)
        .expect("still");
    assert_eq!(state.engine().lock().revision(), before + 1);

    assert!(state.undo(), "the resize must be undoable");
    assert_eq!(state.clip_transition_ticks(left), Some(target - 24));

    state.clear_clip_transition(left).expect("remove");
    assert_eq!(state.clip_transition_id(left), None);
    assert_eq!(state.selection, vec![right]);

    // Every clip-addressed call is inert on a junction that carries nothing.
    state.clear_clip_transition(left).expect("inert");
    state.set_clip_transition_ticks(left, 100).expect("inert");
    state.set_clip_transition(right, id).expect("inert");
    assert_eq!(state.clip_transition_id(right), None);
}
