//! The tracker as a job: the walk that runs off the window, and the write that
//! happens on it.
//!
//! # Why
//!
//! The pass was assertible and unreachable: nothing called it, because a
//! whole-shot walk on the UI thread freezes the window for the length of the
//! shot. Making it reachable is three separate promises, and each one is a
//! different way for the feature to be quietly wrong:
//!
//! - **The walk edits nothing.** It returns keyframes. That is what allows it to
//!   run on a job thread over a copy of the project, and a walk that started
//!   writing again would be a walk that has to run on the window's thread.
//! - **Progress is determinate.** A bar that reports a fraction of a known total
//!   is the difference between a pass that is waited for and a pass that is killed.
//! - **A stopped pass keeps what it followed, as one entry.** Cancel is not undo:
//!   the frames already tracked stay, and they revert in one keystroke or they
//!   were not one gesture.
//!
//! The refusals are asserted as prose because a job surface has nowhere else to
//! put them: the row either queues work or says why it will not.
//!
//! No GPU here. The job's own frame source is a renderer
//! (`super::RenderedFrames`), and the scripted source stands in its place, which
//! is the same substitution the synchronous tests make.

use std::cell::Cell;

use aria_models::{MaskKind, MaskMode, Param, RationalTime, ShapePathPoint};

use super::tests::{Scripted, flat_frame, keyframes, square_frame, tracked};
use super::*;

/// Every `(done, total)` a walk reported, in order.
#[derive(Default)]
struct Reported(Vec<(i64, i64)>);

impl Reported {
    /// The walk over `source`, with progress collected and `cancel` asked per
    /// frame.
    fn walk(
        state: &AppState,
        source: &mut Scripted,
        span: i64,
        cancel: &dyn Fn() -> bool,
    ) -> (Vec<(RationalTime, [f32; 2])>, MaskTrackReport, Self) {
        let (_, walk) = state
            .mask_walk(span, CONFIDENCE_FLOOR)
            .expect("the fixture has a mask to track");
        let mut reported = Self::default();
        let (keys, report) = walk_mask(source, &walk, cancel, &mut |done, total| {
            reported.0.push((done, total))
        });
        (keys, report, reported)
    }

    fn last(&self) -> (i64, i64) {
        *self.0.last().expect("a walk reported no progress at all")
    }
}

/// The mask the fixture's track writes to.
fn tracked_target(state: &AppState) -> MaskTarget {
    state
        .mask_walk(1, CONFIDENCE_FLOOR)
        .expect("the fixture has a mask to track")
        .0
}

/// Each mask of the selected clip in stack order: its kind and how many centre
/// keyframes it holds.
fn centre_keys(state: &AppState) -> Vec<(MaskKind, usize)> {
    let clip = state
        .selected_model()
        .expect("the fixture selects its clip");
    clip.masks
        .iter()
        .map(|mask| {
            let keys = match &mask.center {
                Param::Keyframed { keyframes } => keyframes.len(),
                Param::Constant(_) => 0,
            };
            (mask.kind, keys)
        })
        .collect()
}

/// Send the tracked mask, index 1 of the fixture, back with `edit` applied.
fn edit_tracked(state: &mut AppState, edit: impl FnOnce(&mut Mask)) {
    let clip = state.selected_clip().expect("the fixture selects its clip");
    let mut mask = state
        .active_model_mask()
        .cloned()
        .expect("the fixture tracks a mask");
    edit(&mut mask);
    state
        .apply_command(Command::Edit(EditCommand::SetClipMask {
            clip,
            index: 1,
            mask,
        }))
        .expect("the mask edit lands");
}

/// The tracked mask redrawn as a closed path through `corners`, which keeps its
/// centre.
fn as_path(state: &mut AppState, corners: &[[f32; 2]], closed: bool) {
    let points = corners
        .iter()
        .copied()
        .map(ShapePathPoint::corner)
        .collect();
    state
        .set_selected_mask_path(points, closed)
        .expect("the path lands");
}

const SQUARE: [[f32; 2]; 4] = [[0.4, 0.4], [0.6, 0.4], [0.6, 0.6], [0.4, 0.6]];

