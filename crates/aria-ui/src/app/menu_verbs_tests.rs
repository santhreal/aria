//! What the menu bar's own verbs change, read back off the state each row acts
//! on: the edit point Trim Edit selects, where Extend Edit moves it, the source
//! window Replace with Clip writes, the lanes Delete Empty Tracks keeps, and the
//! easing each Keyframe Interpolation row leaves.
//!
//! Each edit is also checked to be one undo step. What the rows draw, and
//! whether a row is offered at all, is covered by `menu_reach_tests`.

use std::path::PathBuf;

use aria_models::{ClipId, ClipParam, Easing, TemporalEase, TrackKind};

use super::KeyInterpolation;
use crate::app::AppState;
use crate::app::premiere_tests::{with_clip, with_media};
use crate::types::{SelectedEdge, TrimSide, UiTrackKind};

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

fn lanes(state: &AppState, kind: UiTrackKind) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == kind)
        .count()
}

/// The clip that starts at `start`.
fn clip_at(state: &AppState, start: i64) -> ClipId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|clip| clip.timeline_start_ticks == start)
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("a clip starts there")
}

#[test]
fn trim_edit_selects_the_edit_point_nearest_the_playhead() {
    let (mut state, clip) = with_clip();
    let (start, end) = span(&state, clip);
    assert!(
        end - start > 2,
        "the fixture clip is too short to tell its edges apart"
    );
    for (at, side) in [(start + 1, TrimSide::In), (end - 1, TrimSide::Out)] {
        state.set_playhead(at);
        state.select_nearest_edit();
        assert_eq!(
            state.selected_edge,
            Some(SelectedEdge {
                clip: clip.raw(),
                side
            }),
            "playhead at {at}"
        );
    }
}

#[test]
fn extend_edit_moves_the_selected_out_point_to_the_playhead_in_one_step() {
    let (mut state, clip) = with_clip();
    let (start, end) = span(&state, clip);
    let mid = start + (end - start) / 2;
    state.set_playhead(mid);
    state.select_edge(clip, TrimSide::Out);

    state
        .extend_edit_to_playhead()
        .expect("the edit point moves");

    assert_eq!(span(&state, clip), (start, mid));
    assert!(state.undo());
    assert_eq!(span(&state, clip), (start, end));
}

#[test]
fn extend_edit_without_a_selected_edit_point_changes_nothing() {
    let (mut state, clip) = with_clip();
    let before = span(&state, clip);
    state.set_playhead(before.0 + 1);
    state.selected_edge = None;
    let steps = state.engine.lock().history_entries().len();

    state.extend_edit_to_playhead().expect("nothing to move");

    assert_eq!(span(&state, clip), before);
    assert_eq!(state.engine.lock().history_entries().len(), steps);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Select an edit point first.")
    );
}

#[test]
fn delete_empty_tracks_keeps_lanes_with_clips_and_one_lane_of_each_kind() {
    let (mut state, _) = with_clip();
    for (kind, name) in [
        (TrackKind::Video, "V2"),
        (TrackKind::Video, "V3"),
        (TrackKind::Audio, "A2"),
    ] {
        state.add_track(kind, name).expect("a lane");
    }
    state.sync_from_engine();
    let total = state.store.sequence.tracks.len();

    state.delete_empty_tracks().expect("the empty lanes go");

    assert_eq!(
        lanes(&state, UiTrackKind::Video),
        1,
        "the lane holding the clip stays, the empty ones go"
    );
    assert_eq!(
        lanes(&state, UiTrackKind::Audio),
        1,
        "one audio lane stays for sound to land on"
    );
    assert!(state.empty_track_ids().is_empty());
    assert!(state.undo(), "the deletion is one step");
    assert_eq!(state.store.sequence.tracks.len(), total);
}

#[test]
fn keyframe_interpolation_rows_each_write_their_easing_as_one_step() {
    let (mut state, clip) = with_clip();
    let (start, end) = span(&state, clip);
    let at = start + (end - start) / 2;
    state.set_playhead(at);
    state
        .set_keyframe_scalar(ClipParam::Opacity, at, 0.5)
        .expect("a keyframe under the playhead");
    let easing = |state: &AppState| {
        state
            .keyframe_easing_at(ClipParam::Opacity, at)
            .expect("the keyframe")
    };

    state
        .set_keyframe_interpolation(KeyInterpolation::Bezier)
        .expect("bezier");
    assert_eq!(easing(&state).in_ease(), Some(TemporalEase::EASY_EASE));
    assert_eq!(easing(&state).out_ease(), Some(TemporalEase::EASY_EASE));

    state
        .set_keyframe_interpolation(KeyInterpolation::Hold)
        .expect("hold");
    assert!(easing(&state).is_hold());
    // Hold sets, where Toggle Hold would release a held keyframe.
    state
        .set_keyframe_interpolation(KeyInterpolation::Hold)
        .expect("hold again");
    assert!(easing(&state).is_hold());

    state
        .set_keyframe_interpolation(KeyInterpolation::Linear)
        .expect("linear");
    assert_eq!(easing(&state), Easing::Linear);
    assert!(state.undo());
    assert!(easing(&state).is_hold(), "Linear was one step");
}

