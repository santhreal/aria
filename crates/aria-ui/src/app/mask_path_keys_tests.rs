//! The Path row of a path mask on the timeline.
//!
//! # Why
//!
//! A path keyframe holds every vertex, so the Path row is the one timeline row
//! that is no catalog property. Its stopwatch, key button, navigator, keyframe
//! drag and keyframe menu each send the whole mask back, so each press is
//! pinned to the curve it leaves and to one undo step; the row to the curve it
//! draws; and the reveal chords and the Masks twirl to the rows they open and
//! where those rows sit.
//!
//! Not covered: the pointer routing on the drawn timeline, which
//! `panels/timeline/empty_timeline_tests.rs` hit-tests and presses, and the
//! Viewer reshaping a keyframed path, which `mask_path_tests.rs` drives.

use aria_commands::{Command, EditCommand};
use aria_models::{
    ClipId, ClipParam, Easing, Mask, MaskKind, Param, RationalTime, ShapePathPoint, TrackId,
};

use super::path_masks;
use crate::app::AppState;
use crate::app::mask_path::tests::{MASK_PATH, masked_path};
use crate::app::property_rows::{
    MaskPathKeyMenu, PropertyGroup, RevealAction, TimelineMaskPathRow, TimelineOutlineRow,
};
use crate::app::sent_census::Watch;
use crate::app::transform_handle_tests::selected;

/// The tick of the second path keyframe [`keyed`] sets.
const LATE: i64 = 18;

/// The still on the timeline, selected, with [`MASK_PATH`] as its first mask.
fn with_path() -> (AppState, ClipId) {
    let mut state = selected();
    masked_path(&mut state);
    let clip = state.selected_clip().expect("the still is selected");
    (state, clip)
}

/// [`MASK_PATH`] with every point moved `by` to the right.
fn shifted(by: f32) -> Vec<ShapePathPoint> {
    let shift = |[x, y]: [f32; 2]| [x + by, y];
    MASK_PATH
        .iter()
        .map(|point| ShapePathPoint {
            anchor: shift(point.anchor),
            handle_in: shift(point.handle_in),
            handle_out: shift(point.handle_out),
        })
        .collect()
}

/// [`with_path`] with the path keyed to [`MASK_PATH`] at tick 0 and to
/// `shifted(0.1)` at [`LATE`], and the playhead at 0.
fn keyed() -> (AppState, ClipId) {
    let (mut state, clip) = with_path();
    state.set_playhead(0);
    state.animate_mask_path(clip, 0).expect("the stopwatch");
    state.set_playhead(LATE);
    state
        .set_selected_mask_path(shifted(0.1), true)
        .expect("a reshape at LATE");
    state.set_playhead(0);
    (state, clip)
}

/// Mask `index` of `clip` as the session holds it.
fn model_mask(state: &AppState, clip: ClipId, index: usize) -> Mask {
    let engine = state.engine.lock();
    engine.project().clip(clip).expect("the clip").masks[index].clone()
}

/// The Path row of `clip`'s mask `mask` as the timeline draws it now.
fn row(state: &AppState, clip: ClipId, mask: u32) -> TimelineMaskPathRow {
    let (model, _) = state
        .clip_and_lane_kind(clip)
        .expect("the clip is selected");
    state
        .mask_path_row(&model, mask)
        .expect("a path mask has a Path row")
}

/// The timeline ticks of the keyframes `row` draws.
fn ticks(row: &TimelineMaskPathRow) -> Vec<i64> {
    row.keys.iter().map(|key| key.tick).collect()
}

/// The mask rows of `rows` in order: each mask's index, and whether the row
/// is its Path row.
fn mask_rows(rows: &[TimelineOutlineRow]) -> Vec<(u32, bool)> {
    rows.iter()
        .filter_map(|row| match row {
            TimelineOutlineRow::MaskPath(path) => Some((path.mask, true)),
            TimelineOutlineRow::Property(property) => match property.param {
                ClipParam::Mask { mask, .. } => Some((mask, false)),
                _ => None,
            },
            TimelineOutlineRow::GroupHeader { .. } => None,
        })
        .collect()
}

