//! Source view reconciliation and In/Out mark tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `pool_preview_tests.rs` through `use super::*`.

use super::*;

#[test]
fn a_mark_past_a_shortened_source_is_dropped_rather_than_moved() {
    let (mut state, _media, frames) = with_monitor();
    if let Some(view) = state.sources.focused_mut() {
        view.mark_in = Some(frames + 500);
        view.mark_out = Some(frames + 900);
    }

    state.reconcile_source_view();

    let view = state.sources.focused().expect("still open");
    assert_eq!(
        view.mark_in, None,
        "a silently moved in point is a wrong edit"
    );
    assert_eq!(view.mark_out, None);
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
fn four_points_retime_the_take_to_fill_the_marked_span() {
    let (mut state, media) = with_video_monitor();
    state.hover_pool_item(media, 0);
    state.set_source_in();
    state.hover_pool_item(media, 47);
    state.set_source_out();
    state.playhead = aria_models::RationalTime::new(0, state.playhead.rate);
    state.set_mark_in();
    state.playhead = aria_models::RationalTime::new(24, state.playhead.rate);
    state.set_mark_out();

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("four-point overwrite");

    assert_eq!(
        spans(&state),
        vec![(0, 24), (0, 24)],
        "picture and sound both occupy the marked span exactly"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Fit to fill at 200% speed"),
        "forty-eight source frames in twenty-four sequence frames play at double speed"
    );
}
#[test]
fn a_source_in_and_a_marked_span_take_the_span_from_the_source() {
    let (mut state, media) = with_video_monitor();
    state.hover_pool_item(media, 10);
    state.set_source_in();
    state.playhead = aria_models::RationalTime::new(4, state.playhead.rate);
    state.set_mark_in();
    state.playhead = aria_models::RationalTime::new(24, state.playhead.rate);
    state.set_mark_out();

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("three-point overwrite");

    assert_eq!(spans(&state), vec![(4, 24), (4, 24)]);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Overwrite"),
        "no retime"
    );
    let sources: Vec<i64> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.source_start_ticks)
        .collect();
    assert_eq!(
        sources,
        vec![10, 10],
        "both channels start at the source In"
    );
}
#[test]
fn a_still_takes_the_marked_span_without_a_retime() {
    let (mut state, media, _frames) = with_monitor();
    state.hover_pool_item(media, 0);
    state.set_source_in();
    state.hover_pool_item(media, 1);
    state.set_source_out();
    state.playhead = aria_models::RationalTime::new(0, state.playhead.rate);
    state.set_mark_in();
    state.playhead = aria_models::RationalTime::new(30, state.playhead.rate);
    state.set_mark_out();

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("four-point overwrite of a still");

    assert_eq!(spans(&state), vec![(0, 30)], "one picture, no sound");
    assert_eq!(state.status_message.as_deref(), Some("Overwrite"));
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
    state.show_program();
    let (start, end) = spans(&state)[0];
    state.playhead = aria_models::RationalTime::new((start + end) / 2, state.playhead.rate);

    assert!(state.match_frame(), "the clip under the playhead is media");

    let view = state.sources.focused().expect("the monitor opened");
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
    state.show_program();

    assert!(!state.match_frame());
    assert_eq!(
        state.status_message.as_deref(),
        Some("No media clip under the playhead")
    );
}
