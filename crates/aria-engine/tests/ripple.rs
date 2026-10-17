//! Main-track magnet primitives (Phase 7): `ShiftClips` / `RippleInsert`
//! through the command surface, and the worker's ripple gesture compositions
//! (reorder dance, move-off-with-gap-close, pack) as single history entries.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::ApplyOutcome;
use aria_models::{ClipId, Generator, TrackKind};
use common::{
    add_generated, add_media_clip, add_track, import_asset, rt, small_video_asset, temp_engine, tr,
};

fn adjustment() -> Generator {
    Generator::Adjustment
}

fn starts(engine: &aria_engine::Engine, clips: &[ClipId]) -> Vec<i64> {
    clips
        .iter()
        .map(|c| {
            engine
                .project()
                .clip(*c)
                .expect("clip placed")
                .start()
                .value
        })
        .collect()
}

#[test]
fn shift_clips_command_records_one_invertible_entry() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    let a = add_generated(&mut engine, track, adjustment(), tr(0, 50));
    let b = add_generated(&mut engine, track, adjustment(), tr(50, 30));

    let outcome = engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(50),
            delta: rt(40),
        }))
        .expect("shift");
    assert_eq!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::ShiftedTrack(track))
    );
    assert_eq!(starts(&engine, &[a, b]), [0, 90]);

    assert!(engine.undo());
    assert_eq!(starts(&engine, &[a, b]), [0, 50]);
    assert!(engine.redo());
    assert_eq!(starts(&engine, &[a, b]), [0, 90]);
}

/// The worker's magnet reorder: park past the content end, close the old
/// gap, open the new hole (post-close space), land — one undo for all four.
#[test]
fn reorder_dance_is_one_history_entry() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    // Packed lane: A [0,50) B [50,80) C [80,120).
    let a = add_generated(&mut engine, track, adjustment(), tr(0, 50));
    let b = add_generated(&mut engine, track, adjustment(), tr(50, 30));
    let c = add_generated(&mut engine, track, adjustment(), tr(80, 40));

    // Move A after C: post-close insertion tick = 120 - 50 = 70.
    engine.begin_group();
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: track,
            start: rt(120), // park: lane content end
        }))
        .expect("park");
    engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(0),
            delta: rt(-50),
        }))
        .expect("close old gap");
    engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(70),
            delta: rt(50),
        }))
        .expect("open new hole");
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: track,
            start: rt(70),
        }))
        .expect("land");
    engine.commit_group();

    assert_eq!(starts(&engine, &[b, c, a]), [0, 30, 70], "B C A packed");

    assert!(engine.undo(), "one undo reverts the whole reorder");
    assert_eq!(starts(&engine, &[a, b, c]), [0, 50, 80]);

    assert!(engine.redo(), "one redo replays it");
    assert_eq!(starts(&engine, &[b, c, a]), [0, 30, 70]);
}

/// The worker's move-off-the-main-lane gesture: free placement on the
/// destination plus a gap-close shift on the source, one history entry.
#[test]
fn move_off_with_gap_close_is_one_history_entry() {
    let (_dir, mut engine) = temp_engine();
    let main = add_track(&mut engine, TrackKind::Adjustment, "FX1");
    let overlay = add_track(&mut engine, TrackKind::Adjustment, "FX2");
    let a = add_generated(&mut engine, main, adjustment(), tr(0, 50));
    let b = add_generated(&mut engine, main, adjustment(), tr(50, 30));

    engine.begin_group();
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: overlay,
            start: rt(200),
        }))
        .expect("move off");
    engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track: main,
            from: rt(0),
            delta: rt(-50),
        }))
        .expect("close gap");
    engine.commit_group();

    assert_eq!(engine.project().timeline().track_of(a), Some(overlay));
    assert_eq!(starts(&engine, &[b]), [0], "gap closed behind the move");

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().track_of(a), Some(main));
    assert_eq!(starts(&engine, &[a, b]), [0, 50]);
}

