//! Shift+Delete on a selection of clips.
//!
//! # Why
//!
//! A ripple delete closes the clip's span on its own lane and on every
//! sync-locked lane, so one clip's delete can remove or cut another selected
//! clip before that clip's own turn. Three defects of that order: the second
//! delete of a linked pair named a clip the first had already removed and the
//! whole gesture rolled back; a sound inside its picture's span went first,
//! cut the picture, and left the picture's tail behind; and with one lane of
//! the pair out of sync lock, the second delete closed the span again on
//! every sync-locked lane, so those lanes lost twice the span. These tests
//! select through `D` and by hand, delete, and read every lane afterwards.
//!
//! Not caught here: selected clips whose spans overlap without matching on
//! lanes out of sync lock, where each still closes its own span.

use aria_models::{ClipId, TrackId};

use super::AppState;
use super::premiere_tests::with_sequence;

/// One lane pair of three 48-tick clips, end to end from 0.
fn pair_of_lanes() -> AppState {
    with_sequence(1, 3)
}

fn lane(state: &AppState, name: &str) -> TrackId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.name.as_ref() == name)
        .map(|track| TrackId::from_raw(track.id))
        .expect("the workload names its lanes")
}

/// The clip on `name` that starts at `tick`.
fn clip_at(state: &AppState, name: &str, tick: i64) -> ClipId {
    let id = lane(state, name).raw();
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == id)
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.timeline_start_ticks == tick)
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("a clip starts there")
}

/// Every clip span on `name`, in timeline order.
fn spans(state: &AppState, name: &str) -> Vec<(i64, i64)> {
    let id = lane(state, name).raw();
    let mut spans: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == id)
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
}

const END_TO_END: [(i64, i64); 3] = [(0, 48), (48, 96), (96, 144)];

#[test]
fn a_linked_pair_picked_with_d_deletes_both_closes_the_gap_and_undoes_in_one_step() {
    let mut state = pair_of_lanes();
    let pair = vec![clip_at(&state, "Video0", 48), clip_at(&state, "Audio0", 48)];
    state.selection = pair.clone();
    state.link_selected().expect("link the middle clips");
    state.clear_selection();
    state.set_playhead(60);
    state.select_at_playhead();
    let mut picked = state.selection.clone();
    picked.sort_unstable_by_key(|clip| clip.raw());
    let mut expected = pair;
    expected.sort_unstable_by_key(|clip| clip.raw());
    assert_eq!(picked, expected, "D picks the pair under the playhead");

    state
        .ripple_delete_selected()
        .expect("Shift+Delete on the pair");

    for name in ["Video0", "Audio0"] {
        assert_eq!(
            spans(&state, name),
            [(0, 48), (48, 96)],
            "{name}: the pair is gone and the last clip closed up"
        );
    }
    assert!(state.selection.is_empty());

    assert!(state.undo(), "the delete is one undo step");
    for name in ["Video0", "Audio0"] {
        assert_eq!(spans(&state, name), END_TO_END, "{name} after undo");
    }
}

#[test]
fn a_sound_inside_its_selected_picture_leaves_no_piece_of_the_picture_behind() {
    let mut state = pair_of_lanes();
    // Audio0's middle clip in three: [48, 60), [60, 84), [84, 96).
    let audio = lane(&state, "Audio0");
    for tick in [60, 84] {
        state.split_at_tick(audio, tick, false).expect("cut Audio0");
    }
    state.selection = vec![clip_at(&state, "Video0", 48), clip_at(&state, "Audio0", 60)];

    state
        .ripple_delete_selected()
        .expect("Shift+Delete on the picture and the sound inside it");

    for name in ["Video0", "Audio0"] {
        assert_eq!(
            spans(&state, name),
            [(0, 48), (48, 96)],
            "{name}: the picture's whole span is removed from both lanes"
        );
    }

    assert!(state.undo(), "the delete is one undo step");
    assert_eq!(spans(&state, "Video0"), END_TO_END);
    assert_eq!(
        spans(&state, "Audio0"),
        [(0, 48), (48, 60), (60, 84), (84, 96), (96, 144)]
    );
}

#[test]
fn a_pair_with_a_lane_out_of_sync_lock_closes_the_span_once_on_every_lane() {
    const LANES: [&str; 4] = ["Video0", "Audio0", "Video1", "Audio1"];
    // Either lane of the pair out of sync lock, then both: whichever of the two
    // goes first, the second never closes the span again on the lanes the first
    // already closed, Video1 and Audio1 among them.
    for unlocked in [&["Video0"][..], &["Audio0"], &["Video0", "Audio0"]] {
        let mut state = with_sequence(2, 3);
        for name in unlocked {
            let track = lane(&state, name);
            state
                .toggle_track_sync_lock(track)
                .expect("unlock the lane");
        }
        state.selection = vec![clip_at(&state, "Video0", 48), clip_at(&state, "Audio0", 48)];

        state
            .ripple_delete_selected()
            .expect("Shift+Delete on the pair");

        for name in LANES {
            assert_eq!(
                spans(&state, name),
                [(0, 48), (48, 96)],
                "{name} with {unlocked:?} out of sync lock: the span closes once"
            );
        }
        assert!(state.undo(), "the delete is one undo step");
        for name in LANES {
            assert_eq!(spans(&state, name), END_TO_END, "{name} after undo");
        }
    }
}
