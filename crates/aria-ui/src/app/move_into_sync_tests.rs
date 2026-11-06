//! Clip context menu > Move into Sync.
//!
//! # Why
//!
//! A linked clip that places its file away from its partner draws the offset
//! on its bar (`+3`). Without a row that acts on that offset, the only repair
//! was a hand drag to the exact frame. These tests press the row the menu
//! draws over the clip and read the model's own drift measure,
//! [`aria_models::Project::link_drifts`], afterwards: the pressed clip moves,
//! its partner stays, the move is one undo step, and a move that would start
//! before the sequence leaves both clips where they are.
//!
//! Not caught: a link of more than two members, where the drift is measured
//! against the first other member that places the file.

use std::collections::HashMap;

use aria_models::ClipId;

use super::AppState;
use super::fixtures::drifted_pair;
use crate::surfaces::context_menu::{CLIP_ITEMS, ClipMenu, MenuItem};

fn drifts(state: &AppState) -> HashMap<ClipId, i64> {
    state.engine.lock().project().link_drifts()
}

fn start(state: &AppState, clip: ClipId) -> i64 {
    state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline")
        .timeline_start_ticks
}

fn end(state: &AppState, clip: ClipId) -> i64 {
    state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline")
        .timeline_end_ticks
}

/// Open the clip menu over `clip` and press Move into Sync, failing when the
/// menu does not offer it.
fn press(state: &mut AppState, clip: ClipId) {
    let menu = ClipMenu::of(state, clip.raw());
    let row = CLIP_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .find(|row| row.id == "ctx-move-into-sync")
        .expect("the clip menu has a Move into Sync row");
    assert!((row.when)(&menu), "Move into Sync is offered over {clip:?}");
    (row.run)(state, &menu).expect("Move into Sync runs");
    state.sync_from_engine();
}

#[test]
fn the_row_is_offered_only_over_a_clip_out_of_sync() {
    let (mut state, picture, sound) = drifted_pair(3);
    assert_eq!(ClipMenu::of(&state, picture.raw()).drift, Some(3));
    assert_eq!(ClipMenu::of(&state, sound.raw()).drift, Some(-3));

    press(&mut state, picture);
    for clip in [picture, sound] {
        assert_eq!(
            ClipMenu::of(&state, clip.raw()).drift,
            None,
            "{clip:?} is in sync, so the row is not offered"
        );
    }
}

#[test]
fn the_pressed_clip_moves_into_sync_and_its_partner_stays() {
    // Pressed over the picture, the picture moves back; pressed over the
    // sound, the sound moves to the picture.
    for (pressed_picture, picture_at, sound_at) in [(true, 0, 0), (false, 3, 3)] {
        let (mut state, picture, sound) = drifted_pair(3);
        press(&mut state, if pressed_picture { picture } else { sound });
        assert_eq!(drifts(&state), HashMap::new(), "the pair is in sync");
        assert_eq!(start(&state, picture), picture_at);
        assert_eq!(start(&state, sound), sound_at);
        assert_eq!(
            state.status_message.as_deref(),
            Some("Moved into sync"),
            "the status bar reports the move"
        );

        assert!(state.undo(), "the move is one undo step");
        state.sync_from_engine();
        assert_eq!(drifts(&state), HashMap::from([(picture, 3), (sound, -3)]));
        assert_eq!((start(&state, picture), start(&state, sound)), (3, 0));
    }
}

#[test]
fn a_move_that_would_start_before_the_sequence_leaves_both_clips() {
    // Ten frames off the picture's head, then the picture back to the start:
    // it places its file ten frames before the sequence does, so the sound
    // would have to start at -10 to match it.
    let (mut state, picture, sound) = drifted_pair(10);
    let lane = state.track_of(picture).expect("the picture's lane");
    state.toggle_linked_selection();
    state
        .trim_clip(picture, 20, end(&state, picture))
        .expect("ten frames off the picture's head");
    state
        .move_clip(picture, lane, 0)
        .expect("the picture to the start");
    state.toggle_linked_selection();
    state.sync_from_engine();
    assert_eq!(drifts(&state), HashMap::from([(picture, -10), (sound, 10)]));

    press(&mut state, sound);
    assert_eq!(drifts(&state), HashMap::from([(picture, -10), (sound, 10)]));
    assert_eq!((start(&state, picture), start(&state, sound)), (0, 0));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Moving the clip into sync would start it before the sequence")
    );
}
