//! Monitor: 26 of the cases in [`super`].
//!
//! Split out of `pool_preview_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

// --- the scrub mapping --------------------------------------------------------

#[test]
fn a_scrub_spans_the_take_from_edge_to_edge() {
    // The right edge is the last frame, not one past it.
    assert_eq!(scrub_tick(0.0, 100.0, 100), 0);
    assert_eq!(scrub_tick(100.0, 100.0, 100), 99);
    assert_eq!(scrub_tick(50.0, 100.0, 101), 50);
}
#[test]
fn a_scrub_outside_the_row_clamps_instead_of_wrapping() {
    assert_eq!(scrub_tick(-40.0, 100.0, 100), 0);
    assert_eq!(scrub_tick(400.0, 100.0, 100), 99);
}
#[test]
fn a_row_with_nothing_to_scrub_stays_on_the_first_frame() {
    assert_eq!(scrub_tick(70.0, 100.0, 1), 0, "a still has one frame");
    assert_eq!(scrub_tick(70.0, 100.0, 0), 0, "an empty source");
    assert_eq!(scrub_tick(70.0, 0.0, 100), 0, "an unmeasured row");
    assert_eq!(scrub_tick(f32::NAN, 100.0, 100), 0, "no position at all");
}
// --- the tile tick -----------------------------------------------------------

#[test]
fn a_tile_shows_its_first_frame_until_it_is_hovered() {
    let (mut state, media) = with_selection();
    assert_eq!(state.pool_tile_tick(media), 0);

    state.hover_pool_item(media, 12);
    assert_eq!(state.pool_tile_tick(media), 12);
    assert_eq!(
        state.pool_tile_tick(media + 1_000),
        0,
        "hovering one row must not scrub another"
    );

    state.clear_pool_hover();
    assert_eq!(state.pool_tile_tick(media), 0);
}
#[test]
fn a_loaded_source_keeps_its_tick_on_the_tile_after_the_pointer_leaves() {
    let (mut state, media) = with_selection();
    assert!(state.toggle_source_monitor(), "the monitor opens");

    state.hover_pool_item(media, 7);
    state.clear_pool_hover();

    assert_eq!(
        state.source_view.as_ref().map(|view| view.tick),
        Some(7),
        "a mouse-out must not rewind the monitor"
    );
    assert_eq!(
        state.pool_tile_tick(media),
        7,
        "the tile follows the source it has loaded"
    );
}
#[test]
fn a_hover_past_the_end_of_the_source_clamps_the_monitor() {
    let (mut state, media) = with_selection();
    assert!(state.toggle_source_monitor());
    let last = state
        .source_view
        .as_ref()
        .map(|view| view.duration_ticks - 1)
        .expect("a monitor is open");

    state.hover_pool_item(media, last + 500);

    assert_eq!(state.source_view.as_ref().map(|view| view.tick), Some(last));
}
// --- the monitor -------------------------------------------------------------

#[test]
fn space_over_the_pool_loads_the_selection_and_puts_it_away_again() {
    let (mut state, media) = with_selection();

    state.space();
    assert_eq!(
        state.source_view.as_ref().map(|view| view.media),
        Some(media)
    );

    state.space();
    assert!(state.source_view.is_none(), "the same key closes it");
    assert!(!state.playing, "closing the monitor is not a play command");
}
#[test]
fn space_elsewhere_is_transport_and_returns_the_pane_to_the_sequence() {
    let (mut state, _media) = with_selection();
    state.space();
    assert!(state.source_view.is_some());

    state.active_panel = ActivePanel::Timeline;
    state.space();

    assert!(
        state.source_view.is_none(),
        "the sequence cannot play behind a source frame"
    );
    assert!(state.playing, "space is still transport");
}
#[test]
fn opening_the_monitor_stops_playback() {
    let (mut state, _media) = with_selection();
    state.active_panel = ActivePanel::Timeline;
    state.space();
    assert!(state.playing);

    state.active_panel = ActivePanel::Media;
    state.space();

    assert!(state.source_view.is_some());
    assert!(!state.playing, "two monitors must not run at once");
}
#[test]
fn the_monitor_refuses_what_it_cannot_show() {
    let mut state = state();
    state.active_panel = ActivePanel::Media;

    // Nothing selected.
    assert!(!state.toggle_source_monitor());
    assert_eq!(
        state.status_message.as_deref(),
        Some("select a pool item to preview it")
    );

    // A selection that is not in the pool at all.
    state.select_pool_item(4_242);
    assert!(!state.toggle_source_monitor());
    assert_eq!(
        state.status_message.as_deref(),
        Some("that source cannot be previewed")
    );
    assert!(state.source_view.is_none());
}
#[test]
fn a_second_source_replaces_the_first_rather_than_stacking() {
    let (mut state, first) = with_selection();
    assert!(state.toggle_source_monitor());
    // A pool dedupes by path, so a second entry needs a second file.
    let copy = scratch("pool-preview-copy").join("second.png");
    std::fs::copy(fixture(), &copy).expect("copy the fixture");
    let second = state.import_media(copy).expect("import").raw();
    assert_ne!(first, second);
    state.select_pool_item(second);

    assert!(state.toggle_source_monitor(), "a different source opens");

    assert_eq!(
        state.source_view.as_ref().map(|view| view.media),
        Some(second)
    );
}
#[test]
fn undoing_the_import_puts_the_monitor_away() {
    let (mut state, _media) = with_selection();
    assert!(state.toggle_source_monitor());

    assert!(state.undo(), "the import is undone");

    assert!(
        state.source_view.is_none(),
        "the pane cannot show a source the project no longer has"
    );
}
// --- the store ---------------------------------------------------------------

