//! Contracts for the status strip: a status expires, an error does not.
//!
//! The class closed here is "a message posted once stays on the strip for the
//! rest of the session", which made the strip a log of the oldest thing that
//! happened. Ageing is driven by a passed-in `now`, so these assertions need no
//! sleeping and no wall clock.
//!
//! Not covered: the drawn strip and the 500ms ticker that calls `age_status`,
//! which need a render surface.

use std::time::{Duration, Instant};

use super::AppState;
use super::premiere_tests::with_clip;
use super::status::STATUS_TTL;

fn state() -> AppState {
    with_clip().0
}

#[test]
fn a_status_clears_once_its_time_is_spent() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    assert!(!state.age_status(now), "the first look starts the clock");
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(
        !state.age_status(now + STATUS_TTL - Duration::from_millis(1)),
        "not yet"
    );
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(state.age_status(now + STATUS_TTL), "spent");
    assert_eq!(state.status_message, None);
}

#[test]
fn a_second_status_gets_its_own_full_life() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    state.age_status(now);
    // A new message replaces the old one halfway through its life; it must not
    // inherit the remainder.
    let half = now + STATUS_TTL / 2;
    state.status_message = Some("Pasted".into());
    assert!(!state.age_status(half));
    assert!(
        !state.age_status(half + STATUS_TTL - Duration::from_millis(1)),
        "the clock restarted with the new text"
    );
    assert_eq!(state.status_message.as_deref(), Some("Pasted"));
    assert!(state.age_status(half + STATUS_TTL));
    assert_eq!(state.status_message, None);
}

#[test]
fn the_same_text_posted_again_after_expiry_is_shown_again() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    state.age_status(now);
    assert!(state.age_status(now + STATUS_TTL));
    let later = now + STATUS_TTL * 2;
    state.status_message = Some("Copied".into());
    assert!(!state.age_status(later), "a re-post is a new message");
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(state.age_status(later + STATUS_TTL));
    assert_eq!(state.status_message, None);
}

#[test]
fn an_empty_strip_reports_no_change_and_holds_no_clock() {
    let mut state = state();
    let now = Instant::now();
    // The fixture's own import posts a receipt; this test is about the empty
    // strip, so start from one.
    state.status_message = None;
    assert!(!state.age_status(now));
    assert!(state.status_seen.is_none());
    // A status cleared by its own writer must not leave a clock behind that
    // could expire the next message early.
    state.status_message = Some("Analyzing…".into());
    state.age_status(now);
    state.status_message = None;
    assert!(!state.age_status(now + STATUS_TTL));
    assert!(state.status_seen.is_none());
}

#[test]
fn an_error_is_never_aged_out() {
    let mut state = state();
    let now = Instant::now();
    state.last_error = Some("could not open the file".into());
    state.status_message = Some("Copied".into());
    state.age_status(now);
    assert!(state.age_status(now + STATUS_TTL));
    assert_eq!(state.status_message, None, "the receipt went");
    assert_eq!(
        state.last_error.as_deref(),
        Some("could not open the file"),
        "the failure stays until something works"
    );
}

#[test]
fn a_successful_edit_clears_the_error() {
    let (mut state, _clip) = with_clip();
    state.last_error = Some("that lane is locked".into());
    state.add_marker_at_playhead().expect("a marker lands");
    assert_eq!(
        state.last_error, None,
        "an error superseded by a successful action is stale"
    );
}

#[test]
fn undo_and_redo_clear_the_error_too() {
    let (mut state, _clip) = with_clip();
    state.add_marker_at_playhead().expect("a marker lands");
    state.last_error = Some("stale".into());
    assert!(state.undo());
    assert_eq!(state.last_error, None, "undo is a successful action");
    state.last_error = Some("stale again".into());
    assert!(state.redo());
    assert_eq!(state.last_error, None, "so is redo");
}

#[test]
fn a_refused_command_leaves_the_error_up() {
    let (mut state, _clip) = with_clip();
    let track = state.store.sequence.tracks[0].id;
    // An unknown clip id cannot be split, so nothing succeeded and the previous
    // failure is still the newest true thing on the strip.
    state
        .set_track_locked(aria_models::TrackId::from_raw(track), true)
        .expect("lock");
    state.last_error = Some("first failure".into());
    state.select_all_unlocked();
    assert!(
        state.split_at_playhead().is_ok(),
        "a locked lane has nothing selected to split"
    );
    assert_eq!(
        state.last_error.as_deref(),
        Some("first failure"),
        "an attempt that reached no command does not clear the strip"
    );
}
