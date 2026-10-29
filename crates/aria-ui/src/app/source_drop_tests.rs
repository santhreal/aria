//! Where a dropped shot's picture and sound land.
//!
//! The class these close: a drop places a source through the same patching as
//! a Source tab's edits, so a shot with sound lands as linked picture and
//! sound, on a new audio lane when the sequence has none, with that lane and
//! the clips one undo step; puts the channel of the lane under the pointer on
//! that lane; and leaves out a channel patched off. A drag from the Source tab
//! drops its marked range, covering unless Ctrl is held.
//!
//! Not covered: the pointer-to-lane hit test and the drag ghost, which need a
//! window. Lane resolution on `Auto` is asserted in `source_patch_tests`.

use aria_commands::{Patch, SourceChannel};
use aria_models::{MediaId, TrackId, TrackKind};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::{ActivePanel, UiClip};

/// The 24 fps shot with sound.
fn shot() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4")
}

/// A new sequence (one video lane) with the 24 fps shot with sound in the pool.
fn state() -> (AppState, MediaId, TrackId) {
    let mut state = AppState::new(PluginContext::new(), scratch("sourcedrop")).expect("state");
    state.new_project().expect("project");
    let media = state.import_media(shot()).expect("import the shot");
    let picture = TrackId::from_raw(state.store.sequence.tracks[0].id);
    (state, media, picture)
}

/// Open `media` in a Source tab with source ticks 12..23 marked.
fn open_marked(state: &mut AppState, media: MediaId) {
    state.select_pool_item(media.raw());
    state.active_panel = ActivePanel::Media;
    assert!(state.toggle_source_monitor(), "the monitor opens");
    state.hover_pool_item(media.raw(), 12);
    state.set_source_in();
    state.hover_pool_item(media.raw(), 23);
    state.set_source_out();
}

fn clips(state: &AppState, lane: TrackId) -> Vec<&UiClip> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == lane.raw())
        .expect("the lane")
        .clips
        .iter()
        .collect()
}

fn spans(state: &AppState, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = clips(state, lane)
        .iter()
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
}

/// The audio lanes, top first.
fn audio_lanes(state: &AppState) -> Vec<TrackId> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.channel == Some(SourceChannel::Audio))
        .map(|lane| TrackId::from_raw(lane.id))
        .collect()
}

/// Every way a source reaches the timeline, from a sequence with no audio
/// lane: the sound lands on a new audio lane, linked to the picture, and one
/// Undo, named for the edit, takes the clips and the lane back together.
///
/// Closes the class where a placement kept the sound inside the picture when
/// the sequence had no audio lane, and where a placement added the lane it
/// needed as an undo step of its own, so Undo took the clips back and left an
/// empty lane. The Source tab's Insert and Overwrite buttons are asserted the
/// same way in the monitor census. Not covered: a patch that names a lane,
/// which `a_channel_patched_off_is_left_out_of_a_drop` and the monitor census
/// assert.
#[test]
fn every_placement_adds_the_sound_lane_in_its_own_undo_step() {
    type Place = fn(&mut AppState, MediaId, TrackId);
    let entries: [(&str, Place); 4] = [
        ("a pool item dropped on a lane", |state, media, picture| {
            state
                .drop_media_on_timeline(media, Some(picture), 0)
                .expect("drop");
        }),
        ("a Source tab dragged to a lane", |state, media, picture| {
            open_marked(state, media);
            state
                .drop_source_on_timeline(Some(picture), 0, false)
                .expect("drag");
        }),
        ("Overwrite on selected pool items", |state, media, _| {
            state.select_pool_item(media.raw());
            state
                .place_pool_selection(crate::types::EditMode::Overwrite)
                .expect("overwrite");
        }),
        ("a file dropped from the desktop", |state, _, _| {
            state.import_dropped_paths(vec![shot()]).expect("file drop");
        }),
    ];
    for (what, place) in entries {
        let (mut state, media, picture) = state();
        assert!(audio_lanes(&state).is_empty(), "{what}: a new sequence");
        place(&mut state, media, picture);

        let lanes = audio_lanes(&state);
        assert_eq!(lanes.len(), 1, "{what} creates one audio lane");
        let (shown, heard) = (clips(&state, picture), clips(&state, lanes[0]));
        assert_eq!((shown.len(), heard.len()), (1, 1), "{what}");
        assert!(
            shown[0].linked_to.is_some(),
            "{what}: picture and sound link"
        );
        assert_eq!(shown[0].linked_to, heard[0].linked_to, "{what}");
        assert_eq!(
            (shown[0].timeline_start_ticks, shown[0].timeline_end_ticks),
            (heard[0].timeline_start_ticks, heard[0].timeline_end_ticks),
            "{what}: the sound spans the picture"
        );
        assert_eq!(
            state
                .history_entries()
                .last()
                .map(|step| step.label.clone()),
            Some("Source edit".to_string()),
            "{what}: the undo step is named for the edit"
        );

        assert!(state.undo(), "{what}: Undo");
        assert!(
            clips(&state, picture).is_empty(),
            "{what}: Undo keeps a clip"
        );
        assert!(
            audio_lanes(&state).is_empty(),
            "{what}: Undo keeps the lane the edit added"
        );
    }
}

