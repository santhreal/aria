//! What one command costs on a project large enough for a per-command copy to
//! show. The claim under test is that applying an edit is proportional to the
//! edit, not to the project: the engine mutates the project in place and
//! records the inverse action, so a transform on clip 1 of 480 must cost what
//! it costs on clip 1 of 1.
//!
//! Run:
//!   `cargo bench -p aria-engine --bench command_apply`
//!
//! Each case re-applies its command with a different value, so the measured
//! work is a real edit and not a no-op the dispatcher can drop. Undo is
//! measured as a round trip because a one-way undo would drain the stack the
//! iteration after next.

use std::hint::black_box;

use aria_commands::{Command, EditCommand};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    ClipId, ClipTransform, Generator, Rational, RationalTime, TimeRange, TrackId, TrackKind,
};
use criterion::{Criterion, criterion_group, criterion_main};

/// Tracks and clips per track of the measured project: the same shape as the
/// UI's standard workload, so the two numbers describe one editor.
const TRACKS: usize = 8;
const CLIPS_PER_TRACK: usize = 60;
/// Timeline ticks each clip occupies, with a one-tick gap after it.
const CLIP_TICKS: i64 = 12;

fn rt(tick: i64) -> RationalTime {
    RationalTime::new(tick, Rational::FPS_24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, Rational::FPS_24)
}

struct Fixture {
    engine: Engine,
    tracks: Vec<TrackId>,
    /// Every clip, in creation order.
    clips: Vec<ClipId>,
}

/// A project of `TRACKS × CLIPS_PER_TRACK` solid clips. Solids keep the
/// measurement on the command path: no decode, no raster, no file.
fn large_project() -> Fixture {
    let mut engine = Engine::new(EngineConfig { undo_limit: 64 }).expect("engine");
    let mut tracks = Vec::with_capacity(TRACKS);
    let mut clips = Vec::with_capacity(TRACKS * CLIPS_PER_TRACK);
    for index in 0..TRACKS {
        let track = match engine
            .apply(Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Sticker,
                name: format!("ST{index}"),
                index: None,
                pinned: false,
            }))
            .expect("track")
        {
            ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(id)) => id,
            other => panic!("{other:?}"),
        };
        for slot in 0..CLIPS_PER_TRACK {
            let start = slot as i64 * (CLIP_TICKS + 1);
            let clip = match engine
                .apply(Command::Edit(EditCommand::AddGenerated {
                    track,
                    generator: Generator::SolidColor {
                        rgba: [40, 80, 120, 255],
                    },
                    timeline: tr(start, CLIP_TICKS),
                }))
                .expect("clip")
            {
                ApplyOutcome::Edited(aria_commands::EditOutcome::Created(id)) => id,
                other => panic!("{other:?}"),
            };
            clips.push(clip);
        }
        tracks.push(track);
    }
    Fixture {
        engine,
        tracks,
        clips,
    }
}

/// A transform that differs from the last one and stays on the canvas: the
/// dispatcher must see a real change, and a position an editor would refuse is
/// not the thing being measured.
fn transform(step: u32) -> ClipTransform {
    let offset = (step % 100) as f32 * 0.001;
    ClipTransform {
        position: [offset, -offset],
        ..ClipTransform::IDENTITY
    }
}

/// A parameter edit: one clip's transform, on a project of 480.
fn bench_set_transform(c: &mut Criterion) {
    let mut fixture = large_project();
    let clip = *fixture.clips.first().expect("a clip");
    let mut step = 0u32;
    c.bench_function("set_clip_transform_480_clips", |b| {
        b.iter(|| {
            step += 1;
            let outcome = fixture
                .engine
                .apply(Command::Edit(EditCommand::SetClipTransform {
                    clip,
                    transform: transform(step),
                    at: None,
                }))
                .expect("transform");
            black_box(outcome);
        });
    });
}

