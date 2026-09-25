//! Scrub seek cost, cold and warm.
//!
//! Run: `cargo bench -p aria-decoder --bench seek_latency`
//!
//! A scrub is a seek per mouse move, and the number nobody had was what one
//! costs. Two shapes, because they are different products:
//!
//! - **Cold** opens the decoder and asks for one frame far from the start. This
//!   is what clicking into the middle of a clip costs, and it includes demuxer
//!   setup and the walk from the preceding keyframe.
//! - **Warm** holds one open decoder and asks for a sequence of times: a
//!   backward jump (which must re-seek), a short forward step inside the same
//!   GOP (which a backend may serve by rolling forward), and a long forward
//!   jump. Those three are the scrub gestures the preview actually issues.
//!
//! The fixture is the committed `tests/fixtures/fixture-640x360-24fps-3s.mp4`, so the
//! number is comparable between runs and between machines: a bench that encoded
//! its own clip would be measuring the encoder's GOP choices as well.
//!
//! A host with no native decoder — or one whose decoder refuses the fixture —
//! prints why and measures nothing rather than reporting a zero.

use std::path::PathBuf;
use std::time::Duration;

use aria_core::{Rational, RationalTime, VideoDecoder};
use aria_decoder::{OutputMode, open_video_decoder};
use criterion::{Criterion, criterion_group, criterion_main};

/// The committed clip: 640x360, 24 fps, three seconds.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4")
}

const RATE: Rational = Rational::FPS_24;

/// Frames into the clip. 60 is mid-clip at 24 fps, far enough from the start
/// that a seek cannot be answered by the first keyframe alone.
const MID: i64 = 60;

/// The scrub gestures a warm decoder is asked for, in order: back, one step
/// forward, then a long jump forward.
const WARM_TARGETS: [i64; 4] = [MID, 12, 13, 62];

fn at(frame: i64) -> RationalTime {
    RationalTime::new(frame, RATE)
}

/// A decoder over the fixture, or `None` with the reason printed once.
fn decoder() -> Option<Box<dyn VideoDecoder>> {
    let path = fixture();
    if !path.exists() {
        eprintln!("skipping seek latency: {} is missing", path.display());
        return None;
    }
    match open_video_decoder(&path, OutputMode::Cpu) {
        Ok(decoder) => Some(decoder),
        Err(err) => {
            eprintln!("skipping seek latency: no native decoder ({err})");
            None
        }
    }
}

fn bench_seek(c: &mut Criterion) {
    let Some(mut warm) = decoder() else {
        return;
    };

    let mut group = c.benchmark_group("seek_latency");
    // A cold open plus a mid-clip decode is tens of milliseconds on a software
    // path, so the default hundred samples would run for minutes.
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    let path = fixture();
    group.bench_function("cold_open_and_seek", |b| {
        b.iter(|| {
            let mut decoder =
                open_video_decoder(&path, OutputMode::Cpu).expect("the fixture opens");
            let frame = decoder
                .frame_at(at(MID))
                .expect("seek into the fixture")
                .expect("the fixture has a frame at MID");
            std::hint::black_box(frame.pts)
        })
    });

    // One decoder for every iteration: the gesture being measured is a scrub on
    // a clip that is already open, which is every mouse move after the first.
    group.bench_function("warm_scrub", |b| {
        b.iter(|| {
            for target in WARM_TARGETS {
                let frame = warm
                    .frame_at(at(target))
                    .expect("seek within the open fixture");
                std::hint::black_box(frame.map(|frame| frame.pts));
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_seek);
criterion_main!(benches);
