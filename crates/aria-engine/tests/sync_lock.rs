//! Sync lock: which lanes ride along with an insert or a ripple delete.
//!
//! The rule under test is not "an insert shifts things" — `ripple_insert` and
//! `fit_to_fill` already assert that on their own lane. It is *which other*
//! lanes move: a lane follows an edit elsewhere exactly while its sync lock is
//! set, and a lane whose lock is cleared is pinned. Each side goes through one
//! owner (`shift_clips::open_for_insert` and
//! `shift_clips::close_for_ripple_delete`), so the commands are asserted
//! together rather than trusting one to stand for both.
//!
//! The two sides are not mirror images. Opening a hole preserves every clip,
//! cutting a straddler in two; closing a span removes that stretch of timeline,
//! so a rider lane's own material inside it goes as well. Both are asserted
//! here, including the cut that keeps the loss to exactly the span.
//!
//! Also asserted: the whole ride-along is one history entry, and clearing the
//! flag is itself undoable.
mod common;

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{ClipId, MediaId, Rational, TimeRange, TrackId, TrackKind};
use common::{add_media_clip, add_track, import_asset, rt, temp_engine};

/// A still is 1000 ticks per second against a 24 fps timeline, so 1000 source
/// ticks is exactly 24 timeline ticks and every span here is a whole frame
/// count.
const PER_SECOND: i64 = 1000;

fn still() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// An engine with a video lane, an audio lane, and a still in the pool.
fn lanes() -> (
    tempfile::TempDir,
    Engine,
    MediaId,
    TrackId,
    TrackId,
    Rational,
) {
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &still());
    let rate = engine.project().media(media).expect("media").frame_rate;
    let video = add_track(&mut engine, TrackKind::Video, "V1");
    let audio = add_track(&mut engine, TrackKind::Audio, "A1");
    (dir, engine, media, video, audio, rate)
}

fn src(rate: Rational, seconds: i64) -> TimeRange {
    TimeRange::at_rate(0, seconds * PER_SECOND, rate)
}

fn span(engine: &Engine, clip: ClipId) -> (i64, i64) {
    let clip = engine.project().clip(clip).expect("clip present");
    (clip.timeline.start.value, clip.timeline.end_tick())
}

/// Every clip span on `track`, in start order. Used where a span-closing edit
/// leaves a clip the test never named, such as the tail of a cut straddler.
fn lane_spans(engine: &Engine, track: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = engine
        .project()
        .timeline()
        .track(track)
        .expect("track present")
        .clips()
        .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
        .collect();
    spans.sort_unstable();
    spans
}

fn set_sync_lock(engine: &mut Engine, track: TrackId, sync_lock: bool) {
    engine
        .apply(Command::Edit(EditCommand::SetTrackSyncLock {
            track,
            sync_lock,
        }))
        .expect("sync lock");
}

fn insert(
    engine: &mut Engine,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: i64,
) -> ClipId {
    match engine
        .apply(Command::Edit(EditCommand::RippleInsert {
            track,
            media,
            source,
            at: rt(at),
        }))
        .expect("insert")
    {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn a_sync_locked_lane_rides_along_with_an_insert_on_another_lane() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    // Sound sitting under picture, both starting at 0.
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(0));
    add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    // A one-second insert at the head of the video lane. Both lanes default to
    // sync-locked, so the sound moves by the same 24 ticks.
    insert(&mut engine, video, media, src(rate, 1), 0);
    assert_eq!(span(&engine, sound), (24, 72), "sound rode along");
}

#[test]
fn clearing_sync_lock_pins_a_lane_against_an_insert_elsewhere() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(0));
    add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    set_sync_lock(&mut engine, audio, false);
    insert(&mut engine, video, media, src(rate, 1), 0);
    assert_eq!(span(&engine, sound), (0, 48), "pinned lane did not move");
}

#[test]
fn the_lane_being_inserted_on_shifts_even_with_its_own_lock_cleared() {
    let (_dir, mut engine, media, video, _audio, rate) = lanes();
    let picture = add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    // Clearing the target lane's own lock must not turn an insert into an
    // overwrite: the insert *is* the shift on that lane.
    set_sync_lock(&mut engine, video, false);
    insert(&mut engine, video, media, src(rate, 1), 0);
    assert_eq!(span(&engine, picture), (24, 72), "the target lane opened");
}

#[test]
fn a_ride_along_insert_is_one_undo_entry() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(0));
    let picture = add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    insert(&mut engine, video, media, src(rate, 1), 0);
    assert_eq!(span(&engine, sound), (24, 72));

    assert!(engine.undo(), "undo");
    assert_eq!(span(&engine, sound), (0, 48), "one undo closed both lanes");
    assert_eq!(span(&engine, picture), (0, 48));

    assert!(engine.redo(), "redo");
    assert_eq!(span(&engine, sound), (24, 72), "redo re-opened both");
    assert_eq!(span(&engine, picture), (24, 72));
}

