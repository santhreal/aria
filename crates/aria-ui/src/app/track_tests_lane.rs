//! Lane: 18 of the cases in [`super`].
//!
//! Split out of `track_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn moving_a_lane_up_and_down_walks_the_projected_stack() {
    let (mut state, top, middle, main) = with_two_video_lanes();

    // The middle lane moves up one row and the top two swap.
    state.move_track_up(TrackId::from_raw(middle)).unwrap();
    assert_eq!(rows(&state), vec![middle, top, main]);

    // Back down again restores the original stack.
    state.move_track_down(TrackId::from_raw(middle)).unwrap();
    assert_eq!(rows(&state), vec![top, middle, main]);
}
#[test]
fn a_move_off_the_end_of_the_stack_is_inert() {
    let (mut state, top, middle, main) = with_two_video_lanes();
    let order = vec![top, middle, main];

    // Already at the top: no reorder, no error.
    state.move_track_up(TrackId::from_raw(top)).unwrap();
    assert_eq!(rows(&state), order);
    // Already at the bottom row.
    state.move_track_down(TrackId::from_raw(main)).unwrap();
    assert_eq!(rows(&state), order);

    // An unknown lane is inert rather than a panic.
    state.move_track_up(TrackId::from_raw(9999)).unwrap();
    state.move_track_down(TrackId::from_raw(9999)).unwrap();
    assert_eq!(rows(&state), order);
}
#[test]
fn the_main_lane_cannot_be_lifted_off_the_bottom() {
    // The model re-applies the lane zones after every move, so raising the main
    // track is accepted and then undone by the invariant. The panel must not end
    // up showing a stack the model does not hold.
    let (mut state, top, middle, main) = with_two_video_lanes();
    state.move_track_up(TrackId::from_raw(main)).unwrap();
    assert_eq!(rows(&state), vec![top, middle, main]);
}
#[test]
fn a_reorder_is_undoable() {
    let (mut state, top, middle, main) = with_two_video_lanes();
    state.move_track_up(TrackId::from_raw(middle)).unwrap();
    assert_eq!(rows(&state), vec![middle, top, main]);
    assert!(state.undo(), "a reorder pushes an undo entry");
    assert_eq!(rows(&state), vec![top, middle, main]);
}
#[test]
fn solo_round_trips_through_the_projection() {
    let (mut state, _) = with_clip();
    let track = rows(&state)[0];
    assert!(!lane(&state, track).solo, "a fresh lane is not soloed");

    state.toggle_track_solo(TrackId::from_raw(track)).unwrap();
    assert!(
        lane(&state, track).solo,
        "solo must be projected, not hardcoded false"
    );

    state.toggle_track_solo(TrackId::from_raw(track)).unwrap();
    assert!(!lane(&state, track).solo);

    // Undo restores the flag, and an unknown lane is inert.
    state.toggle_track_solo(TrackId::from_raw(track)).unwrap();
    assert!(state.undo());
    assert!(!lane(&state, track).solo);
    state.toggle_track_solo(TrackId::from_raw(9999)).unwrap();
}
#[test]
fn only_an_audio_lane_can_be_the_duck_source() {
    let (mut state, _) = with_clip();
    let video = rows(&state)[0];
    assert_eq!(lane(&state, video).kind, UiTrackKind::Video);

    // A visual lane is refused silently: the menu item is a no-op, not an error.
    state
        .toggle_track_duck_source(TrackId::from_raw(video))
        .unwrap();
    assert!(!state.track_is_duck_source(TrackId::from_raw(video)));

    state.add_track(TrackKind::Audio, "A1").expect("add audio");
    let audio = rows(&state)
        .into_iter()
        .find(|id| lane(&state, *id).kind == UiTrackKind::Audio)
        .expect("audio lane projected");

    state
        .toggle_track_duck_source(TrackId::from_raw(audio))
        .unwrap();
    assert!(state.track_is_duck_source(TrackId::from_raw(audio)));
    state
        .toggle_track_duck_source(TrackId::from_raw(audio))
        .unwrap();
    assert!(!state.track_is_duck_source(TrackId::from_raw(audio)));
}
#[test]
fn opening_and_closing_a_gap_shifts_the_clips_after_the_playhead() {
    let (mut state, clip) = with_clip();
    let track = rows(&state)[0];
    assert_eq!(clip_start(&state, track, clip.raw()), 0);

    // The playhead sits at 0, so the clip is at-or-after it and moves.
    state.set_playhead(0);
    state
        .shift_clips_from_playhead(TrackId::from_raw(track), 12)
        .unwrap();
    assert_eq!(
        clip_start(&state, track, clip.raw()),
        12,
        "opening a gap pushes the clip right"
    );

    state
        .shift_clips_from_playhead(TrackId::from_raw(track), -12)
        .unwrap();
    assert_eq!(
        clip_start(&state, track, clip.raw()),
        0,
        "closing it pulls the clip back"
    );

    // A zero shift never reaches the engine, so the store is untouched.
    let before = state.store.sequence.tracks.clone();
    state
        .shift_clips_from_playhead(TrackId::from_raw(track), 0)
        .unwrap();
    assert_eq!(state.store.sequence.tracks, before);
}
#[test]
fn shifting_a_whole_lane_ignores_the_playhead() {
    let (mut state, clip) = with_clip();
    let track = rows(&state)[0];
    // Park the playhead past the clip: a from-playhead shift would be a no-op,
    // while `shift_track` starts from the lane's first clip and still moves it.
    let end = lane(&state, track)
        .clips
        .iter()
        .map(|c| c.timeline_end_ticks)
        .max()
        .unwrap_or(0);
    state.set_playhead(end);
    state.shift_track(TrackId::from_raw(track), 6).unwrap();
    assert_eq!(clip_start(&state, track, clip.raw()), 6);

    // A zero shift is inert here too.
    state.shift_track(TrackId::from_raw(track), 0).unwrap();
    assert_eq!(clip_start(&state, track, clip.raw()), 6);
}
#[test]
fn the_edit_mode_decides_whether_a_drop_shifts_the_lane() {
    let (mut state, clip) = with_clip();
    let track = rows(&state)[0];
    let media = fixture_media(&state, track, clip.raw());
    let original_end = lane(&state, track)
        .clips
        .iter()
        .map(|c| c.timeline_end_ticks)
        .max()
        .unwrap_or(0);

    // Placement mode puts the clip exactly where it was dropped and leaves the
    // rest of the lane where it was.
    state.edit_mode = EditMode::Overwrite;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(track)), original_end)
        .unwrap();
    assert_eq!(
        clip_start(&state, track, clip.raw()),
        0,
        "a placement drop must not move existing clips"
    );
    let placed = lane(&state, track).clips.len();
    assert_eq!(placed, 2);

    // Ripple mode at tick 0 opens a hole first, so the existing clips slide right
    // and nothing is lost.
    state.edit_mode = EditMode::Ripple;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(track)), 0)
        .unwrap();
    let ripple = lane(&state, track);
    assert_eq!(
        ripple.clips.len(),
        placed + 1,
        "ripple insert must add a clip, not cover one"
    );
    assert!(
        clip_start(&state, track, clip.raw()) > 0,
        "the original clip was pushed right"
    );
}
#[test]
fn ripple_insert_at_the_playhead_pushes_downstream_clips() {
    let (mut state, clip) = with_clip();
    let track = rows(&state)[0];
    let media = fixture_media(&state, track, clip.raw());

    state.set_playhead(0);
    state.pool_selection.replace(media.raw());
    state
        .place_pool_selection(crate::types::EditMode::Ripple)
        .unwrap();
    assert!(
        clip_start(&state, track, clip.raw()) > 0,
        "the original clip was pushed right, not replaced"
    );
}
/// The class closed below is "a lane can only be reordered from a menu". A header
/// drag names its own drop row, so the rules are: the indicator only appears where
/// the drop would change the stack, a drop is one edit, and abandoning it costs
/// nothing.
#[test]
fn dragging_a_header_onto_another_row_reorders_the_stack_once() {
    let (mut state, top, middle, main) = with_two_video_lanes();
    assert_eq!(rows(&state), vec![top, middle, main]);

    state.begin_track_drag(TrackId::from_raw(middle));
    assert!(state.track_dragging());

    // The dragged lane's own row is not a target: there is nothing to indicate.
    assert!(
        !state.hover_track_drop(1),
        "own row must not become a target"
    );
    assert_eq!(state.track_drop_row, None);
    // A row past the end of the stack is not a target either.
    assert!(!state.hover_track_drop(9));
    assert_eq!(state.track_drop_row, None);

    assert!(state.hover_track_drop(0), "a new target must notify");
    assert_eq!(state.track_drop_row, Some(0));
    assert!(
        !state.hover_track_drop(0),
        "the same target must not notify"
    );

    let before = state.engine().lock().revision();
    state.drop_track_on_row(0).expect("drop");
    assert_eq!(rows(&state), vec![middle, top, main]);
    assert_eq!(
        state.engine().lock().revision(),
        before + 1,
        "one drop must be one edit"
    );
    assert!(!state.track_dragging(), "the drag must end on release");
    assert_eq!(state.track_drop_row, None, "the indicator must clear");

    assert!(state.undo(), "a drop must be undoable");
    assert_eq!(rows(&state), vec![top, middle, main]);
}
#[test]
fn an_abandoned_or_empty_header_drag_costs_nothing() {
    let (mut state, top, middle, main) = with_two_video_lanes();
    let order = rows(&state);
    let before = state.engine().lock().revision();

    // A drop on the lane's own row writes nothing.
    state.begin_track_drag(TrackId::from_raw(middle));
    state.drop_track_on_row(1).expect("same row");
    assert_eq!(rows(&state), order);
    assert_eq!(state.engine().lock().revision(), before);

    // Escape's path: cancel leaves the stack and the indicator alone.
    state.begin_track_drag(TrackId::from_raw(top));
    assert!(state.hover_track_drop(2));
    state.cancel_track_drag();
    assert!(!state.track_dragging());
    assert_eq!(state.track_drop_row, None);
    assert_eq!(rows(&state), order);

    // With no drag in flight every hover and drop is inert.
    assert!(!state.hover_track_drop(0));
    state.drop_track_on_row(0).expect("inert");
    assert_eq!(rows(&state), order);

    // An unknown lane cannot start one.
    state.begin_track_drag(TrackId::from_raw(9999));
    assert!(!state.track_dragging());

    // The main lane is pinned to the bottom of the stack, so a drop that would
    // raise it leaves the order alone whatever the engine records.
    state.begin_track_drag(TrackId::from_raw(main));
    state.drop_track_on_row(0).expect("pinned");
    assert_eq!(rows(&state), order, "the main lane left the bottom");
}
#[test]
fn a_typed_offset_shifts_the_selection_on_every_lane_it_spans() {
    let (mut state, lane_a, clip_a, lane_b, clip_b) = with_two_selected_lanes();
    assert_eq!(clip_start(&state, lane_a, clip_a), 0);
    assert_eq!(clip_start(&state, lane_b, clip_b), 0);

    let origins = state.selection_shift_origins();
    assert_eq!(origins.len(), 2, "both lanes must be spanned");
    assert!(origins.iter().all(|(_, start)| *start == 0));

    state.shift_selection(12).expect("shift");
    assert_eq!(clip_start(&state, lane_a, clip_a), 12);
    assert_eq!(clip_start(&state, lane_b, clip_b), 12);

    // Back the other way lands on the original.
    state.shift_selection(-12).expect("shift back");
    assert_eq!(clip_start(&state, lane_a, clip_a), 0);
    assert_eq!(clip_start(&state, lane_b, clip_b), 0);

    // A zero offset writes nothing.
    let before = state.engine().lock().revision();
    state.shift_selection(0).expect("zero");
    assert_eq!(state.engine().lock().revision(), before);
}
#[test]
fn a_shift_off_the_front_of_the_sequence_is_refused_whole() {
    let (mut state, lane_a, clip_a, lane_b, clip_b) = with_two_selected_lanes();
    state.shift_selection(24).expect("make room");

    let before = state.engine().lock().revision();
    state.shift_selection(-96).expect("refused, not an error");
    assert_eq!(
        clip_start(&state, lane_a, clip_a),
        24,
        "the first lane moved on a refused shift"
    );
    assert_eq!(
        clip_start(&state, lane_b, clip_b),
        24,
        "the second lane moved on a refused shift"
    );
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a refused shift wrote an edit"
    );
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("before the start")),
        "a refusal must say why: {:?}",
        state.status_message
    );

    // With nothing selected there is nothing to shift, and it says so.
    state.clear_selection();
    state.status_message = None;
    state.shift_selection(6).expect("inert");
    assert!(state.selection_shift_origins().is_empty());
    assert!(state.status_message.is_some());
}
#[test]
fn the_typed_offset_parses_a_signed_frame_count_and_nothing_else() {
    use crate::types::RenameTarget;

    let (mut state, lane_a, clip_a, _, _) = with_two_selected_lanes();

    for (typed, expect) in [("12", 12i64), ("+6", 6), ("-3", -3)] {
        let before = clip_start(&state, lane_a, clip_a);
        state.begin_rename(RenameTarget::ShiftFrames);
        assert_eq!(state.rename_draft, "", "the offset draft starts empty");
        for ch in typed.chars() {
            state.rename_push(ch);
        }
        state.commit_rename().expect("commit");
        assert_eq!(
            clip_start(&state, lane_a, clip_a),
            before + expect,
            "{typed} did not shift by {expect}"
        );
        assert!(state.rename.is_none(), "the entry must close on commit");
    }

    // A value that is not a whole number of frames says so and shifts nothing.
    let before = clip_start(&state, lane_a, clip_a);
    state.begin_rename(RenameTarget::ShiftFrames);
    for ch in "2.5s".chars() {
        state.rename_push(ch);
    }
    state.commit_rename().expect("commit");
    assert_eq!(clip_start(&state, lane_a, clip_a), before);
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("whole number")),
        "a bad offset must name the problem: {:?}",
        state.status_message
    );

    // An empty draft cancels.
    state.begin_rename(RenameTarget::ShiftFrames);
    state.commit_rename().expect("commit");
    assert_eq!(clip_start(&state, lane_a, clip_a), before);
    assert!(state.rename.is_none());
}
/// Lane routing: which lane an edit lands on, and which lanes ride along.
///
/// The class closed here is "a routing flag exists in the model but the panel
/// and the drop path never consult it". Both flags default to set, so every
/// assertion below also pins the pre-existing behaviour: a default sequence
/// must behave exactly as it did before routing was a thing.
#[test]
fn both_routing_flags_start_set_on_a_new_lane() {
    let (state, _) = with_clip();
    let main = rows(&state)[0];
    let lane = lane(&state, main);
    assert!(lane.targeted, "a new lane takes edits");
    assert!(lane.sync_lock, "a new lane rides along");
}
#[test]
fn toggling_targeting_and_sync_lock_round_trips_through_the_projection() {
    let (mut state, _) = with_clip();
    let main = rows(&state)[0];
    let id = TrackId::from_raw(main);

    state.toggle_track_targeted(id).unwrap();
    assert!(!lane(&state, main).targeted, "cleared");
    state.toggle_track_targeted(id).unwrap();
    assert!(lane(&state, main).targeted, "set again");

    state.toggle_track_sync_lock(id).unwrap();
    assert!(!lane(&state, main).sync_lock, "cleared");
    state.toggle_track_sync_lock(id).unwrap();
    assert!(lane(&state, main).sync_lock, "set again");
}
#[test]
fn a_toggle_on_an_unknown_lane_is_inert() {
    let (mut state, _) = with_clip();
    let missing = TrackId::from_raw(9999);
    state.toggle_track_targeted(missing).unwrap();
    state.toggle_track_sync_lock(missing).unwrap();
    assert!(state.last_error.is_none(), "no refusal was reported");
}