/// The same edit with its undo and redo, which is what Ctrl+Z costs: the
/// inverse the apply recorded, run back and forward again.
fn bench_undo_redo(c: &mut Criterion) {
    let mut fixture = large_project();
    let clip = *fixture.clips.first().expect("a clip");
    let mut step = 0u32;
    c.bench_function("transform_undo_redo_480_clips", |b| {
        b.iter(|| {
            step += 1;
            fixture
                .engine
                .apply(Command::Edit(EditCommand::SetClipTransform {
                    clip,
                    transform: transform(step),
                    at: None,
                }))
                .expect("transform");
            assert!(fixture.engine.undo(), "the transform had no inverse");
            assert!(fixture.engine.redo(), "the undo had nothing to redo");
        });
    });
}

/// A structural edit: split, then undo it, so the clip count returns and the
/// next iteration measures the same work rather than a growing timeline.
fn bench_split(c: &mut Criterion) {
    let mut fixture = large_project();
    let clip = *fixture.clips.first().expect("a clip");
    c.bench_function("split_clip_480_clips", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .engine
                    .apply(Command::Edit(EditCommand::SplitClip {
                        clip,
                        at: rt(CLIP_TICKS / 2),
                    }))
                    .expect("split"),
            );
            assert!(fixture.engine.undo(), "the split had no inverse");
        });
    });
}

/// A lane change: move one clip between two tracks and back, which touches two
/// tracks' clip lists on every iteration.
fn bench_move_between_tracks(c: &mut Criterion) {
    let mut fixture = large_project();
    let clip = *fixture.clips.first().expect("a clip");
    let home = *fixture.tracks.first().expect("a track");
    let away = *fixture.tracks.get(1).expect("a second track");
    // Well past the last clip on either lane, so the move never has to displace
    // a neighbour: the measurement is the move, not a ripple.
    let free = rt(CLIPS_PER_TRACK as i64 * (CLIP_TICKS + 1) + CLIP_TICKS);
    c.bench_function("move_clip_between_tracks_480_clips", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .engine
                    .apply(Command::Edit(EditCommand::MoveClip {
                        clip,
                        to_track: away,
                        start: free,
                    }))
                    .expect("move away"),
            );
            fixture
                .engine
                .apply(Command::Edit(EditCommand::MoveClip {
                    clip,
                    to_track: home,
                    start: rt(0),
                }))
                .expect("move home");
        });
    });
}

/// The same parameter edit on a project of one clip. Read against
/// `set_clip_transform_480_clips`: the two should agree, because an apply that
/// copied the project would not.
fn bench_set_transform_small(c: &mut Criterion) {
    let mut engine = Engine::new(EngineConfig { undo_limit: 64 }).expect("engine");
    let track = match engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Sticker,
            name: "ST0".into(),
            index: None,
            pinned: false,
        }))
        .expect("track")
    {
        ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(id)) => id,
        other => panic!("{other:?}"),
    };
    let clip = match engine
        .apply(Command::Edit(EditCommand::AddGenerated {
            track,
            generator: Generator::SolidColor {
                rgba: [40, 80, 120, 255],
            },
            timeline: tr(0, CLIP_TICKS),
        }))
        .expect("clip")
    {
        ApplyOutcome::Edited(aria_commands::EditOutcome::Created(id)) => id,
        other => panic!("{other:?}"),
    };
    let mut step = 0u32;
    c.bench_function("set_clip_transform_1_clip", |b| {
        b.iter(|| {
            step += 1;
            let outcome = engine
                .apply(Command::Edit(EditCommand::SetClipTransform {
                    clip,
                    transform: transform(step),
                    at: None,
                }))
                .expect("transform");
            black_box(outcome);
        });
    });
}

criterion_group!(
    benches,
    bench_set_transform,
    bench_set_transform_small,
    bench_undo_redo,
    bench_split,
    bench_move_between_tracks
);
criterion_main!(benches);
