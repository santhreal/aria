//! File > Import Recent.
//!
//! # Why
//!
//! A recent list that drops the newest file, repeats one, grows without end,
//! or forgets everything on restart offers the wrong files; a row for a file
//! that was deleted must not run an import. These cases drive File > Import's
//! own path, reopen the data directory, and run the menu rows.
//!
//! What they do not catch: the native file picker itself, which
//! `menu_reach_tests` answers through its pick table.

use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;
use gpui::AppContext;

use super::{LIMIT, file, load, promoted};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::chrome::menu_file::IMPORT_RECENT_PREFIX;
use crate::chrome::menu_items::items_of;
use crate::types::AppMenu;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

#[test]
fn new_imports_go_first_once_each_and_the_list_stops_at_its_limit() {
    let list = paths(&["/b", "/c", "/d"]);
    assert_eq!(
        promoted(&list, &paths(&["/a", "/c"])),
        paths(&["/a", "/c", "/b", "/d"])
    );
    assert_eq!(promoted(&list, &[]), list, "nothing added keeps the order");
    let many: Vec<PathBuf> = (0..LIMIT + 5)
        .map(|n| PathBuf::from(format!("/{n}")))
        .collect();
    let cut = promoted(&list, &many);
    assert_eq!(cut.len(), LIMIT);
    assert_eq!(cut, many[..LIMIT]);
}

#[test]
fn an_unreadable_list_loads_empty() {
    let (state, _) = with_clip();
    std::fs::write(file(&state.data_dir), b"{not json").expect("write");
    assert!(load(&state.data_dir).is_empty());
}

/// The Import Recent rows of the File menu: (id, label, disabled reason).
fn recent_rows(state: &AppState) -> Vec<(&'static str, &'static str, Option<&'static str>)> {
    items_of(AppMenu::File, state)
        .into_iter()
        .filter(|item| item.under == Some("Import Recent"))
        .map(|item| (item.id, item.label, item.blocked_by(state)))
        .collect()
}

#[test]
fn an_import_is_listed_survives_a_restart_and_clears() {
    let (mut state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    assert_eq!(
        recent_rows(&state),
        [(
            "file-import-recent-clear",
            "Clear Recent Imports",
            Some("No file has been imported yet")
        )]
    );

    // A copy of its own: the pool keeps one entry per path, and the fixture
    // project already holds the shared fixture.
    let picked = data_dir.join("recent.png");
    std::fs::copy(fixture(), &picked).expect("copy");
    let pooled = state.store.media.len();
    state.import_paths(vec![picked.clone()]);
    assert_eq!(
        state.store.media.len(),
        pooled + 1,
        "the import reached the pool"
    );
    let placed = state.store.sequence.clip_count();
    let listed = picked.display().to_string();
    let rows = recent_rows(&state);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].0, format!("{IMPORT_RECENT_PREFIX}{listed}"));
    assert_eq!(rows[0].1, listed);
    assert_eq!(rows[0].2, None);
    assert_eq!(rows[1].2, None, "a list with a file can be cleared");

    // Running the row imports the file again.
    let run = items_of(AppMenu::File, &state)
        .into_iter()
        .find(|item| item.id == rows[0].0)
        .expect("the row");
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    let placed_after = cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            (run.run)(run.id, state, cx);
            state.store.sequence.clip_count()
        })
    });
    assert_eq!(
        placed_after,
        placed + 1,
        "the row imported and placed the file again"
    );

    let reopened = AppState::new(PluginContext::new(), data_dir.clone()).expect("reopen");
    assert_eq!(&*reopened.import_recents, [picked]);

    let rows_after_clear = cx.update(|app| {
        entity.update(app, |state: &mut AppState, _| {
            state.clear_import_recents();
            assert!(state.import_recents.is_empty());
            recent_rows(state).len()
        })
    });
    assert!(!file(&data_dir).exists(), "the saved list is deleted");
    assert_eq!(rows_after_clear, 1, "only the disabled Clear row is left");
}

#[test]
fn a_deleted_file_is_listed_disabled_and_imports_nothing() {
    let (mut state, _) = with_clip();
    let gone = state.data_dir.join("gone.png");
    std::fs::copy(fixture(), &gone).expect("copy");
    state.import_paths(vec![gone.clone()]);
    std::fs::remove_file(&gone).expect("delete");
    let rows = recent_rows(&state);
    assert_eq!(rows[0].2, Some("The file is missing"));

    let pooled = state.store.media.len();
    state.import_recent(Path::new(&gone));
    assert_eq!(state.store.media.len(), pooled);
    assert_eq!(
        state.status_message.as_deref(),
        Some(format!("{} is missing", gone.display()).as_str())
    );
}
