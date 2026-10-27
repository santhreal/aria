//! `OverwriteMoveClip` and `OverwriteMoveClipUnlinked`: the move a clip drag
//! drops.
//!
//! # Why
//!
//! A drag let go over another clip was refused, so a clip landed only in a gap
//! at least its own length. The overwrite move covers what it lands on by the
//! rule of `OverwriteClip`. The rule is asserted at every landing tick across a
//! lane of three clips, for a clip moved along that lane and for one moved onto
//! it from another lane, with both commands: every other clip keeps exactly its
//! ticks outside the landing span, the moved clip fills the span, one undo puts
//! every clip back on its ticks and one redo lands the same result. A linked
//! companion covers what it lands on with the linked command and stays with
//! the unlinked one, and one that would land before the sequence start lands
//! at it. A refused move leaves the project and the history as they were.
//!
//! Not covered: which command a gesture sends, which the UI's
//! `move_drop_tests` drive, and retimed clips, whose source ticks a trim
//! scales; the trim commands assert that.

mod common;

use aria_commands::{Command, EditCommand};
use aria_engine::Engine;
use aria_models::{ClipId, Generator, Rational, RationalTime, TrackId, TrackKind};
use common::{add_generated, add_track, rt, temp_engine, tr};

type Move = fn(ClipId, TrackId, RationalTime) -> EditCommand;

fn linked(clip: ClipId, to_track: TrackId, start: RationalTime) -> EditCommand {
    EditCommand::OverwriteMoveClip {
        clip,
        to_track,
        start,
    }
}

fn unlinked(clip: ClipId, to_track: TrackId, start: RationalTime) -> EditCommand {
    EditCommand::OverwriteMoveClipUnlinked {
        clip,
        to_track,
        start,
    }
}

const MOVES: [(&str, Move); 2] = [("linked", linked), ("unlinked", unlinked)];

fn solid() -> Generator {
    Generator::SolidColor {
        rgba: [255, 0, 0, 255],
    }
}

/// Every lane in stack order, each with its clips as `(id, start, end)`.
fn lanes(engine: &Engine) -> Vec<(TrackId, Vec<(ClipId, i64, i64)>)> {
    engine
        .project()
        .timeline()
        .tracks_ordered()
        .map(|track| {
            let clips = track
                .clips_ordered()
                .map(|clip| (clip.id, clip.timeline.start.value, clip.timeline.end_tick()))
                .collect();
            (track.id, clips)
        })
        .collect()
}

/// The `(start, end)` of every clip on `track`, in order.
fn spans(engine: &Engine, track: TrackId) -> Vec<(i64, i64)> {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("the lane")
        .clips_ordered()
        .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
        .collect()
}

fn span(engine: &Engine, clip: ClipId) -> (i64, i64) {
    let clip = engine.project().clip(clip).expect("the clip");
    (clip.timeline.start.value, clip.timeline.end_tick())
}

/// What the overwrite rule leaves of `others` around a landing on
/// `[at, end)`, with the landing itself: the part of each clip before the span
/// and the part after it, whatever the clip's shape.
fn covered(others: &[(i64, i64)], at: i64, end: i64) -> Vec<(i64, i64)> {
    let mut lane = vec![(at, end)];
    for &(start, stop) in others {
        if start < at {
            lane.push((start, stop.min(at)));
        }
        if stop > end {
            lane.push((start.max(end), stop));
        }
    }
    lane.sort_unstable();
    lane
}

#[test]
fn every_landing_covers_by_the_overwrite_rule_and_undoes_exactly() {
    for (name, send) in MOVES {
        for from_other_lane in [false, true] {
            let (_dir, mut engine) = temp_engine();
            let lane = add_track(&mut engine, TrackKind::Sticker, "Lane");
            let other = add_track(&mut engine, TrackKind::Sticker, "Other");
            // The last clip is long enough for the moved one to land inside it.
            let placed: Vec<ClipId> = [(0, 24), (36, 24), (72, 48)]
                .into_iter()
                .map(|(start, len)| add_generated(&mut engine, lane, solid(), tr(start, len)))
                .collect();
            let (moved, len, from) = if from_other_lane {
                (
                    add_generated(&mut engine, other, solid(), tr(0, 10)),
                    10,
                    "another lane",
                )
            } else {
                (placed[1], 24, "its own lane")
            };
            let moved_from = span(&engine, moved);
            let others: Vec<(i64, i64)> = spans(&engine, lane)
                .into_iter()
                .filter(|&span| from_other_lane || span != moved_from)
                .collect();

            for at in 0..=130 {
                let case = format!("{name}: a clip from {from} let go at {at}");
                let before = lanes(&engine);
                engine
                    .apply(Command::Edit(send(moved, lane, rt(at))))
                    .unwrap_or_else(|err| panic!("{case}: refused: {err}"));
                assert_eq!(
                    spans(&engine, lane),
                    covered(&others, at, at + len),
                    "{case}"
                );
                assert_eq!(
                    span(&engine, moved),
                    (at, at + len),
                    "{case}: the moved clip"
                );
                assert!(spans(&engine, other).is_empty(), "{case}: the lane it left");

                let after = lanes(&engine);
                assert!(engine.undo(), "{case}: undo");
                assert_eq!(lanes(&engine), before, "{case}: after undo");
                assert!(engine.redo(), "{case}: redo");
                assert_eq!(lanes(&engine), after, "{case}: after redo");
                assert!(engine.undo(), "{case}: the second undo");
            }
        }
    }
}