/// `paths` are the masks with a Path row in `order`, and each one's Path row
/// comes first among its mask's rows, after every row of an earlier mask.
fn assert_paths_lead(order: &[(u32, bool)], paths: &[u32]) {
    let drawn: Vec<u32> = order
        .iter()
        .filter(|(_, path)| *path)
        .map(|(mask, _)| *mask)
        .collect();
    assert_eq!(drawn, paths, "the Path rows in {order:?}");
    for &mask in paths {
        let at = order
            .iter()
            .position(|&entry| entry == (mask, true))
            .expect("drawn above");
        assert_eq!(
            order.get(at + 1),
            Some(&(mask, false)),
            "mask {mask}'s Path row does not lead its rows: {order:?}"
        );
        assert!(
            order[..at].iter().all(|&(earlier, _)| earlier < mask),
            "mask {mask}'s Path row is above an earlier mask's rows: {order:?}"
        );
    }
}

#[test]
fn a_path_mask_has_a_path_row_and_a_shape_mask_has_none() {
    let (mut state, clip) = with_path();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("a rectangle under the path");
    let (model, _) = state.clip_and_lane_kind(clip).expect("selected");
    assert_eq!(path_masks(&model).collect::<Vec<_>>(), [0]);

    let path = row(&state, clip, 0);
    assert_eq!(
        (&*path.label, &*path.value_label),
        ("Mask 1 Path", "4 vertices")
    );
    assert!(!path.keyframed && !path.keyed_at_playhead && path.keys.is_empty());
    assert!(
        state.mask_path_row(&model, 1).is_none(),
        "a rectangle drew a Path row"
    );
    assert!(state.mask_path_row(&model, 7).is_none(), "no mask 7");
}

#[test]
fn the_masks_twirl_puts_each_path_row_at_the_head_of_its_mask() {
    let (mut state, clip) = with_path();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("a rectangle");
    state
        .add_selected_mask_path(MASK_PATH.to_vec())
        .expect("a second path");
    state.toggle_property_group(clip, PropertyGroup::Masks);
    let track = state.store.sequence.tracks[0].id;
    assert_paths_lead(&mask_rows(&state.track_outline_rows(track)), &[0, 2]);
}

#[test]
fn the_stopwatch_keys_the_path_and_again_keeps_what_the_playhead_shows() {
    let (mut state, clip) = with_path();
    let watch = Watch::on(&state);
    state.set_playhead(6);
    state.animate_mask_path(clip, 0).expect("stopwatch on");
    assert_eq!(watch.take(), ["Set mask"]);
    let on = row(&state, clip, 0);
    assert!(on.keyframed && on.keyed_at_playhead);
    assert_eq!(ticks(&on), [6]);

    state.set_playhead(LATE);
    state
        .set_selected_mask_path(shifted(0.1), true)
        .expect("a reshape at LATE");
    assert_eq!(
        ticks(&row(&state, clip, 0)),
        [6, LATE],
        "a reshape of a keyframed path keys it at the playhead"
    );
    state.set_playhead(12);
    let shown = model_mask(&state, clip, 0).path_at(12.0).into_owned();
    assert_ne!(shown, MASK_PATH, "tick 12 is between two different paths");

    watch.take();
    state.animate_mask_path(clip, 0).expect("stopwatch off");
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(model_mask(&state, clip, 0).path, Param::Constant(shown));
    assert!(!row(&state, clip, 0).keyframed);

    assert!(state.undo());
    assert_eq!(
        ticks(&row(&state, clip, 0)),
        [6, LATE],
        "one undo brings every keyframe back"
    );
}

