//! Contracts for the tool-strip clock.
//!
//! The class closed here is "the readout is paint": the playhead could be read
//! but not typed, so a jump to an exact timecode meant scrubbing. The rules that
//! matter are that a refusal keeps the playhead where it was, that focus seeds
//! the draft from what is on screen, and that the clock takes keys through the
//! same field table as every other box rather than a block in the shell.
//!
//! Not covered: the click that takes focus, which needs a render surface. It
//! calls `focus_clock(true)` and nothing else.

use super::premiere_tests::with_clip;
use crate::keys::fields;

#[test]
fn focus_seeds_the_draft_from_the_playhead() {
    let (mut state, _) = with_clip();
    state.set_playhead(50);
    assert!(!state.clock_focused());
    assert_eq!(state.clock_text(), "00:00:02:02");

    state.focus_clock(true);
    assert!(state.clock_focused());
    assert_eq!(state.clock_text(), "00:00:02:02");
}

#[test]
fn a_typed_timecode_seeks() {
    let (mut state, _) = with_clip();
    state.focus_clock(true);
    while state.clock_draft.as_deref().is_some_and(|d| !d.is_empty()) {
        state.clock_pop();
    }
    for ch in "00:00:01:00".chars() {
        state.clock_push(ch);
    }
    state.commit_clock();

    assert_eq!(state.store.playhead_ticks, 24);
    assert!(!state.clock_focused(), "committing releases the field");
    assert!(state.last_error.is_none());
}

#[test]
fn a_bare_frame_count_seeks() {
    let (mut state, _) = with_clip();
    state.focus_clock(true);
    while state.clock_draft.as_deref().is_some_and(|d| !d.is_empty()) {
        state.clock_pop();
    }
    for ch in "36".chars() {
        state.clock_push(ch);
    }
    state.commit_clock();
    assert_eq!(state.store.playhead_ticks, 36);
}

#[test]
fn nonsense_restores_the_previous_value() {
    let (mut state, _) = with_clip();
    state.set_playhead(48);
    state.focus_clock(true);
    while state.clock_draft.as_deref().is_some_and(|d| !d.is_empty()) {
        state.clock_pop();
    }
    for ch in "99:99".chars() {
        state.clock_push(ch);
    }
    state.commit_clock();

    assert_eq!(state.store.playhead_ticks, 48, "a refusal must not seek");
    assert_eq!(
        state.clock_text(),
        "00:00:02:00",
        "the readout goes back to the playhead"
    );
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("not a timecode")),
        "a refusal must say what it wanted: {:?}",
        state.last_error
    );
}

#[test]
fn escape_leaves_without_seeking() {
    let (mut state, _) = with_clip();
    state.set_playhead(24);
    state.focus_clock(true);
    for ch in "12".chars() {
        state.clock_push(ch);
    }
    state.focus_clock(false);

    assert_eq!(state.store.playhead_ticks, 24);
    assert!(!state.clock_focused());
    assert_eq!(state.clock_text(), "00:00:01:00");
}

#[test]
fn the_draft_takes_digits_and_separators_only() {
    let (mut state, _) = with_clip();
    state.focus_clock(true);
    while state.clock_draft.as_deref().is_some_and(|d| !d.is_empty()) {
        state.clock_pop();
    }
    for ch in "1a2;3 4:5".chars() {
        state.clock_push(ch);
    }
    assert_eq!(state.clock_draft.as_deref(), Some("12;34:5"));
}

#[test]
fn the_clock_takes_keys_through_the_field_table() {
    let (mut state, _) = with_clip();
    assert!(
        fields::focused(&state).is_none(),
        "an idle clock holds no keys"
    );

    state.focus_clock(true);
    let field = fields::focused(&state).expect("the clock holds focus");
    assert_eq!(field.id, "aria.field.clock");

    let marker = {
        state.set_playhead(12);
        state.add_marker_at_playhead().expect("marker");
        state.store.sequence.markers[0].id
    };
    state.begin_rename(crate::types::RenameTarget::Marker(marker));
    let field = fields::focused(&state).expect("rename outranks the clock");
    assert_eq!(field.id, "aria.field.rename");
}

#[test]
fn a_seek_past_the_end_is_clamped_not_refused() {
    let (mut state, _) = with_clip();
    let end = state.store.sequence.duration_ticks;
    state.focus_clock(true);
    while state.clock_draft.as_deref().is_some_and(|d| !d.is_empty()) {
        state.clock_pop();
    }
    for ch in "10:00:00:00".chars() {
        state.clock_push(ch);
    }
    state.commit_clock();

    assert_eq!(state.store.playhead_ticks, end);
    assert!(state.last_error.is_none(), "a clamp is not a refusal");
}
