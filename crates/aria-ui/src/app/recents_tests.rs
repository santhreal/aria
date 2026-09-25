//! The recents list is state the editor holds, not a file Home reads per draw.
//!
//! Home is animated, so a list read from disk per frame cost a file read plus a
//! stat of every path in it, sixty times a second, to answer a question that
//! only changes when a project is opened, saved, forgotten, or deleted. These
//! tests pin both halves of that: a draw sees the held list, and the four verbs
//! that can change it leave the held list in step.

use super::AppState;
use aria_plugin::PluginContext;
use std::path::PathBuf;

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("recents")
}

fn project(dir: &PathBuf, name: &str) -> PathBuf {
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

    // Written behind the state's back, the way another window would. Nothing
    // reads the file except the loader, so the held list cannot notice: this is
    // exactly what makes a draw free.
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
    state.save_project().unwrap();
    let saved = state
        .project_path
        .as_deref()
        .expect("a saved project has a path");
    assert_eq!(
        state.recents()[0].path.as_ref(),
        &saved,
        "a save is remembered without a trip through Home"
    );
}