#[test]
fn the_key_button_answers_the_diamond_the_row_drew() {
    let (mut state, clip) = keyed();
    state.set_playhead(9);
    let drawn = row(&state, clip, 0);
    assert!(!drawn.keyed_at_playhead);
    let shown = model_mask(&state, clip, 0).path_at(9.0).into_owned();

    let watch = Watch::on(&state);
    state
        .key_mask_path(clip, 0, drawn.keyed_at_playhead)
        .expect("key");
    assert_eq!(watch.take(), ["Set mask"]);
    let keyed_here = row(&state, clip, 0);
    assert_eq!(ticks(&keyed_here), [0, 9, LATE]);
    assert!(keyed_here.keyed_at_playhead);
    assert_eq!(
        model_mask(&state, clip, 0).path_at(9.0).as_ref(),
        shown.as_slice(),
        "keying the path moved it"
    );

    state
        .key_mask_path(clip, 0, keyed_here.keyed_at_playhead)
        .expect("unkey");
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(ticks(&row(&state, clip, 0)), [0, LATE]);

    // A press on a diamond the curve no longer has sends nothing.
    state.key_mask_path(clip, 0, true).expect("a stale press");
    assert!(watch.take().is_empty());
    assert_eq!(ticks(&row(&state, clip, 0)), [0, LATE]);
}

#[test]
fn the_navigator_steps_between_path_keyframes_and_stops_at_the_ends() {
    let (mut state, clip) = keyed();
    let watch = Watch::on(&state);
    state.set_playhead(4);
    let between = row(&state, clip, 0);
    assert!(between.has_prev_key && between.has_next_key);

    state.step_mask_path_key(clip, 0, true);
    assert_eq!(state.playhead.value, LATE);
    assert!(!row(&state, clip, 0).has_next_key);
    state.step_mask_path_key(clip, 0, true);
    assert_eq!(state.playhead.value, LATE, "a step past the last keyframe");
    state.step_mask_path_key(clip, 0, false);
    assert_eq!(state.playhead.value, 0);
    state.step_mask_path_key(clip, 0, false);
    assert_eq!(state.playhead.value, 0, "a step before the first keyframe");
    assert!(watch.take().is_empty(), "stepping edited the mask");
}

#[test]
fn a_dragged_path_keyframe_snaps_and_takes_its_path_in_one_undo_step() {
    let (mut state, clip) = keyed();
    state.set_playhead(12);
    let watch = Watch::on(&state);

    // 18 - 5 = 13 lies within 3 ticks of the playhead at 12.
    state.begin_mask_path_key_drag(clip, 0, LATE);
    assert!(
        state.update_mask_path_key_drag(-5, 3, false),
        "the keyframe moved"
    );
    assert_eq!(state.snap_guide.map(|guide| guide.tick), Some(12));
    let carried = row(&state, clip, 0);
    assert_eq!(
        ticks(&carried),
        [0, 12],
        "the row draws the keyframe in flight"
    );
    assert!(carried.keys[1].selected);
    assert!(watch.take().is_empty(), "a drag in flight sent an edit");

    state.commit_mask_path_key_drag().expect("drop");
    assert_eq!(watch.take(), ["Set mask"]);
    assert!(state.snap_guide.is_none());
    assert_eq!(ticks(&row(&state, clip, 0)), [0, 12]);
    assert_eq!(
        model_mask(&state, clip, 0).path_at(12.0).as_ref(),
        shifted(0.1).as_slice(),
        "the keyframe left its path behind"
    );
    assert!(state.undo());
    assert_eq!(ticks(&row(&state, clip, 0)), [0, LATE]);

    // Alt passes the snap by.
    state.begin_mask_path_key_drag(clip, 0, LATE);
    state.update_mask_path_key_drag(-5, 3, true);
    assert!(state.snap_guide.is_none());
    state.commit_mask_path_key_drag().expect("drop");
    assert_eq!(ticks(&row(&state, clip, 0)), [0, 13]);

    // A keyframe cannot leave its clip.
    let end = state
        .clip_and_lane_kind(clip)
        .expect("selected")
        .0
        .timeline
        .end_tick();
    state.begin_mask_path_key_drag(clip, 0, 13);
    state.update_mask_path_key_drag(end * 4, 0, false);
    state.commit_mask_path_key_drag().expect("drop");
    assert_eq!(ticks(&row(&state, clip, 0)), [0, end]);
}

