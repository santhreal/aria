//! The unsaved-changes prompt.
//!
//! The class these close: an action that replaces the session used to run
//! immediately, so unsaved work went with one mis-click. Every such action now
//! goes through `request`, and the tests assert the two halves of that — a clean
//! project is never interrupted, a dirty one always is — plus what each of the
//! three answers does. The intent variants are covered rather than only the one
//! that prompted the work, because the defect class is "this path forgot to
//! ask".
//!
//! Not covered: `PickProject` and the drop-on-Home path past the prompt, which
//! open a native file dialog and a plugin classifier respectively.

use aria_plugin::PluginContext;

use crate::app::unsaved::PendingIntent;
use crate::app::{AppState, DialogKind};
use crate::scratch_pool::scratch;
use crate::types::UiRoute;

/// A project with one unsaved edit in it.
fn dirty() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("unsaved")).expect("state");
    state.new_project().expect("a project to edit");
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("an edit to lose");
    assert!(state.store.is_dirty, "the fixture is not dirty");
    state
}

#[test]
fn a_clean_project_leaves_without_being_asked() {
    let mut state = AppState::new(PluginContext::new(), scratch("unsaved")).expect("state");
    state.new_project().expect("project");
    assert!(!state.store.is_dirty);
    assert!(state.request(PendingIntent::GoHome), "it should have run");
    assert_eq!(state.route, UiRoute::Home);
    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.pending_intent().is_none());
}

#[test]
fn a_dirty_project_parks_the_intent_and_asks() {
    let mut state = dirty();
    assert!(
        !state.request(PendingIntent::GoHome),
        "it should have asked"
    );
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);
    assert_eq!(state.pending_intent(), Some(&PendingIntent::GoHome));
    assert_eq!(state.route, UiRoute::Editor, "nothing happened yet");
}

#[test]
fn every_guarded_entry_point_asks() {
    // The recurring defect is one path that forgot the prompt, so each wrapper is
    // exercised rather than a representative one.
    for intent in [
        PendingIntent::GoHome,
        PendingIntent::NewProject,
        PendingIntent::OpenProject(std::path::PathBuf::from("/nowhere.aria")),
        PendingIntent::OpenSample,
        PendingIntent::DropOnHome(vec![]),
        PendingIntent::Quit,
    ] {
        let mut state = dirty();
        assert!(!state.request(intent.clone()));
        assert_eq!(
            state.dialog,
            DialogKind::UnsavedChanges,
            "{intent:?} did not ask"
        );
        assert_eq!(state.pending_intent(), Some(&intent));
    }
}

#[test]
fn closing_the_project_is_guarded() {
    let mut state = dirty();
    state.close_project();
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);
    assert_eq!(state.route, UiRoute::Editor);
}

/// Ctrl+Q, File > Quit and the window's close control share `request_quit`.
/// A dirty session must not quit until the prompt is answered, Stay must keep
/// it open, and a clean one must quit with no prompt at all.
#[test]
fn quitting_waits_for_the_answer_when_work_is_unsaved() {
    let mut clean = AppState::new(PluginContext::new(), scratch("unsaved")).expect("state");
    clean.new_project().expect("project");
    clean.request_quit();
    assert!(clean.quit_requested(), "a clean session quits at once");
    assert_eq!(clean.dialog, DialogKind::None);

    let mut state = dirty();
    state.request_quit();
    assert!(
        !state.quit_requested(),
        "quit before the prompt was answered"
    );
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);
    assert_eq!(state.pending_intent(), Some(&PendingIntent::Quit));
    state.cancel_pending();
    assert!(!state.quit_requested(), "Stay quit anyway");

    state.request_quit();
    state.discard_and_continue();
    assert!(state.quit_requested(), "Discard did not quit");
}

#[test]
fn the_prompt_opens_on_home_too() {
    // Home keeps the project loaded, so a dirty session is still at risk there —
    // and `open_dialog` refuses every dialog but Settings on Home, which is why
    // the prompt sets the field itself.
    let mut state = dirty();
    state.request_go_home();
    state.discard_and_continue();
    assert_eq!(state.route, UiRoute::Home);
    assert!(state.store.is_dirty, "going Home saved nothing");

    state.request_new_project();
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);
}

#[test]
fn staying_leaves_the_project_exactly_as_it_was() {
    let mut state = dirty();
    let lanes = state.store.sequence.tracks.len();
    state.request_new_project();
    state.cancel_pending();
    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.pending_intent().is_none());
    assert_eq!(state.route, UiRoute::Editor);
    assert!(state.store.is_dirty);
    assert_eq!(state.store.sequence.tracks.len(), lanes);
}

#[test]
fn discarding_runs_the_intent_and_loses_the_edits() {
    let mut state = dirty();
    let lanes = state.store.sequence.tracks.len();
    state.request_new_project();
    state.discard_and_continue();
    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.pending_intent().is_none());
    assert!(!state.store.is_dirty, "a new session is not dirty");
    assert!(
        state.store.sequence.tracks.len() < lanes,
        "the added lane survived the discard"
    );
}

#[test]
fn saving_writes_the_file_then_runs_the_intent() {
    let mut state = dirty();
    let path = state.data_dir.join("guarded.aria");
    state.project_path = Some(path.clone());
    state.request_new_project();
    state.save_and_continue();
    assert!(path.is_file(), "the save did not happen");
    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.pending_intent().is_none());
    assert!(!state.store.is_dirty);
    assert_eq!(state.project_path, None, "the new project ran");
}

#[test]
fn a_failed_save_keeps_the_prompt_and_the_work() {
    let mut state = dirty();
    // A directory that does not exist and cannot be created: the save fails, so
    // the answer must not be treated as given.
    state.project_path = Some(std::path::PathBuf::from("/proc/aria/nope.aria"));
    state.request_new_project();
    state.save_and_continue();
    assert_eq!(
        state.dialog,
        DialogKind::UnsavedChanges,
        "the prompt closed"
    );
    assert_eq!(state.pending_intent(), Some(&PendingIntent::NewProject));
    assert!(state.store.is_dirty, "the edits went");
    assert!(state.last_error.is_some(), "the failure was silent");
}

#[test]
fn leaving_the_prompt_any_other_way_drops_the_intent() {
    // Escape closes a dialog through `open_dialog(None)`. A parked intent that
    // survived that would fire later against an answer nobody gave.
    let mut state = dirty();
    state.request_new_project();
    state.open_dialog(DialogKind::None);
    assert!(state.pending_intent().is_none());
    assert_eq!(state.route, UiRoute::Editor);
    assert!(state.store.is_dirty);
}

#[test]
fn the_prompt_names_the_project_it_is_about_to_close() {
    let mut state = dirty();
    let name = state.store.project_name.clone();
    assert!(!name.is_empty(), "the fixture has no project name");
    assert_eq!(state.unsaved_subject(), name);

    state.store_mut().project_name = "   ".into();
    assert_eq!(
        state.unsaved_subject(),
        "This project",
        "an unnamed project still gets a sentence"
    );
}
