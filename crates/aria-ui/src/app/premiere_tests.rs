//! Contracts for the Premiere-shaped editing surface: in/out points, trim to
//! playhead, edit-mode-sensitive delete, linked selection, and marker
//! projection.
//!
//! These close the class "a keystroke mutates one field but the projected store
//! or the sibling command path never learns about it". They do not cover the
//! drawing of markers on the ruler, which needs a render surface.

use super::AppState;
use crate::types::EditMode;
use aria_models::{ClipId, MediaId};
use aria_plugin::PluginContext;
use std::path::PathBuf;
fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("premiere")
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}
/// A project with one clip on the timeline, selected.
pub(crate) fn with_clip() -> (AppState, ClipId) {
    with_media(fixture())
}

/// A project with one clip of `asset` on the timeline, selected.
///
/// The same six steps for any pool file, so a test that needs a clip with a
/// picture and a clip with sound asks for them here rather than growing its own
/// way into the engine.
pub(crate) fn with_media(asset: PathBuf) -> (AppState, ClipId) {
    let mut state = AppState::new(PluginContext::new(), scratch()).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(asset).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .next()
        .expect("one clip");
    state.select_clip(clip);
    (state, clip)
}

/// A state carrying a measurement-sized sequence: `lanes` lane pairs of
/// `clips` clips each, polled once so the store and the projection memo both
/// describe it.
///
/// The load goes through the engine, which only this module can reach, so a
/// test that needs a big timeline asks for one here instead of a caller
/// growing its own way in.
pub(crate) fn with_sequence(lanes: usize, clips: usize) -> AppState {
    let (mut state, _) = with_clip();
    state
        .engine
        .lock()
        .reset_project(crate::workload::sequence(lanes, clips));
    state.sync_from_engine();
    state
}
fn clip_bounds(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("clip still present");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}
fn clip_count(state: &AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum()
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "premiere_tests_draft.rs"]
mod premiere_tests_draft;
#[path = "premiere_tests_playhead.rs"]
mod premiere_tests_playhead;
