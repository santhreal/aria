//! Contracts for per-lane display options: height, waveform, filmstrip.
//!
//! The class closed here is "a view option is kept in the session, so reopening
//! the project throws the operator's timeline layout away". Every assertion goes
//! through the projected store — the panel draws from the projection — and the
//! persistence case round-trips a save and an open, because that is the failure
//! the class is about.
//!
//! Not covered: the drawn grip and the pointer arithmetic of the drag itself,
//! which need a render surface. The preview/commit split is covered through
//! `set_lane_preview` / `take_lane_preview`, which is the state the drag drives.

use aria_models::TrackDisplay;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::UiTrack;

fn lane(state: &AppState, id: u64) -> &UiTrack {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == id)
        .expect("lane still projected")
}

/// The lane the fixture clip sits on.
fn fixture_lane(state: &AppState) -> u64 {
    state.store.sequence.tracks[0].id
}

#[test]
fn a_new_lane_ships_with_the_default_display() {
    let (state, _clip) = with_clip();
    let track = fixture_lane(&state);
    let display = state.lane_display(track);
    assert_eq!(display.height, TrackDisplay::DEFAULT_HEIGHT);
    assert!(display.waveform, "waveforms are on until turned off");
    assert!(display.filmstrip, "filmstrips are on until turned off");
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32
    );
}

#[test]
fn a_height_change_is_projected_and_undoable() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_height(track, 96.0).expect("resize");
    assert_eq!(lane(&state, track).height, 96.0);
    assert!(state.undo(), "a resize is an edit, so it undoes");
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32
    );
    assert!(state.redo());
    assert_eq!(lane(&state, track).height, 96.0);
}

#[test]
fn a_height_outside_the_range_is_clamped_rather_than_refused() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state
        .set_lane_height(track, 4.0)
        .expect("a tiny drag lands");
    assert_eq!(
        lane(&state, track).height,
        *TrackDisplay::HEIGHT.start() as f32
    );
    state
        .set_lane_height(track, 4000.0)
        .expect("a huge drag lands");
    assert_eq!(
        lane(&state, track).height,
        *TrackDisplay::HEIGHT.end() as f32
    );
}

#[test]
fn one_flag_change_leaves_the_others_alone() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_height(track, 72.0).expect("resize");
    state.set_lane_waveform(track, false).expect("hide waves");
    let display = state.lane_display(track);
    assert_eq!(display.height, 72, "a flag toggle must not resize the lane");
    assert!(!display.waveform);
    assert!(display.filmstrip, "the other flag is untouched");
    state.set_lane_filmstrip(track, false).expect("hide strips");
    assert!(!state.lane_display(track).filmstrip);
    assert!(!state.lane_display(track).waveform, "still off");
    assert_eq!(state.lane_display(track).height, 72);
}

#[test]
fn a_locked_lane_still_takes_a_display_change() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state
        .set_track_locked(aria_models::TrackId::from_raw(track), true)
        .expect("lock");
    state
        .set_lane_height(track, 120.0)
        .expect("how a lane is drawn is not an edit to its clips");
    assert_eq!(lane(&state, track).height, 120.0);
}

#[test]
fn the_layout_survives_save_and_open() {
    let dir = crate::scratch_pool::scratch("lane-display");
    let path = dir.join("layout.aria");
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_height(track, 88.0).expect("resize");
    state.set_lane_filmstrip(track, false).expect("hide strips");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");

    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let track = fixture_lane(&state);
    let display = state.lane_display(track);
    assert_eq!(
        display.height, 88,
        "the lane height is part of the sequence"
    );
    assert!(!display.filmstrip, "so is the strip choice");
    assert!(display.waveform);
}

#[test]
fn a_resize_in_flight_is_a_preview_and_costs_one_undo_entry() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    let depth = state.history_depth();
    for y in [60.0, 72.0, 84.0] {
        state.set_lane_preview(track, y);
    }
    assert_eq!(state.lane_preview(track), Some(84.0));
    assert_eq!(state.lane_preview(track + 1), None, "one lane at a time");
    assert_eq!(
        state.history_depth(),
        depth,
        "a preview must not reach the engine"
    );
    let height = state.take_lane_preview().expect("the drag ended somewhere");
    state.set_lane_height(track, height).expect("commit");
    assert_eq!(state.lane_preview(track), None);
    assert_eq!(lane(&state, track).height, 84.0);
    assert_eq!(
        state.history_depth(),
        depth + 1,
        "the whole gesture is one edit"
    );
}

#[test]
fn an_abandoned_preview_changes_nothing() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_preview(track, 150.0);
    assert!(state.take_lane_preview().is_some());
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32,
        "escaping a resize leaves the lane as it was"
    );
}

#[test]
fn a_preview_is_clamped_to_the_accepted_range() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_preview(track, -40.0);
    assert_eq!(
        state.lane_preview(track),
        Some(*TrackDisplay::HEIGHT.start() as f32),
        "the preview never shows a height the engine would refuse"
    );
    state.set_lane_preview(track, 9_000.0);
    assert_eq!(
        state.lane_preview(track),
        Some(*TrackDisplay::HEIGHT.end() as f32)
    );
}

#[test]
fn collapsing_keeps_the_height_it_will_expand_back_to() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_height(track, 110.0).expect("resize");
    state.toggle_lane_collapsed(track).expect("collapse");
    let display = state.lane_display(track);
    assert!(display.collapsed);
    assert_eq!(
        display.height, 110,
        "a collapsed lane keeps the height the operator gave it"
    );
    state.toggle_lane_collapsed(track).expect("expand");
    let display = state.lane_display(track);
    assert!(!display.collapsed);
    assert_eq!(display.height, 110, "expanding restores that height");
}

#[test]
fn a_collapse_is_projected_undoable_and_persisted() {
    let dir = crate::scratch_pool::scratch("lane-collapse");
    let path = dir.join("collapsed.aria");
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&mut state);
    state.set_lane_collapsed(track, true).expect("collapse");
    assert!(lane(&state, track).collapsed, "the panel draws from this");
    assert!(state.undo());
    assert!(!lane(&state, track).collapsed);
    assert!(state.redo());

    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let track = fixture_lane(&state);
    assert!(
        state.lane_display(track).collapsed,
        "a collapsed lane comes back collapsed"
    );
}

#[test]
fn collapsing_one_lane_leaves_the_others_expanded() {
    let (mut state, _clip) = with_clip();
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("a second lane");
    let rows: Vec<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.id)
        .collect();
    assert!(rows.len() >= 2, "the fixture has two lanes to tell apart");
    state.set_lane_collapsed(rows[0], true).expect("collapse");
    assert!(lane(&state, rows[0]).collapsed);
    for other in &rows[1..] {
        assert!(
            !lane(&state, *other).collapsed,
            "lane {other} was collapsed by proxy"
        );
    }
}
