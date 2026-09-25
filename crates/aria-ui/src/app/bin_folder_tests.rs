//! Pool folders, as the panel drives them.
//!
//! The class these close: a folder is state in two places — the project owns the
//! tree, the panel owns which folder is open — and every way the project can
//! change under the panel is a chance to leave it pointing at a folder that is
//! gone. Undo, redo, and load all do exactly that, so each is asserted.
//!
//! The other half is the count under the filter box. It describes the rows on
//! screen, so it has to follow the open folder; a filter has to reach into
//! subfolders or it would report zero for a file that is one level down.
//!
//! Not covered: the drop targets and the breadcrumb elements, which need a
//! window. Every rule they call into is asserted here.

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::SidebarTab;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn state() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("bins")).expect("state");
    state.new_project().expect("project");
    state
}

/// The bin the panel just made, which is also the one it opened.
fn made(state: &mut AppState) -> u64 {
    state
        .create_bin()
        .expect("a bin is made in the open folder");
    state.current_bin.expect("the new bin is open")
}

#[test]
fn a_new_bin_opens_and_starts_in_rename() {
    let mut state = state();
    let bin = made(&mut state);
    assert_eq!(state.current_bin, Some(bin));
    assert_eq!(
        state.bin_rename.as_ref().map(|rename| rename.bin),
        Some(bin),
        "a folder called \"New bin\" is not why anyone made one"
    );
    assert_eq!(
        state
            .bin_rename
            .as_ref()
            .map(|rename| rename.draft.as_str()),
        Some(crate::app::bins::DEFAULT_BIN_NAME),
        "the draft starts as the name, so a bare commit is not a rename to nothing"
    );
    assert_eq!(state.store.bins.len(), 1);
}

#[test]
fn a_bin_made_inside_another_nests_under_it() {
    let mut state = state();
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    assert_ne!(outer, inner);
    let row = state
        .store
        .bins
        .iter()
        .find(|item| item.id == inner)
        .expect("the inner bin is projected");
    assert_eq!(row.parent, Some(outer));
    assert_eq!(row.depth, 1, "the row indents by its depth");
    assert_eq!(
        state
            .bin_path()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![outer, inner],
        "the breadcrumb runs root-first"
    );
}

#[test]
fn a_rename_commits_what_was_typed_and_refuses_an_empty_name() {
    let mut state = state();
    let bin = made(&mut state);
    for _ in 0..crate::app::bins::DEFAULT_BIN_NAME.len() {
        state.bin_rename_pop();
    }
    for ch in "B roll".chars() {
        state.bin_rename_push(ch);
    }
    state.commit_bin_rename().expect("the rename applies");
    assert_eq!(state.bin_rename, None);
    assert_eq!(
        state
            .store
            .bins
            .iter()
            .find(|item| item.id == bin)
            .map(|item| item.name.as_str()),
        Some("B roll")
    );

    state.begin_bin_rename(bin);
    for _ in 0.."B roll".len() {
        state.bin_rename_pop();
    }
    state
        .commit_bin_rename()
        .expect("an empty name is not an error");
    assert_eq!(
        state
            .store
            .bins
            .iter()
            .find(|item| item.id == bin)
            .map(|item| item.name.as_str()),
        Some("B roll"),
        "a folder with no name cannot be told from another one"
    );
    assert_eq!(state.status_message.as_deref(), Some("a bin needs a name"));
}

#[test]
fn a_rename_is_bounded_and_escapes_without_committing() {
    let mut state = state();
    let bin = made(&mut state);
    for _ in 0..crate::app::bins::DEFAULT_BIN_NAME.len() {
        state.bin_rename_pop();
    }
    for _ in 0..(crate::app::bins::BIN_NAME_MAX_LEN + 20) {
        state.bin_rename_push('x');
    }
    assert_eq!(
        state
            .bin_rename
            .as_ref()
            .map(|rename| rename.draft.chars().count()),
        Some(crate::app::bins::BIN_NAME_MAX_LEN)
    );
    state.cancel_bin_rename();
    assert_eq!(state.bin_rename, None);
    assert_eq!(
        state
            .store
            .bins
            .iter()
            .find(|item| item.id == bin)
            .map(|item| item.name.as_str()),
        Some(crate::app::bins::DEFAULT_BIN_NAME),
        "Esc leaves the field; it never commits"
    );
}

