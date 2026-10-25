//! Contracts for the clipboard and for zoom fitting.
//!
//! The class closed here is "a paste variant exists as a menu label but does the
//! same thing as plain paste". Each assertion reads the projected store after the
//! edit, because that is what the timeline draws: Insert has to be visible as a
//! *moved* neighbour, Attributes as a changed look on an *unmoved* clip.
//!
//! Fitting is asserted as a relation between two zooms rather than an absolute
//! number, so the test survives a change to the chrome widths it divides by.
//!
//! Not covered: the drawn rows, and dragging the playhead — both need a render
//! surface.

use aria_models::{ClipId, ClipParam, MediaId, TrackId};

use super::AppState;
use super::premiere_tests::with_clip;

fn clips(state: &AppState) -> Vec<(u64, i64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|c| (c.id, c.timeline_start_ticks, c.timeline_end_ticks))
        .collect()
}

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    clips(state)
        .into_iter()
        .find(|(id, _, _)| *id == clip.raw())
        .map(|(_, start, end)| (start, end))
        .expect("clip still projected")
}

/// The fixture clip's lane and media, for a second placement on the same lane.
fn lane_and_media(state: &AppState, clip: ClipId) -> (TrackId, MediaId) {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("fixture clip");
    (
        TrackId::from_raw(ui.track_id),
        MediaId::from_raw(ui.media_id.expect("fixture carries media")),
    )
}

/// Two clips packed on one lane: the fixture, then a copy of the same media
/// directly after it.
fn packed() -> (AppState, ClipId, ClipId) {
    let (mut state, first) = with_clip();
    let (track, media) = lane_and_media(&state, first);
    let (_, end) = span(&state, first);
    state
        .drop_media_on_timeline(media, Some(track), end)
        .expect("place the neighbor");
    let second = clips(&state)
        .into_iter()
        .map(|(id, _, _)| id)
        .find(|id| *id != first.raw())
        .map(ClipId::from_raw)
        .expect("two clips on the lane");
    (state, first, second)
}

#[test]
fn plain_paste_lands_at_the_playhead_on_the_source_lane() {
    let (mut state, clip) = with_clip();
    let (_, end) = span(&state, clip);
    state.copy_selected();
    // The playhead clamps to the sequence end, so the paste target is whatever
    // the transport settled on rather than the tick asked for.
    state.set_playhead(end * 4);
    let at = state.playhead.value;
    state.paste_clipboard().expect("paste");
    let starts: Vec<i64> = clips(&state)
        .into_iter()
        .map(|(_, start, _)| start)
        .collect();
    assert!(
        starts.contains(&at),
        "pasted at the playhead {at}: {starts:?}"
    );
    assert_eq!(clips(&state).len(), 2);
    assert_eq!(span(&state, clip), (0, end), "the source did not move");
}

#[test]
fn paste_insert_opens_a_hole_instead_of_overwriting() {
    let (mut state, first, second) = packed();
    let (start, end) = span(&state, first);
    let duration = end - start;
    let (second_start, _) = span(&state, second);
    state.select_clip(first);
    state.copy_selected();
    state.set_playhead(second_start);
    state.paste_insert().expect("paste insert");
    assert_eq!(clips(&state).len(), 3, "the copy is a third clip");
    assert_eq!(
        span(&state, second).0,
        second_start + duration,
        "the neighbor rippled right by the copy's length"
    );
    assert_eq!(span(&state, first), (start, end), "the source did not move");
    let pasted = clips(&state)
        .into_iter()
        .find(|(id, _, _)| *id != first.raw() && *id != second.raw())
        .expect("the pasted clip");
    assert_eq!(
        (pasted.1, pasted.2),
        (second_start, second_start + duration),
        "the copy fills the hole exactly"
    );
}

#[test]
fn paste_insert_is_inert_with_an_empty_clipboard() {
    let (mut state, _) = with_clip();
    let before = clips(&state);
    state.paste_insert().expect("inert");
    assert_eq!(clips(&state), before);
}

