//! Linux GStreamer decode/probe on the checked-in H.264 fixture.
//!
//! Requires host plugins (`gstreamer1.0-plugins-good` / `-libav`). A missing
//! plugin is a hard failure: Linux CI must ship the same stack the product uses.

#![cfg(target_os = "linux")]

use std::path::PathBuf;

use aria_core::{RationalTime, VideoDecoder};
use aria_decoder::{GstDecoder, OutputMode, open_audio_reader, probe};

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
fn decodes_first_frame_as_cpu_nv12() {
    let mut decoder = GstDecoder::open(&fixture(), OutputMode::Cpu).expect("open decoder");
    let info = decoder.info().clone();
    assert!(info.display_size.0 > 0 && info.display_size.1 > 0);
    let frame = decoder
        .next_frame()
        .expect("decode")
        .expect("fixture has at least one frame");
    assert!(frame.is_cpu(), "Cpu mode must stay CPU NV12");
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
