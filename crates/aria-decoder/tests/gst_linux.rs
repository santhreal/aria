//! Linux GStreamer decode/probe on the checked-in H.264 fixture.
//!
//! Requires host plugins (`gstreamer1.0-plugins-good` / `-libav`). A missing
//! plugin is a hard failure: Linux CI must ship the same stack the product uses.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

use aria_core::{PixelFormat, RationalTime, VideoDecoder, VideoFrame};
use aria_decoder::{AheadDecoder, GstDecoder, OutputMode, open_audio_reader, probe};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opengop_h264.mp4")
}

#[test]
fn probes_opengop_h264() {
    let meta = probe(&fixture()).expect("probe H.264 fixture");
    assert!(meta.width > 0, "probe width is 0");
    assert!(meta.height > 0, "probe height is 0");
    assert!(!meta.is_image, "H.264 fixture classified as a still");
    assert!(
        meta.frame_count > 0,
        "probe reported no duration; GStreamer discoverer/decoder must set one"
    );
}

#[test]
fn decodes_first_frame_as_cpu_planes() {
    let mut decoder = GstDecoder::open(&fixture(), OutputMode::Cpu).expect("open decoder");
    let info = decoder.info().clone();
    assert!(info.display_size.0 > 0 && info.display_size.1 > 0);
    assert!(
        matches!(info.pixel_format, PixelFormat::Nv12 | PixelFormat::Yuv420p),
        "unexpected CPU pixel_format: {:?}",
        info.pixel_format
    );
    let frame = decoder
        .next_frame()
        .expect("decode")
        .expect("fixture has at least one frame");
    assert!(frame.is_cpu(), "Cpu mode must stay on CPU planes");
    assert_eq!(frame.format, info.pixel_format);
    assert_eq!(
        (frame.width(), frame.height()),
        (info.display_size.0, info.display_size.1)
    );
}

#[test]
fn gpu_mode_opens_and_yields_a_frame() {
    let mut decoder = GstDecoder::open(&fixture(), OutputMode::Gpu).expect("open gpu decoder");
    let frame = decoder
        .next_frame()
        .expect("decode")
        .expect("fixture has at least one frame");
    // LINEAR DMABuf is a GPU frame; tiled/software hosts fall back to CPU.
    assert!(
        frame.is_gpu() || frame.is_cpu(),
        "Gpu mode must yield a frame on one of the two paths"
    );
    if frame.is_gpu() {
        let surface = frame.gpu().expect("gpu surface");
        assert_eq!(surface.kind(), aria_core::GpuSurfaceKind::DmaBuf);
        assert_ne!(surface.handle(), 0);
    }
}

#[test]
fn seek_lands_at_or_before_target() {
    let mut decoder = GstDecoder::open(&fixture(), OutputMode::Cpu).expect("open decoder");
    let rate = decoder.info().frame_rate;
    let duration = decoder
        .info()
        .duration
        .unwrap_or(RationalTime::new(rate.num.max(1) as i64, rate));
    let mid = RationalTime::new((duration.value / 2).max(1), duration.rate);
    let frame = decoder
        .frame_at(mid)
        .expect("seek decode")
        .expect("mid-file frame exists");
    assert!(
        frame.pts.compare(mid) != core::cmp::Ordering::Greater
            || frame.pts.seconds() + 1.0 >= mid.seconds(),
        "seek landed at {:?}, target {:?}",
        frame.pts,
        mid
    );
}

#[test]
fn audio_reader_opens_or_names_missing_track() {
    let path = fixture();
    match open_audio_reader(&path, 48_000, 2) {
        Ok(mut reader) => {
            let mut buf = [0.0f32; 256];
            let _ = reader.read(&mut buf).expect("read audio");
        }
        Err(err) => {
            let msg = err.to_string();
            assert!(
                msg.contains("audio") || msg.contains("GStreamer") || msg.contains("open"),
                "unexpected audio error: {msg}"
            );
        }
    }
}

/// The 30 fps fixture the render and engine tests use: an IDR every 30
/// frames, with B-frames.
fn gop_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4")
}

