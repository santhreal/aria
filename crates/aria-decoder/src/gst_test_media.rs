//! Synthetic MP4 files the GStreamer decode tests open.

use std::ops::Range;
use std::path::{Path, PathBuf};

use aria_core::Rotation;
use gstreamer as gst;
use gstreamer::prelude::*;

/// The AAC encoder this host has; a missing one is a hard failure, as for
/// every other plugin the product needs.
fn aac_encoder() -> &'static str {
    ["avenc_aac", "voaacenc", "fdkaacenc"]
        .into_iter()
        .find(|name| gst::ElementFactory::find(name).is_some())
        .expect("an AAC encoder (gstreamer1.0-libav avenc_aac)")
}

/// A 4 s 60 fps 160x120 H.264 MP4 with a keyframe every 60 frames and a
/// 48 kHz AAC track, whose 1024-sample packets never start on a whole second.
/// `bframes` above zero makes `mp4mux` write an `edts` edit list whose
/// composition offset shifts `segment.start` ahead of `segment.time` by that
/// many frame periods.
pub(crate) fn h264_aac_mp4(dir: &Path, bframes: u32) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("keys-60-b{bframes}-aac.mp4"));
    let desc = format!(
        "videotestsrc num-buffers=240 pattern=ball ! video/x-raw,width=160,height=120,framerate=60/1 \
         ! x264enc key-int-max=60 bframes={bframes} b-adapt=false speed-preset=ultrafast option-string=scenecut=0:min-keyint=60 \
         ! h264parse ! queue ! mp4mux name=mux ! filesink location=\"{}\" \
         audiotestsrc num-buffers=190 samplesperbuffer=1024 ! audio/x-raw,rate=48000,channels=2 \
         ! {} ! queue ! mux.",
        path.display(),
        aac_encoder()
    );
    run_to_eos(&desc);
    path
}

/// Run the pipeline `desc` describes to its end of stream.
fn run_to_eos(desc: &str) {
    let pipeline = gst::parse::launch(desc).expect("the fixture pipeline parses");
    pipeline
        .set_state(gst::State::Playing)
        .expect("the fixture pipeline plays");
    let bus = pipeline.bus().expect("a pipeline has a bus");
    let msg = bus
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(60),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the fixture finishes within 60 s");
    pipeline
        .set_state(gst::State::Null)
        .expect("the fixture pipeline stops");
    assert!(
        matches!(msg.view(), gst::MessageView::Eos(_)),
        "the fixture failed: {msg:?}"
    );
}

/// A 72-frame 24 fps VP8 WebM written for streaming, with no `Duration`
/// element and no cues, as a browser's `MediaRecorder` writes one. With
/// `muxed` set, the muxer writes it for a seekable file instead, stating its
/// duration.
pub(crate) fn vp8_webm(dir: &Path, muxed: bool) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("vp8-streamable-{}.webm", !muxed));
    let desc = format!(
        "videotestsrc num-buffers=72 ! video/x-raw,width=64,height=48,framerate=24/1 \
         ! vp8enc deadline=1 keyframe-max-dist=24 ! webmmux streamable={} \
         ! filesink location=\"{}\"",
        !muxed,
        path.display(),
    );
    run_to_eos(&desc);
    path
}

/// Frame size of [`rotated_h264_mp4`] as encoded, before its display matrix.
pub(crate) const ROTATED_SIZE: (u32, u32) = (64, 32);

/// A 4-frame 30 fps H.264 MP4 of [`ROTATED_SIZE`] whose track header states
/// `rotation` in its display matrix, as a phone writes one for video held
/// upright (`Cw90`). `mp4mux` writes no matrix from an `image-orientation`
/// tag, so the identity matrix it writes is replaced in the file.
pub(crate) fn rotated_h264_mp4(dir: &Path, rotation: Rotation) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("rotated-{}.mp4", rotation.degrees()));
    let (width, height) = ROTATED_SIZE;
    let desc = format!(
        "videotestsrc num-buffers=4 ! video/x-raw,width={width},height={height},framerate=30/1 \
         ! x264enc speed-preset=ultrafast ! h264parse ! mp4mux ! filesink location=\"{}\"",
        path.display(),
    );
    run_to_eos(&desc);
    let mut file = std::fs::read(&path).expect("the fixture was written");
    let tkhd = ["moov", "trak", "tkhd"]
        .into_iter()
        .try_fold(0..file.len(), |within, kind| child_box(&file, within, kind))
        .expect("the fixture has a track header");
    // Version 0 headers hold 32-bit times, version 1 headers 64-bit ones.
    let matrix = tkhd.start + if file[tkhd.start] == 1 { 52 } else { 40 };
    for (i, value) in display_matrix(rotation).into_iter().enumerate() {
        file[matrix + i * 4..matrix + i * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    std::fs::write(&path, file).expect("the fixture is rewritten");
    path
}

/// The ISO BMFF display matrix `(a, b, u, c, d, v, x, y, w)` that turns the
/// picture `rotation` clockwise, in 16.16 fixed point with `w` in 2.30.
fn display_matrix(rotation: Rotation) -> [i32; 9] {
    const ONE: i32 = 0x1_0000;
    let (a, b, c, d) = match rotation {
        Rotation::None => (ONE, 0, 0, ONE),
        Rotation::Cw90 => (0, ONE, -ONE, 0),
        Rotation::Cw180 => (-ONE, 0, 0, -ONE),
        Rotation::Cw270 => (0, -ONE, ONE, 0),
    };
    [a, b, 0, c, d, 0, 0, 0, 0x4000_0000]
}

/// The payload of the first box of type `kind` among the boxes in `within`.
fn child_box(file: &[u8], within: Range<usize>, kind: &str) -> Option<Range<usize>> {
    let mut at = within.start;
    while at + 8 <= within.end {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().ok()?) as usize;
        if size < 8 || at + size > within.end {
            return None;
        }
        if &file[at + 4..at + 8] == kind.as_bytes() {
            return Some(at + 8..at + size);
        }
        at += size;
    }
    None
}