/// Track the fixture's mask over five frames, run `edit` while the walk's
/// keyframes are in flight, then adopt them, the order a job runs in.
fn adopted_after(prepare: fn(&mut AppState), edit: fn(&mut AppState)) -> (AppState, usize) {
    let mut state = tracked();
    prepare(&mut state);
    let mut source = Scripted::walking(5, 4, 0);
    let (keys, report, _) = Reported::walk(&state, &mut source, 4, &|| false);
    let target = tracked_target(&state);
    edit(&mut state);
    let depth = state.engine().lock().history_depth();
    state.adopt_track(&target, &keys, report);
    (state, depth)
}

/// A cancel that answers false for the first `frames` questions and true after.
///
/// The job's cancel is a flag another thread sets mid-pass, and this is that
/// shape: the pass is stopped somewhere in the middle rather than before it
/// starts, which is the case a Cancel press creates.
fn stop_after(frames: usize) -> impl Fn() -> bool {
    let asked = Cell::new(0usize);
    move || {
        let seen = asked.get();
        asked.set(seen + 1);
        seen >= frames
    }
}

#[test]
fn a_pass_reports_progress_all_the_way_to_the_total() {
    let state = tracked();
    let mut source = Scripted::walking(5, 4, 2);
    let (keys, report, reported) = Reported::walk(&state, &mut source, 4, &|| false);

    assert_eq!(keys.len(), 5, "one keyframe per frame including the first");
    assert_eq!(report.written, keys.len());
    assert_eq!(
        reported.0.first().copied(),
        Some((0, 4)),
        "a bar that starts without a total cannot be determinate: {:?}",
        reported.0
    );
    assert_eq!(
        reported.last(),
        (4, 4),
        "a completed pass stopped short of its own total: {:?}",
        reported.0
    );
    let mut previous = -1;
    for (done, total) in &reported.0 {
        assert_eq!(*total, 4, "the total moved mid-pass: {:?}", reported.0);
        assert!(
            *done > previous,
            "progress went backwards: {:?}",
            reported.0
        );
        previous = *done;
    }
}

#[test]
fn a_stopped_pass_keeps_the_frames_it_followed() {
    let state = tracked();
    let mut source = Scripted::walking(8, 4, 0);
    // Two frames of motion, then Cancel is pressed.
    let (keys, report, reported) = Reported::walk(&state, &mut source, 7, &stop_after(2));

    assert!(report.cancelled, "a cancelled pass did not say so");
    assert_eq!(keys.len(), 3, "kept the wrong number of tracked frames");
    assert_eq!(report.written, keys.len());
    assert_eq!(
        reported.last(),
        (2, 7),
        "a stopped pass reported itself finished: {:?}",
        reported.0
    );
    assert_eq!(
        source.asked,
        vec![0, 1, 2],
        "read frames past the cancel: {:?}",
        source.asked
    );
}

#[test]
fn a_lost_pass_reports_progress_short_of_the_total() {
    let state = tracked();
    let mut source = Scripted {
        frames: vec![
            square_frame(40, 30),
            square_frame(44, 30),
            flat_frame(),
            square_frame(52, 30),
        ],
        asked: Vec::new(),
    };
    let (keys, report, reported) = Reported::walk(&state, &mut source, 3, &|| false);

    assert_eq!(report.lost_at, Some(2), "the tick that lost the feature");
    assert_eq!(keys.len(), 2, "wrote a keyframe past the loss");
    assert_eq!(
        reported.last(),
        (1, 3),
        "a lost pass reported a frame it did not accept: {:?}",
        reported.0
    );
}

#[test]
fn the_walk_writes_nothing_of_its_own() {
    // The property that lets the walk run on a job thread: it touches no
    // session. If this fails, the pass has gone back to editing as it goes and
    // cannot be run off the window.
    let mut state = tracked();
    let depth = state.engine().lock().history_depth();
    let mut source = Scripted::walking(4, 4, 0);
    let (keys, report, _) = Reported::walk(&state, &mut source, 3, &|| false);

    assert_eq!(report.written, 4);
    assert!(
        keyframes(&state).is_empty(),
        "the walk wrote keyframes by itself: {:?}",
        keyframes(&state)
    );
    assert_eq!(
        state.engine().lock().history_depth(),
        depth,
        "the walk left a history entry behind"
    );

    let target = tracked_target(&state);
    state
        .write_track(target.clip, target.index, &keys)
        .expect("the write lands");
    assert_eq!(
        keyframes(&state).len(),
        keys.len(),
        "the write dropped keyframes the walk followed"
    );
}

