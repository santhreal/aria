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
use crate::gst_test_media::h264_aac_mp4;
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

fn segment_with_time(start_ms: u64, time_ms: u64) -> gst::Event {
    let mut segment = gst::FormattedSegment::<gst::ClockTime>::new();
    segment.set_start(ms(start_ms));
    segment.set_time(ms(time_ms));
    gst::event::Segment::new(&segment)
}

/// A snap that has seen `qtdemux` (or not), armed for `target_ms`, with the
/// seek's flush and segment delivered.
fn seeked(qtdemux: bool, target_ms: u64, start_ms: u64) -> KeySnap {
    crate::gst::ensure_init().expect("GStreamer initializes");
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
    assert_eq!(snap.seek_key(), Some(ms(2000)));
}

#[test]
fn a_segment_on_the_video_keyframe_is_left_alone() {
    let snap = seeked(true, 1500, 1000);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
    assert_eq!(snap.first_fed(), Some(ms(1000)));
    assert_eq!(snap.seek_key(), Some(ms(1000)));
}

/// A fragmented MP4 is not adjusted by `qtdemux`: its segment starts at the
/// target and no video keyframe need lie after the restart point.
#[test]
fn a_segment_at_the_target_is_left_alone() {
    let snap = seeked(true, 2000, 2000);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
    assert_eq!(snap.seek_key(), Some(ms(1000)));
}

/// Another demuxer is not adjusted to an audio packet; its segment start is
/// trusted.
#[test]
fn another_demuxer_is_left_alone() {
    let snap = seeked(false, 2000, 1984);
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
    assert_eq!(snap.seek_key(), Some(ms(1000)));
}

/// A delta first buffer means the decoder was not restarted at a keyframe
/// before the target; nothing is consumed, and the decode has no keyframe to
/// start from, including the one after it.
#[test]
fn a_delta_first_buffer_is_left_alone() {
    let snap = seeked(true, 2000, 1984);
    assert_eq!(feed(&snap, &[(1017, false), (2000, true)]), [true, true]);
    assert_eq!(snap.seek_key(), None);
}

/// Buffers and a segment from before the seek's flush are the old position's,
/// still in flight: they pass and do not decide.
#[test]
fn buffers_and_a_segment_before_the_flush_do_not_decide() {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let snap = KeySnap::default();
    snap.qtdemux.store(true, Ordering::Release);
    snap.arm(ms(2000));
    snap.event(&segment(500));
    assert_eq!(feed(&snap, &[(500, true)]), [true]);
    assert_eq!(snap.seek_key(), None);
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(1984));
    assert_eq!(feed(&snap, &[(1000, true), (2000, true)]), [false, true]);
    assert_eq!(snap.seek_key(), Some(ms(2000)));
}

/// A new seek forgets the keyframe the last one started at until its own
/// decode starts.
#[test]
fn a_new_seek_forgets_the_last_seeks_keyframe() {
    let snap = seeked(true, 1500, 1000);
    assert_eq!(feed(&snap, &[(1000, true)]), [true]);
    assert_eq!(snap.seek_key(), Some(ms(1000)));
    snap.arm(ms(3500));
    assert_eq!(snap.seek_key(), None);
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(3000));
    assert_eq!(feed(&snap, &[(3000, true)]), [true]);
    assert_eq!(snap.seek_key(), Some(ms(3000)));
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
    assert_eq!(snap.seek_key(), None);
}

