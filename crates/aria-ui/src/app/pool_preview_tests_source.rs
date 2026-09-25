//! Source: 6 of the cases in [`super`].
//!
//! Split out of `pool_preview_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn a_mark_past_a_shortened_source_is_dropped_rather_than_moved() {
    let (mut state, _media, frames) = with_monitor();
    if let Some(view) = state.source_view.as_mut() {
        view.mark_in = Some(frames + 500);
        view.mark_out = Some(frames + 900);
    }

    state.reconcile_source_view();

    let view = state.source_view.as_ref().expect("still open");
    assert_eq!(
        view.mark_in, None,
        "a silently moved in point is a wrong edit"
    );
    assert_eq!(view.mark_out, None);
}
#[test]
fn the_scrub_bar_rectangle_survives_a_relink() {
    let mut store = PoolPreviewStore::default();
    store.sync_media(1, 1);
    store.record_source_bar(10.0, 300.0);
    store.sync_media(1, 2);

    assert_eq!(store.source_bar(), Some((10.0, 300.0)));
}
#[test]
fn a_timeline_in_point_is_where_the_edit_lands() {
    let (mut state, _media, _frames) = with_monitor();
    state.playhead = aria_models::RationalTime::new(48, state.playhead.rate);
    state.set_mark_in();
    // The playhead moves on; the mark is the destination, not the playhead.
    state.playhead = aria_models::RationalTime::new(0, state.playhead.rate);

    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("three-point insert");

    assert_eq!(
        spans(&state).first().map(|(start, _)| *start),
        Some(48),
        "a three-point edit lands on the timeline in point"
    );
}
#[test]
fn a_marked_timeline_span_retimes_the_take_to_fill_it() {
    let (mut state, _media, _frames) = with_monitor();
    state.playhead = aria_models::RationalTime::new(0, state.playhead.rate);
    state.set_mark_in();
    state.playhead = aria_models::RationalTime::new(30, state.playhead.rate);
    state.set_mark_out();

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("four-point overwrite");

    assert_eq!(
        spans(&state),
        vec![(0, 30)],
        "the clip occupies the marked span exactly"
    );
    assert_eq!(state.status_message.as_deref(), Some("Fit to fill"));
}
#[test]
fn match_frame_opens_the_source_of_the_clip_under_the_playhead() {
    let (mut state, media, _frames) = with_monitor();
    // Cut a take in, then look at the middle of it from the sequence.
    state.hover_pool_item(media, 1);
    state.set_source_in();
    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("cut in");
    state.close_source_monitor();
    let (start, end) = spans(&state)[0];
    state.playhead = aria_models::RationalTime::new((start + end) / 2, state.playhead.rate);

    assert!(state.match_frame(), "the clip under the playhead is media");

    let view = state.source_view.as_ref().expect("the monitor opened");
    assert_eq!(view.media, media);
    assert_eq!(
        view.mark_in,
        Some(1),
        "the take that was used is marked, not the whole file"
    );
    assert!(view.tick >= 1, "the monitor lands inside the take");
}
#[test]
fn match_frame_says_so_when_there_is_nothing_under_the_playhead() {
    let (mut state, _media, _frames) = with_monitor();
    state.close_source_monitor();

    assert!(!state.match_frame());
    assert_eq!(
        state.status_message.as_deref(),
        Some("no media clip under the playhead")
    );
}