/// The pack-on-enable gesture: one shift per gap, one history entry total.
#[test]
fn pack_gaps_as_one_history_entry() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    // Leading gap + inner gap: A [10,30) B [60,80).
    let a = add_generated(&mut engine, track, adjustment(), tr(10, 20));
    let b = add_generated(&mut engine, track, adjustment(), tr(60, 20));

    engine.begin_group();
    engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(10),
            delta: rt(-10),
        }))
        .expect("close leading gap");
    engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(50),
            delta: rt(-30),
        }))
        .expect("close inner gap");
    engine.commit_group();

    assert_eq!(starts(&engine, &[a, b]), [0, 20]);

    assert!(engine.undo(), "one undo restores both gaps");
    assert_eq!(starts(&engine, &[a, b]), [10, 60]);
}

/// `RippleInsert` through the wire surface: a single history entry that
/// shifts and places atomically, undone/redone as one.
#[test]
fn ripple_insert_command_is_single_history_entry() {
    let Some(asset) = small_video_asset() else {
        eprintln!("skipping: no mp4 asset available");
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &asset);
    let media_rate = engine.project().media(media).expect("media").frame_rate;
    let track = add_track(&mut engine, TrackKind::Video, "V1");
    let src = |start: i64, dur: i64| aria_models::TimeRange::at_rate(start, dur, media_rate);

    let first = add_media_clip(&mut engine, track, media, src(0, 24), rt(0));
    let second = add_media_clip(&mut engine, track, media, src(24, 24), rt(24));

    let outcome = engine
        .apply(Command::Edit(EditCommand::RippleInsert {
            track,
            media,
            source: src(0, 12),
            at: rt(24),
        }))
        .expect("ripple insert");
    let ApplyOutcome::Edited(EditOutcome::Created(inserted)) = outcome else {
        panic!("expected Created, got {outcome:?}");
    };
    let shifted = engine.project().clip(second).unwrap().start().value;
    assert!(shifted > 24, "later clip shifted right");
    assert_eq!(engine.project().clip(inserted).unwrap().start().value, 24);
    assert_eq!(engine.project().clip(first).unwrap().start().value, 0);

    assert!(
        engine.undo(),
        "one undo removes the insert and closes the hole"
    );
    assert!(engine.project().clip(inserted).is_none());
    assert_eq!(engine.project().clip(second).unwrap().start().value, 24);

    assert!(engine.redo());
    assert_eq!(engine.project().timeline().clip_count(), 3);
    assert_eq!(
        engine.project().clip(second).unwrap().start().value,
        shifted
    );
}

#[test]
fn ripple_move_on_a_packed_lane_closes_the_hole_and_undoes_as_one() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    let a = add_generated(&mut engine, track, adjustment(), tr(0, 50));
    let b = add_generated(&mut engine, track, adjustment(), tr(50, 30));
    let c = add_generated(&mut engine, track, adjustment(), tr(80, 40));

    engine
        .apply(Command::Edit(EditCommand::RippleMoveClip {
            clip: a,
            to_track: track,
            start: rt(120),
        }))
        .expect("ripple move");
    // Close [0,50), then drop A at 120-50 = 70: B 0, A 70, C 80? After close B0 C30,
    // dest 120>=50 so dest=70, open at 70 +50 shifts C from 30 to 80, place A at 70:
    // B 0-30, A 70-120, C 80-120 overlap — dest should be the packed end after close (70).
    // After close: B[0,30) C[30,70). dest 120-50=70. Open from 70: nothing. Place A at 70:
    // B[0,30) C[30,70) A[70,120).
    assert_eq!(starts(&engine, &[a, b, c]), [70, 0, 30]);

    assert!(engine.undo(), "one undo restores the packed lane");
    assert_eq!(starts(&engine, &[a, b, c]), [0, 50, 80]);
    assert!(engine.redo());
    assert_eq!(starts(&engine, &[a, b, c]), [70, 0, 30]);
}

#[test]
fn ripple_move_to_the_head_of_the_lane_pushes_the_rest() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    let a = add_generated(&mut engine, track, adjustment(), tr(0, 50));
    let b = add_generated(&mut engine, track, adjustment(), tr(50, 30));
    let c = add_generated(&mut engine, track, adjustment(), tr(80, 40));

    engine
        .apply(Command::Edit(EditCommand::RippleMoveClip {
            clip: c,
            to_track: track,
            start: rt(0),
        }))
        .expect("ripple move to head");
    // Lift C, close from 120 is empty, dest 0, open +40: A50 B80, place C at 0.
    assert_eq!(starts(&engine, &[a, b, c]), [40, 90, 0]);
    assert!(engine.undo());
    assert_eq!(starts(&engine, &[a, b, c]), [0, 50, 80]);
}

