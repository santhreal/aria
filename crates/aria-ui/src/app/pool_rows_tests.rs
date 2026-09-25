//! What the pool view answers, and that it answers without copying the pool.
//!
//! # Why
//!
//! The list and the count were two walks over two sets of clones, and the class
//! of defect that hid in them is a count that describes a different set from the
//! rows above it. These pin both to one walk, and pin the no-copy property by
//! identity rather than by reading the code: a row the view hands back is the
//! same object the projection holds, at the same address.

use super::fixtures::asset;
use super::premiere_tests::with_media;
use crate::types::{MediaSort, SidebarTab, UiClipKind};

/// A pool of three imported items, unfiltered, at the root folder.
fn pool() -> crate::app::AppState {
    let (mut state, _) = with_media(asset("fixture-640x360-24fps-3s.mp4"));
    for name in ["fixture-640x360-30fps-3s.mp4", "fixture-sine-48k-2s.wav"] {
        state
            .import_media(asset(name))
            .expect("a pool worth filtering");
    }
    state.sync_from_engine();
    state
}

#[test]
fn a_row_is_the_projections_own_item_and_not_a_copy() {
    // The property the clones cost: a view over the pool is a view, so the
    // sidebar can draw a pool of hundreds without duplicating every name, path
    // and codec string once per frame.
    let state = pool();
    let rows = state.pool_rows(SidebarTab::Media);
    assert!(rows.len() >= 3, "the fixture imported nothing");
    for (index, row) in rows.iter().enumerate() {
        let held = state
            .store
            .media
            .iter()
            .find(|item| item.id == row.id)
            .expect("a drawn row that the projection does not hold");
        assert!(
            std::ptr::eq(row, held),
            "row {index} is a copy of the projection's item, not the item"
        );
    }
}

#[test]
fn the_count_describes_the_rows_above_it() {
    // The defect this closes: "3 of 40" computed beside the rows rather than
    // from them, so a filter change could move one and not the other. Every
    // order is checked, because the sort runs after the filter and a sort that
    // dropped or duplicated an index would show up here first.
    let mut state = pool();
    for order in [
        MediaSort::Name,
        MediaSort::Duration,
        MediaSort::Resolution,
        MediaSort::DateAdded,
    ] {
        state.media_sort = order;
        let rows = state.pool_rows(SidebarTab::Media);
        assert_eq!(
            rows.len(),
            rows.ids().len(),
            "{order:?} drew a different number of ids than rows"
        );
        assert_eq!(
            rows.total(),
            state.store.media.len(),
            "{order:?} counted a different pool than the projection holds"
        );
    }
}

#[test]
fn a_query_narrows_the_rows_and_leaves_the_total_alone() {
    let mut state = pool();
    let total = state.pool_rows(SidebarTab::Media).total();
    state.media_query = "nothing-matches-this".into();
    let rows = state.pool_rows(SidebarTab::Media);
    assert!(rows.is_empty(), "a query that matches nothing drew rows");
    assert_eq!(
        rows.total(),
        total,
        "the total is what the query narrowed, so a query cannot change it"
    );
}

#[test]
fn the_audio_tab_holds_only_sound() {
    let state = pool();
    let rows = state.pool_rows(SidebarTab::Audio);
    assert!(rows.len() >= 1, "the fixture imported no sound");
    for row in rows.iter() {
        assert_eq!(
            row.kind,
            UiClipKind::Audio,
            "the audio tab drew {}, which is not sound",
            row.name
        );
    }
    assert_eq!(
        rows.total(),
        rows.len(),
        "with no query, the audio tab's total is its own rows"
    );
}

#[test]
fn a_sort_reorders_the_same_set() {
    // A sort is an ordering, not a filter: the ids are a permutation whichever
    // order is chosen.
    let mut state = pool();
    state.media_sort = MediaSort::Name;
    let mut by_name = state.pool_rows(SidebarTab::Media).ids();
    state.media_sort = MediaSort::Duration;
    let mut by_duration = state.pool_rows(SidebarTab::Media).ids();
    assert_ne!(
        by_name, by_duration,
        "the fixture cannot tell the two orders apart, so this proves nothing"
    );
    by_name.sort_unstable();
    by_duration.sort_unstable();
    assert_eq!(by_name, by_duration, "a sort dropped or added an item");
}
