//! `MoveClipUnlinked` and `TrimClipUnlinked`: the move and trim sent with
//! linked selection off.
//!
//! # Why
//!
//! `MoveClip` and `TrimClip` carry a clip's linked companions. The unlinked
//! pair must leave a linked companion where it is, still carry a grouped one on
//! a move (a group is never split by the toggle), and undo as one step. A
//! regression that routed the unlinked rows through the linked path, or that
//! dropped the group closure, shows up here as a companion at the wrong tick.
//!
//! Not caught here: which command a gesture sends; the UI suite
//! `linked_selection_tests` drives that.

mod common;

use aria_commands::{Command, EditCommand};
use aria_engine::Engine;
use aria_models::{ClipId, Generator, TrackId, TrackKind};
use common::{add_generated, add_track, temp_engine, tr};

/// Two 24-tick solids on two lanes, both starting at 0, joined by `join`.
fn pair(
    join: fn(Vec<ClipId>) -> EditCommand,
) -> (tempfile::TempDir, Engine, TrackId, ClipId, ClipId) {
    let (dir, mut engine) = temp_engine();
    let upper = add_track(&mut engine, TrackKind::Sticker, "Upper");
    let lower = add_track(&mut engine, TrackKind::Sticker, "Lower");
    let solid = Generator::SolidColor {
        rgba: [255, 0, 0, 255],
    };
    let a = add_generated(&mut engine, upper, solid.clone(), tr(0, 24));
    let b = add_generated(&mut engine, lower, solid, tr(0, 24));
    engine
        .apply(Command::Edit(join(vec![a, b])))
        .expect("join the pair");
    (dir, engine, upper, a, b)
}

fn link(clips: Vec<ClipId>) -> EditCommand {
    EditCommand::LinkClips { clips }
}

fn group(clips: Vec<ClipId>) -> EditCommand {
    EditCommand::GroupClips { clips }
}

fn start(engine: &Engine, clip: ClipId) -> i64 {
    engine.project().clip(clip).expect("clip").start().value
}

fn end(engine: &Engine, clip: ClipId) -> i64 {
    engine
        .project()
        .clip(clip)
        .expect("clip")
        .timeline
        .end_tick()
}

#[test]
fn an_unlinked_move_leaves_the_link_and_undoes_in_one_step() {
    let (_dir, mut engine, upper, a, b) = pair(link);
    engine
        .apply(Command::Edit(EditCommand::MoveClipUnlinked {
            clip: a,
            to_track: upper,
            start: tr(12, 24).start,
        }))
        .expect("move without links");
    assert_eq!((start(&engine, a), start(&engine, b)), (12, 0));
    assert_eq!(
        engine.project().clip(a).unwrap().link,
        engine.project().clip(b).unwrap().link,
        "the pair stays linked"
    );

    assert!(engine.undo());
    assert_eq!((start(&engine, a), start(&engine, b)), (0, 0));

    // The linked move, for contrast, carries the companion.
    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: a,
            to_track: upper,
            start: tr(12, 24).start,
        }))
        .expect("linked move");
    assert_eq!((start(&engine, a), start(&engine, b)), (12, 12));
}

#[test]
fn an_unlinked_move_still_carries_the_group() {
    let (_dir, mut engine, upper, a, b) = pair(group);
    engine
        .apply(Command::Edit(EditCommand::MoveClipUnlinked {
            clip: a,
            to_track: upper,
            start: tr(6, 24).start,
        }))
        .expect("move a group member");
    assert_eq!((start(&engine, a), start(&engine, b)), (6, 6));
    assert!(engine.undo(), "one step undoes the whole group");
    assert_eq!((start(&engine, a), start(&engine, b)), (0, 0));
}

#[test]
fn an_unlinked_trim_leaves_the_link_and_undoes_in_one_step() {
    let (_dir, mut engine, _upper, a, b) = pair(link);
    engine
        .apply(Command::Edit(EditCommand::TrimClipUnlinked {
            clip: a,
            timeline: tr(0, 20),
        }))
        .expect("trim without links");
    assert_eq!((end(&engine, a), end(&engine, b)), (20, 24));
    assert!(engine.undo());
    assert_eq!((end(&engine, a), end(&engine, b)), (24, 24));

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: a,
            timeline: tr(0, 20),
        }))
        .expect("linked trim");
    assert_eq!((end(&engine, a), end(&engine, b)), (20, 20));
}