/// WHY: an MP4 with B-frames carries an `edts` edit list that shifts buffer
/// PTS and `segment.start` ahead of stream time (`segment.time`) by the
/// composition offset (two frame periods, ~33 ms at 60 fps). When a seek to a
/// keyframe at stream time `2000 ms` snaps to the preceding AAC packet at
/// stream time `1984 ms`, `segment.start` is `2017 ms > 2000 ms`. Comparing
/// `segment.start` directly against unshifted `target` evaluated
/// `2017 < 2000 == false`, leaving `KeySnap` idle and decoding the entire
/// preceding 60-frame 4K HEVC GOP on every cold seek (~540 ms on santhserver).
#[test]
fn a_segment_with_an_edit_list_offset_skips_to_the_video_keyframe_and_bounds_at_shifted_target() {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let snap = KeySnap::default();
    snap.qtdemux.store(true, Ordering::Release);
    snap.arm(ms(2000));
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment_with_time(2017, 1984));
    let fed = feed(
        &snap,
        &[
            (1033, true),
            (1050, false),
            (2016, false),
            (2033, true),
            (2050, false),
        ],
    );
    assert_eq!(fed, [false, false, false, true, true]);
    assert_eq!(snap.first_fed(), Some(ms(2033)));
    assert_eq!(snap.seek_key(), Some(ms(2033)));

    // Without a keyframe, the skip must bound against `target + offset` (2033 ms),
    // not unshifted `target` (2000 ms): delta buffer at 2016 ms is still before
    // the shifted target and must be dropped.
    let fallback = KeySnap::default();
    fallback.qtdemux.store(true, Ordering::Release);
    fallback.arm(ms(2000));
    fallback.event(&gst::event::FlushStop::new(true));
    fallback.event(&segment_with_time(2017, 1984));
    let fed_fallback = feed(
        &fallback,
        &[(1033, true), (2016, false), (2050, false), (2067, false)],
    );
    assert_eq!(fed_fallback, [false, false, true, true]);
    assert_eq!(fallback.seek_key(), None);
}

/// Without a seek nothing is consumed, including after an unrelated flush.
#[test]
fn an_unarmed_snap_passes_everything() {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let snap = KeySnap::default();
    snap.qtdemux.store(true, Ordering::Release);
    snap.event(&gst::event::FlushStop::new(true));
    snap.event(&segment(1984));
    assert_eq!(feed(&snap, &[(1000, true), (1017, false)]), [true, true]);
    assert_eq!(snap.seek_key(), None);
}

/// The caps the decoder's sink pad receives for `codec`.
fn caps(codec: &str) -> gst::Event {
    gst::event::Caps::new(&gst::Caps::builder(codec).build())
}

/// An H.265 or H.266 decoder outputs only the leading frames that decode,
/// so no keyframe time marks frames before it as undecodable; every other
/// video codec reports it, and the last caps received decide.
#[test]
fn the_seek_key_is_withheld_only_for_codecs_whose_leading_frames_decode() {
    for (codec, key) in [
        ("video/x-h264", Some(ms(1000))),
        ("video/mpeg", Some(ms(1000))),
        ("video/x-h265", None),
        ("video/x-h266", None),
    ] {
        let snap = seeked(true, 2000, 2000);
        snap.event(&caps(codec));
        assert_eq!(feed(&snap, &[(1000, true)]), [true], "{codec}");
        assert_eq!(snap.seek_key(), key, "{codec}");
    }
    let snap = seeked(true, 2000, 2000);
    snap.event(&caps("video/x-h265"));
    snap.event(&caps("video/x-h264"));
    assert_eq!(feed(&snap, &[(1000, true)]), [true]);
    assert_eq!(snap.seek_key(), Some(ms(1000)));
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
    let mp4 = h264_aac_mp4(dir.path(), 0);
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
    let mp4 = h264_aac_mp4(dir.path(), 0);
    let mut decoder = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open the fixture");
    for (target, key) in [(1500, 1000), (2750, 2000)] {
        let (shown, first_fed) = seek_and_decode(&mut decoder, target as f64 / 1000.0);
        assert_eq!(shown, key, "frame shown after a seek to {target} ms");
        assert_eq!(first_fed, Some(key));
    }
}

#[test]
fn a_seek_to_a_video_keyframe_in_a_bframe_edit_list_mp4_starts_at_that_keyframe() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 2);
    let mut decoder = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open the fixture");
    for key in [1000, 2000, 3000] {
        let (shown, first_fed) = seek_and_decode(&mut decoder, key as f64 / 1000.0);
        assert_eq!(
            shown, key,
            "frame shown after a seek to the keyframe at {key} ms"
        );
        let fed_ms = first_fed.expect("a keyframe buffer reached the decoder");
        assert!(
            (key..=key + 100).contains(&fed_ms),
            "the decoder started at buffer PTS {fed_ms} ms instead of the shifted keyframe at ~{} ms",
            key + 33
        );
    }
}
