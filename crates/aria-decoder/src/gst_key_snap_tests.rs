//! Tests for [`super`]: where the video decoder starts after a key-unit seek.
//!
//! The class this closes: a seek whose target is on a video keyframe, in an
//! MP4 with an AAC track, decoding the whole GOP before it and clipping it,
//! because `qtdemux` snaps the segment to the audio packet before the target.
//! The invariant held here is that after any key-unit seek the first buffer
//! the video decoder receives is the latest video keyframe at or before the
//! target. The state-machine tests cover each branch of [`KeySnap`] with
//! synthetic buffers and events; the pipeline tests hold the invariant on a
//! real MP4 with H.264 and AAC.
//!
//! Not covered: a demuxer other than `qtdemux` (left alone by construction),
//! and the decode time saved, which the `scrub` bench scenario measures.

use aria_core::{Rational, RationalTime, VideoDecoder};
use gstreamer as gst;

use super::*;
use crate::{GstDecoder, OutputMode};

fn ms(n: u64) -> gst::ClockTime {
    gst::ClockTime::from_mseconds(n)
}

fn buffer(pts_ms: u64, key: bool) -> gst::Buffer {
    let mut buffer = gst::Buffer::new();
    {
        let buffer = buffer.get_mut().expect("a new buffer is writable");
        buffer.set_pts(ms(pts_ms));
        if !key {
            buffer.set_flags(gst::BufferFlags::DELTA_UNIT);
        }
    }
    buffer
}

fn segment(start_ms: u64) -> gst::Event {
    let mut segment = gst::FormattedSegment::<gst::ClockTime>::new();
    segment.set_start(ms(start_ms));
    gst::event::Segment::new(&segment)
}

/// A snap that has seen `qtdemux` (or not), armed for `target_ms`, with the
/// seek's flush and segment delivered.
fn seeked(qtdemux: bool, target_ms: u64, start_ms: u64) -> KeySnap {
    crate::gst::ensure_init().expect("GStreamer initialises");
    let snap = KeySnap::default();
    snap.qtdemux.store(qtdemux, Ordering::Release);
    snap.arm(ms(target_ms));
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(start_ms));
    snap
}

/// Each buffer of `stream` (pts in ms, keyframe) and whether it reached the
/// decoder.
fn feed(snap: &KeySnap, stream: &[(u64, bool)]) -> Vec<bool> {
    stream
        .iter()
        .map(|&(pts, key)| snap.admit(&buffer(pts, key)))
        .collect()
}

#[test]
fn a_segment_snapped_to_an_audio_packet_skips_to_the_video_keyframe() {
    // Target 2000 on a keyframe; the AAC packet before it starts at 1984, so
    // the video track restarts at the keyframe at 1000.
    let snap = seeked(true, 2000, 1984);
    let fed = feed(
        &snap,
        &[
            (1000, true),
            (1017, false),
            (1983, false),
            (2000, true),
            (2017, false),
        ],
    );
    assert_eq!(fed, [false, false, false, true, true]);
    assert_eq!(snap.first_fed(), Some(ms(2000)));
}

#[test]
fn a_segment_on_the_video_keyframe_is_left_alone() {
    let snap = seeked(true, 1500, 1000);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
    assert_eq!(snap.first_fed(), Some(ms(1000)));
}

/// A fragmented MP4 is not adjusted by `qtdemux`: its segment starts at the
/// target and no video keyframe need lie after the restart point.
#[test]
fn a_segment_at_the_target_is_left_alone() {
    let snap = seeked(true, 2000, 2000);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
}

/// Another demuxer is not adjusted to an audio packet; its segment start is
/// trusted.
#[test]
fn another_demuxer_is_left_alone() {
    let snap = seeked(false, 2000, 1984);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
}

/// A delta first buffer means the decoder was not restarted at a keyframe
/// before the target; nothing is consumed.
#[test]
fn a_delta_first_buffer_is_left_alone() {
    let snap = seeked(true, 2000, 1984);
    assert_eq!(feed(&snap, &[(1017, false), (2000, true)]), [true, true]);
}

