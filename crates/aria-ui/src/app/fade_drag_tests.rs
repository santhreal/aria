//! Contracts for the fade handles on a clip card.
//!
//! The class closed here is "a fade can only be typed into the inspector, so it
//! belongs to the selected clip". A dragged handle addresses the clip it sits
//! on, clamps to that clip's length instead of being refused, and costs one
//! undo entry per gesture.
//!
//! Not covered: the mouse-down/mouse-up pair itself, which needs a render
//! surface. `panels::timeline::clip::finish_fade` turns a pixel delta into the
//! tick value passed here and does nothing else.

use aria_models::ClipId;

use super::AppState;
use super::premiere_tests::with_clip;

fn fades(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("the clip is projected");
    (ui.fade_in_ticks, ui.fade_out_ticks)
}

fn duration(state: &AppState, clip: ClipId) -> i64 {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("the clip is projected");
    ui.timeline_end_ticks - ui.timeline_start_ticks
}

#[test]
fn a_dragged_handle_sets_the_fade_on_that_clip() {
    let (mut state, clip) = with_clip();
    assert_eq!(fades(&state, clip), (0, 0));

    state.set_clip_fade_in(clip.raw(), 18).expect("fade in");
    assert_eq!(fades(&state, clip), (18, 0));

    state.set_clip_fade_out(clip.raw(), 24).expect("fade out");
    assert_eq!(fades(&state, clip), (18, 24));
}

#[test]
fn a_handle_works_on_an_unselected_clip() {
    let (mut state, clip) = with_clip();
    state.clear_selection();
    assert!(state.selected_ui_clip().is_none());

    state.set_clip_fade_in(clip.raw(), 10).expect("fade in");
    assert_eq!(fades(&state, clip), (10, 0));
}

#[test]
fn one_fade_leaves_the_other_alone() {
    let (mut state, clip) = with_clip();
    state.set_clip_fade_in(clip.raw(), 30).expect("fade in");
    state.set_clip_fade_out(clip.raw(), 5).expect("fade out");
    state.set_clip_fade_in(clip.raw(), 12).expect("fade in");
    assert_eq!(fades(&state, clip), (12, 5));
}

#[test]
fn a_drag_past_the_end_stalls_at_the_whole_clip() {
    let (mut state, clip) = with_clip();
    let len = duration(&state, clip);
    state
        .set_clip_fade_in(clip.raw(), len + 500)
        .expect("clamped, not refused");
    assert_eq!(fades(&state, clip).0, len);
}

#[test]
fn a_drag_past_the_start_stalls_at_zero() {
    let (mut state, clip) = with_clip();
    state.set_clip_fade_in(clip.raw(), 40).expect("fade in");
    state
        .set_clip_fade_in(clip.raw(), -900)
        .expect("clamped, not refused");
    assert_eq!(fades(&state, clip).0, 0);
}

#[test]
fn a_gesture_is_one_undo_entry() {
    let (mut state, clip) = with_clip();
    let before = state.history_depth();
    state.set_clip_fade_in(clip.raw(), 20).expect("fade in");
    assert_eq!(state.history_depth(), before + 1);

    assert!(state.undo());
    assert_eq!(fades(&state, clip), (0, 0));
    assert!(state.redo());
    assert_eq!(fades(&state, clip), (20, 0));
}

#[test]
fn a_drag_that_lands_where_it_started_costs_nothing() {
    let (mut state, clip) = with_clip();
    state.set_clip_fade_in(clip.raw(), 20).expect("fade in");
    let depth = state.history_depth();
    state.set_clip_fade_in(clip.raw(), 20).expect("no-op");
    assert_eq!(state.history_depth(), depth);
}

#[test]
fn a_handle_on_a_clip_that_is_gone_does_nothing() {
    let (mut state, clip) = with_clip();
    let depth = state.history_depth();
    state
        .set_clip_fade_in(clip.raw() + 9_999, 20)
        .expect("inert");
    assert_eq!(state.history_depth(), depth);
}

#[test]
fn a_dragged_fade_survives_save_and_open() {
    let (mut state, clip) = with_clip();
    state.set_clip_fade_in(clip.raw(), 16).expect("fade in");
    state.set_clip_fade_out(clip.raw(), 9).expect("fade out");

    let dir = crate::scratch_pool::scratch("fade-drag");
    let path = dir.join("fades.aria");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.fade_in_ticks == 16)
        .expect("the faded clip came back");
    assert_eq!(ui.fade_out_ticks, 9);
}
