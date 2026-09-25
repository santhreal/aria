//! Where a dragged pool item lands.
//!
//! The class these close: placement used to ignore the pointer entirely — first
//! lane of the right kind, playhead tick — so every drop went to the same place.
//! The rules asserted here are the two the panel now depends on: a named lane is
//! used when it can hold the media, and a lane is added only by an edge drop.
//!
//! Not covered: the pointer-to-tick conversion and the drag ghost, which need a
//! window. `px_to_ticks` is tested where it lives.

use aria_models::{MediaId, TrackId, TrackKind};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::UiTrackKind;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A project with one still imported and nothing on the timeline.
fn state() -> (AppState, MediaId) {
    let mut state = AppState::new(PluginContext::new(), scratch("mediadrop")).expect("state");
    state.new_project().expect("project");
    let media = state.import_media(fixture()).expect("import the still");
    (state, media)
}

fn lanes(state: &AppState) -> Vec<(u64, UiTrackKind, usize)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| (track.id, track.kind, track.clips.len()))
        .collect()
}

#[test]
fn a_drop_lands_on_the_lane_it_names_at_the_tick_it_names() {
    let (mut state, media) = state();
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("first placement");
    let first = lanes(&state)[0].0;
    let second = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("a second video lane");
    assert_ne!(second.raw(), first);

    state
        .drop_media_on_timeline(media, Some(second), 48)
        .expect("second placement");
    let rows = lanes(&state);
    let on_second = rows
        .iter()
        .find(|(id, _, _)| *id == second.raw())
        .expect("the lane went missing");
    assert_eq!(on_second.2, 1, "the clip did not land on the named lane");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == second.raw())
        .and_then(|track| track.clips.first())
        .expect("clip");
    assert_eq!(clip.timeline_start_ticks, 48, "the tick was not honoured");
}

#[test]
fn a_drop_never_adds_a_lane() {
    // The lane count is the assertion: a mis-aimed drop that grew the sequence
    // is the defect this rule exists for.
    let (mut state, media) = state();
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("the first drop creates the lane it needs");
    let before = lanes(&state).len();
    let target = TrackId::from_raw(lanes(&state)[0].0);
    for tick in [0, 120, 240] {
        state
            .drop_media_on_timeline(media, Some(target), tick)
            .expect("place");
    }
    assert_eq!(lanes(&state).len(), before, "a drop added a lane");
}

#[test]
fn a_lane_that_cannot_hold_the_media_is_not_used() {
    // Audio dropped on a video lane belongs on an audio lane: the time the
    // operator aimed at is worth keeping, the lane is not.
    let (mut state, media) = state();
    let video = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("a video lane");
    let audio = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("an audio lane");
    // The fixture is a still, so its kind is video: aiming it at the audio lane
    // is the same mismatch the other way round.
    state
        .drop_media_on_timeline(media, Some(audio), 0)
        .expect("place");
    let rows = lanes(&state);
    let on_audio = rows.iter().find(|(id, _, _)| *id == audio.raw()).unwrap().2;
    let on_video = rows.iter().find(|(id, _, _)| *id == video.raw()).unwrap().2;
    assert_eq!(on_audio, 0, "a still landed on an audio lane");
    assert_eq!(on_video, 1, "the still did not fall back to a video lane");
}

#[test]
fn an_edge_drop_adds_one_lane_and_places_on_it() {
    let (mut state, media) = state();
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("a lane to sit beside");
    let before = lanes(&state);
    state
        .drop_media_on_new_lane(media, true, 24)
        .expect("edge drop");
    let after = lanes(&state);
    assert_eq!(after.len(), before.len() + 1, "the edge drop added no lane");
    let new_lane = after
        .iter()
        .find(|(id, _, _)| !before.iter().any(|(old, _, _)| old == id))
        .expect("no new lane");
    assert_eq!(new_lane.2, 1, "the clip did not land on the new lane");
    assert_eq!(new_lane.1, UiTrackKind::Video);
}

#[test]
fn a_new_lane_is_named_after_how_many_of_its_kind_there_are() {
    let (mut state, _) = state();
    // A new project already ships a video lane, so the name follows the count
    // rather than assuming the sequence is empty.
    let video_before = lanes(&state)
        .iter()
        .filter(|(_, kind, _)| *kind == UiTrackKind::Video)
        .count();
    let first = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("a video lane");
    let second = state
        .add_lane_of_kind(TrackKind::Video, None)
        .expect("another video lane");
    let audio = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("an audio lane");
    let name = |id: TrackId| {
        state
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.id == id.raw())
            .map(|track| track.name.clone())
            .expect("lane")
    };
    assert_eq!(name(first), format!("V{}", video_before + 1));
    assert_eq!(name(second), format!("V{}", video_before + 2));
    assert_eq!(name(audio), "A1");
}

#[test]
fn an_edge_drop_of_unknown_media_changes_nothing() {
    let (mut state, _) = state();
    let before = lanes(&state);
    state
        .drop_media_on_new_lane(MediaId::from_raw(9_999), true, 0)
        .expect("a stale drag is not an error");
    assert_eq!(lanes(&state), before);
}

/// The window a new card settles in, and what closes it.
///
/// The class: an entry animation keyed on an element id replays whenever the
/// card re-enters the element tree — a scroll, a zoom, a lane collapse — which
/// would make "just dropped" mean "just visible". So the answer comes from the
/// edit that created the clip, and only that clip.
///
/// Not covered: expiry by wall clock, which would need the test to sleep for a
/// catalog duration. The window is `RESIZE.duration_ms()`, asserted in the motion
/// catalog tests.
#[test]
fn only_the_clip_an_edit_just_created_is_landing() {
    let (mut state, media) = state();
    assert_eq!(state.landing_clip(), None, "nothing has been created yet");

    state
        .drop_media_on_timeline(media, None, 0)
        .expect("placement");
    let first = state.store.sequence.tracks[0].clips[0].id;
    assert_eq!(
        state.landing_clip(),
        Some(first),
        "the dropped clip is the one that settles"
    );

    state
        .drop_media_on_timeline(media, None, 480)
        .expect("second placement");
    let ids: Vec<u64> = state.store.sequence.tracks[0]
        .clips
        .iter()
        .map(|clip| clip.id)
        .collect();
    let second = *ids.iter().find(|id| **id != first).expect("a second clip");
    assert_eq!(
        state.landing_clip(),
        Some(second),
        "the newest clip lands, and only one card settles at a time"
    );
}

/// An edit that creates nothing leaves the landing record alone rather than
/// clearing it: a nudge on the clip that is still settling must not cut its
/// animation short.
#[test]
fn an_edit_that_creates_nothing_does_not_disturb_a_settling_clip() {
    let (mut state, media) = state();
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("placement");
    let clip = state.store.sequence.tracks[0].clips[0].id;
    assert_eq!(state.landing_clip(), Some(clip));

    state.select_clip(aria_models::ClipId::from_raw(clip));
    state.set_playhead(120);
    assert_eq!(
        state.landing_clip(),
        Some(clip),
        "a selection and a playhead move are not edits"
    );
}
