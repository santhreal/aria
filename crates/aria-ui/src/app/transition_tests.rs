//! Contracts for transitions at a cut.
//!
//! The class closed here is "a surface offers a blend at a junction the engine
//! refuses, or a duration edit silently replaces the blend". The catalog is
//! enumerated from `aria_models::transition_catalog` at run time, so a new blend
//! id is covered without a second list, and every one of them is applied,
//! replaced, resized, and removed. The crossfade curves are enumerated from
//! `AudioTransition::ALL` the same way, on a cut between sounds.

use aria_models::{AudioTransition, ClipId, TransitionMedia, transition_catalog};

use super::fixtures::with_audio;
use super::premiere_tests::with_clip;
use crate::app::AppState;
use crate::app::clip_transitions::{MAX_TRANSITION_TICKS, MIN_TRANSITION_TICKS};
use crate::app::decline::Decline;

/// A project with two abutting clips on one video track, the left one selected.
///
/// A split is the only way to guarantee abutment, which is exactly the junction
/// `AddTransition` accepts.
fn with_cut() -> (AppState, ClipId, ClipId) {
    split_in_two(with_clip())
}

/// The same cut between two halves of a sound on an audio lane.
fn with_sound_cut() -> (AppState, ClipId, ClipId) {
    split_in_two(with_audio())
}

