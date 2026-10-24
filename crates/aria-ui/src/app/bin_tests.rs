//! Contracts for how the bin reads the pool, and for a marker's note.
//!
//! The class closed here is "the panel computes what it draws": the filter, the
//! order, the match count, and the properties rows all come from `AppState`, so
//! a count cannot disagree with the rows beneath it and a filter cannot be
//! spelled one way in the list and another in the header.
//!
//! The marker half closes "a field exists on the model and no path writes it":
//! a comment is set, cleared, and carried through every other marker edit.

use std::path::PathBuf;

use aria_plugin::PluginContext;

use super::AppState;
use crate::types::{MediaSort, RenameTarget, SidebarTab, UiClipKind};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A project with the still fixture imported once.
fn with_media() -> AppState {
    let mut state =
        AppState::new(PluginContext::new(), crate::scratch_pool::scratch("bin")).expect("state");
    state.new_project().expect("project");
    state.import_media(fixture()).expect("import");
    state
}

#[test]
fn the_filter_matches_name_path_and_metadata_and_the_count_follows_the_rows() {
    let mut state = with_media();
    let total = state.store.media.len();
    assert_eq!(total, 1);
    let item = state.store.media[0].clone();

    // No filter: every item, and the count says so.
    let rows = state.pool_rows(SidebarTab::Media);
    assert_eq!(rows.len(), total);
    assert_eq!((rows.len(), rows.total()), (total, total));

    // The name matches case-insensitively.
    state.media_query = item.name.to_uppercase();
    assert_eq!(state.pool_rows(SidebarTab::Media).len(), 1);

    // So does a fragment of the path, which the row never prints.
    state.media_query = "fixtures".into();
    assert_eq!(state.pool_rows(SidebarTab::Media).len(), 1);

    // And the metadata: the frame size is searchable even though the name has
    // no digits in it.
    state.media_query = format!("{}x{}", item.width, item.height);
    assert_eq!(
        state.pool_rows(SidebarTab::Media).len(),
        1,
        "a resolution search must reach the item"
    );

    // A miss leaves an empty list, and the count reports the pool it filtered.
    state.media_query = "nothing-matches-this".into();
    let rows = state.pool_rows(SidebarTab::Media);
    assert!(rows.is_empty());
    assert_eq!((rows.len(), rows.total()), (0, total));

    // Whitespace is not a filter.
    state.media_query = "   ".into();
    assert_eq!(state.pool_rows(SidebarTab::Media).len(), total);
}

/// The count and the rows come off one walk of the pool, so the two can never
/// drift; this pins that they agree under every order.
#[test]
fn the_count_equals_the_rows_under_every_order() {
    let mut state = with_media();
    for order in MediaSort::ALL {
        state.media_sort = *order;
        let rows = state.pool_rows(SidebarTab::Media);
        assert_eq!(
            rows.len(),
            rows.iter().count(),
            "{order:?} disagrees with its own rows"
        );
        assert!(
            rows.total() >= rows.len(),
            "{order:?} filtered more rows in than the pool holds"
        );
    }
}

/// The Audio tab is a kind filter, so its total is the audio pool rather than
/// the whole one: a count of "1 of 1" beside no rows would be a lie.
#[test]
fn the_audio_tab_counts_only_audio() {
    let state = with_media();
    let audio = state
        .store
        .media
        .iter()
        .filter(|item| item.kind == UiClipKind::Audio)
        .count();
    let rows = state.pool_rows(SidebarTab::Audio);
    assert_eq!((rows.len(), rows.total()), (audio, audio));
}

#[test]
fn an_import_records_when_it_happened() {
    let state = with_media();
    let imported = state.store.media[0]
        .imported_at
        .expect("an import must record its time");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_secs();
    assert!(
        imported <= now && now - imported < 600,
        "import time {imported} is not near now {now}"
    );
}

/// The comparator, driven directly: three entries with known times, one of them
/// from a project saved before import times existed.
#[test]
fn date_added_puts_the_newest_first_and_the_undated_last() {
    let mut state = with_media();
    let base = state.store.media[0].clone();
    let dated = |id: u64, name: &str, at: Option<u64>| {
        let mut item = base.clone();
        item.id = id;
        item.name = name.into();
        item.imported_at = at;
        item
    };
    state.store_mut().media = vec![
        dated(1, "older", Some(1_000)),
        dated(2, "undated", None),
        dated(3, "newer", Some(2_000)),
    ];

    state.media_sort = MediaSort::DateAdded;
    let order: Vec<String> = state
        .pool_rows(SidebarTab::Media)
        .iter()
        .map(|item| item.name.to_string())
        .collect();
    assert_eq!(order, ["newer", "older", "undated"]);

    state.media_sort = MediaSort::Name;
    let by_name: Vec<String> = state
        .pool_rows(SidebarTab::Media)
        .iter()
        .map(|item| item.name.to_string())
        .collect();
    assert_eq!(
        by_name,
        ["newer", "older", "undated"],
        "name order is unrelated to import order and must not have changed"
    );
}