#[test]
fn a_press_on_a_path_keyframe_that_never_moves_brings_the_playhead_to_it() {
    let (mut state, clip) = keyed();
    state.set_playhead(5);
    let watch = Watch::on(&state);
    state.begin_mask_path_key_drag(clip, 0, LATE);
    assert!(!state.update_mask_path_key_drag(0, 3, false));
    state.commit_mask_path_key_drag().expect("release");
    assert_eq!(state.playhead.value, LATE);
    assert!(watch.take().is_empty(), "a click edited the mask");
    assert_eq!(ticks(&row(&state, clip, 0)), [0, LATE]);
}

#[test]
fn the_keyframe_menu_eases_holds_and_deletes_the_keyframe_it_opened_on() {
    let (mut state, clip) = keyed();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("a rectangle, now the active mask");
    assert_eq!(state.active_mask(), Some(1));
    state.open_mask_path_key_menu(MaskPathKeyMenu {
        clip,
        mask: 0,
        tick: 0,
        hold: false,
        x: 40.0,
        y: 60.0,
    });
    assert_eq!(
        state.active_mask(),
        Some(0),
        "the Viewer handles a mask other than the menu's"
    );
    assert_eq!(state.mask_path_key_menu().map(|menu| menu.tick), Some(0));

    let watch = Watch::on(&state);
    for easing in [
        Easing::EaseOut,
        Easing::EaseIn,
        Easing::EaseInOut,
        Easing::Hold,
        Easing::Linear,
    ] {
        state
            .ease_mask_path_key(clip, 0, 0, easing)
            .expect("an easing");
        assert_eq!(watch.take(), ["Set mask"], "{easing:?}");
        let drawn = row(&state, clip, 0);
        assert_eq!(drawn.keys[0].easing, easing);
        assert_eq!(drawn.keys[0].hold, easing == Easing::Hold, "{easing:?}");
        if easing == Easing::Hold {
            assert_eq!(
                *model_mask(&state, clip, 0).path_at(9.0),
                MASK_PATH,
                "a held keyframe let the path move"
            );
        }
    }
    state
        .ease_mask_path_key(clip, 0, 0, Easing::Linear)
        .expect("the same easing");
    assert!(
        watch.take().is_empty(),
        "an easing that changed nothing was sent"
    );

    state.remove_mask_path_key(clip, 0, 0).expect("delete");
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(ticks(&row(&state, clip, 0)), [LATE]);
    state
        .remove_mask_path_key(clip, 0, LATE)
        .expect("delete the last");
    assert_eq!(
        model_mask(&state, clip, 0).path,
        Param::Constant(shifted(0.1)),
        "the last keyframe's path is the path"
    );
    state.close_mask_path_key_menu();
    assert_eq!(state.mask_path_key_menu(), None);
}

#[test]
fn a_press_meant_for_a_path_on_a_shape_mask_or_no_mask_sends_nothing() {
    let (mut state, clip) = with_path();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("a rectangle");
    let before = state.clip_and_lane_kind(clip).expect("selected").0.masks;
    let watch = Watch::on(&state);
    for mask in [1, 7] {
        state.animate_mask_path(clip, mask).expect("stopwatch");
        state.key_mask_path(clip, mask, false).expect("key");
        state
            .ease_mask_path_key(clip, mask, 0, Easing::Hold)
            .expect("ease");
        state.remove_mask_path_key(clip, mask, 0).expect("delete");
        state.begin_mask_path_key_drag(clip, mask, 0);
        state.update_mask_path_key_drag(5, 0, false);
        state.commit_mask_path_key_drag().expect("drop");
    }
    assert!(watch.take().is_empty());
    assert_eq!(
        state.clip_and_lane_kind(clip).expect("selected").0.masks,
        before
    );
}