fn split_in_two((mut state, left): (AppState, ClipId)) -> (AppState, ClipId, ClipId) {
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

/// Which kind of blend the selection's right junction takes, read where the
/// surfaces read it: the projected field on the selected clip.
fn junction_media(state: &AppState) -> Option<TransitionMedia> {
    state
        .selected_ui_clip_ref()
        .and_then(|clip| clip.transition_media)
}

#[test]
fn a_cut_can_carry_a_transition_and_a_lone_clip_cannot() {
    let (state, _) = with_clip();
    assert_eq!(
        junction_media(&state),
        None,
        "a clip with nothing to its right has no junction"
    );

    let (state, _, right) = with_cut();
    assert_eq!(
        junction_media(&state),
        Some(TransitionMedia::Picture),
        "the left side of a cut between pictures must accept a blend"
    );

    // The right clip of the cut has nothing after it, so its own junction is
    // not offered either.
    let mut state = state;
    state.select_clip(right);
    assert_eq!(
        junction_media(&state),
        None,
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
    let id = aria_models::shipped_transition_catalog()[0].id.as_str();
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
    assert_eq!(junction_media(&state), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
}

#[test]
fn breaking_the_abutment_drops_the_dead_junction() {
    let (mut state, left, right) = with_cut();
    let id = aria_models::shipped_transition_catalog()[0].id.as_str();
    state.set_selected_transition(id).expect("apply");
    assert_eq!(transitions_on_track(&state), 1);

    // Deleting the incoming side leaves no junction, so the transition must not
    // survive as a phantom on the projected clip.
    state.select_clip(right);
    state.clear_selected().expect("delete right");
    state.select_clip(left);
    assert_eq!(
        transitions_on_track(&state),
        0,
        "a transition outlived its junction"
    );
    assert_eq!(junction_media(&state), None);

    // Undo restores both the clip and the transition.
    assert!(state.undo(), "undo did nothing");
    assert_eq!(
        transitions_on_track(&state),
        1,
        "undo did not restore the transition"
    );
}

// The class closed below is "the wedge on the timeline is a readout". A wedge
// belongs to a clip, so its menu and its edge drag must address that clip without
// taking the selection, and an edge drag must be one edit.

/// Both edges read the same window, the two directions are opposite, and a drag
/// past the bounds stalls at them.
#[test]
fn dragging_either_wedge_edge_sets_the_window_symmetrically() {
    use crate::app::clip_transitions::transition_ticks_after_drag;
    use aria_models::TransitionAlignment;

    let current = 48;
    let center = TransitionAlignment::Center;
    assert_eq!(
        transition_ticks_after_drag(current, 6, true, center),
        current + 12,
        "pulling the right edge outward must widen by twice the move"
    );
    assert_eq!(
        transition_ticks_after_drag(current, -6, false, center),
        current + 12,
        "pulling the left edge outward must widen by the same amount"
    );
    assert_eq!(
        transition_ticks_after_drag(current, -6, true, center),
        current - 12
    );
    assert_eq!(
        transition_ticks_after_drag(current, 6, false, center),
        current - 12
    );
    for align in [TransitionAlignment::Start, TransitionAlignment::End] {
        assert_eq!(
            transition_ticks_after_drag(current, 6, true, align),
            current + 6,
            "{align:?}: pulling the right edge outward widens 1:1"
        );
        assert_eq!(
            transition_ticks_after_drag(current, -6, false, align),
            current + 6,
            "{align:?}: pulling the left edge outward widens 1:1"
        );
        assert_eq!(
            transition_ticks_after_drag(current, -6, true, align),
            current - 6
        );
        assert_eq!(
            transition_ticks_after_drag(current, 6, false, align),
            current - 6
        );
    }
    assert_eq!(
        transition_ticks_after_drag(current, 0, true, center),
        current,
        "a drag that moved nothing must not change the window"
    );
    assert_eq!(
        transition_ticks_after_drag(current, 10_000, true, center),
        MAX_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(current, -10_000, true, center),
        MIN_TRANSITION_TICKS
    );
    // Saturating, not panicking, at the ends of the tick domain. Widening from
    // `i64::MIN` by the largest possible move still lands below the floor, so the
    // floor is the answer; the point is that neither end overflows.
    assert_eq!(
        transition_ticks_after_drag(i64::MAX, i64::MAX, true, center),
        MAX_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(i64::MIN, i64::MIN, false, center),
        MIN_TRANSITION_TICKS
    );
    assert_eq!(
        transition_ticks_after_drag(i64::MIN, i64::MAX, false, center),
        MIN_TRANSITION_TICKS
    );
}

/// A wedge acts on its own clip: the menu and the drag work while another clip is
/// selected, and neither changes the selection.
#[test]
fn a_wedge_edits_its_own_junction_without_taking_the_selection() {
    let (mut state, left, right) = with_cut();
    let id = aria_models::shipped_transition_catalog()[0].id.as_str();

    state.select_clip(right);
    assert_eq!(
        state.clip_transition_media(right),
        None,
        "the fixture's right clip must have no junction of its own"
    );
    assert_eq!(
        state.clip_transition_media(left),
        Some(TransitionMedia::Picture)
    );

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

/// A cut between sounds takes every crossfade curve and no picture blend, and a
/// cut between pictures the reverse. Each refusal leaves the junction as it was
/// and names the chord that fits the cut it hit.
#[test]
fn each_cut_takes_only_the_transitions_of_its_media() {
    let (mut state, _, _) = with_sound_cut();
    assert_eq!(junction_media(&state), Some(TransitionMedia::Audio));
    for curve in AudioTransition::ALL {
        state
            .set_selected_transition(curve.id())
            .unwrap_or_else(|err| panic!("apply {}: {err:?}", curve.id()));
        assert_eq!(state.selected_transition_id().as_deref(), Some(curve.id()));
        let ui = state.selected_ui_clip().expect("clip");
        let projected = ui.transition.as_ref().expect("projected transition");
        assert_eq!(&*projected.label, curve.label());
        assert_eq!(transitions_on_track(&state), 1, "{} stacked", curve.id());
    }
    let last = AudioTransition::ALL[AudioTransition::ALL.len() - 1].id();
    for spec in transition_catalog() {
        state.set_selected_transition(&spec.id).expect("inert");
        assert_eq!(
            state.selected_transition_id().as_deref(),
            Some(last),
            "{} replaced a crossfade",
            spec.id
        );
        assert_eq!(
            state.status_message.as_deref(),
            Some(Decline::SoundCut.reason())
        );
    }

    let (mut state, _, _) = with_cut();
    for curve in AudioTransition::ALL {
        state.set_selected_transition(curve.id()).expect("inert");
        assert_eq!(state.selected_transition_id(), None, "{}", curve.id());
        assert_eq!(
            state.status_message.as_deref(),
            Some(Decline::PictureCut.reason())
        );
    }
}

/// Ctrl+T and Ctrl+Shift+T each find the nearest cut of their own media, and a
/// selected edge on a cut of the other kind declines with the chord that fits.
#[test]
fn each_chord_puts_its_default_on_a_cut_of_its_own_media() {
    const NO_CUT: &str = "No cut between two clips to put a transition on";

    let (mut state, left, right) = with_sound_cut();
    state.clear_selection();
    state
        .add_default_transition(TransitionMedia::Picture)
        .expect("inert");
    assert_eq!(state.clip_transition_id(left), None);
    assert_eq!(state.status_message.as_deref(), Some(NO_CUT));
    state
        .add_default_transition(TransitionMedia::Audio)
        .expect("crossfade");
    assert_eq!(
        state.clip_transition_id(left).as_deref(),
        Some(AudioTransition::DEFAULT.id())
    );
    state.clear_clip_transition(left).expect("remove");

    // The In edge of the right clip addresses the same cut.
    state.select_edge(right, crate::types::TrimSide::In);
    state
        .add_default_transition(TransitionMedia::Picture)
        .expect("inert");
    assert_eq!(state.clip_transition_id(left), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::SoundCut.reason())
    );
    state
        .add_default_transition(TransitionMedia::Audio)
        .expect("crossfade");
    assert_eq!(
        state.clip_transition_id(left).as_deref(),
        Some(AudioTransition::DEFAULT.id())
    );

    let (mut state, left, _) = with_cut();
    state.clear_selection();
    state
        .add_default_transition(TransitionMedia::Audio)
        .expect("inert");
    assert_eq!(state.clip_transition_id(left), None);
    assert_eq!(state.status_message.as_deref(), Some(NO_CUT));
    state
        .add_default_transition(TransitionMedia::Picture)
        .expect("blend");
    assert_eq!(
        state.clip_transition_id(left),
        transition_catalog().first().map(|spec| spec.id.clone())
    );
}

#[test]
fn transition_alignment_projects_and_switches_from_menu_and_inspector() {
    use crate::surfaces::clip_pick::{self, Pick};
    use crate::surfaces::context_menu::{ClipMenu, TRANSITION_ITEMS};
    use aria_models::TransitionAlignment;

    let (mut state, left, _) = with_cut();
    let id = aria_models::shipped_transition_catalog()[0].id.clone();
    state.select_clip(left);
    state.set_selected_transition(&id).expect("add transition");
    assert_eq!(
        state.selected_transition_alignment(),
        Some(TransitionAlignment::Center)
    );

    for (align, expected_checked, expected_window) in [
        (TransitionAlignment::Start, "• Start at Cut", (48, 72)),
        (TransitionAlignment::End, "• End at Cut", (24, 48)),
        (TransitionAlignment::Center, "• Center at Cut", (36, 60)),
    ] {
        clip_pick::apply(&mut state, Pick::TransitionAlignment(align)).expect("pick alignment");
        assert_eq!(state.selected_transition_alignment(), Some(align));
        let ui_tr = state
            .ui_clip_ref(left)
            .and_then(|c| c.transition.as_ref())
            .expect("projected transition");
        assert_eq!(ui_tr.window(48), expected_window);
        let facts = ClipMenu::of(&state, left.raw());
        let labels: Vec<&str> = TRANSITION_ITEMS
            .iter()
            .filter_map(|item| item.row().map(|row| (row.label)(&facts)))
            .collect();
        assert!(
            labels.contains(&expected_checked),
            "expected {expected_checked:?} in {labels:?}"
        );
    }
}
