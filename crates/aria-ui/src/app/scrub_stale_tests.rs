//! Moving the playhead: what it must leave correct, and what it must not spend.
//!
//! The class these close is a projected value that depends on the tick and is
//! only recomputed by the next edit. The live multicam angle was one of those,
//! and nothing proved it was the last: `set_playhead` writes the tick into the
//! store and the poll that follows re-samples, so anything sampled at the
//! playhead and not re-sampled would read one scrub behind for as long as
//! nothing else changed.
//!
//! Neither case here names a field. One projects the same project at two ticks
//! and demands the two stores differ in `playhead_ticks` and nowhere else; the
//! other holds everything a scrub left and demands it equal a full
//! re-projection at the same tick. So a new store field that reads the playhead,
//! or a new value sampled at it, turns this red until it is either derived on
//! read or re-taken by the poll.
//!
//! The third case is the reason the first two are worth having: because the
//! projection reads the tick as a value, a scrub does not rebuild the store at
//! all, and that is asserted as a count rather than described in a comment.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, RationalTime, TrackKind};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;

/// A selected clip whose picture moves between two ticks, so a value sampled at
/// the playhead is genuinely different on either side of them.
///
/// Scale rather than opacity: the quad the overlay draws is what the resolver
/// answers at the tick, and a scaled layer changes it.
fn animated() -> (AppState, ClipId, i64, i64) {
    let (mut state, clip) = with_clip();
    state.select_clip(clip);
    let ui = state.selected_ui_clip().expect("the clip is selected");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    let first = start + (end - start) / 4;
    let last = start + (end - start) * 3 / 4;

    state.set_playhead(first);
    state
        .add_keyframe_at_playhead(ClipParam::Scale)
        .expect("a first scale key");
    state.set_playhead(last);
    let mut edited = state.selected_ui_clip().expect("clip").transform;
    edited.scale_x = 0.4;
    edited.scale_y = 0.4;
    state
        .set_selected_transform(edited)
        .expect("a second scale key");
    (state, clip, first, last)
}

/// Ticks either side of both keys, and past the clip, so a boundary is crossed
/// in both directions and one tick lands where the picture is gone.
fn crossings(first: i64, last: i64) -> [i64; 6] {
    [0, first, first + 1, (first + last) / 2, last, last + 1000]
}

#[test]
fn scrub_does_not_leave_a_stale_projection() {
    let (mut state, _clip, first, last) = animated();
    for tick in crossings(first, last) {
        state.set_playhead(tick);
        state.sync_from_engine();
        let store = Arc::clone(&state.store);
        // Moved out rather than copied, so the comparison is against the value
        // the scrub actually left.
        let detail = state.selected.take();

        state.rebuild_store();
        assert_eq!(
            *store, *state.store,
            "at tick {tick} a scrub left a store a re-projection disagrees with"
        );
        assert!(
            detail == state.selected,
            "at tick {tick} a value sampled at the playhead was not re-sampled"
        );
    }
}

#[test]
fn a_projection_reads_the_playhead_only_as_the_playhead() {
    let (state, _clip, first, last) = animated();
    let engine = state.engine.lock();
    let rate = engine.project().timeline().frame_rate;
    let selection: Vec<ClipId> = state.selection.to_vec();
    let at = |tick: i64| {
        crate::projection::project_to_ui_with(
            engine.project(),
            RationalTime::new(tick, rate),
            &selection,
            engine.is_dirty(),
            engine.can_undo(),
            engine.can_redo(),
        )
    };
    let ticks = crossings(first, last);
    let reference = at(ticks[0]);
    for tick in ticks {
        let mut projected = at(tick);
        assert_eq!(
            projected.playhead_ticks, tick,
            "the projection carries the tick it was asked for"
        );
        // Aligned on the one field the tick is allowed to reach, so anything
        // else the projection derived from it is left as a difference.
        projected.playhead_ticks = reference.playhead_ticks;
        assert_eq!(
            projected, reference,
            "the projection at tick {tick} derives something from the playhead: \
             derive it on read, or re-take it in the playhead-only poll"
        );
    }
}

#[test]
fn a_scrub_costs_no_reprojection() {
    let (mut state, _clip, first, last) = animated();
    state.sync_from_engine();
    let projections = state.projections;
    for tick in crossings(first, last) {
        state.set_playhead(tick);
        state.sync_from_engine();
        // The frame polls more than once, and none of them may rebuild either.
        state.sync_from_engine();
    }
    assert_eq!(
        state.projections, projections,
        "a scrub walked every track, clip and keyframe again"
    );

    // The memo is still a memo: an edit does rebuild. Applied on the engine
    // directly, so the poll is the only thing that can carry it in.
    state
        .engine
        .lock()
        .apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "V2".into(),
            index: None,
            pinned: true,
        }))
        .expect("add a track");
    state.sync_from_engine();
    assert!(
        state.projections > projections,
        "an edit no longer reaches the projection"
    );
}
