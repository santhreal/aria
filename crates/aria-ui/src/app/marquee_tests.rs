//! Contracts for the timeline rubber band.
//!
//! The class closed here is "selection is one clip at a time": there was no way
//! to select a range of clips without clicking each one, and Select All was the
//! only multi-selection. The rules that matter are that a band takes what it
//! touches rather than only what it encloses, that it skips a locked lane, that
//! it spans the rows between its two corners in either direction, and that an
//! abandoned band changes nothing.
//!
//! Not covered: the pointer itself, which needs a render surface. The panel maps
//! a pointer x to a tick and the lane under it reports its own id; every
//! decision after that is here.

use aria_models::{ClipId, TrackId};

use super::AppState;
use super::premiere_tests::with_clip;

/// The fixture clip spans ticks 0..120 on the first lane.
const CLIP_END: i64 = 120;

fn selected(state: &AppState) -> Vec<u64> {
    let mut ids: Vec<u64> = state.selection.iter().map(|id| id.raw()).collect();
    ids.sort_unstable();
    ids
}

fn first_track(state: &AppState) -> u64 {
    state.store.sequence.tracks[0].id
}

#[test]
fn a_band_selects_what_it_touches() {
    let (mut state, clip) = with_clip();
    state.clear_selection();
    let track = first_track(&state);

    state.begin_marquee(track, CLIP_END - 10);
    assert!(state.drag_marquee(track, CLIP_END + 40));
    assert_eq!(state.commit_marquee(), 1);
    assert_eq!(selected(&state), vec![clip.raw()]);
}

#[test]
fn a_band_that_misses_selects_nothing() {
    let (mut state, _) = with_clip();
    let track = first_track(&state);

    state.begin_marquee(track, CLIP_END + 10);
    state.drag_marquee(track, CLIP_END + 60);
    assert_eq!(state.commit_marquee(), 0);
    assert!(selected(&state).is_empty());
}

#[test]
fn a_click_with_no_drag_deselects() {
    let (mut state, _) = with_clip();
    let track = first_track(&state);
    assert!(!selected(&state).is_empty(), "the fixture selects its clip");

    state.begin_marquee(track, 40);
    assert_eq!(
        state.commit_marquee(),
        0,
        "a zero-width band encloses nothing"
    );
    assert!(selected(&state).is_empty());
}

#[test]
fn a_band_drawn_backwards_is_the_same_band() {
    let (mut state, clip) = with_clip();
    state.clear_selection();
    let track = first_track(&state);

    state.begin_marquee(track, CLIP_END + 40);
    state.drag_marquee(track, CLIP_END - 10);
    assert_eq!(state.commit_marquee(), 1);
    assert_eq!(selected(&state), vec![clip.raw()]);
}

#[test]
fn a_band_skips_a_locked_lane() {
    let (mut state, _) = with_clip();
    let track = first_track(&state);
    state
        .set_track_locked(TrackId::from_raw(track), true)
        .expect("lock the lane");
    state.clear_selection();

    state.begin_marquee(track, 0);
    state.drag_marquee(track, CLIP_END);
    assert_eq!(state.commit_marquee(), 0, "a locked lane is not selectable");
    assert!(selected(&state).is_empty());
}

#[test]
fn a_band_spans_the_rows_between_its_corners() {
    let (mut state, _) = with_clip();
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("a second lane");
    let tracks: Vec<u64> = state.store.sequence.tracks.iter().map(|t| t.id).collect();
    assert!(tracks.len() >= 2, "the test added a second lane");
    let (first, last) = (tracks[0], tracks[tracks.len() - 1]);

    state.begin_marquee(first, 0);
    state.drag_marquee(last, CLIP_END);
    for (row, id) in tracks.iter().enumerate() {
        assert!(
            state.marquee_covers_row(row),
            "lane {id} is between the corners and must be covered"
        );
    }

    // One lane only: the other rows are outside it.
    state.cancel_marquee();
    state.begin_marquee(first, 0);
    assert!(state.marquee_covers_row(0));
    for (row, id) in tracks.iter().enumerate().skip(1) {
        assert!(
            !state.marquee_covers_row(row),
            "lane {id} is outside the band"
        );
    }
}

#[test]
fn an_abandoned_band_changes_nothing() {
    let (mut state, clip) = with_clip();
    state.select_clip(clip);
    let before = selected(&state);
    let track = first_track(&state);

    state.begin_marquee(track, CLIP_END + 10);
    state.drag_marquee(track, CLIP_END + 60);
    state.cancel_marquee();

    assert!(state.marquee.is_none());
    assert_eq!(selected(&state), before);
    assert_eq!(state.commit_marquee(), 0, "there is no band left to commit");
}

#[test]
fn dragging_to_the_same_tick_is_not_a_change() {
    let (mut state, _) = with_clip();
    let track = first_track(&state);
    state.begin_marquee(track, 20);
    assert!(!state.drag_marquee(track, 20), "the corner did not move");
    assert!(state.drag_marquee(track, 21));
}

#[test]
fn a_band_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let track = first_track(&state);
    let depth = state.history_depth();

    state.begin_marquee(track, 0);
    state.drag_marquee(track, CLIP_END);
    state.commit_marquee();

    assert_eq!(state.history_depth(), depth, "selecting is not undoable");
}

#[test]
fn the_selection_a_band_leaves_is_usable() {
    let (mut state, clip) = with_clip();
    state.clear_selection();
    let track = first_track(&state);

    state.begin_marquee(track, 0);
    state.drag_marquee(track, CLIP_END);
    assert_eq!(state.commit_marquee(), 1);
    assert_eq!(state.selected_clip(), Some(ClipId::from_raw(clip.raw())));
}

#[test]
fn an_empty_click_does_not_rebuild_an_empty_selection() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    let before = state.store.clone();
    let track = first_track(&state);

    state.begin_marquee(track, 40);
    assert_eq!(state.commit_marquee(), 0);
    assert!(
        std::sync::Arc::ptr_eq(&before, &state.store),
        "an already-empty selection rebuilt the projection"
    );
}