#[test]
fn a_track_written_from_a_job_is_one_undo_entry() {
    let mut state = tracked();
    let mut source = Scripted::walking(5, 4, 0);
    let (keys, report, _) = Reported::walk(&state, &mut source, 4, &|| false);
    let target = tracked_target(&state);
    let depth = state.engine().lock().history_depth();

    state.adopt_track(&target, &keys, report);

    assert_eq!(
        state.engine().lock().history_depth(),
        depth + 1,
        "a track adopted from a job left more than one undo entry"
    );
    assert_eq!(keyframes(&state).len(), 5);
    assert!(state.undo(), "the track undoes");
    assert!(
        keyframes(&state).is_empty(),
        "one undo left keyframes behind: {:?}",
        keyframes(&state)
    );
    let message = state
        .status_message
        .as_deref()
        .expect("an adopted track said nothing");
    assert!(
        message.contains('5'),
        "the track did not say how many frames it followed: {message:?}"
    );
}

#[test]
fn an_adopted_track_says_it_was_stopped_rather_than_finished() {
    // A cancelled pass and a completed pass write the same way and read
    // differently: a stopped track must not be reported as finished.
    let mut state = tracked();
    let mut source = Scripted::walking(8, 4, 0);
    let (keys, report, _) = Reported::walk(&state, &mut source, 7, &stop_after(2));
    let target = tracked_target(&state);

    state.adopt_track(&target, &keys, report);

    let message = state
        .status_message
        .as_deref()
        .expect("an adopted track said nothing");
    assert!(
        message.contains("Stopped"),
        "a stopped track reported as finished: {message:?}"
    );
    assert_eq!(keyframes(&state).len(), 3);
}

#[test]
fn a_track_lands_on_its_mask_when_the_stack_shifts_while_it_runs() {
    // A job walks for as long as the shot lasts and the stack stays editable
    // the whole time. A target held as an index alone sent the keyframes to
    // whichever mask took that index: the one below a removed mask, or the one
    // a move put there. Edits that leave the index alone are the controls.
    use MaskKind::{Circle, Rectangle, Star};
    let cases: [(&str, fn(&mut AppState), Vec<(MaskKind, usize)>); 6] = [
        (
            "remove the mask above it",
            |state| state.remove_selected_mask(0).expect("remove"),
            vec![(Rectangle, 5), (Star, 0)],
        ),
        (
            "move it to the bottom",
            |state| state.move_selected_mask(1, 2).expect("move"),
            vec![(Circle, 0), (Star, 0), (Rectangle, 5)],
        ),
        (
            "move the bottom mask to the top",
            |state| state.move_selected_mask(2, 0).expect("move"),
            vec![(Star, 0), (Circle, 0), (Rectangle, 5)],
        ),
        (
            "remove the mask below it",
            |state| state.remove_selected_mask(2).expect("remove"),
            vec![(Circle, 0), (Rectangle, 5)],
        ),
        (
            "add a mask",
            |state| state.add_selected_mask(Circle).expect("add"),
            vec![(Circle, 0), (Rectangle, 5), (Star, 0), (Circle, 0)],
        ),
        (
            "change its mode and feather",
            |state| {
                edit_tracked(state, |mask| {
                    mask.mode = MaskMode::Subtract;
                    mask.feather = Param::Constant(0.4);
                })
            },
            vec![(Circle, 0), (Rectangle, 5), (Star, 0)],
        ),
    ];
    for (edit, run, expected) in cases {
        let (state, depth) = adopted_after(|_| {}, run);
        assert_eq!(centre_keys(&state), expected, "{edit}");
        assert_eq!(state.last_error, None, "{edit}");
        assert_eq!(
            state.engine().lock().history_depth(),
            depth + 1,
            "{edit}: the track is not one entry"
        );
    }
}