#[test]
fn paste_attributes_copies_the_look_and_leaves_timing_alone() {
    let (mut state, first, second) = packed();
    // Give the source a look: a scaled, half-opaque transform, a filter, and one
    // effect.
    state.select_clip(first);
    let mut transform = state.selected_ui_clip().expect("source").transform;
    transform.scale_x = 1.5;
    transform.opacity = 0.5;
    state.set_selected_transform(transform).expect("transform");
    let (source_start, source_end) = span(&state, first);
    for tick in [source_start + 1, source_end - 1] {
        state.set_playhead(tick);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("source opacity keyframe");
    }
    state.set_selected_filter("warm").expect("filter");
    state.set_selected_filter_amount(0.75).expect("amount");
    let effect = state
        .effect_browse_list()
        .first()
        .map(|(spec, _)| spec.id.clone())
        .expect("the catalog ships an effect");
    state.add_effect_to_selected(&effect).expect("effect");
    state.copy_selected();

    let (start, end) = span(&state, second);
    state.select_clip(second);
    let before = state.selected_ui_clip().expect("target before paste");
    let projections = state.projections;
    let history = state.history_depth();
    state.paste_attributes().expect("paste attributes");

    let target = state.selected_ui_clip().expect("target");
    assert!((target.transform.scale_x - 1.5).abs() < 1e-4);
    assert!((target.transform.opacity - 0.5).abs() < 1e-4);
    assert_eq!(
        target.filter.as_ref().map(|f| f.id.as_str()),
        Some("warm"),
        "the filter came across"
    );
    assert!((target.filter.as_ref().expect("filter").amount - 0.75).abs() < 1e-3);
    assert_eq!(
        target
            .effects
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        vec![effect.as_str()],
        "the chain came across"
    );
    assert_eq!(
        state.keyframes_of(ClipParam::Opacity).len(),
        2,
        "the complete animated transform came across"
    );
    assert_eq!(
        span(&state, second),
        (start, end),
        "timing is not an attribute"
    );
    assert_eq!(
        state.projections,
        projections + 1,
        "one attribute paste must produce one projection"
    );
    assert_eq!(
        state.history_depth(),
        history + 1,
        "one attribute paste must produce one undo entry"
    );
    assert!(state.undo(), "the attribute transaction must undo");
    assert_eq!(
        state.selected_ui_clip().expect("target after undo"),
        before,
        "one undo must restore every target attribute and its timing"
    );
    assert!(
        state.keyframes_of(ClipParam::Opacity).is_empty(),
        "undo left pasted transform keyframes behind"
    );
}

#[test]
fn paste_attributes_replaces_the_target_chain_rather_than_appending() {
    let (mut state, first, second) = packed();
    let effect = state
        .effect_browse_list()
        .first()
        .map(|(spec, _)| spec.id.clone())
        .expect("the catalog ships an effect");
    state.select_clip(second);
    state
        .add_effect_to_selected(&effect)
        .expect("target effect");
    state
        .add_effect_to_selected(&effect)
        .expect("target effect");
    state.select_clip(first);
    state.copy_selected();
    state.select_clip(second);
    state.paste_attributes().expect("paste attributes");
    assert!(
        state.selected_ui_clip().expect("target").effects.is_empty(),
        "an empty source chain empties the target"
    );
}

#[test]
fn paste_attributes_onto_the_source_is_inert() {
    let (mut state, clip) = with_clip();
    state.copy_selected();
    let mut transform = state.selected_ui_clip().expect("clip").transform;
    transform.opacity = 0.25;
    state.set_selected_transform(transform).expect("transform");
    state.select_clip(clip);
    let before = state.selected_ui_clip().expect("clip");
    state.paste_attributes().expect("inert");
    assert_eq!(state.selected_ui_clip().expect("clip"), before);
}

#[test]
fn fitting_a_selection_zooms_in_further_than_fitting_the_sequence() {
    let (mut state, first, _) = packed();
    state.last_window_w = 1440.0;
    state.zoom_fit_sequence();
    let whole = state.zoom;
    state.select_clip(first);
    state.zoom_fit_selection();
    assert!(
        state.zoom > whole,
        "a clip is shorter than the sequence: {} vs {whole}",
        state.zoom
    );
    assert_eq!(state.store.zoom, state.zoom, "the store followed");
}

#[test]
fn fitting_a_selection_with_nothing_selected_is_inert() {
    let (mut state, _, _) = packed();
    state.last_window_w = 1440.0;
    state.zoom_fit_sequence();
    let whole = state.zoom;
    state.clear_selection();
    assert_eq!(state.selection_span(), None);
    state.zoom_fit_selection();
    assert_eq!(state.zoom, whole);
}