#[test]
fn an_audio_lane_takes_the_sound_as_a_linked_clip() {
    let (mut state, media, picture) = state();
    let sound = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("an audio lane");
    state
        .drop_media_on_timeline(media, Some(picture), 24)
        .expect("drop");

    assert_eq!(spans(&state, picture), vec![(24, 72)]);
    assert_eq!(spans(&state, sound), vec![(24, 72)]);
    let (shown, heard) = (clips(&state, picture)[0], clips(&state, sound)[0]);
    assert!(shown.linked_to.is_some(), "picture and sound are linked");
    assert_eq!(shown.linked_to, heard.linked_to);
    let shown = aria_models::ClipId::from_raw(shown.id);
    assert!(
        state
            .engine
            .lock()
            .project()
            .timeline()
            .detached_to_audio_lane(shown),
        "the picture is silent; its sound plays once, from the audio lane"
    );
}

#[test]
fn the_lane_under_the_pointer_takes_its_channel() {
    let (mut state, media, picture) = state();
    let first = state.add_lane_of_kind(TrackKind::Audio, None).expect("A1");
    let second = state.add_lane_of_kind(TrackKind::Audio, None).expect("A2");
    // Auto would put the sound on A1; the pointer is over A2.
    state
        .drop_media_on_timeline(media, Some(second), 0)
        .expect("drop on A2");

    assert!(clips(&state, first).is_empty());
    assert_eq!(spans(&state, second), vec![(0, 48)]);
    assert_eq!(
        spans(&state, picture),
        vec![(0, 48)],
        "the picture follows its patch"
    );
}

#[test]
fn a_channel_patched_off_is_left_out_of_a_drop() {
    let (mut state, media, picture) = state();
    let sound = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("an audio lane");
    state
        .set_source_patch(SourceChannel::Audio, Patch::Off)
        .expect("sound off");
    state
        .drop_media_on_timeline(media, Some(picture), 0)
        .expect("drop");

    assert!(clips(&state, sound).is_empty());
    let placed = clips(&state, picture);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].volume, 0.0, "the picture is placed silent");

    // The lane under the pointer wins over its channel's patch.
    state
        .drop_media_on_timeline(media, Some(sound), 96)
        .expect("drop on the audio lane");
    assert_eq!(spans(&state, sound), vec![(96, 144)]);
}

#[test]
fn a_source_drag_drops_the_marked_range_and_covers_unless_insert() {
    let (mut state, media, picture) = state();
    open_marked(&mut state, media);

    state
        .drop_source_on_timeline(Some(picture), 24, false)
        .expect("overwrite drop");
    assert_eq!(spans(&state, picture), vec![(24, 36)]);
    assert_eq!(clips(&state, picture)[0].source_start_ticks, 12);
    assert_eq!(state.status_message.as_deref(), Some("Overwrite"));

    state
        .drop_source_on_timeline(Some(picture), 24, true)
        .expect("insert drop");
    assert_eq!(
        spans(&state, picture),
        vec![(24, 36), (36, 48)],
        "the insert pushes the first take right instead of covering it"
    );
    assert_eq!(state.status_message.as_deref(), Some("Insert"));
    assert!(
        state.sources.focused().is_some(),
        "the Source tab stays open for the next take"
    );
}

#[test]
fn a_source_drag_with_no_source_open_changes_nothing() {
    let (mut state, _media, picture) = state();
    state
        .drop_source_on_timeline(Some(picture), 0, false)
        .expect("nothing to drop");
    assert!(clips(&state, picture).is_empty());
}