fn picture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4")
        .canonicalize()
        .expect("the fixture exists")
}

/// The source frame under the Source playhead lands on the sequence frame
/// under the sequence playhead, and the clip keeps its place and its length.
#[test]
fn replace_with_source_lines_the_source_frame_up_with_the_sequence_playhead() {
    let (mut state, whole) = with_media(picture());
    let (start, end) = span(&state, whole);
    // A third of the source, so the new window has media on both sides.
    state.set_playhead(start + (end - start) / 3);
    state.split_at_playhead().expect("cut the clip");
    let left = clip_at(&state, start);
    let (start, end) = span(&state, left);
    let ui = state.ui_clip_ref(left).expect("the left part");
    let length = ui.source_end_ticks - ui.source_start_ticks;
    let media = ui.media_id.expect("a media clip");

    assert!(state.open_source(media), "the source opens in the monitor");
    state
        .sources
        .focused_mut()
        .expect("the Source tab is focused")
        .tick = length;
    // Program on screen: the row takes the Source tab focused last.
    state.show_program();
    state.select_clip(left);
    let mid = start + (end - start) / 2;
    state.set_playhead(mid);

    state.replace_with_source().expect("the clip is repointed");

    let offset = length * (mid - start) / (end - start);
    let ui = state.ui_clip_ref(left).expect("the clip is still there");
    assert_eq!(
        (ui.source_start_ticks, ui.source_end_ticks),
        (length - offset, 2 * length - offset)
    );
    assert_eq!(span(&state, left), (start, end));
    assert!(state.undo());
    let ui = state.ui_clip_ref(left).expect("the clip is still there");
    assert_eq!((ui.source_start_ticks, ui.source_end_ticks), (0, length));
}

/// A window that would run off the end of the source is refused with a
/// reason, and the clip is left as it was.
#[test]
fn replace_with_source_refuses_a_window_past_the_end_of_the_source() {
    let (mut state, clip) = with_media(picture());
    let (start, _) = span(&state, clip);
    let ui = state.ui_clip_ref(clip).expect("the clip");
    let before = (ui.source_start_ticks, ui.source_end_ticks);
    let media = ui.media_id.expect("a media clip");
    assert!(state.open_source(media));
    // The clip spans the whole source, so any start but zero overruns it.
    state.sources.focused_mut().expect("the Source tab").tick = 1;
    state.select_clip(clip);
    state.set_playhead(start);

    state.replace_with_source().expect("refused, not failed");

    let ui = state.ui_clip_ref(clip).expect("the clip");
    assert_eq!((ui.source_start_ticks, ui.source_end_ticks), before);
    assert_eq!(
        state.status_message.as_deref(),
        Some("The source does not hold enough media around its playhead to fill the clip.")
    );
}

#[test]
fn remove_all_effects_takes_every_effect_off_in_one_step() {
    let (mut state, clip) = with_clip();
    let effect = aria_models::effect_catalog()
        .iter()
        .next()
        .map(|spec| spec.id.clone())
        .expect("an effect is registered");
    for _ in 0..2 {
        state
            .add_effect_to_selected(effect.clone())
            .expect("the effect lands");
    }
    assert_eq!(state.last_effect.as_deref(), Some(effect.as_str()));
    assert_eq!(state.ui_clip_ref(clip).expect("the clip").effects.len(), 2);

    state
        .remove_all_effects_selected()
        .expect("the effects come off");
    assert!(
        state
            .ui_clip_ref(clip)
            .expect("the clip")
            .effects
            .is_empty()
    );

    assert!(state.undo(), "one step");
    assert_eq!(state.ui_clip_ref(clip).expect("the clip").effects.len(), 2);

    state.apply_last_effect().expect("the last effect again");
    assert_eq!(state.ui_clip_ref(clip).expect("the clip").effects.len(), 3);
}