/// Two lanes: a clip at 0 on each, and a clip after it on each that the move
/// lands across. The two clips at 0 are joined by `join`.
fn pair(
    join: fn(Vec<ClipId>) -> EditCommand,
) -> (tempfile::TempDir, Engine, [TrackId; 2], [ClipId; 4]) {
    let (dir, mut engine) = temp_engine();
    let upper = add_track(&mut engine, TrackKind::Sticker, "Upper");
    let lower = add_track(&mut engine, TrackKind::Sticker, "Lower");
    let a = add_generated(&mut engine, upper, solid(), tr(0, 24));
    let b = add_generated(&mut engine, lower, solid(), tr(0, 24));
    let x = add_generated(&mut engine, upper, solid(), tr(48, 24));
    let y = add_generated(&mut engine, lower, solid(), tr(40, 24));
    engine
        .apply(Command::Edit(join(vec![a, b])))
        .expect("join the pair");
    (dir, engine, [upper, lower], [a, b, x, y])
}

fn link(clips: Vec<ClipId>) -> EditCommand {
    EditCommand::LinkClips { clips }
}

fn group(clips: Vec<ClipId>) -> EditCommand {
    EditCommand::GroupClips { clips }
}

#[test]
fn a_companion_covers_its_own_lane_unless_the_move_leaves_it() {
    // (join, command, whether the companion moves)
    let cases: [(&str, fn(Vec<ClipId>) -> EditCommand, Move, bool); 4] = [
        ("linked pair, linked move", link, linked, true),
        ("linked pair, unlinked move", link, unlinked, false),
        ("grouped pair, linked move", group, linked, true),
        ("grouped pair, unlinked move", group, unlinked, true),
    ];
    for (case, join, send, follows) in cases {
        let (_dir, mut engine, [upper, lower], [a, b, x, y]) = pair(join);
        let before = lanes(&engine);
        engine
            .apply(Command::Edit(send(a, upper, rt(36))))
            .unwrap_or_else(|err| panic!("{case}: refused: {err}"));
        assert_eq!(span(&engine, a), (36, 60), "{case}: the dragged clip");
        assert_eq!(span(&engine, x), (60, 72), "{case}: the clip it landed on");
        if follows {
            assert_eq!(span(&engine, b), (36, 60), "{case}: the companion");
            assert_eq!(
                span(&engine, y),
                (60, 64),
                "{case}: the clip the companion landed on"
            );
        } else {
            assert_eq!(span(&engine, b), (0, 24), "{case}: the companion");
            assert_eq!(
                span(&engine, y),
                (40, 64),
                "{case}: the clip beside the companion"
            );
        }
        assert_eq!(spans(&engine, upper).len(), 2, "{case}: the upper lane");
        assert_eq!(spans(&engine, lower).len(), 2, "{case}: the lower lane");
        assert!(engine.undo(), "{case}: one undo");
        assert_eq!(lanes(&engine), before, "{case}: after undo");
    }
}

#[test]
fn a_companion_moved_past_the_sequence_start_stops_there() {
    // The dragged clip lands where it is let go. A companion the same distance
    // would land before the sequence start, so it lands at the start instead
    // and the move is taken.
    for (name, send) in MOVES {
        let (_dir, mut engine) = temp_engine();
        let upper = add_track(&mut engine, TrackKind::Sticker, "Upper");
        let lower = add_track(&mut engine, TrackKind::Sticker, "Lower");
        let dragged = add_generated(&mut engine, upper, solid(), tr(30, 24));
        let companion = add_generated(&mut engine, lower, solid(), tr(10, 24));
        engine
            .apply(Command::Edit(group(vec![dragged, companion])))
            .expect("group the pair");
        engine
            .apply(Command::Edit(send(dragged, upper, rt(5))))
            .unwrap_or_else(|err| panic!("{name}: refused: {err}"));
        assert_eq!(span(&engine, dragged), (5, 29), "{name}: the dragged clip");
        assert_eq!(span(&engine, companion), (0, 24), "{name}: the companion");
    }
}

/// Refuse `command`, then check nothing changed and nothing was recorded: one
/// undo takes back the last step of the setup, `last`.
fn refused(engine: &mut Engine, command: EditCommand, last: ClipId, case: &str) {
    let before = lanes(engine);
    assert!(
        engine.apply(Command::Edit(command)).is_err(),
        "{case}: the move was taken"
    );
    assert_eq!(lanes(engine), before, "{case}: the refusal left a trace");
    assert!(engine.undo(), "{case}: undo");
    assert!(
        engine.project().clip(last).is_none(),
        "{case}: the refusal recorded a history step"
    );
}

#[test]
fn a_refused_move_leaves_no_trace() {
    for (name, send) in MOVES {
        // A picture lane cannot hold a sound.
        let (_dir, mut engine, [upper, _], [a, ..]) = pair(group);
        let sound = add_track(&mut engine, TrackKind::Audio, "Sound");
        let last = add_generated(&mut engine, upper, solid(), tr(100, 24));
        refused(
            &mut engine,
            send(a, sound, rt(0)),
            last,
            &format!("{name}: onto a lane of another kind"),
        );

        // Grouped clips keep their offset, so the dragged one lands on its
        // companion when both end up on the companion's lane.
        let (_dir, mut engine, [upper, lower], [a, ..]) = pair(group);
        let last = add_generated(&mut engine, upper, solid(), tr(100, 24));
        refused(
            &mut engine,
            send(a, lower, rt(12)),
            last,
            &format!("{name}: onto its own companion"),
        );

        let (_dir, mut engine, [upper, _], [a, ..]) = pair(group);
        let last = add_generated(&mut engine, upper, solid(), tr(100, 24));
        refused(
            &mut engine,
            send(a, upper, RationalTime::new(-1, Rational::FPS_24)),
            last,
            &format!("{name}: before the sequence start"),
        );
    }
}