#[test]
fn a_rippled_four_point_edit_moves_sync_locked_lanes_by_the_span_it_opens() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(0));
    add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    // Fill 12 ticks with two seconds of source: the ride-along is the *span*
    // (12), not what the take would have been worth (48).
    engine
        .apply(Command::Edit(EditCommand::FitToFill {
            track: video,
            media,
            source: src(rate, 2),
            at: rt(0),
            duration: rt(12),
            ripple: true,
        }))
        .expect("fit to fill");
    assert_eq!(span(&engine, sound), (12, 60), "sound rode the span");
}

#[test]
fn a_covering_four_point_edit_leaves_other_lanes_alone() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(0));
    add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));

    // Without `ripple` the span is covered rather than opened, so nothing on
    // any lane moves — sync lock has nothing to ride.
    engine
        .apply(Command::Edit(EditCommand::FitToFill {
            track: video,
            media,
            source: src(rate, 2),
            at: rt(0),
            duration: rt(12),
            ripple: false,
        }))
        .expect("fit to fill");
    assert_eq!(span(&engine, sound), (0, 48), "a cover rides nothing");
}

#[test]
fn clearing_sync_lock_is_undoable() {
    let (_dir, mut engine, _media, _video, audio, _rate) = lanes();
    assert!(
        engine
            .project()
            .timeline()
            .track(audio)
            .expect("lane")
            .sync_lock
    );

    set_sync_lock(&mut engine, audio, false);
    assert!(
        !engine
            .project()
            .timeline()
            .track(audio)
            .expect("lane")
            .sync_lock
    );

    assert!(engine.undo(), "undo");
    assert!(
        engine
            .project()
            .timeline()
            .track(audio)
            .expect("lane")
            .sync_lock,
        "undo restored the flag"
    );
}

#[test]
fn targeting_is_a_flag_of_its_own_and_does_not_disturb_the_others() {
    let (_dir, mut engine, _media, video, _audio, _rate) = lanes();
    engine
        .apply(Command::Edit(EditCommand::SetTrackTargeted {
            track: video,
            targeted: false,
        }))
        .expect("targeted");
    let lane = engine.project().timeline().track(video).expect("lane");
    assert!(!lane.targeted, "targeting cleared");
    assert!(lane.sync_lock, "sync lock untouched");
    assert!(lane.enabled, "enabled untouched");
    assert!(!lane.locked, "locked untouched");
}

/// A ripple delete removes a stretch of timeline, so a sync-locked lane loses
/// the same stretch: its material under the deleted shot goes, and what followed
/// moves up by the same amount.
///
/// The lanes cannot merely shift. Audio a1 occupies exactly the span being
/// removed and a2 sits directly after it, which is the ordinary shape of picture
/// over its own sound. Shifting the whole lane would put a1 at tick -24; leaving
/// a1 in place would put a2 on top of it. Either reading is a state the model
/// refuses, which is why closing the span is the operation and not a shift.
#[test]
fn a_sync_locked_lane_rides_along_with_a_ripple_delete_on_another_lane() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();

    let v1 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(0));
    let v2 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(24));
    let a1 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(0));
    let a2 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(24));

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: v1 }))
        .expect("ripple delete");

    assert!(engine.project().clip(v1).is_none());
    assert_eq!(span(&engine, v2), (0, 24), "video v2 shifted left");
    assert!(
        engine.project().clip(a1).is_none(),
        "the sound under the deleted shot went with it"
    );
    assert_eq!(
        span(&engine, a2),
        (0, 24),
        "sync-locked audio a2 shifted left"
    );
}

/// A rider clip straddling the out point is cut there, so only the part under
/// the deleted shot is lost and the rest moves up with the lane.
///
/// This is the boundary case: without the cut, the straddler is either swept
/// along whole (dragging sound that belongs after the edit) or left in place for
/// the following clips to land on top of. Cutting first means the lane loses
/// exactly the span the edit removed and not a tick more.
#[test]
fn a_rider_clip_straddling_the_out_point_is_cut_at_the_boundary() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();

    let v1 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(0));
    let v2 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(24));
    // Audio 12..60 straddles the out point at 24; audio 72..96 follows it.
    let straddle = add_media_clip(&mut engine, audio, media, src(rate, 2), rt(12));
    let after = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(72));

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: v1 }))
        .expect("ripple delete");

    assert_eq!(span(&engine, v2), (0, 24), "video v2 shifted left");
    assert!(
        engine.project().clip(straddle).is_none(),
        "the 12..24 head of the straddler was inside the removed span"
    );
    assert_eq!(
        span(&engine, after),
        (48, 72),
        "audio after the span moved up by its length"
    );
    // The straddler's 24..60 tail became its own clip and moved up with the lane.
    let audio_spans = lane_spans(&engine, audio);
    assert_eq!(
        audio_spans,
        vec![(0, 36), (48, 72)],
        "the cut tail keeps its 36 ticks and lands where the span used to start"
    );

    // One undo puts the straddler back whole.
    assert!(engine.undo(), "one undo restores the lane");
    assert_eq!(span(&engine, v1), (0, 24));
    assert_eq!(span(&engine, straddle), (12, 60));
    assert_eq!(span(&engine, after), (72, 96));
    assert_eq!(lane_spans(&engine, audio), vec![(12, 60), (72, 96)]);
}

