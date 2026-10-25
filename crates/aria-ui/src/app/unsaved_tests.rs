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
//! Not covered: `PickProject` and the drop-on-Home path past a Save, which open
//! a native file dialog and a plugin classifier respectively. The Discard test
//! drives both, with the dialog answered and a project file dropped.

use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;

use crate::app::picker::Answer;
use crate::app::recovery::recovery_path;
use crate::app::unsaved::PendingIntent;
use crate::app::{AppState, DialogKind};
use crate::scratch_pool::scratch;
use crate::surfaces::context_menu::{MenuItem, RECENT_ITEMS};
use crate::surfaces::home_pick::{self, Pick};
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

/// A recent project that moved is relinked through a file dialog, and the file
/// the dialog answers with replaces the session. Over unsaved work that waits
/// for the prompt, from Home's relink and from the recent row's Open alike.
#[test]
fn relinking_a_moved_recent_asks_before_it_replaces_unsaved_work() {
    let gone = std::path::PathBuf::from("/nowhere/moved-away.aria");
    let relinks: [(&str, fn(&mut AppState, &std::path::PathBuf)); 2] = [
        ("Home's relink", |state, gone| {
            home_pick::apply(state, Pick::RelinkRecent(gone.clone()))
                .expect("a relink is never refused");
        }),
        ("the recent row's Open", |state, gone| {
            let open = RECENT_ITEMS
                .iter()
                .filter_map(MenuItem::row)
                .find(|row| row.id == "ctx-recent-open")
                .expect("the recent menu offers Open");
            (open.run)(state, gone).expect("Open is never refused");
        }),
    ];
    for (entry, relink) in relinks {
        let mut state = dirty();
        let chosen = scratch("unsaved-relink").join("found.aria");
        state.supply_pick(Answer::file(chosen.clone()));
        relink(&mut state, &gone);
        assert_eq!(state.picks_asked(), 1, "{entry} asked for no project");
        assert_eq!(
            state.dialog,
            DialogKind::UnsavedChanges,
            "{entry} replaced unsaved work without asking"
        );
        assert_eq!(
            state.pending_intent(),
            Some(&PendingIntent::OpenProject(chosen)),
            "{entry} parked something other than the chosen project"
        );
        assert!(state.store.is_dirty, "{entry} dropped the unsaved edit");
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

/// `open_dialog` opens only Settings, Install Plugin and About on Home, so
/// `request` sets the prompt itself. `go_home` does not ask, so the session
/// arrives on Home dirty.
#[test]
fn the_prompt_opens_on_home_too() {
    let mut state = dirty();
    state.go_home();
    assert_eq!(state.route, UiRoute::Home);
    state.request_new_project();
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);
    assert_eq!(state.pending_intent(), Some(&PendingIntent::NewProject));
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

/// The lane every Discard must lose.
const LOST: &str = "Discarded lane";

/// A saved, empty project outside the data directory of the session under test.
fn saved_elsewhere() -> PathBuf {
    let mut maker = AppState::new(PluginContext::new(), scratch("unsaved-other")).expect("state");
    maker.new_project().expect("a project to save");
    let path = maker.data_dir.join("elsewhere.aria");
    maker.project_path = Some(path.clone());
    maker.save_project().expect("save");
    path
}

fn has_lane(state: &AppState, name: &str) -> bool {
    state
        .engine()
        .lock()
        .project()
        .timeline()
        .tracks_ordered()
        .any(|track| track.name == name)
}

/// Every way of leaving the project, walked from [`PendingIntent::GoHome`].
///
/// Each arm names the intent after it, so a new variant does not compile until
/// it takes a place in the walk, and [`discard_leaves_no_unsaved_work_behind`]
/// then holds it to the same rule. The intents that open a project open
/// `other`.
fn after(intent: &PendingIntent, other: &Path) -> Option<PendingIntent> {
    match intent {
        PendingIntent::GoHome => Some(PendingIntent::NewProject),
        PendingIntent::NewProject => Some(PendingIntent::OpenProject(other.to_path_buf())),
        PendingIntent::OpenProject(_) => Some(PendingIntent::PickProject),
        PendingIntent::PickProject => Some(PendingIntent::OpenSample),
        PendingIntent::OpenSample => Some(PendingIntent::DropOnHome(vec![other.to_path_buf()])),
        PendingIntent::DropOnHome(_) => Some(PendingIntent::Quit),
        PendingIntent::Quit => None,
    }
}

/// Discard loses the edits whichever way the project is left.
///
/// The class this closes: Home and Quit replace nothing, and a Discard that
/// only ran the intent left the edits behind. Home then held a dirty session
/// that nothing on it leads back to, and Quit on Home asked about it a second
/// time, so the window stayed open; a Discard on Quit left the recovery copy on
/// disk, and the next launch offered the discarded work as work a crash
/// interrupted. Each intent starts from a dirty session with its recovery copy
/// written. After Discard the edit is gone from the session, or the session is
/// ending, and no copy is on disk for the next launch.
///
/// Not covered: a replacement the plugin host refuses, which keeps the prompt
/// up, because no fixture makes `replace_session` fail; and the chord that
/// reaches `request_quit` on Home, which the headed tour presses.
#[test]
fn discard_leaves_no_unsaved_work_behind() {
    let other = saved_elsewhere();
    let mut next = Some(PendingIntent::GoHome);
    while let Some(intent) = next {
        next = after(&intent, &other);
        let dir = scratch("unsaved-discard");
        let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
        state.new_project().expect("a project to edit");
        state
            .add_track(aria_models::TrackKind::Video, LOST)
            .expect("an edit to lose");
        state.write_recovery().expect("a recovery copy of the edit");
        if intent == PendingIntent::PickProject {
            state.supply_pick(Answer::file(other.clone()));
        }
        assert!(!state.request(intent.clone()), "{intent:?} did not ask");
        state.discard_and_continue();
        assert_eq!(state.dialog, DialogKind::None, "{intent:?} kept the prompt");
        assert!(state.pending_intent().is_none(), "{intent:?} stayed parked");
        assert_eq!(state.last_error, None, "{intent:?} failed");
        assert!(
            !recovery_path(&dir).exists(),
            "{intent:?} left the recovery copy on disk"
        );
        if intent == PendingIntent::Quit {
            assert!(state.quit_requested(), "Discard did not quit");
            drop(state);
            let relaunched = AppState::new(PluginContext::new(), dir).expect("state");
            assert_eq!(
                relaunched.pending_recovery(),
                None,
                "the next launch offered the discarded work"
            );
            continue;
        }
        assert!(
            !has_lane(&state, LOST),
            "{intent:?} kept the discarded lane"
        );
        if intent == PendingIntent::GoHome {
            assert_eq!(state.route, UiRoute::Home);
            assert!(!state.store.is_dirty, "Home holds the discarded work");
            assert_eq!(state.project_path, None, "Home kept the closed project");
            state.request_quit();
            assert!(
                state.quit_requested(),
                "Quit on Home asked about discarded work"
            );
            assert_eq!(state.dialog, DialogKind::None);
        } else {
            assert_eq!(state.route, UiRoute::Editor, "{intent:?}");
        }
    }
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
    assert_eq!(
        state.unsaved_line(),
        format!("Changes to “{name}” are not saved.")
    );

    state.store_mut().project_name = "   ".into();
    assert_eq!(
        state.unsaved_line(),
        "Changes to this project are not saved.",
        "an unnamed project still gets a sentence"
    );
}
