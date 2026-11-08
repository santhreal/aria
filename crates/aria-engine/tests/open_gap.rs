//! Open Gap: one gap opened across the named lanes and every sync-locked lane.
//!
//! WHY: Paste Insert opened its gap with one lane shift per pasted lane, which
//! moved only the clips starting at or after the playhead on those lanes. A
//! clip the playhead was inside stayed, the copy overlapped it, and the paste
//! was refused; sync-locked lanes stayed put. `OpenGap` opens the gap through
//! `shift_clips::open_lanes_for_insert`, the rule a ripple insert already
//! uses, so the class closed is a gap opened by other rules than an insert's.
//! Asserted: the cut and the shift on the named lane, the same gap on a
//! sync-locked lane, a lane with its sync lock cleared left alone, a lane named
//! twice opened once, one undo entry, and a payload with no lane or no length
//! refused with nothing changed.
//!
//! Not covered: a refused lane part-way through, whose rollback is the one
//! `fit_to_fill` uses and is asserted there.
mod common;

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{MediaId, RationalTime, TimeRange, TrackId, TrackKind};
use common::{add_media_clip, add_track, import_asset, rt, temp_engine};

/// A still is 1000 ticks per second against a 24 fps timeline, so two seconds
/// of it are 48 timeline ticks.
fn two_seconds(engine: &Engine, media: MediaId) -> TimeRange {
    let rate = engine.project().media(media).expect("media").frame_rate;
    TimeRange::at_rate(0, 2000, rate)
}

/// Three picture lanes, each holding one clip over 0..48: the lane the gap
/// is opened on, a sync-locked lane, and a lane with its sync lock cleared.
fn dressed() -> (tempfile::TempDir, Engine, [TrackId; 3]) {
    let (dir, mut engine) = temp_engine();
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let media = import_asset(&mut engine, &still);
    let source = two_seconds(&engine, media);
    let lanes = ["V1", "V2", "V3"].map(|name| add_track(&mut engine, TrackKind::Video, name));
    for lane in lanes {
        add_media_clip(&mut engine, lane, media, source, rt(0));
    }
    engine
        .apply(Command::Edit(EditCommand::SetTrackSyncLock {
            track: lanes[2],
            sync_lock: false,
        }))
        .expect("clear V3's sync lock");
    (dir, engine, lanes)
}

fn spans(engine: &Engine, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = engine
        .project()
        .timeline()
        .track(lane)
        .expect("lane present")
        .clips()
        .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
        .collect();
    spans.sort_unstable();
    spans
}

fn every_lane(engine: &Engine, lanes: [TrackId; 3]) -> [Vec<(i64, i64)>; 3] {
    lanes.map(|lane| spans(engine, lane))
}

fn open(
    engine: &mut Engine,
    tracks: Vec<TrackId>,
    at: i64,
    duration: i64,
) -> Result<ApplyOutcome, aria_engine::EngineError> {
    let rate = engine.project().timeline().frame_rate;
    engine.apply(Command::Edit(EditCommand::OpenGap {
        tracks,
        at: RationalTime::new(at, rate),
        duration: RationalTime::new(duration, rate),
    }))
}

#[test]
fn a_gap_inside_a_clip_cuts_it_and_opens_on_every_sync_locked_lane() {
    let (_dir, mut engine, lanes) = dressed();
    let before = every_lane(&engine, lanes);
    let outcome = open(&mut engine, vec![lanes[0]], 12, 24).expect("open the gap");
    assert_eq!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::ShiftedTrack(lanes[0])),
        "the outcome reports the lane named"
    );
    let opened = [
        vec![(0, 12), (36, 72)],
        vec![(0, 12), (36, 72)],
        vec![(0, 48)],
    ];
    assert_eq!(
        every_lane(&engine, lanes),
        opened,
        "the named lane and the sync-locked lane are cut and opened; the cleared lane stays"
    );

    assert!(engine.undo(), "undo");
    assert_eq!(
        every_lane(&engine, lanes),
        before,
        "one undo closes every lane"
    );
    assert!(engine.redo(), "redo");
    assert_eq!(
        every_lane(&engine, lanes),
        opened,
        "redo opens every lane again"
    );
}

#[test]
fn a_lane_named_twice_opens_once() {
    let (_dir, mut engine, lanes) = dressed();
    open(&mut engine, vec![lanes[0], lanes[0]], 12, 24).expect("open the gap");
    assert_eq!(spans(&engine, lanes[0]), vec![(0, 12), (36, 72)]);
}

#[test]
fn a_lane_with_its_sync_lock_cleared_opens_when_it_is_named() {
    let (_dir, mut engine, lanes) = dressed();
    open(&mut engine, vec![lanes[2]], 12, 24).expect("open the gap");
    assert_eq!(
        every_lane(&engine, lanes),
        [
            vec![(0, 12), (36, 72)],
            vec![(0, 12), (36, 72)],
            vec![(0, 12), (36, 72)]
        ],
        "the named lane opens whatever its own sync lock, and the sync-locked lanes ride"
    );
}

#[test]
fn a_gap_with_no_lane_or_no_length_is_refused_and_changes_nothing() {
    let (_dir, mut engine, lanes) = dressed();
    // Room before the clip on the named lane, so a negative length would close
    // it rather than collide.
    let rate = engine.project().timeline().frame_rate;
    open(&mut engine, vec![lanes[0]], 0, 24).expect("make room before the clips");
    let before = every_lane(&engine, lanes);
    let depth = engine.history_entries().len();
    for (tracks, at, duration) in [
        (Vec::new(), 30, 24),
        (vec![lanes[0]], 30, 0),
        (vec![lanes[0]], 24, -12),
    ] {
        let named = format!("{tracks:?} at {at} for {duration}");
        assert!(
            open(&mut engine, tracks, at, duration).is_err(),
            "{named} opened a gap"
        );
        assert_eq!(every_lane(&engine, lanes), before, "{named} moved a clip");
        assert_eq!(
            engine.history_entries().len(),
            depth,
            "{named} wrote history"
        );
    }
    let other_rate = engine.apply(Command::Edit(EditCommand::OpenGap {
        tracks: vec![lanes[0]],
        at: RationalTime::new(30, rate),
        duration: RationalTime::new(24, aria_models::Rational::FPS_30),
    }));
    assert!(other_rate.is_err(), "a length at another rate opened a gap");
    assert_eq!(every_lane(&engine, lanes), before);
}