/// Buffers and a segment from before the seek's flush are the old position's,
/// still in flight: they pass and do not decide.
#[test]
fn buffers_and_a_segment_before_the_flush_do_not_decide() {
    crate::gst::ensure_init().expect("GStreamer initialises");
    let snap = KeySnap::default();
    snap.qtdemux.store(true, Ordering::Release);
    snap.arm(ms(2000));
    snap.event(&segment(500));
    assert_eq!(feed(&snap, &[(500, true)]), [true]);
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(1984));
    assert_eq!(feed(&snap, &[(1000, true), (2000, true)]), [false, true]);
}

/// The skip ends at the target even when no keyframe came: a stream that
/// broke the `qtdemux` guarantee loses frames, never the rest of the file.
#[test]
fn a_skip_without_a_keyframe_ends_after_the_target() {
    let snap = seeked(true, 2000, 1984);
    let fed = feed(
        &snap,
        &[(1000, true), (1990, false), (2017, false), (2034, false)],
    );
    assert_eq!(fed, [false, false, true, true]);
}

/// Without a seek nothing is consumed, including after an unrelated flush.
#[test]
fn an_unarmed_snap_passes_everything() {
    crate::gst::ensure_init().expect("GStreamer initialises");
    let snap = KeySnap::default();
    snap.qtdemux.store(true, Ordering::Release);
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(1984));
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
}

/// The AAC encoder this host has; a missing one is a hard failure, as for
/// every other plugin the product needs.
fn aac_encoder() -> &'static str {
    ["avenc_aac", "voaacenc", "fdkaacenc"]
        .into_iter()
        .find(|name| gst::ElementFactory::find(name).is_some())
        .expect("an AAC encoder (gstreamer1.0-libav avenc_aac)")
}

/// A 4 s 60 fps H.264 MP4 with a keyframe every 60 frames and a 48 kHz AAC
/// track, whose 1024-sample packets never start on a whole second.
fn h264_aac_mp4(dir: &std::path::Path) -> std::path::PathBuf {
    crate::gst::ensure_init().expect("GStreamer initialises");
    let path = dir.join("keys-60-aac.mp4");
    let desc = format!(
        "videotestsrc num-buffers=240 pattern=ball ! video/x-raw,width=160,height=120,framerate=60/1 \
         ! x264enc key-int-max=60 bframes=0 speed-preset=ultrafast option-string=scenecut=0:min-keyint=60 \
         ! h264parse ! queue ! mp4mux name=mux ! filesink location=\"{}\" \
         audiotestsrc num-buffers=190 samplesperbuffer=1024 ! audio/x-raw,rate=48000,channels=2 \
         ! {} ! queue ! mux.",
        path.display(),
        aac_encoder()
    );
    let pipeline = gst::parse::launch(&desc).expect("the fixture pipeline parses");
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
    path
}

/// Seek `decoder` to `secs` and decode one frame: the frame's time and the
/// first buffer the video decoder received, both in whole milliseconds.
fn seek_and_decode(decoder: &mut GstDecoder, secs: f64) -> (u64, Option<u64>) {
    let target = RationalTime::new((secs * 60.0).round() as i64, Rational::new(60, 1));
    decoder.seek(target).expect("seek");
    let frame = decoder.next_frame().expect("decode").expect("a frame");
    let shown = (frame.pts.seconds() * 1000.0).round() as u64;
    let first_fed = decoder.key_snap().first_fed().map(|pts| pts.mseconds());
    (shown, first_fed)
}

#[test]
fn a_seek_to_a_video_keyframe_starts_the_decoder_at_that_keyframe() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path());
    let mut decoder = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open the fixture");
    for key in [1000, 2000, 3000] {
        let (shown, first_fed) = seek_and_decode(&mut decoder, key as f64 / 1000.0);
        assert_eq!(
            shown, key,
            "frame shown after a seek to the keyframe at {key} ms"
        );
        assert_eq!(
            first_fed,
            Some(key),
            "the decoder did not start at the keyframe at {key} ms"
        );
    }
}

#[test]
fn a_seek_between_keyframes_starts_the_decoder_at_the_keyframe_before() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path());
    let mut decoder = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open the fixture");
    for (target, key) in [(1500, 1000), (2750, 2000)] {
        let (shown, first_fed) = seek_and_decode(&mut decoder, target as f64 / 1000.0);
        assert_eq!(shown, key, "frame shown after a seek to {target} ms");
        assert_eq!(first_fed, Some(key));
    }
}
