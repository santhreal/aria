//! WHY: a copy cut short (an interrupted transfer, a card pulled mid-write)
//! keeps the index of the whole file, so it states the full length while
//! holding only its first part. Opening it at a start inside that length but
//! past the data seeked to nothing and prerolled no frame, and the open failed
//! with "GStreamer produced no video preroll. The file may have no video
//! track." A frame request or a scrub past the cut reported a file with no
//! video rather than one that ends early.
//!
//! These tests close that class at `GstDecoder::open_at`, the open every
//! look-ahead decoder takes: for a faststart file cut at several points, every
//! start across the stated length opens, a start inside the data reads the
//! frame on screen there, the one within half a frame period of it, and a
//! start past it reads no frame, which the renderer reports as the file ending
//! early. They do not cover a file whose index is at the end, which a cut
//! leaves unopenable at any start and the probe refuses.
use std::path::Path;

use aria_core::{Rational, RationalTime, VideoDecoder};

use crate::{GstDecoder, OutputMode};

/// Faststart: the index precedes the media data, so a cut keeps it.
const FIXTURE: &str = "tests/fixtures/fixture-640x360-30fps-3s.mp4";
const FRAME: f64 = 1.0 / 30.0;
/// The starts tried, in milliseconds, across the stated three seconds.
const STARTS_MS: std::ops::RangeInclusive<i64> = 1..=11;

fn at(ms: i64) -> RationalTime {
    RationalTime::new(ms, Rational::new(1000, 1))
}

/// The pts of the last frame `path` decodes from its start.
fn last_decoded(path: &Path) -> f64 {
    let mut decoder = GstDecoder::open(path, OutputMode::Cpu).expect("the cut file opens at zero");
    let mut last = None;
    while let Ok(Some(frame)) = decoder.next_frame() {
        last = Some(frame.pts.seconds());
    }
    last.expect("the cut keeps at least one frame")
}

#[test]
fn every_start_in_a_cut_file_opens_and_reads_a_frame_only_inside_the_data() {
    if super::ensure_init().is_err() {
        return;
    }
    let whole = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(FIXTURE),
    )
    .expect("the fixture reads");
    let dir = tempfile::tempdir().expect("a scratch directory");
    for percent in [30, 50, 70] {
        let cut = dir.path().join(format!("cut-{percent}.mp4"));
        std::fs::write(&cut, &whole[..whole.len() * percent / 100]).expect("the cut writes");
        let last = last_decoded(&cut);
        let (mut inside, mut past) = (0, 0);
        for step in STARTS_MS {
            let ms = step * 250;
            let seconds = ms as f64 / 1000.0;
            let mut decoder = GstDecoder::open_at(&cut, OutputMode::Cpu, at(ms))
                .unwrap_or_else(|err| panic!("{percent}% cut, start {seconds} s: {err}"));
            let frame = decoder
                .frame_at(at(ms))
                .unwrap_or_else(|err| panic!("{percent}% cut, read at {seconds} s: {err}"));
            if seconds <= last - FRAME {
                inside += 1;
                let frame = frame.unwrap_or_else(|| {
                    panic!("{percent}% cut ends at {last} s and read no frame at {seconds} s")
                });
                let pts = frame.pts.seconds();
                assert!(
                    (pts - seconds).abs() <= FRAME / 2.0 + 1e-6,
                    "{percent}% cut, {seconds} s: read the frame at {pts} s"
                );
            } else if seconds >= last + 2.0 * FRAME {
                past += 1;
                assert!(
                    frame.is_none(),
                    "{percent}% cut ends at {last} s and read a frame at {seconds} s"
                );
            }
        }
        assert!(
            inside > 0 && past > 0,
            "{percent}% cut ends at {last} s: the starts reach {inside} inside and {past} past it"
        );
    }
}

/// The retry at stream start is for a start the data does not reach: on the
/// whole file every start opens where it was asked and reads its own frame.
#[test]
fn every_start_in_the_whole_file_reads_its_own_frame() {
    if super::ensure_init().is_err() {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(FIXTURE);
    for step in STARTS_MS {
        let ms = step * 250;
        let seconds = ms as f64 / 1000.0;
        let mut decoder = GstDecoder::open_at(&path, OutputMode::Cpu, at(ms))
            .unwrap_or_else(|err| panic!("start {seconds} s: {err}"));
        let frame = decoder
            .frame_at(at(ms))
            .expect("the whole file decodes")
            .unwrap_or_else(|| panic!("no frame at {seconds} s"));
        let pts = frame.pts.seconds();
        assert!(
            (pts - seconds).abs() <= FRAME / 2.0 + 1e-6,
            "{seconds} s read the frame at {pts} s"
        );
    }
}
