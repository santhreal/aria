//! Contracts for a declined command.
//!
//! The class closed here is "a command that finds nothing to act on does
//! nothing and says nothing". What is asserted:
//!
//! - Every reason has its own words, walked from the registry rather than
//!   retyped, so a reason added without wording is red here.
//! - The shared reader declines once for the whole family: a command that acts
//!   on the selection and finds none says so through it, and says it in the
//!   status line rather than as an error, because nothing failed.
//!
//! The transition family's own three refusals are asserted next to the family,
//! in `transition_tests.rs`.

use std::collections::BTreeSet;

use super::super::premiere_tests::with_clip;
use super::Decline;

#[test]
fn every_reason_has_its_own_words() {
    let mut seen = BTreeSet::new();
    for why in Decline::ALL {
        let reason = why.reason();
        assert!(!reason.trim().is_empty(), "{why:?} has no wording");
        assert!(
            seen.insert(reason),
            "{why:?} repeats another reason: {reason}"
        );
    }
}

#[test]
fn a_command_with_no_selection_says_so_and_does_not_fail() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.status_message = None;

    state
        .set_selected_transition("crossfade")
        .expect("declining is not an error");

    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
    assert_eq!(state.last_error, None);
}

/// The reader is the one place the selection is read for a command, so it is
/// also the one place the refusal is written: a caller cannot forget it.
#[test]
fn the_shared_reader_returns_the_selection_and_says_nothing_when_there_is_one() {
    let (mut state, clip) = with_clip();
    state.status_message = None;

    assert_eq!(state.selected_clip_or_decline(), Some(clip));
    assert_eq!(state.status_message, None, "a selection is not a refusal");

    state.clear_selection();
    assert_eq!(state.selected_clip_or_decline(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
}