#[test]
fn properties_describe_the_selected_item_and_nothing_otherwise() {
    let mut state = with_media();
    assert!(
        state.media_properties().is_empty(),
        "nothing is selected on a fresh bin"
    );

    let id = state.store.media[0].id;
    state.select_pool_item(id);
    let rows = state.media_properties();
    let labels: Vec<&str> = rows.iter().map(|(label, _)| *label).collect();
    assert_eq!(
        labels,
        vec![
            "Name",
            "Container",
            "Frame size",
            "Frame rate",
            "Duration",
            "Video codec",
            "Audio",
            "Imported",
            "Status",
            "Path"
        ]
    );
    let value = |name: &str| {
        rows.iter()
            .find(|(label, _)| *label == name)
            .map(|(_, value)| value.clone())
            .expect(name)
    };
    assert_eq!(value("Container"), "PNG");
    assert_eq!(value("Status"), "Linked");
    assert_eq!(value("Audio"), "No");
    assert!(value("Frame size").contains('×'));
    assert!(
        value("Imported").starts_with("20"),
        "an import stamp must be a date: {}",
        value("Imported")
    );

    // A selection that no longer exists describes nothing rather than panicking
    // or describing the wrong item.
    state.select_pool_item(u64::MAX);
    assert!(state.media_properties().is_empty());
}

#[test]
fn the_filter_box_takes_the_keyboard_only_while_focused() {
    let mut state = with_media();
    assert!(!state.media_search_focus);
    state.focus_media_search(true);
    for ch in "sun".chars() {
        state.media_query_push(ch);
    }
    assert_eq!(state.media_query, "sun");
    state.media_query_pop();
    assert_eq!(state.media_query, "su");
    state.clear_media_query();
    assert!(state.media_query.is_empty());
    state.focus_media_search(false);
    assert!(!state.media_search_focus);
}

/// A held key cannot grow the filter without bound: the box has the same cap as
/// every other draft.
#[test]
fn the_filter_is_bounded() {
    let mut state = with_media();
    for _ in 0..500 {
        state.media_query_push('a');
    }
    assert!(state.media_query.chars().count() <= 64);
}

#[test]
fn a_marker_note_is_written_cleared_and_survives_every_other_marker_edit() {
    let mut state = with_media();
    state.set_playhead(0);
    state.add_marker_at_playhead().expect("marker");
    let marker = state.store.sequence.markers[0].id;
    assert_eq!(state.store.sequence.markers[0].comment, "");

    state
        .set_marker_comment(marker, "regrade this shot".into())
        .expect("note");
    assert_eq!(state.store.sequence.markers[0].comment, "regrade this shot");

    // Renaming, moving, and recolouring all go through one `SetMarker`, so each
    // one has to carry the note it did not touch.
    state
        .rename_marker(marker, "Chorus".into())
        .expect("rename");
    assert_eq!(state.store.sequence.markers[0].comment, "regrade this shot");
    state.cycle_marker_color(marker).expect("colour");
    assert_eq!(state.store.sequence.markers[0].comment, "regrade this shot");
    state.move_marker(marker, 0).expect("move");
    assert_eq!(state.store.sequence.markers[0].comment, "regrade this shot");
    assert_eq!(state.store.sequence.markers[0].name, "Chorus");

    // Clearing is a real edit, and it survives the round trip through the
    // engine rather than being treated as "no change".
    state
        .set_marker_comment(marker, String::new())
        .expect("clear");
    assert_eq!(state.store.sequence.markers[0].comment, "");

    // An unknown id is inert, not an error.
    assert!(state.set_marker_comment(u64::MAX, "x".into()).is_ok());
}

/// The note draft is the only inline entry where an empty commit means
/// something: a blank name cancels, a blank note clears.
#[test]
fn an_empty_draft_clears_a_note_but_cancels_a_rename() {
    let mut state = with_media();
    state.set_playhead(0);
    state.add_marker_at_playhead().expect("marker");
    let marker = state.store.sequence.markers[0].id;
    let name = state.store.sequence.markers[0].name.clone();
    state
        .set_marker_comment(marker, "stale note".into())
        .expect("note");

    state.begin_rename(RenameTarget::Marker(marker));
    state.commit_rename().expect("commit");
    assert_eq!(
        state.store.sequence.markers[0].name, name,
        "an empty name draft cancels"
    );

    state.begin_rename(RenameTarget::MarkerComment(marker));
    assert_eq!(
        state.rename_draft, "stale note",
        "the draft seeds from the note"
    );
    while !state.rename_draft.is_empty() {
        state.rename_pop();
    }
    state.commit_rename().expect("commit");
    assert_eq!(
        state.store.sequence.markers[0].comment, "",
        "an empty note draft clears the note"
    );
    assert!(state.rename.is_none());
}
