//! Regression tests for empty-timeline interactions.
//!
//! Asserts that timeline gestures (marquee, scrub, split, ripple, zoom, scroll, razor)
//! on a project with an empty default track execute safely without panic.

use crate::app::AppState;
use crate::types::EditorTool;
use aria_models::TrackId;
use aria_plugin::PluginContext;

fn empty_state() -> AppState {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("empty-timeline-test");
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("new project");
    state
}

#[test]
fn empty_timeline_gestures_do_not_panic() {
    let mut state = empty_state();

    // One empty default track / 0 clips baseline.
    assert_eq!(state.store.sequence.tracks.len(), 1);
    assert!(state.store.sequence.tracks[0].clips.is_empty());
    assert_eq!(state.store.sequence.duration_ticks, 0);

    // 1. Scrub playhead
    state.set_playhead(100);
    assert_eq!(state.store.playhead_ticks, 0);

    // 2. Split at playhead with no clips
    let split_res = state.split_at_playhead();
    assert!(split_res.is_ok());

    // 3. Ripple delete selected with no selection
    let ripple_res = state.ripple_delete_selected();
    assert!(ripple_res.is_ok());

    // 4. Select all and clear selection
    state.select_all_unlocked();
    assert!(state.selection.is_empty());
    state.clear_selection();

    // 5. Tool switching on empty timeline
    state.tool = EditorTool::Razor;
    state.tool = EditorTool::Hand;
    state.tool = EditorTool::Pointer;

    // 6. Zoom and scroll gestures on empty timeline
    state.set_zoom(2.0, false);
    state.zoom_fit_sequence();
    state.store_mut().scroll_x = 500.0;
    state.rebuild_store();

    // 7. Marquee operations
    assert!(!state.marquee_covers_row(0));
    assert!(!state.marquee_covers_row(5));

    // 8. Ripple trim on nonexistent clip
    let trim_res = state.ripple_trim(
        aria_models::ClipId::from_raw(999),
        TrackId::from_raw(1),
        0,
        24,
        false,
        1,
    );
    assert!(trim_res.is_err());
}
