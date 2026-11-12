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
//!
//! The last three cases cover what a scrub hands on. Every mover that takes a
//! tick lands the playhead inside the sequence, with a scrub's grain at the
//! tick it landed on. A drag faster than the audio pump replaces the pending
//! grain on every move while only the first move at a revision builds the
//! mix, so the mix has to move to each replacing grain, and never to a grain
//! of a later revision. Not covered: the device stream the pump starts, which
//! these tests never open.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, RationalTime, TrackKind};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::audio::{AudioHandle, ScrubGrain};

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

#[test]
fn a_move_past_either_end_lands_on_that_end() {
    type Move = fn(&mut AppState, i64);
    let (mut state, _) = with_clip();
    let end = state.store.sequence.duration_ticks;
    assert!(end > 3, "the clip is long enough to move inside");
    let movers: [(&str, Move, bool); 3] = [
        ("set_playhead", AppState::set_playhead, false),
        ("scrub_playhead", AppState::scrub_playhead, true),
        (
            "scrub_playhead_on_ruler",
            AppState::scrub_playhead_on_ruler,
            true,
        ),
    ];
    let asks = [
        (i64::MIN, 0),
        (-1, 0),
        (end / 2, end / 2),
        (end + 1, end),
        (i64::MAX, end),
    ];
    for (name, mover, scrubs) in movers {
        for (asked, want) in asks {
            state.set_playhead(end / 3);
            let _ = state.audio.take_pending_scrub();
            mover(&mut state, asked);
            assert_eq!(state.playhead.value, want, "{name}({asked})");
            assert_eq!(state.store.playhead_ticks, want, "{name}({asked}): store");
            let grain = state.audio.take_pending_scrub().map(|grain| grain.tick);
            assert_eq!(grain, scrubs.then_some(want), "{name}({asked}): grain");
        }
    }
}

#[test]
fn a_scrub_faster_than_the_pump_keeps_the_mix_its_first_move_built() {
    let (mut state, _) = with_clip();
    for tick in 1..=40 {
        state.scrub_playhead(tick);
    }
    let grain = state.audio.take_pending_scrub().expect("the latest grain");
    assert_eq!(grain.tick, 40);
    assert!(
        grain.mixer.is_some(),
        "the grain the pump takes holds no mix, so the scrub is silent"
    );
}

#[test]
fn a_replaced_grain_hands_its_mix_on_only_within_its_revision() {
    type Mix =
        Arc<parking_lot::Mutex<Option<Result<Option<aria_render::ExportAudioMixer>, String>>>>;
    let mix = || -> Mix { Arc::new(parking_lot::Mutex::new(Some(Ok(None)))) };
    let grain = |mixer: Option<Mix>, revision: u64, tick: i64| ScrubGrain {
        mixer,
        revision,
        tick,
        fps_num: 24,
        fps_den: 1,
    };
    let held = |audio: &AudioHandle| audio.take_pending_scrub().and_then(|grain| grain.mixer);
    let audio = AudioHandle::new();

    let first = mix();
    audio.queue_scrub(grain(Some(Arc::clone(&first)), 3, 1));
    assert!(
        audio.scrub_mix_ready(3),
        "the pending grain holds revision 3's mix"
    );
    assert!(
        !audio.scrub_mix_ready(4),
        "revision 3's mix stood in for revision 4"
    );
    audio.queue_scrub(grain(None, 3, 2));
    let kept = held(&audio).expect("the replacing grain lost the mix");
    assert!(
        Arc::ptr_eq(&kept, &first),
        "the replacing grain took another mix"
    );

    let own = mix();
    audio.queue_scrub(grain(Some(Arc::clone(&first)), 3, 1));
    audio.queue_scrub(grain(Some(Arc::clone(&own)), 3, 2));
    let kept = held(&audio).expect("the grain lost its own mix");
    assert!(
        Arc::ptr_eq(&kept, &own),
        "the pending mix replaced the grain's own"
    );

    audio.queue_scrub(grain(Some(first), 3, 1));
    audio.queue_scrub(grain(None, 4, 2));
    assert!(
        held(&audio).is_none(),
        "the grain after an edit took the mix from before it"
    );
}