#[test]
fn a_source_that_changed_drops_its_own_pictures_and_no_others() {
    let mut store = PoolPreviewStore::default();
    store.sync_media(1, 3);
    store.sync_media(2, 3);
    assert!(store.wants(1, 0), "nothing rendered yet");
    // A request in flight blocks a second one, whatever it is of.
    store.start(1, 0);
    assert!(!store.wants(1, 0));
    assert!(!store.wants(2, 40), "one decode at a time");

    store.finish(3, 1, 0, None);
    assert!(
        !store.wants(1, 0),
        "a source that produced no frame is not retried every repaint"
    );
    assert!(store.wants(2, 40), "the slot is free again");

    // Entry 1 was relinked or given a proxy: its own answers go, and entry 2 —
    // which nothing happened to — keeps what it learned. A timeline edit changes
    // no stamp at all, so it drops nothing.
    store.sync_media(1, 4);
    assert!(
        store.wants(1, 0),
        "the picture is of the source it replaced"
    );
    store.start(2, 40);
    store.finish(3, 2, 40, None);
    store.sync_media(1, 5);
    assert!(
        !store.wants(2, 40),
        "another entry's relink threw away this entry's answer"
    );
}
#[test]
fn a_picture_that_arrives_after_its_source_changed_is_dropped() {
    let mut store = PoolPreviewStore::default();
    store.sync_media(9, 1);
    store.start_source(9, 0);
    store.sync_media(9, 2);

    // The render was started against the old source and lands after the relink.
    store.finish_source(1, 9, 0, None);

    assert!(
        store.wants_source(9, 0),
        "a stale answer must not stand in for the source now in the pool"
    );
}
#[test]
fn the_monitor_keeps_the_last_frame_of_the_source_it_holds() {
    let mut store = PoolPreviewStore::default();
    store.sync_media(9, 1);
    assert!(store.wants_source(9, 0));
    store.start_source(9, 0);
    assert!(!store.wants_source(9, 0), "one monitor render at a time");
}
#[test]
fn a_row_rectangle_survives_a_relink() {
    let mut store = PoolPreviewStore::default();
    store.sync_media(5, 1);
    store.record_rect(5, 120.0, 200.0);
    store.sync_media(5, 2);

    assert_eq!(
        store.rect(5),
        Some((120.0, 200.0)),
        "where a row is painted is layout, not content"
    );
    assert_eq!(store.rect(6), None);
}
#[test]
fn a_stamp_reads_the_source_and_not_the_pool_around_it() {
    let (state, media, _frames) = with_monitor();
    let stamp = state.media_stamp(media).expect("the source is in the pool");
    assert_eq!(
        state.media_stamp(media),
        Some(stamp),
        "the same entry stamped twice must agree, or nothing is ever cached"
    );
    assert_eq!(
        state.media_stamp(media.wrapping_add(1_000)),
        None,
        "an entry the pool does not have cannot be stamped"
    );
}
#[test]
fn an_unmarked_source_cuts_in_whole() {
    let (state, _media, frames) = with_monitor();
    assert_eq!(
        state.source_range(),
        Some((0, frames)),
        "no marks means the whole take"
    );
}
#[test]
fn an_out_point_includes_the_frame_on_screen() {
    let (mut state, media, frames) = with_monitor();
    assert!(frames > 3, "the fixture is long enough to mark inside");
    state.hover_pool_item(media, 2);
    state.set_source_out();

    assert_eq!(state.source_view.as_ref().unwrap().mark_out, Some(3));
    assert_eq!(
        state.source_range(),
        Some((0, 3)),
        "an out point is exclusive, so frame 2 is kept"
    );
}
#[test]
fn a_mark_that_would_invert_the_range_drops_the_other_one() {
    let (mut state, media, frames) = with_monitor();
    assert!(frames > 6);
    // Out at frame 2, then an in point after it.
    state.hover_pool_item(media, 2);
    state.set_source_out();
    state.hover_pool_item(media, 5);
    state.set_source_in();
    let view = state.source_view.as_ref().unwrap();
    assert_eq!(view.mark_in, Some(5));
    assert_eq!(
        view.mark_out, None,
        "an out point before the in point is dropped"
    );

    // And the other way round.
    state.hover_pool_item(media, 1);
    state.set_source_out();
    let view = state.source_view.as_ref().unwrap();
    assert_eq!(view.mark_out, Some(2));
    assert_eq!(view.mark_in, None);
}
#[test]
fn clearing_the_marks_restores_the_whole_take() {
    let (mut state, media, frames) = with_monitor();
    state.hover_pool_item(media, 1);
    state.set_source_in();
    state.set_source_out();
    state.clear_source_marks();

    assert_eq!(state.source_range(), Some((0, frames)));
}
#[test]
fn marks_belong_to_the_monitor_in_front_of_them() {
    let (mut state, media, _frames) = with_monitor();
    state.hover_pool_item(media, 2);
    state.mark_in_here();
    assert_eq!(state.source_view.as_ref().unwrap().mark_in, Some(2));
    assert_eq!(state.mark_in, None, "the sequence is not marked from here");

    state.clear_mark_in_here();
    assert_eq!(state.source_view.as_ref().unwrap().mark_in, None);

    // With the monitor away, the same keys are the sequence's again.
    state.close_source_monitor();
    state.mark_in_here();
    assert_eq!(state.mark_in, Some(state.playhead.value));
    state.mark_out_here();
    assert_eq!(state.mark_out, Some(state.playhead.value));
    state.clear_mark_out_here();
    assert_eq!(state.mark_out, None);
}
#[test]
fn the_bar_and_the_tile_share_one_tick() {
    let (mut state, media, frames) = with_monitor();
    assert!(frames > 1);
    state.pool_preview.record_source_bar(40.0, 200.0);

    state.scrub_source(240.0);

    let tick = frames - 1;
    assert_eq!(state.source_view.as_ref().unwrap().tick, tick);
    assert_eq!(
        state.pool_tile_tick(media),
        tick,
        "the tile follows the bar, so the two pictures agree"
    );
}
#[test]
fn an_unmeasured_bar_does_not_move_the_monitor() {
    let (mut state, media, _frames) = with_monitor();
    state.hover_pool_item(media, 1);

    state.scrub_source(500.0);

    assert_eq!(
        state.source_view.as_ref().unwrap().tick,
        1,
        "a pointer position means nothing without a painted rectangle"
    );
}
#[test]
fn cutting_in_places_the_marked_range_at_the_playhead() {
    let (mut state, media, frames) = with_monitor();
    assert!(frames > 3);
    state.hover_pool_item(media, 1);
    state.set_source_in();
    state.hover_pool_item(media, 2);
    state.set_source_out();

    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("cut in");

    let clips: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .collect();
    assert_eq!(clips.len(), 1, "one clip for one cut-in");
    assert_eq!(clips[0].timeline_start_ticks, state.playhead.value);
    assert!(
        state.source_view.is_some(),
        "the monitor stays open, so takes can be cut in one after another"
    );
    let _ = media;
}
#[test]
fn overwriting_covers_instead_of_rippling() {
    let (mut state, _media, _frames) = with_monitor();
    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("first cut in");
    let first = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.timeline_start_ticks)
        .collect::<Vec<_>>();
    assert_eq!(first, vec![0]);

    // The playhead has not moved, so the second cut-in lands on the first clip.
    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("overwrite");

    let starts = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.timeline_start_ticks)
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        vec![0],
        "the covered clip is replaced rather than pushed aside"
    );
}
#[test]
fn an_empty_range_is_refused_rather_than_cut_in() {
    let (mut state, media, _frames) = with_monitor();
    // In at the last frame and out at the first cannot both stand, so force the
    // degenerate case the engine must never be handed.
    state.hover_pool_item(media, 0);
    state.set_source_out();
    if let Some(view) = state.source_view.as_mut() {
        view.mark_in = Some(view.mark_out.expect("out"));
    }

    assert_eq!(state.source_range(), None);
    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("refused, not failed");
    assert_eq!(
        state.status_message.as_deref(),
        Some("the marked range is empty")
    );
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .all(|lane| lane.clips.is_empty()),
        "nothing was placed"
    );
}
