//! `inverse_edits` tests covering structural undo (part 2). A `#[path]` child
//! of the `inverse_edits` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn multi_step_undo_unwinds_in_order() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let a = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 10),
            }))
            .expect("a"),
    );
    let b = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(20, 10),
            }))
            .expect("b"),
    );

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: a }))
        .expect("ripple");
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: b,
            to_track: track,
            start: rt(5),
        }))
        .expect("move");

    assert_eq!(engine.project().clip(b).unwrap().start().value, 5);

    assert!(engine.undo());
    assert_eq!(engine.project().clip(b).unwrap().start().value, 10);

    assert!(engine.undo());
    assert!(engine.project().clip(a).is_some());
    assert_eq!(engine.project().clip(b).unwrap().start().value, 20);
}

#[test]
fn redo_trim_after_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 48),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(12, 20),
        }))
        .expect("trim");
    assert_eq!(engine.project().clip(clip_id).unwrap().timeline, tr(12, 20));

    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip_id).unwrap().timeline, tr(0, 48));

    assert!(engine.redo());
    assert_eq!(engine.project().clip(clip_id).unwrap().timeline, tr(12, 20));
}

#[test]
fn redo_ripple_delete_after_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let first = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 10),
            }))
            .expect("first"),
    );
    let second = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(20, 10),
            }))
            .expect("second"),
    );

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: first }))
        .expect("ripple");
    assert!(engine.project().clip(first).is_none());
    assert_eq!(engine.project().clip(second).unwrap().start().value, 10);

    assert!(engine.undo());
    assert!(engine.project().clip(first).is_some());
    assert_eq!(engine.project().clip(second).unwrap().start().value, 20);

    assert!(engine.redo());
    assert!(engine.project().clip(first).is_none());
    assert_eq!(engine.project().clip(second).unwrap().start().value, 10);
}

#[test]
fn redo_move_after_undo() {
    let (_dir, mut engine) = temp_engine();
    let v1 = common::add_track(&mut engine, TrackKind::Adjustment, "FX");
    let v2 = common::add_track(&mut engine, TrackKind::Adjustment, "FX2");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track: v1,
                generator: Generator::Adjustment,
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: clip_id,
            to_track: v2,
            start: rt(30),
        }))
        .expect("move");

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().track_of(clip_id), Some(v1));
    assert_eq!(engine.project().clip(clip_id).unwrap().start().value, 0);

    assert!(engine.redo());
    assert_eq!(engine.project().timeline().track_of(clip_id), Some(v2));
    assert_eq!(engine.project().clip(clip_id).unwrap().start().value, 30);
}

#[test]
fn redo_remove_clip_after_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(5, 15),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::RemoveClip { clip: clip_id }))
        .expect("remove");
    assert!(engine.project().clip(clip_id).is_none());

    assert!(engine.undo());
    assert!(engine.project().clip(clip_id).is_some());

    assert!(engine.redo());
    assert!(engine.project().clip(clip_id).is_none());
}

#[test]
fn new_edit_clears_redo_stack() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 12),
        }))
        .expect("trim");

    assert!(engine.undo());
    assert!(engine.can_redo());

    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: clip_id,
            to_track: track,
            start: rt(6),
        }))
        .expect("move");

    assert!(!engine.can_redo());
    assert_eq!(engine.project().clip(clip_id).unwrap().start().value, 6);
}

#[test]
fn failed_move_overlap_does_not_push_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let _a = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 20),
            }))
            .expect("a"),
    );
    let b = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(30, 20),
            }))
            .expect("b"),
    );

    let err = engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: b,
            to_track: track,
            start: rt(5),
        }))
        .unwrap_err();
    assert!(format!("{err}").contains("overlap") || format!("{err}").contains("Overlap"));
    assert_eq!(engine.project().clip(b).unwrap().start().value, 30);
    // add-track + two adds are undoable; the failed move must not push a fourth step.
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert!(!engine.can_undo());
}