#[test]
fn clearing_sync_lock_pins_a_lane_against_a_ripple_delete_elsewhere() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();

    let v1 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(0));
    let v2 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(24));
    let a1 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(0));
    let a2 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(24));

    set_sync_lock(&mut engine, audio, false);

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: v1 }))
        .expect("ripple delete");

    assert_eq!(span(&engine, v2), (0, 24), "video v2 shifted left");
    assert_eq!(span(&engine, a1), (0, 24), "audio a1 untouched");
    assert_eq!(span(&engine, a2), (24, 48), "pinned audio a2 did not move");
}

#[test]
fn a_ripple_delete_with_sync_locked_lanes_is_one_undo_entry() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();

    let v1 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(0));
    let v2 = add_media_clip(&mut engine, video, media, src(rate, 1), rt(24));
    let a1 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(0));
    let a2 = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(24));

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: v1 }))
        .expect("ripple delete");

    assert!(engine.undo(), "one undo restores all lanes");
    assert!(engine.project().clip(v1).is_some(), "v1 restored");
    assert_eq!(span(&engine, v1), (0, 24));
    assert_eq!(span(&engine, v2), (24, 48));
    assert_eq!(span(&engine, a1), (0, 24));
    assert_eq!(span(&engine, a2), (24, 48));

    assert!(engine.redo(), "redo re-applies the multi-lane delete");
    assert!(engine.project().clip(v1).is_none());
    assert_eq!(span(&engine, v2), (0, 24));
    assert!(engine.project().clip(a1).is_none(), "a1 removed again");
    assert_eq!(span(&engine, a2), (0, 24));
}

/// A ripple move closes the moved span on every sync-locked lane and opens the
/// landing hole on them, so a Shift-drag of linked picture drags its sound with
/// it instead of leaving it at the old ticks. The rider lane here carries no
/// linked member of its own — its material moves only because its lock is set.
#[test]
fn a_ripple_move_keeps_a_sync_locked_rider_lane_in_step() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    // Picture A [0,48), B [48,72); unlinked sound S [0,24). All lanes locked.
    let a = add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));
    let b = add_media_clip(&mut engine, video, media, src(rate, 1), rt(48));
    add_media_clip(&mut engine, audio, media, src(rate, 1), rt(0));

    // Ripple-move A from 0 to 96: picture closes A's span and lands it at 96.
    engine
        .apply(Command::Edit(EditCommand::RippleMoveClip {
            clip: a,
            to_track: video,
            start: rt(96),
        }))
        .expect("ripple move");

    // Picture: closing A's span pulls B up from [48,72) to [0,24); A lands in
    // the reopened hole at [48,96), leaving the gap between them as the ride-
    // along room every sync-locked lane opened together.
    assert_eq!(span(&engine, b), (0, 24));
    assert_eq!(span(&engine, a), (48, 96));

    // Sound sat wholly inside A's old span, so the span close removes it — the
    // same loss a ripple delete there would deal — and the landing hole leaves
    // exactly one reopened clip-length gap behind.
    assert_eq!(lane_spans(&engine, audio), Vec::<(i64, i64)>::new());
}

/// Clearing the lock pins a lane against a ripple move elsewhere, the same as
/// for inserts and deletes.
#[test]
fn clearing_sync_lock_pins_a_lane_against_a_ripple_move() {
    let (_dir, mut engine, media, video, audio, rate) = lanes();
    let a = add_media_clip(&mut engine, video, media, src(rate, 2), rt(0));
    add_media_clip(&mut engine, video, media, src(rate, 1), rt(48));
    let sound = add_media_clip(&mut engine, audio, media, src(rate, 1), rt(0));
    set_sync_lock(&mut engine, audio, false);

    engine
        .apply(Command::Edit(EditCommand::RippleMoveClip {
            clip: a,
            to_track: video,
            start: rt(96),
        }))
        .expect("ripple move");
    assert_eq!(span(&engine, sound), (0, 24), "pinned lane did not move");
}