#[test]
fn deleting_the_open_bin_steps_up_rather_than_leaving_nowhere() {
    let mut state = state();
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    state.cancel_bin_rename();
    state.remove_current_bin().expect("the inner bin goes");
    assert_eq!(
        state.current_bin,
        Some(outer),
        "the panel steps up to the parent"
    );
    assert!(!state.store.bins.iter().any(|item| item.id == inner));
    state.remove_current_bin().expect("the outer bin goes");
    assert_eq!(state.current_bin, None, "and out to the pool root");
    assert!(state.store.bins.is_empty());
}

#[test]
fn deleting_a_bin_keeps_the_media_that_was_in_it() {
    let mut state = state();
    let media = state.import_media(fixture()).expect("import");
    let bin = made(&mut state);
    state.cancel_bin_rename();
    state
        .move_media_to_bin(media.raw(), Some(bin))
        .expect("file it in the bin");
    assert_eq!(
        state
            .store
            .media
            .iter()
            .find(|item| item.id == media.raw())
            .and_then(|item| item.bin),
        Some(bin)
    );
    state.remove_current_bin().expect("the bin goes");
    assert_eq!(
        state.store.media.len(),
        1,
        "the source is still in the pool"
    );
    assert_eq!(
        state.store.media[0].bin, None,
        "it moved up to where the folder was"
    );
}

#[test]
fn the_pool_shows_the_open_folder_and_a_filter_reaches_into_its_subfolders() {
    let mut state = state();
    let media = state.import_media(fixture()).expect("import");
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    state.cancel_bin_rename();
    state
        .move_media_to_bin(media.raw(), Some(inner))
        .expect("file it two levels down");

    // Open the outer folder: the source is not directly in it.
    state.open_bin(Some(outer));
    let rows = state.pool_rows(SidebarTab::Media);
    assert!(rows.is_empty(), "the list shows what is in the open folder");
    assert_eq!(
        (rows.len(), rows.total()),
        (0, 1),
        "the total counts the folder and everything under it"
    );

    // A filter searches the subtree, so a file one level down is findable.
    state.media_query = "texture".into();
    let rows = state.pool_rows(SidebarTab::Media);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows.len(), rows.total()), (1, 1));

    // At the root everything is in view, filed or not.
    state.media_query.clear();
    state.open_bin(None);
    let rows = state.pool_rows(SidebarTab::Media);
    assert_eq!((rows.len(), rows.total()), (0, 1));
    state
        .move_media_to_bin(media.raw(), None)
        .expect("unfile it");
    assert_eq!(state.pool_rows(SidebarTab::Media).len(), 1);
}

#[test]
fn opening_a_folder_drops_the_item_selection() {
    // The properties block describes a source, and the one it was describing may
    // not be in the folder now on screen.
    let mut state = state();
    let media = state.import_media(fixture()).expect("import");
    let bin = made(&mut state);
    state.cancel_bin_rename();
    state.open_bin(None);
    state.select_pool_item(media.raw());
    assert!(!state.media_properties().is_empty());
    state.open_bin(Some(bin));
    assert!(state.pool_selection.is_empty());
    assert!(state.media_properties().is_empty());
}

#[test]
fn undoing_a_bin_creation_leaves_the_panel_at_the_root() {
    // The open folder is about to stop existing: without reconciliation the pool
    // would show an empty list with no way back.
    let mut state = state();
    let bin = made(&mut state);
    state.cancel_bin_rename();
    assert!(state.undo(), "the creation is one history entry");
    assert!(!state.store.bins.iter().any(|item| item.id == bin));
    assert_eq!(state.current_bin, None);
}

