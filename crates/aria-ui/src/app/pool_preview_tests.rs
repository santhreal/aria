//! Pool preview: the scrub mapping, the tile tick, and the source monitor.
//!
//! The class these close: a picture of a source is shown by three surfaces that
//! must agree on one tick — the tile, the monitor, and the render request — so
//! every rule that picks a tick is asserted here rather than in one caller.
//! Space is the other half: it is transport everywhere except over the pool, and
//! a monitor that opened while the sequence kept playing would move the playhead
//! under a picture that is not of it.
//!
//! Not covered: the rendered pixels and the measuring canvas, which need a
//! window and a GPU. The store's keying is asserted, so a picture cannot outlive
//! the source it is of — and cannot be thrown away by an edit that left that
//! source alone — even though nothing here decodes.

use super::{PoolPreviewStore, scrub_tick};
use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::ActivePanel;
use aria_plugin::PluginContext;
fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}
fn state() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("pool-preview")).expect("state");
    state.new_project().expect("project");
    state
}
/// A pool with one still in it, selected, and the pool panel focused — the state
/// a Space press is answered in.
fn with_selection() -> (AppState, u64) {
    let mut state = state();
    let media = state.import_media(fixture()).expect("import");
    let id = media.raw();
    state.select_pool_item(id);
    state.active_panel = ActivePanel::Media;
    (state, id)
}
// --- source marks and cutting in ---------------------------------------------

/// A monitor open on the still, and the tick it is parked on.
fn with_monitor() -> (AppState, u64, i64) {
    let (mut state, media) = with_selection();
    assert!(state.toggle_source_monitor(), "the monitor opens");
    let frames = state
        .source_view
        .as_ref()
        .expect("open")
        .duration_ticks
        .max(1);
    (state, media, frames)
}
// --- three-point, four-point, and match frame --------------------------------

/// Every clip on every lane, as `(start, end)` in timeline ticks.
fn spans(state: &AppState) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    out.sort_unstable();
    out
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "pool_preview_tests_monitor.rs"]
mod pool_preview_tests_monitor;
#[path = "pool_preview_tests_source.rs"]
mod pool_preview_tests_source;
