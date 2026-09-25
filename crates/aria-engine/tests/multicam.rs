//! Defect class: Multicam group creation, placement, angle switching, and history.
//!
//! A multicam group is created from selected media with sync offsets, placed on
//! a timeline track as a multi-angle clip, and active angles are switched at the playhead.
//! Undoing angle switches and group creation cleanly restores previous state; redo
//! re-applies each mutation.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{
    MediaId, MediaSource, MulticamAngle, MulticamCut, MulticamGroupId, Rational, TimeRange,
    TrackId, TrackKind,
};
use common::{temp_engine, tr};

fn engine_with_three_media(duration: i64) -> (tempfile::TempDir, Engine, [MediaId; 3], TrackId) {
    let (dir, mut engine) = temp_engine();
    let (m1, m2, m3, track) = {
        let proj = engine.project_mut();
        let a = proj.add_media(MediaSource::new(
            "/media/cam_a.mp4",
            1920,
            1080,
            Rational::FPS_24,
            duration,
            true,
        ));
        let b = proj.add_media(MediaSource::new(
            "/media/cam_b.mp4",
            1920,
            1080,
            Rational::FPS_24,
            duration,
            true,
        ));
        let c = proj.add_media(MediaSource::new(
            "/media/cam_c.mp4",
            1920,
            1080,
            Rational::FPS_24,
            duration,
            true,
        ));
        let track_id = proj.add_track(TrackKind::Video, "V1");
        (a, b, c, track_id)
    };
    (dir, engine, [m1, m2, m3], track)
}

#[test]
fn multicam_engine_create_place_switch_and_undo_redo() {
    let (_dir, mut engine, [m1, m2, m3], track) = engine_with_three_media(1000);

    // 1. Create a multicam group with 3 angles
    let angles = vec![
        MulticamAngle::new(m1, 0).with_name("Cam 1"),
        MulticamAngle::new(m2, 24).with_name("Cam 2"),
        MulticamAngle::new(m3, -12).with_name("Cam 3"),
    ];

    let outcome = engine
        .apply(Command::Edit(EditCommand::CreateMulticamGroup {
            name: "Interview Multicam".into(),
            angles,
        }))
        .expect("create multicam group");

    assert_eq!(outcome, ApplyOutcome::Edited(EditOutcome::UpdatedPool));
    assert_eq!(engine.project().multicam_group_count(), 1);

    let group_id = engine
        .project()
        .multicam_group_iter()
        .next()
        .expect("group exists")
        .id;

    // 2. Place a multicam clip on track V1
    let place_outcome = engine
        .apply(Command::Edit(EditCommand::AddMulticamClip {
            track,
            group: group_id,
            source: tr(0, 240),
            start: common::rt(0),
        }))
        .expect("place multicam clip");

    let clip_id = match place_outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created clip, got {other:?}"),
    };

    let clip = engine.project().clip(clip_id).expect("clip on timeline");
    assert_eq!(clip.multicam_group(), Some(group_id));

    // 3. Switch angle to Cam 2 at tick 48
    engine
        .apply(Command::Edit(EditCommand::SwitchMulticamAngle {
            group: group_id,
            at: 48,
            angle: 1,
        }))
        .expect("switch to angle 1");

    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 48)
            .unwrap(),
        1
    );

    // 4. Switch angle to Cam 3 at tick 96
    engine
        .apply(Command::Edit(EditCommand::SwitchMulticamAngle {
            group: group_id,
            at: 96,
            angle: 2,
        }))
        .expect("switch to angle 2");

    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 96)
            .unwrap(),
        2
    );

    // 5. Undo switch at 96
    assert!(engine.undo(), "undo switch at 96");
    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 96)
            .unwrap(),
        1,
        "after undoing switch at 96, active angle at 96 falls back to cut at 48 (angle 1)"
    );

    // 6. Undo switch at 48
    assert!(engine.undo(), "undo switch at 48");
    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 48)
            .unwrap(),
        0,
        "after undoing switch at 48, active angle falls back to initial angle 0"
    );

    // 7. Undo clip placement
    assert!(engine.undo(), "undo add multicam clip");
    assert!(engine.project().clip(clip_id).is_none());

    // 8. Undo group creation
    assert!(engine.undo(), "undo create group");
    assert_eq!(engine.project().multicam_group_count(), 0);

    // 9. Redo all
    assert!(engine.redo(), "redo create group");
    assert_eq!(engine.project().multicam_group_count(), 1);

    assert!(engine.redo(), "redo add multicam clip");
    assert!(engine.project().clip(clip_id).is_some());

    assert!(engine.redo(), "redo switch at 48");
    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 48)
            .unwrap(),
        1
    );

    assert!(engine.redo(), "redo switch at 96");
    assert_eq!(
        engine
            .project()
            .active_multicam_angle_at(group_id, 96)
            .unwrap(),
        2
    );
}