/// A frame's time in nanoseconds and an FNV-1a hash of its visible pixel
/// bytes. Row padding past the visible width is left out: a decoder's buffer
/// pool does not clear it, so it differs between two decodes of one frame.
fn identity(frame: &VideoFrame) -> (i64, u64) {
    let ns = i128::from(frame.pts.value) * 1_000_000_000 * i128::from(frame.pts.rate.den)
        / i128::from(frame.pts.rate.num);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let image = frame.cpu().expect("Cpu mode yields CPU frames");
    let width = frame.visible.width as usize;
    let sample = frame.format.bytes_per_sample();
    for (index, plane) in image.planes.iter().enumerate() {
        let row = match frame.format {
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => width * 4,
            PixelFormat::Yuv420p if index > 0 => width.div_ceil(2),
            _ if index > 0 => width.next_multiple_of(2) * sample,
            _ => width * sample,
        }
        .min(plane.stride);
        for line in plane.data.chunks(plane.stride.max(1)).take(plane.rows) {
            for byte in &line[..row.min(line.len())] {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    (
        i64::try_from(ns).expect("fixture times fit in i64 ns"),
        hash,
    )
}

fn open(path: &Path) -> GstDecoder {
    GstDecoder::open(path, OutputMode::Cpu).expect("open decoder")
}

/// Sequential decode presents the first frame at zero and each later frame
/// one frame period after the one before, once.
///
/// Closes two defects of H.264 files with B-frames, whose MP4 edit list
/// starts buffer timestamps two frames in: frames were labeled with the raw
/// buffer time, so every frame showed two frames late against seek targets,
/// and the preroll sample was returned twice, so frame 0 decoded twice. Does
/// not cover variable-frame-rate files.
#[test]
fn sequential_frames_start_at_zero_one_per_frame_period() {
    for (path, frames) in [(fixture(), 240), (gop_fixture(), 90)] {
        let mut decoder = open(&path);
        let rate = decoder.info().frame_rate;
        let mut ticks = Vec::new();
        while let Some(frame) = decoder.next_frame().expect("decode") {
            ticks.push(aria_core::resample(frame.pts, rate).value);
        }
        let expected: Vec<i64> = (0..frames).collect();
        assert_eq!(ticks, expected, "{}", path.display());
    }
}

/// Every way a caller reaches a frame returns the frame sequential decode
/// returns at that time: `frame_at` stepping forward (rolling), stepping
/// backward (a seek per call), on a fresh decoder, and the decode-ahead
/// wrapper in both directions.
///
/// Defends playback and export against showing a different frame than a
/// paused seek shows at the same time, which reads as a flash at a GOP
/// boundary. Does not cover GPU frames or files with variable frame rates.
#[test]
fn every_access_path_returns_the_sequential_frame() {
    let mut wrong = Vec::new();
    for path in [fixture(), gop_fixture()] {
        let mut decoder = open(&path);
        let mut sequential = Vec::new();
        let mut times = Vec::new();
        while let Some(frame) = decoder.next_frame().expect("decode") {
            sequential.push(identity(&frame));
            times.push(frame.pts);
        }
        assert!(
            sequential.len() > 30,
            "{} decoded too few frames",
            path.display()
        );
        let at = |tick: usize| times[tick];
        let first_pts = sequential[0].0;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let mut check = |how: &str, tick: usize, frame: Option<VideoFrame>| {
            let got = frame.as_ref().map(identity);
            if got != Some(sequential[tick]) {
                wrong.push(format!(
                    "{name:?} {how} tick {tick}: got {:?}, sequential {:?}",
                    got.map(|(ns, _)| ns - first_pts),
                    sequential[tick].0 - first_pts
                ));
            }
        };
        let ticks = 0..sequential.len();
        let mut forward = open(&path);
        for tick in ticks.clone() {
            check("forward", tick, forward.frame_at(at(tick)).expect("decode"));
        }
        let mut backward = open(&path);
        for tick in ticks.clone().rev() {
            check(
                "backward",
                tick,
                backward.frame_at(at(tick)).expect("decode"),
            );
        }
        for tick in ticks.clone().step_by(7) {
            check(
                "cold",
                tick,
                open(&path).frame_at(at(tick)).expect("decode"),
            );
        }
        let mut ahead = AheadDecoder::new(Box::new(open(&path)));
        for tick in ticks.clone() {
            check(
                "ahead forward",
                tick,
                ahead.frame_at(at(tick)).expect("decode"),
            );
        }
        for tick in ticks.clone().rev() {
            check(
                "ahead backward",
                tick,
                ahead.frame_at(at(tick)).expect("decode"),
            );
        }
    }
    assert!(
        wrong.is_empty(),
        "{} frames differ from sequential decode:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// A seek lands on a frame sequential decode returns, at or before its
/// target, for every target in the file.
///
/// Defends against a seek onto an open-GOP sync sample that is not an IDR:
/// the decoder flags the frames after it corrupt, and the ones it returns
/// after them differ from sequential decode. Does not cover GPU frames.
#[test]
fn every_seek_lands_on_a_sequential_frame_at_or_before_its_target() {
    let mut wrong = Vec::new();
    for path in [fixture(), gop_fixture()] {
        let mut decoder = open(&path);
        let rate = decoder.info().frame_rate;
        // The frame tick and pixels: a leading frame returned after a seek can
        // have a time that differs from its sequential one inside its tick.
        let id = |frame: &VideoFrame| {
            (
                aria_core::resample(frame.pts, rate).value,
                identity(frame).1,
            )
        };
        let mut sequential = Vec::new();
        let mut times = Vec::new();
        while let Some(frame) = decoder.next_frame().expect("decode") {
            sequential.push(id(&frame));
            times.push(frame.pts);
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let mut seeking = open(&path);
        for (tick, &at) in times.iter().enumerate().rev() {
            seeking.seek(at).expect("seek");
            let landed = seeking.next_frame().expect("decode").map(|f| id(&f));
            match landed.and_then(|id| sequential.iter().position(|seq| *seq == id)) {
                Some(index) if index <= tick => {}
                Some(index) => wrong.push(format!("{name:?} seek {tick}: landed on {index}")),
                None => wrong.push(format!(
                    "{name:?} seek {tick}: landed on a frame sequential decode never returns"
                )),
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} seeks landed wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