#[test]
fn a_track_whose_mask_is_gone_or_reshaped_writes_nothing_and_says_so() {
    // Writing to whatever sits nearest would put a moving centre on a mask the
    // operator drew elsewhere or redrew; a refusal costs one more pass.
    let path: fn(&mut AppState) = |state| as_path(state, &SQUARE, true);
    let cases: [(&str, fn(&mut AppState), fn(&mut AppState)); 6] = [
        (
            "remove it",
            |_| {},
            |state| state.remove_selected_mask(1).expect("remove"),
        ),
        (
            "drag it",
            |_| {},
            |state| edit_tracked(state, |mask| mask.center = Param::Constant([0.3, 0.2])),
        ),
        (
            "change its shape",
            |_| {},
            |state| edit_tracked(state, |mask| mask.kind = MaskKind::Heart),
        ),
        ("redraw its path", path, |state| {
            as_path(state, &SQUARE[..3], true)
        }),
        ("open its path", path, |state| {
            as_path(state, &SQUARE, false)
        }),
        (
            "remove it after a mask above it moves",
            |_| {},
            |state| {
                state.move_selected_mask(0, 2).expect("move");
                state.remove_selected_mask(0).expect("remove");
            },
        ),
    ];
    for (edit, prepare, run) in cases {
        let (state, depth) = adopted_after(prepare, run);
        assert!(
            centre_keys(&state).iter().all(|&(_, keys)| keys == 0),
            "{edit}: keyframes landed on {:?}",
            centre_keys(&state)
        );
        assert_eq!(
            state.engine().lock().history_depth(),
            depth,
            "{edit}: a refused track left a history entry"
        );
        let error = state.last_error.as_deref().unwrap_or_default();
        assert!(
            error.contains("Track it again"),
            "{edit}: the refusal is not reported: {error:?}"
        );
    }
}

#[test]
fn a_lost_track_says_where_it_stopped() {
    let mut state = tracked();
    let mut source = Scripted {
        frames: vec![square_frame(40, 30), square_frame(44, 30), flat_frame()],
        asked: Vec::new(),
    };
    let (keys, report, _) = Reported::walk(&state, &mut source, 2, &|| false);
    let target = tracked_target(&state);

    state.adopt_track(&target, &keys, report);

    let message = state
        .status_message
        .as_deref()
        .expect("an adopted track said nothing");
    assert!(
        message.contains("lost"),
        "a lost track reported as finished: {message:?}"
    );
}

#[test]
fn a_job_that_cannot_start_refuses_in_words() {
    // Every reason a track cannot start is a sentence, because a job row has
    // nowhere else to put it: the palette row either queues work or says why not.
    let no_mask = crate::app::transform_handle_tests::selected();
    let reason = no_mask
        .mask_walk(4, CONFIDENCE_FLOOR)
        .expect_err("a clip with no mask cannot be tracked");
    assert!(
        reason.contains("mask") && reason.contains(' '),
        "a maskless clip refused without prose: {reason:?}"
    );

    let masked = tracked();
    let reason = masked
        .mask_walk(0, CONFIDENCE_FLOOR)
        .expect_err("a zero span cannot be tracked");
    assert!(
        reason.contains(' '),
        "a zero span refused without prose: {reason:?}"
    );
}

#[test]
fn the_span_a_job_tracks_ends_with_the_clip() {
    // The job asks for no length: it tracks the rest of the shot. A span that
    // ran past the clip would ask the renderer for frames the clip does not
    // show, and one that stopped early would leave the tail untracked.
    let mut state = tracked();
    let ui = state
        .selected_ui_clip()
        .expect("the fixture selects a clip");
    assert_eq!(
        state.mask_track_span(),
        ui.timeline_end_ticks - 1 - state.playhead.value,
        "the job tracked something other than the rest of the clip"
    );

    state.set_playhead(ui.timeline_end_ticks - 1);
    assert_eq!(
        state.mask_track_span(),
        0,
        "the playhead on the last frame still has frames ahead of it"
    );
    assert!(
        state
            .mask_walk(state.mask_track_span(), CONFIDENCE_FLOOR)
            .is_err(),
        "a track with nothing ahead of it started anyway"
    );
}

#[test]
fn the_track_has_a_palette_row_that_needs_a_window() {
    // The row is the whole point of this module: the pass was unreachable
    // before it. It belongs in the windowed table because it starts a job, and a
    // row in the plain table could not.
    let row = crate::commands::BUILTIN_JOB_COMMANDS
        .iter()
        .find(|row| row.id == "pal-track-mask")
        .expect("the mask track has no palette row");
    assert!(
        row.label.to_lowercase().contains("mask"),
        "the row does not name what it tracks: {:?}",
        row.label
    );
}
