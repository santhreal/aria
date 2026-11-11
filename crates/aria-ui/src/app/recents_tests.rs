//! The recents list is state the editor holds, not a file Home reads per draw.
//!
//! Home is animated, so a list read from disk per frame cost a file read plus a
//! stat of every path in it, sixty times a second, to answer a question that
//! only changes when a project is opened, saved, forgotten, or deleted. These
//! tests pin both halves of that: a draw sees the held list, and the four verbs
//! that can change it leave the held list in step.

use super::AppState;
use super::premiere_tests::save_fixture;
use aria_plugin::PluginContext;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("recents")
}

fn project(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "{}").expect("write a project file");
    path
}

#[test]
fn home_draws_the_list_the_state_holds() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    assert!(
        state.recents().is_empty(),
        "a fresh data directory has nothing to come back to"
    );

    let held = project(&dir, "held.aria");
    state.remember_recent(&held);
    assert_eq!(state.recents().len(), 1);
    assert_eq!(state.recents()[0].path.as_ref(), &held);
    assert!(!state.recents()[0].missing, "the file is right there");

    // Written behind the state's back, the way another window would, once the
    // state's own write has landed. Nothing
    // reads the file except the loader, so the held list cannot notice: this is
    // exactly what makes a draw free.
    state.flush_recents();
    crate::app::recents::forget(&dir, &held);
    assert_eq!(
        state.recents().len(),
        1,
        "a draw must not re-read the file behind the list"
    );

    // Arriving at Home is one of the two moments that can notice.
    state.go_home();
    assert!(state.recents().is_empty());
}

#[test]
fn the_verbs_that_change_the_list_keep_the_held_copy_in_step() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    let kept = project(&dir, "kept.aria");
    let doomed = project(&dir, "doomed.aria");

    state.remember_recent(&kept);
    state.remember_recent(&doomed);
    assert_eq!(state.recents().len(), 2);
    assert_eq!(
        state.recents()[0].path.as_ref(),
        &doomed,
        "the newest sits at the top"
    );

    state
        .delete_recent(&doomed)
        .expect("delete the project file");
    assert!(!doomed.exists(), "deleting takes the file with it");
    assert_eq!(state.recents().len(), 1);
    assert_eq!(state.recents()[0].path.as_ref(), &kept);

    state.forget_recent(&kept);
    assert!(state.recents().is_empty());
    assert!(kept.exists(), "forgetting leaves the file alone");
}

#[test]
fn saving_a_project_puts_it_where_home_will_draw_it() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let saved = save_fixture(&mut state);
    assert_eq!(
        state.recents()[0].path.as_ref(),
        &saved,
        "a save is remembered without a trip through Home"
    );
}

/// How long [`held_write`] holds a write before it gives up and writes.
const HOLD: Duration = Duration::from_secs(5);
/// Lets every write [`held_write`] holds go on.
static OPEN: AtomicBool = AtomicBool::new(false);
/// Set when [`held_write`] gave up: the caller waited on the write.
static GAVE_UP: AtomicBool = AtomicBool::new(false);

/// Writes the recents file once [`OPEN`] is set, as a disk whose sync takes
/// as long as the test lets it.
fn held_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let start = Instant::now();
    while !OPEN.load(Ordering::SeqCst) {
        if start.elapsed() > HOLD {
            GAVE_UP.store(true, Ordering::SeqCst);
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    aria_settings::replace_file(path, bytes)
}

fn on_disk(dir: &Path) -> Vec<PathBuf> {
    let (views, _) = crate::app::recents::visible_recents(dir);
    views
        .iter()
        .map(|view| PathBuf::clone(&view.path))
        .collect()
}

fn held(state: &AppState) -> Vec<PathBuf> {
    state
        .recents()
        .iter()
        .map(|view| PathBuf::clone(&view.path))
        .collect()
}

/// WHY: closes the class of a recents write on the window's thread. A save
/// synced `recents.json` and its folder after the project's own two syncs,
/// which held Ctrl+S for seconds on a loaded disk. Here the disk holds every
/// write until the test lets it go: the save and a second remember return
/// with both projects on the held list, Home entered meanwhile keeps that
/// list, and the file ends with both. Not caught: a slow read of the file,
/// which a verb still does on the window's thread when no write is in flight.
#[test]
fn a_save_returns_while_its_recents_write_is_held() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    state.recents_writer.write = held_write;
    state.new_project().unwrap();
    let saved = save_fixture(&mut state);
    let other = project(&dir, "other.aria");
    state.remember_recent(&other);

    assert!(
        !GAVE_UP.load(Ordering::SeqCst),
        "the save waited on the recents write"
    );
    assert!(
        !dir.join(crate::app::recents::RECENTS_FILE).exists(),
        "a held write reached the file"
    );
    assert_eq!(held(&state), [other.clone(), saved.clone()]);
    state.go_home();
    assert_eq!(
        held(&state),
        [other.clone(), saved.clone()],
        "Home read the file under a write still in flight"
    );

    OPEN.store(true, Ordering::SeqCst);
    state.flush_recents();
    assert_eq!(on_disk(&dir), [other, saved], "the newest list is on disk");
}

/// Sleeps before it writes, so a write is in flight when the state drops.
fn slow_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(300));
    aria_settings::replace_file(path, bytes)
}

/// WHY: a session that ends with a write in flight still lands it, so the
/// last project saved is on the list the next launch reads. Not caught: a
/// process killed before the write lands.
#[test]
fn a_session_dropped_mid_write_lands_it() {
    let dir = scratch();
    let kept = project(&dir, "kept.aria");
    {
        let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
        state.recents_writer.write = slow_write;
        state.remember_recent(&kept);
    }
    assert_eq!(on_disk(&dir), [kept]);
}
