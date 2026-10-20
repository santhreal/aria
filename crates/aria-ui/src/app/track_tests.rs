//! Contracts for the track cluster: stack order, solo, the duck-source flag, and
//! the two ripple primitives.
//!
//! The class closed here is "a track command exists in `aria-commands` but no
//! operator path reaches it, so the projection and the model drift". Every
//! assertion reads the *projected* store after the edit, because the panel draws
//! from the projection and a command that mutates the model without republishing
//! is invisible on screen.
//!
//! Not covered: the drawn header chrome and dragging a lane by its header, which
//! need a render surface.

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::{EditMode, UiTrack, UiTrackKind};
use aria_models::{MediaId, TrackId, TrackKind};
/// Projected lane ids, top row first — the order the panel draws.
fn rows(state: &AppState) -> Vec<u64> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.id)
        .collect()
}
fn lane(state: &AppState, id: u64) -> &UiTrack {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == id)
        .expect("lane still projected")
}
fn clip_start(state: &AppState, track: u64, clip: u64) -> i64 {
    lane(state, track)
        .clips
        .iter()
        .find(|c| c.id == clip)
        .expect("clip present")
        .timeline_start_ticks
}
/// The media the fixture clip carries, for a second placement.
fn fixture_media(state: &AppState, track: u64, clip: u64) -> MediaId {
    lane(state, track)
        .clips
        .iter()
        .find(|c| c.id == clip)
        .and_then(|c| c.media_id)
        .map(MediaId::from_raw)
        .expect("the fixture clip carries media")
}
/// Two video lanes *above* the fixture's lane. The fixture's lane is the main
/// track, and `Timeline::enforce_lane_zones` pins the main track to the bottom of
/// the stack, so a reorder test that used it would be asserting against an
/// invariant rather than against the reorder.
fn with_two_video_lanes() -> (AppState, u64, u64, u64) {
    let (mut state, _) = with_clip();
    let existing = rows(&state);
    assert_eq!(existing.len(), 1, "the fixture starts with one lane");
    state.add_track(TrackKind::Video, "V2").expect("add V2");
    state.add_track(TrackKind::Video, "V3").expect("add V3");
    let after = rows(&state);
    assert_eq!(after.len(), 3);
    let main = existing[0];
    assert_eq!(
        after[2], main,
        "the main lane stays at the bottom of the stack"
    );
    (state, after[0], after[1], main)
}
// The class closed below is "shifting time is one lane and one fixed step". A
// typed offset moves the selection on every lane it spans, from the earliest
// selected clip on each, and is rejected whole rather than half-applied.

/// Two video lanes each carrying one clip at tick 0, both selected.
fn with_two_selected_lanes() -> (AppState, u64, u64, u64, u64) {
    let (mut state, first) = with_clip();
    let top_lane = rows(&state)[0];
    let media = fixture_media(&state, top_lane, first.raw());
    state.add_track(TrackKind::Video, "V2").expect("add V2");
    let second_lane = *rows(&state)
        .iter()
        .find(|id| **id != top_lane)
        .expect("a second lane");
    // A cover, not an insert: in Ripple mode this second placement would open a
    // hole on V2 and every sync-locked lane would ride along, so the clip
    // already on V1 would not be at tick 0 any more. What is under test here is
    // shifting a selection, not routing.
    state.edit_mode = EditMode::Overwrite;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(second_lane)), 0)
        .expect("place on V2");
    let second = lane(&state, second_lane).clips[0].id;
    state.select_clip(first);
    state.add_clip_to_selection(aria_models::ClipId::from_raw(second));
    (state, top_lane, first.raw(), second_lane, second)
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "track_tests_drop.rs"]
mod track_tests_drop;
#[path = "track_tests_lane.rs"]
mod track_tests_lane;
