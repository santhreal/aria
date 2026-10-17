//! Public command dispatch and history coverage for clip unlinking.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::ApplyOutcome;
use aria_models::{Generator, TrackKind};
use common::{add_generated, add_track, temp_engine, tr};

#[test]
fn unlink_command_undo_redo_dissolves_and_restores_full_group() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "Stickers");
    let a = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        tr(0, 24),
    );
    let b = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [0, 0, 255, 255],
        },
        tr(24, 24),
    );
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: a,
            transition_id: "crossfade".into(),
        }))
        .expect("add transition");

    let linked = engine
        .apply(Command::Edit(EditCommand::LinkClips { clips: vec![a, b] }))
        .expect("link clips");
    assert_eq!(
        linked,
        ApplyOutcome::Edited(EditOutcome::Updated(a)),
        "LinkClips reports its first input"
    );
    let group = engine.project().clip(a).unwrap().link;
    assert!(group.is_some());
    assert_eq!(engine.project().clip(b).unwrap().link, group);

    let unlinked = engine
        .apply(Command::Edit(EditCommand::UnlinkClips { clips: vec![b] }))
        .expect("unlink clips");
    assert_eq!(
        unlinked,
        ApplyOutcome::Edited(EditOutcome::Updated(b)),
        "UnlinkClips reports its first input"
    );
    assert_eq!(engine.project().clip(a).unwrap().link, None);
    assert_eq!(engine.project().clip(b).unwrap().link, None);
    assert_eq!(engine.project().clip(a).unwrap().start().value, 0);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 24);
    assert!(
        engine
            .project()
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(a)
            .is_some(),
        "unlinking is non-structural"
    );

    assert!(engine.undo());
    assert_eq!(engine.project().clip(a).unwrap().link, group);
    assert_eq!(engine.project().clip(b).unwrap().link, group);

    assert!(engine.redo());
    assert_eq!(engine.project().clip(a).unwrap().link, None);
    assert_eq!(engine.project().clip(b).unwrap().link, None);
    assert!(
        engine
            .project()
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(a)
            .is_some()
    );
}

#[test]
fn group_and_ungroup_commands_move_together_trim_separately_and_undo_redo() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "Stickers");
    let a = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        tr(0, 24),
    );
    let b = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [0, 255, 0, 255],
        },
        tr(24, 24),
    );

    // Grouping fewer than 2 clips fails.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::GroupClips { clips: vec![a] }))
            .is_err()
    );

    let grouped = engine
        .apply(Command::Edit(EditCommand::GroupClips { clips: vec![a, b] }))
        .expect("group clips");
    assert_eq!(grouped, ApplyOutcome::Edited(EditOutcome::Updated(a)));
    let group = engine.project().clip(a).unwrap().group;
    assert!(group.is_some());
    assert_eq!(engine.project().clip(b).unwrap().group, group);
    assert_eq!(engine.project().grouped_clips(group.unwrap()), vec![a, b]);
    assert_eq!(engine.project().move_companions(a), vec![a, b]);

    // Moving one member moves the group as one undo step.
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: track,
            start: tr(12, 24).start,
        }))
        .expect("move grouped clip right");
    assert_eq!(engine.project().clip(a).unwrap().start().value, 12);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 36);

    assert!(engine.undo());
    assert_eq!(engine.project().clip(a).unwrap().start().value, 0);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 24);

    assert!(engine.redo());
    assert_eq!(engine.project().clip(a).unwrap().start().value, 12);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 36);

    // Trims stay per-clip on grouped clips.
    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: a,
            timeline: tr(14, 22),
        }))
        .expect("trim grouped clip");
    assert_eq!(engine.project().clip(a).unwrap().start().value, 14);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 36);

    // Ungroup dissolves the group and undo/redo restores it.
    let ungrouped = engine
        .apply(Command::Edit(EditCommand::UngroupClips { clips: vec![b] }))
        .expect("ungroup clips");
    assert_eq!(ungrouped, ApplyOutcome::Edited(EditOutcome::Updated(b)));
    assert_eq!(engine.project().clip(a).unwrap().group, None);
    assert_eq!(engine.project().clip(b).unwrap().group, None);

    assert!(engine.undo());
    assert_eq!(engine.project().clip(a).unwrap().group, group);
    assert_eq!(engine.project().clip(b).unwrap().group, group);

    assert!(engine.redo());
    assert_eq!(engine.project().clip(a).unwrap().group, None);
    assert_eq!(engine.project().clip(b).unwrap().group, None);
}

#[test]
fn set_clips_enabled_toggles_and_undoes_in_one_step() {
    let (_dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "Stickers");
    let a = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        tr(0, 24),
    );
    let b = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [0, 0, 255, 255],
        },
        tr(24, 24),
    );
    assert!(engine.project().clip(a).unwrap().enabled);
    assert!(engine.project().clip(b).unwrap().enabled);

    let outcome = engine
        .apply(Command::Edit(EditCommand::SetClipsEnabled {
            clips: vec![a, b],
            enabled: false,
        }))
        .expect("disable clips");
    assert_eq!(outcome, ApplyOutcome::Edited(EditOutcome::Updated(a)));
    assert!(!engine.project().clip(a).unwrap().enabled);
    assert!(!engine.project().clip(b).unwrap().enabled);
    assert!(engine.project().clip(a).unwrap().is_silent());

    assert!(engine.undo());
    assert!(engine.project().clip(a).unwrap().enabled);
    assert!(engine.project().clip(b).unwrap().enabled);

    assert!(engine.redo());
    assert!(!engine.project().clip(a).unwrap().enabled);
    assert!(!engine.project().clip(b).unwrap().enabled);
}