#[test]
fn reveal_chords_open_path_rows_with_their_masks() {
    let (mut state, clip) = with_path();
    state
        .add_selected_mask_path(MASK_PATH.to_vec())
        .expect("a second path");
    state.set_playhead(0);
    state
        .animate_mask_path(clip, 0)
        .expect("key the first path");
    let track = state.store.sequence.tracks[0].id;
    let paths = |state: &AppState| -> Vec<u32> {
        mask_rows(&state.track_outline_rows(track))
            .into_iter()
            .filter(|(_, path)| *path)
            .map(|(mask, _)| mask)
            .collect()
    };

    // U opens what is animated: the keyed path alone.
    state.reveal_properties(RevealAction::AnimatedOrModified, false);
    assert_eq!(paths(&state), [0]);
    // UU opens what is modified, and every path is.
    state.reveal_properties(RevealAction::AnimatedOrModified, false);
    assert_eq!(paths(&state), [0, 1]);
    // Alt+M opens every mask row, each Path row at the head of its mask.
    state.reveal_properties(RevealAction::Masks, false);
    assert_paths_lead(&mask_rows(&state.track_outline_rows(track)), &[0, 1]);
    // Shift+Alt+M takes them away again.
    state.reveal_properties(RevealAction::Masks, true);
    assert!(paths(&state).is_empty());
    // Alt+` closes them.
    state.reveal_properties(RevealAction::Masks, false);
    state.reveal_properties(RevealAction::CloseAll, false);
    assert!(paths(&state).is_empty());
}

/// Where [`a_path_row_on_a_later_clip_speaks_timeline_time_and_stores_clip_time`]
/// starts its clip.
const START: i64 = 30;

/// A Path row on a clip that starts at [`START`] draws, steps to, drags, eases
/// and deletes its keyframes at timeline ticks and stores them at clip ticks.
/// A clip at the start of the sequence has the two equal, so a row that mixes
/// them up passes every other test here and puts each key [`START`] ticks off.
#[test]
fn a_path_row_on_a_later_clip_speaks_timeline_time_and_stores_clip_time() {
    let (mut state, clip) = with_path();
    let track = state.ui_clip_ref(clip).expect("the clip").track_id;
    state
        .apply_command(Command::Edit(EditCommand::MoveClip {
            clip,
            to_track: TrackId::from_raw(track),
            start: RationalTime::new(START, state.playhead.rate),
        }))
        .expect("move the still later");
    state.select_clip(clip);
    let stored = |state: &AppState| -> Vec<i64> {
        model_mask(state, clip, 0)
            .path
            .keyframes()
            .iter()
            .map(|key| key.tick)
            .collect()
    };

    state.set_playhead(START);
    state.animate_mask_path(clip, 0).expect("the stopwatch");
    state.set_playhead(START + LATE);
    state
        .set_selected_mask_path(shifted(0.1), true)
        .expect("a reshape at LATE");
    assert_eq!(stored(&state), [0, LATE], "the keys are not in clip time");
    assert_eq!(ticks(&row(&state, clip, 0)), [START, START + LATE]);
    assert!(row(&state, clip, 0).keyed_at_playhead);

    state.set_playhead(START + 2);
    state.step_mask_path_key(clip, 0, true);
    assert_eq!(
        state.playhead.value,
        START + LATE,
        "the step missed the key"
    );

    state.begin_mask_path_key_drag(clip, 0, START + LATE);
    state.update_mask_path_key_drag(-4, 0, false);
    state.commit_mask_path_key_drag().expect("the drag");
    assert_eq!(stored(&state), [0, LATE - 4], "the drag moved another key");

    state
        .ease_mask_path_key(clip, 0, START, Easing::Hold)
        .expect("ease the first key");
    let eased = model_mask(&state, clip, 0).path.easing_at(0);
    assert_eq!(eased, Some(Easing::Hold), "the menu eased no key");

    state
        .remove_mask_path_key(clip, 0, START + LATE - 4)
        .expect("delete the last key");
    assert_eq!(stored(&state), [0], "the delete missed the key");
}