#[test]
fn ripple_move_takes_a_linked_companion_on_another_lane() {
    let (_dir, mut engine) = temp_engine();
    let picture = add_track(&mut engine, TrackKind::Adjustment, "V");
    let sound = add_track(&mut engine, TrackKind::Adjustment, "A");
    let a = add_generated(&mut engine, picture, adjustment(), tr(0, 50));
    let audio = add_generated(&mut engine, sound, adjustment(), tr(0, 50));
    let rest = add_generated(&mut engine, picture, adjustment(), tr(50, 30));
    engine
        .apply(Command::Edit(EditCommand::LinkClips {
            clips: vec![a, audio],
        }))
        .expect("link");

    engine
        .apply(Command::Edit(EditCommand::RippleMoveClip {
            clip: a,
            to_track: picture,
            start: rt(120),
        }))
        .expect("ripple move linked pair");
    // Same as the packed-lane case for picture: rest closes to 0, A lands at 70.
    // Audio takes the same delta and the same close adjustment on its own lane.
    assert_eq!(starts(&engine, &[a, rest, audio]), [70, 0, 70]);

    assert!(engine.undo(), "one undo restores both lanes");
    assert_eq!(starts(&engine, &[a, rest, audio]), [0, 50, 0]);
    assert!(engine.redo());
    assert_eq!(starts(&engine, &[a, rest, audio]), [70, 0, 70]);
}

#[test]
fn move_clip_takes_a_linked_companion_on_another_lane() {
    let (_dir, mut engine) = temp_engine();
    let picture = add_track(&mut engine, TrackKind::Adjustment, "V");
    let sound = add_track(&mut engine, TrackKind::Adjustment, "A");
    let a = add_generated(&mut engine, picture, adjustment(), tr(0, 50));
    let audio = add_generated(&mut engine, sound, adjustment(), tr(0, 50));
    engine
        .apply(Command::Edit(EditCommand::LinkClips {
            clips: vec![a, audio],
        }))
        .expect("link");

    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: picture,
            start: rt(80),
        }))
        .expect("move linked pair");
    assert_eq!(starts(&engine, &[a, audio]), [80, 80]);

    assert!(engine.undo(), "one undo restores both lanes");
    assert_eq!(starts(&engine, &[a, audio]), [0, 0]);
    assert!(engine.redo());
    assert_eq!(starts(&engine, &[a, audio]), [80, 80]);
}

#[test]
fn trim_clip_takes_a_linked_companion_on_another_lane() {
    let (_dir, mut engine) = temp_engine();
    let picture = add_track(&mut engine, TrackKind::Adjustment, "V1");
    let sound = add_track(&mut engine, TrackKind::Adjustment, "A1");
    let video = add_generated(&mut engine, picture, adjustment(), tr(0, 60));
    let audio = add_generated(&mut engine, sound, adjustment(), tr(0, 60));
    engine
        .apply(Command::Edit(EditCommand::LinkClips {
            clips: vec![video, audio],
        }))
        .expect("link");

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: video,
            timeline: tr(12, 36),
        }))
        .expect("trim linked pair");
    let v = engine.project().clip(video).expect("video clip");
    let a = engine.project().clip(audio).expect("audio clip");
    assert_eq!(
        (v.start().value, v.timeline.end_tick()),
        (12, 48),
        "primary clip trimmed"
    );
    assert_eq!(
        (a.start().value, a.timeline.end_tick()),
        (12, 48),
        "linked companion trimmed by the same start/end delta"
    );

    assert!(engine.undo(), "one undo restores both lanes");
    let v = engine.project().clip(video).expect("video clip");
    let a = engine.project().clip(audio).expect("audio clip");
    assert_eq!((v.start().value, v.timeline.end_tick()), (0, 60));
    assert_eq!((a.start().value, a.timeline.end_tick()), (0, 60));

    assert!(engine.redo());
    let v = engine.project().clip(video).expect("video clip");
    let a = engine.project().clip(audio).expect("audio clip");
    assert_eq!((v.start().value, v.timeline.end_tick()), (12, 48));
    assert_eq!((a.start().value, a.timeline.end_tick()), (12, 48));
}
