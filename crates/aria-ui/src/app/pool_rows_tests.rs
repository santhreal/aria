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

/// A pool of four imported items, unfiltered, at the root folder: footage at 24
/// and 30 fps, 3 s and 2 s long, and a 2 s WAV whose ticks are milliseconds.
fn pool() -> crate::app::AppState {
    let (mut state, _) = with_media(asset("fixture-640x360-24fps-3s.mp4"));
    for name in [
        "fixture-640x360-30fps-3s.mp4",
        "fixture-sine-48k-2s.wav",
        "fixture-640x360-24fps-3s-av.mp4",
    ] {
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

/// How long a row's source runs, exact at whatever rate it was probed.
fn length(row: &crate::types::UiMediaItem) -> aria_core::RationalTime {
    aria_core::RationalTime::new(row.duration_ticks, row.frame_rate)
}

/// The rows as `(name, stated length)`, by name.
fn stated(state: &crate::app::AppState) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = state
        .store
        .media
        .iter()
        .map(|row| (row.name.to_string(), row.duration_label.to_string()))
        .collect();
    rows.sort();
    rows
}

#[test]
fn the_duration_sort_orders_by_time_across_rates() {
    // The defect this closes: the sort compared raw ticks, and an audio-only
    // source counts milliseconds, so a 2 s WAV (2000 ticks) sorted above a 3 s
    // clip (72 frames). Every adjacent pair is checked, so any mix of rates the
    // pool holds is covered rather than the one pair reported.
    let mut state = pool();
    state.media_sort = MediaSort::Duration;
    let rows = state.pool_rows(SidebarTab::Media);
    assert!(
        rows.iter().any(|row| row.kind == UiClipKind::Audio)
            && rows.iter().any(|row| row.kind != UiClipKind::Audio),
        "the fixture holds one tick unit only, so this proves nothing"
    );
    let lengths: Vec<aria_core::RationalTime> = rows.iter().map(length).collect();
    for (at, pair) in lengths.windows(2).enumerate() {
        assert_ne!(
            pair[0].compare(pair[1]),
            std::cmp::Ordering::Less,
            "row {at} runs shorter than row {} under a longest-first sort",
            at + 1
        );
    }
}

#[test]
fn a_row_states_the_length_placement_gives_it() {
    // The defect this closes: a row formatted its source's own ticks at the
    // sequence rate, so a 2 s WAV probed in milliseconds read as 00:01:23:08 and
    // 30 fps footage read long in a 24 fps sequence. The lengths are the files'
    // own, as a container probe reports them.
    let mut state = pool();
    assert_eq!(
        state.store.sequence.frame_rate,
        aria_models::Rational::FPS_24,
        "the expected timecodes below are at 24 fps"
    );
    let row = |name: &str, length: &str| (name.to_string(), length.to_string());
    assert_eq!(
        stated(&state),
        vec![
            row("fixture-640x360-24fps-3s", "00:00:03:00"),
            row("fixture-640x360-24fps-3s-av", "00:00:02:00"),
            row("fixture-640x360-30fps-3s", "00:00:03:00"),
            row("fixture-sine-48k-2s", "00:00:02:00"),
        ]
    );

    // The row and a placement of the whole source agree, so what the pool
    // promises is what lands on the timeline.
    let wav = state
        .store
        .media
        .iter()
        .find(|row| row.kind == UiClipKind::Audio)
        .map(|row| (row.id, row.duration_label.clone()))
        .expect("the fixture imported a WAV");
    state
        .drop_media_on_timeline(aria_models::MediaId::from_raw(wav.0), None, 0)
        .expect("place the sound");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.media_id == Some(wav.0))
        .map(|clip| clip.duration_label.clone())
        .expect("the WAV landed on a lane");
    assert_eq!(placed, wav.1);
}

#[test]
fn only_a_picture_states_a_frame_rate() {
    // A WAV's tick rate is 1000 per second, which is a clock and not a frame
    // rate; the properties block states it for footage only.
    let mut state = pool();
    let rows: Vec<(u64, UiClipKind, String)> = state
        .store
        .media
        .iter()
        .map(|row| (row.id, row.kind, row.name.to_string()))
        .collect();
    for (id, kind, name) in rows {
        state.select_pool_item(id);
        let properties = state.media_properties();
        let value = |wanted: &str| {
            properties
                .iter()
                .find(|(label, _)| *label == wanted)
                .map(|(_, value)| value.to_string())
        };
        let label = state
            .store
            .media
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.duration_label.to_string());
        assert_eq!(value("Duration"), label, "{name}");
        match kind {
            UiClipKind::Audio => assert_eq!(value("Frame rate"), None, "{name} states one"),
            _ => assert!(
                value("Frame rate").is_some_and(|rate| rate.ends_with(" fps")),
                "{name} states no frame rate"
            ),
        }
    }
}
