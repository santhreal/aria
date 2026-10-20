//! Autosave and crash recovery.
//!
//! The class these close: work between two explicit saves used to exist only in
//! memory, so a crash took it. Time is passed in rather than read, so the
//! interval is asserted exactly instead of slept through, and the "crash" is a
//! second `AppState` over the same data directory — which is what a relaunch is.
//!
//! Not covered: the half-second ticker that calls `autosave_tick`, which needs a
//! window.

use std::time::{Duration, Instant};

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::recovery::{AUTOSAVE_INTERVAL, recovery_path};
use crate::scratch_pool::scratch;

fn dirty(dir: std::path::PathBuf) -> AppState {
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.new_project().expect("a project to edit");
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("an edit to lose");
    assert!(state.store.is_dirty);
    state
}

#[test]
fn a_fresh_data_directory_offers_nothing() {
    let state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    assert!(state.pending_recovery().is_none());
}

#[test]
fn a_dirty_session_writes_a_copy_and_then_waits_out_the_interval() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    assert!(state.autosave_tick(now), "the first dirty tick writes");
    assert!(recovery_path(&state.data_dir).is_file());

    assert!(
        !state.autosave_tick(now + AUTOSAVE_INTERVAL - Duration::from_millis(1)),
        "it wrote again before the interval was up"
    );
    assert!(
        state.autosave_tick(now + AUTOSAVE_INTERVAL),
        "the interval elapsed and nothing was written"
    );
}

#[test]
fn a_clean_session_writes_nothing() {
    let mut state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    state.new_project().expect("project");
    assert!(!state.autosave_tick(Instant::now()));
    assert!(!recovery_path(&state.data_dir).exists());
}

#[test]
fn an_autosave_does_not_make_the_session_look_saved() {
    // The copy goes straight to disk rather than through Save: a session that
    // read clean after an autosave would allow a quit on work that only the
    // recovery file holds.
    let mut state = dirty(scratch("recovery"));
    let revision = state.revision;
    assert!(state.autosave_tick(Instant::now()));
    assert!(state.store.is_dirty, "the autosave cleared the dirty flag");
    assert_eq!(state.revision, revision, "the autosave was an edit");
    assert!(state.project_path.is_none());
}

#[test]
fn saving_the_project_clears_the_offer() {
    let mut state = dirty(scratch("recovery"));
    state.write_recovery().expect("a copy to clear");
    state.project_path = Some(state.data_dir.join("kept.aria"));
    state.save_project().expect("save");
    assert!(state.pending_recovery().is_none());
    assert!(!recovery_path(&state.data_dir).exists());
}

#[test]
fn starting_a_new_project_clears_the_offer() {
    let mut state = dirty(scratch("recovery"));
    state.write_recovery().expect("a copy to clear");
    state.new_project().expect("new");
    assert!(state.pending_recovery().is_none());
}

#[test]
fn a_relaunch_offers_the_copy_back_named_after_the_project() {
    let dir = scratch("recovery");
    let path = dir.join("afternoon.aria");
    let mut state = dirty(dir.clone());
    state.project_path = Some(path.clone());
    state.save_project().expect("a saved baseline");
    state
        .add_track(aria_models::TrackKind::Audio, "A2")
        .expect("work after the save");
    let lanes = state.store.sequence.tracks.len();
    state.write_recovery().expect("the crash-time copy");
    drop(state);

    // A relaunch: a new session over the same data directory.
    let mut relaunched = AppState::new(PluginContext::new(), dir).expect("state");
    let offer = relaunched
        .pending_recovery()
        .expect("no offer after a crash");
    assert_eq!(offer.project.as_deref(), Some(path.as_path()));
    assert_eq!(offer.name, "afternoon");

    relaunched.restore_recovery().expect("restore");
    assert_eq!(
        relaunched.store.sequence.tracks.len(),
        lanes,
        "the recovered session is missing the work"
    );
    assert_eq!(
        relaunched.project_path,
        Some(path),
        "restoring aimed at the recovery copy instead of the project"
    );
    assert!(
        relaunched.store.is_dirty,
        "the recovered session reads saved, so a quit would lose it"
    );
    assert_eq!(relaunched.route, crate::types::UiRoute::Editor);
    assert!(
        relaunched
            .status_message
            .as_deref()
            .is_some_and(|text| text.contains("afternoon")),
        "the restore said nothing: {:?}",
        relaunched.status_message
    );
    assert!(
        relaunched.pending_recovery().is_none(),
        "the offer survived being taken"
    );
}

#[test]
fn an_untitled_crash_is_offered_without_a_project_path() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    assert!(state.project_path.is_none());
    state.write_recovery().expect("copy");
    drop(state);

    let mut relaunched = AppState::new(PluginContext::new(), dir).expect("state");
    let offer = relaunched.pending_recovery().expect("offer");
    assert_eq!(offer.project, None);
    assert_eq!(offer.name, "Untitled project");
    relaunched.restore_recovery().expect("restore");
    assert_eq!(
        relaunched.project_path, None,
        "an untitled recovery must not claim the recovery file as its project"
    );
    assert!(relaunched.store.is_dirty);
}

#[test]
fn discarding_the_offer_removes_it() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    state.write_recovery().expect("copy");
    state.discard_recovery();
    assert!(state.pending_recovery().is_none());
    assert!(!recovery_path(&dir).exists());
    assert!(state.status_message.is_some());
}

#[test]
fn restoring_nothing_is_not_an_error() {
    let mut state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    state
        .restore_recovery()
        .expect("a missing copy is no error");
}