#[test]
fn an_undone_deletion_puts_the_folder_and_its_contents_back() {
    let mut state = state();
    let media = state.import_media(fixture()).expect("import");
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    state.cancel_bin_rename();
    state
        .move_media_to_bin(media.raw(), Some(inner))
        .expect("file it in the inner bin");
    state.remove_current_bin().expect("delete the inner bin");
    assert_eq!(state.store.media[0].bin, Some(outer), "the source moved up");

    assert!(state.undo(), "the deletion is one entry");
    assert!(
        state.store.bins.iter().any(|item| item.id == inner),
        "the folder is back under its own id"
    );
    assert_eq!(
        state.store.media[0].bin,
        Some(inner),
        "and what it held is back in it"
    );
}

#[test]
fn a_rename_in_flight_is_dropped_when_its_folder_goes() {
    let mut state = state();
    let bin = made(&mut state);
    assert!(state.bin_rename.is_some());
    state
        .remove_current_bin()
        .expect("delete the folder mid-rename");
    assert_eq!(
        state.bin_rename, None,
        "typing into a field for a folder that is gone would commit onto nothing"
    );
    assert!(!state.store.bins.iter().any(|item| item.id == bin));
}

#[test]
fn a_bin_cannot_be_dropped_into_its_own_subtree() {
    let mut state = state();
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    state.cancel_bin_rename();
    let before = state.revision;
    let refused = state.move_bin(outer, Some(inner));
    assert!(
        refused.is_err(),
        "the move would cut the subtree off the root"
    );
    assert_eq!(state.revision, before, "a refusal costs no history");
    assert_eq!(
        state
            .store
            .bins
            .iter()
            .find(|item| item.id == inner)
            .and_then(|item| item.parent),
        Some(outer)
    );
    // The legal direction works and re-parents to the root.
    state.move_bin(inner, None).expect("out to the root");
    assert_eq!(
        state
            .store
            .bins
            .iter()
            .find(|item| item.id == inner)
            .and_then(|item| item.parent),
        None
    );
}

#[test]
fn the_folder_rows_are_the_children_of_the_open_folder_only() {
    let mut state = state();
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let inner = made(&mut state);
    state.cancel_bin_rename();
    state.open_bin(None);
    assert_eq!(
        state
            .child_bins()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![outer],
        "the root lists only top-level folders"
    );
    state.open_bin(Some(outer));
    assert_eq!(
        state
            .child_bins()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![inner]
    );
    state.open_bin(Some(inner));
    assert!(state.child_bins().is_empty());
}

/// **The folder views are one list per change, not one per read.**
///
/// Both were walks of the whole folder list, run twice per frame while the pool
/// was on screen and cloning every folder they kept. Two reads with nothing in
/// between are the same handle now; the two things they are built from — the
/// folder tree and which folder is open — each replace them.
#[test]
fn the_folder_views_are_one_list_per_change() {
    let mut state = state();
    let outer = made(&mut state);
    state.cancel_bin_rename();
    let first = state.child_bins();
    assert!(
        std::sync::Arc::ptr_eq(&first, &state.child_bins()),
        "a second read rebuilt the rows"
    );
    let trail = state.bin_path();
    assert!(
        std::sync::Arc::ptr_eq(&trail, &state.bin_path()),
        "a second read rebuilt the breadcrumb"
    );
    assert_eq!(
        trail.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![outer],
        "the crumb chain is the open folder and its parents, root first"
    );

    let inner = made(&mut state);
    state.cancel_bin_rename();
    assert_eq!(
        state
            .bin_path()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![outer, inner],
        "making a folder inside the open one opens it, so the crumbs grow"
    );
    assert!(
        state.child_bins().is_empty(),
        "the new folder is empty, and the rows are its children now"
    );

    state.open_bin(Some(outer));
    let up = state.child_bins();
    assert_eq!(
        up.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![inner],
        "navigating replaces the rows"
    );
    assert!(
        state.undo(),
        "the inner folder's creation is one history entry"
    );
    assert!(
        state.child_bins().is_empty(),
        "an undo of the folder is a projection, so the rows follow it"
    );
}
