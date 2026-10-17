//! Sequence > Linked Selection.
//!
//! # Why
//!
//! Linked clips (a picture and its sound) select, move, and trim together while
//! linked selection is on. Turned off, each clip of the pair is its own clip:
//! a click selects one, a drag or a nudge moves one, an edge drag trims one.
//! The toggle is session state read at every gesture, so these tests drive the
//! gesture entry points (`select_clip_linked`, `move_clip`, `trim_clip`,
//! `nudge_selected`, `move_selection_by`) rather than the engine commands, and
//! check both clips of the pair afterwards. Grouped clips stay together with
//! linked selection off: a group is formed on purpose, a link by import.
//!
//! Not caught here: the pointer path from a drawn clip bar to these entry
//! points; the timeline gesture tests and the chord census in
//! `operator_surface_tests` drive those with the toggle in both states.

use aria_models::ClipId;

use super::AppState;
use super::fixtures::linked_pair;

/// The pair's two clips, picture first, and each one's timeline span.
fn pair(state: &AppState) -> [(ClipId, i64, i64); 2] {
    let spans: Vec<(ClipId, i64, i64)> = state
        .selection
        .iter()
        .map(|id| {
            let ui = state.ui_clip_ref(*id).expect("a clip of the pair");
            (*id, ui.timeline_start_ticks, ui.timeline_end_ticks)
        })
        .collect();
    [spans[0], spans[1]]
}

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

#[test]
fn a_click_selects_the_pair_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, ..), (sound, ..)] = pair(&state);

    state.select_clip_linked(picture, true);
    assert_eq!(
        state.selection.len(),
        2,
        "on: the link selects with the clip"
    );

    state.toggle_linked_selection();
    assert!(!state.linked_selection);
    state.select_clip_linked(picture, true);
    assert_eq!(state.selection, vec![picture], "off: one clip");
    state.select_clip_linked(sound, true);
    assert_eq!(state.selection, vec![sound]);

    state.toggle_linked_selection();
    state.select_clip_linked(sound, true);
    assert_eq!(state.selection.len(), 2, "back on: the pair again");
}

#[test]
fn a_move_carries_the_link_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    let track = state.track_of(picture).expect("the picture's lane");

    state
        .move_clip(picture, track, p_start + 48)
        .expect("move on");
    assert_eq!(span(&state, picture).0, p_start + 48);
    assert_eq!(span(&state, sound).0, s_start + 48, "on: the sound follows");

    state.toggle_linked_selection();
    state
        .move_clip(picture, track, p_start + 96)
        .expect("move off");
    assert_eq!(span(&state, picture).0, p_start + 96);
    assert_eq!(span(&state, sound).0, s_start + 48, "off: the sound stays");
    assert!(state.undo(), "the solo move is one undo step");
    assert_eq!(span(&state, picture).0, p_start + 48);
    assert_eq!(span(&state, sound).0, s_start + 48);
}

#[test]
fn a_trim_carries_the_link_only_while_linked_selection_is_on() {
    let mut state = linked_pair();
    let [(picture, p_start, p_end), (sound, s_start, s_end)] = pair(&state);
    // Two frames off the tail: short enough that either clip survives it.
    let cut = 2;

    state
        .trim_clip(picture, p_start, p_end - cut)
        .expect("trim on");
    assert_eq!(span(&state, picture), (p_start, p_end - cut));
    assert_eq!(
        span(&state, sound),
        (s_start, s_end - cut),
        "on: the sound takes the same delta"
    );

    state.toggle_linked_selection();
    state
        .trim_clip(picture, p_start, p_end - 2 * cut)
        .expect("trim off");
    assert_eq!(span(&state, picture), (p_start, p_end - 2 * cut));
    assert_eq!(
        span(&state, sound),
        (s_start, s_end - cut),
        "off: the sound keeps its edge"
    );
}

#[test]
fn a_nudge_with_linked_selection_off_moves_only_the_selected_clip() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    state.toggle_linked_selection();
    state.select_clip_linked(picture, true);
    state.nudge_selected(5).expect("nudge");
    assert_eq!(span(&state, picture).0, p_start + 5);
    assert_eq!(span(&state, sound).0, s_start, "the sound stays");

    // The step chords move the whole selection by the same rule.
    state.move_selection_by(3).expect("step");
    assert_eq!(span(&state, picture).0, p_start + 8);
    assert_eq!(span(&state, sound).0, s_start);

    state.toggle_linked_selection();
    state.select_clip_linked(picture, true);
    state.move_selection_by(2).expect("step on");
    assert_eq!(span(&state, picture).0, p_start + 10);
    assert_eq!(span(&state, sound).0, s_start + 2, "on: the link steps too");
}

#[test]
fn a_group_moves_together_whatever_the_toggle() {
    let mut state = linked_pair();
    let [(picture, p_start, _), (sound, s_start, _)] = pair(&state);
    state.unlink_selected().expect("unlink the pair");
    state.group_selected().expect("group the pair");
    state.toggle_linked_selection();
    let track = state.track_of(picture).expect("the picture's lane");

    state.select_clip_linked(picture, true);
    assert_eq!(state.selection.len(), 2, "a click selects the group");

    state.move_clip(picture, track, p_start + 24).expect("move");
    assert_eq!(span(&state, picture).0, p_start + 24);
    assert_eq!(span(&state, sound).0, s_start + 24, "the group follows");

    state.move_selection_by(4).expect("step");
    assert_eq!(span(&state, picture).0, p_start + 28);
    assert_eq!(
        span(&state, sound).0,
        s_start + 28,
        "the step moves the group once, not once per member"
    );
}
