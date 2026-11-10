//! WHY: a transport stream states no duration, and GStreamer's estimate
//! from its clock references read a 60 s stream of 192-byte packets (an
//! AVCHD `.MTS`) as 60.29 s and a 1 s stream as no length, which failed the
//! import. These tests close the class at the probe: a transport stream
//! lasts from its first video timestamp to one frame past its last, whatever
//! its packet stride, frame order, other streams, timestamp wrap, or where
//! the end of the file cuts a packet. Not caught: a stream whose first or
//! last timestamp is more than `HEAD` or `MAX_TAIL` bytes from its end of
//! the file.

use std::io::Cursor;

use aria_core::{Rational, RationalTime, VideoDecoder};

use super::*;
use crate::OutputMode;

const VIDEO: u16 = 0x100;
const SOUND: u16 = 0x101;
const PMT: u16 = 0x1000;
const NULL: u16 = 0x1FFF;
/// One frame at 24 fps, in 90 kHz ticks.
const FRAME: i64 = 3750;

/// A 188-byte packet of `pid` carrying `payload`, padded.
fn packet(pid: u16, unit_start: bool, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![
        SYNC,
        (u8::from(unit_start) << 6) | (pid >> 8) as u8,
        (pid & 0xFF) as u8,
        0x10,
    ];
    out.extend_from_slice(payload);
    out.resize(PACKET, 0xFF);
    out
}

/// A table section `id` holding `body`, behind a zero pointer field, with a
/// placeholder CRC.
fn section(id: u8, body: &[u8]) -> Vec<u8> {
    let length = body.len() + 4;
    let mut out = vec![0, id, 0xB0 | (length >> 8) as u8, length as u8];
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]);
    out
}

/// The program association table pointing program 1 at [`PMT`], and the
/// program map listing `streams` as (stream type, PID).
fn tables(streams: &[(u8, u16)]) -> Vec<Vec<u8>> {
    let pat = section(
        0x00,
        &[0, 1, 0xC1, 0, 0, 0, 1, 0xE0 | (PMT >> 8) as u8, PMT as u8],
    );
    let mut map = vec![0, 1, 0xC1, 0, 0, 0xE1, 0x00, 0xF0, 0];
    for (kind, pid) in streams {
        map.extend_from_slice(&[*kind, 0xE0 | (pid >> 8) as u8, *pid as u8, 0xF0, 0]);
    }
    vec![
        packet(0, true, &pat),
        packet(PMT, true, &section(0x02, &map)),
    ]
}

/// The packet starting a PES of `pid` stamped `pts`.
fn pes(pid: u16, pts: i64) -> Vec<u8> {
    let mut payload = vec![0, 0, 1, 0xE0, 0, 0, 0x80, 0x80, 5];
    payload.extend_from_slice(&[
        0x21 | ((pts >> 29) & 0x0E) as u8,
        (pts >> 22) as u8,
        ((pts >> 14) & 0xFE) as u8 | 1,
        (pts >> 7) as u8,
        ((pts << 1) & 0xFE) as u8 | 1,
    ]);
    packet(pid, true, &payload)
}

/// `packets` written at `stride` bytes apiece: 192-byte packets carry a
/// 4-byte prefix.
fn stream(packets: &[Vec<u8>], stride: usize) -> Vec<u8> {
    packets
        .iter()
        .flat_map(|packet| {
            let mut out = vec![0x40; stride - PACKET];
            out.extend_from_slice(packet);
            out
        })
        .collect()
}

/// The duration of `bytes`, in 90 kHz ticks.
fn ticks(bytes: Vec<u8>) -> Option<i64> {
    duration_from_reader(Cursor::new(bytes)).map(|duration| {
        assert_eq!(duration.rate, Rational::new(PTS_RATE, 1));
        duration.value
    })
}

/// The tables, then a video PES per stamp in `stamps`.
fn video(stamps: impl IntoIterator<Item = i64>) -> Vec<Vec<u8>> {
    let mut packets = tables(&[(0x1B, VIDEO)]);
    packets.extend(stamps.into_iter().map(|pts| pes(VIDEO, pts)));
    packets
}

const START: i64 = 3600 * 90_000;

#[test]
fn a_stream_lasts_from_its_first_frame_to_one_past_its_last_at_either_stride() {
    let frames = video((0..48).map(|k| START + k * FRAME));
    for stride in [PACKET, PACKET + 4] {
        assert_eq!(ticks(stream(&frames, stride)), Some(48 * FRAME), "{stride}");
    }
}

#[test]
fn frames_out_of_order_end_at_the_latest() {
    // Decode order of an open GOP with two B-frames per reference frame: the
    // first frame decoded is the third shown.
    let order = [2, 0, 1, 5, 3, 4, 8, 6, 7, 11, 9, 10];
    let frames = video(order.map(|k| START + k * FRAME));
    assert_eq!(ticks(stream(&frames, PACKET)), Some(12 * FRAME));
}

#[test]
fn the_first_video_stream_is_timed_not_the_sound() {
    // The sound is listed first and runs a second past the picture.
    let mut packets = tables(&[(0x0F, SOUND), (0x1B, VIDEO)]);
    for k in 0..24 {
        packets.push(pes(SOUND, START + k * 1920));
        packets.push(pes(VIDEO, START + k * FRAME));
    }
    packets.push(pes(SOUND, START + 2 * 90_000));
    assert_eq!(ticks(stream(&packets, PACKET)), Some(24 * FRAME));
    // With no picture, the first stream is timed.
    let mut packets = tables(&[(0x0F, SOUND)]);
    packets.extend((0..10).map(|k| pes(SOUND, START + k * 1920)));
    assert_eq!(ticks(stream(&packets, PACKET)), Some(10 * 1920));
}

#[test]
fn a_timestamp_that_wraps_counts_on() {
    let first = PTS_WRAP - 2 * FRAME;
    let frames = video((0..6).map(|k| (first + k * FRAME) % PTS_WRAP));
    assert_eq!(ticks(stream(&frames, PACKET)), Some(6 * FRAME));
}

#[test]
fn the_tail_is_read_from_a_packet_boundary_and_widened_to_the_last_frame() {
    // Over a megabyte of null packets after the last frame: the first tail
    // starts inside a packet and holds no timestamp.
    let mut packets = video((0..48).map(|k| START + k * FRAME));
    let fill = usize::try_from(FIRST_TAIL).expect("a size") / PACKET + 1000;
    packets.extend(std::iter::repeat_n(packet(NULL, false, &[]), fill));
    packets.push(packet(VIDEO, false, &[0xAB; 40]));
    let mut bytes = stream(&packets, PACKET + 4);
    // The file ends inside a packet.
    bytes.truncate(bytes.len() - 100);
    assert_eq!(ticks(bytes), Some(48 * FRAME));
}

#[test]
fn a_tail_short_of_the_reorder_limit_is_widened_to_the_last_frame_shown() {
    // The last frame shown is decoded before a megabyte of null packets, and
    // three frames shown ahead of it are decoded after.
    let mut packets = video((0..44).chain([47]).map(|k| START + k * FRAME));
    let fill = usize::try_from(FIRST_TAIL).expect("a size") / PACKET + 1000;
    packets.extend(std::iter::repeat_n(packet(NULL, false, &[]), fill));
    packets.extend([44, 45, 46].map(|k| pes(VIDEO, START + k * FRAME)));
    assert_eq!(ticks(stream(&packets, PACKET)), Some(48 * FRAME));
}

#[test]
fn a_file_that_is_not_a_timed_transport_stream_has_no_duration() {
    assert_eq!(ticks(Vec::new()), None);
    let mp4 = [&[0, 0, 0, 16][..], b"ftypisom", &[0; 1000]].concat();
    assert_eq!(ticks(mp4), None);
    // No program map.
    let untabled: Vec<Vec<u8>> = (0..8).map(|k| pes(VIDEO, START + k * FRAME)).collect();
    assert_eq!(ticks(stream(&untabled, PACKET)), None);
    // A program map and no timestamp.
    let mut silent = tables(&[(0x1B, VIDEO)]);
    silent.extend(std::iter::repeat_n(packet(VIDEO, false, &[1; 20]), 8));
    assert_eq!(ticks(stream(&silent, PACKET)), None);
}

/// A stream GStreamer muxes probes to every frame written and decodes its
/// last, at either packet stride, a second long or two.
#[test]
fn a_muxed_transport_stream_probes_to_its_last_frame() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let fps = Rational::new(24, 1);
    for (frames, blu_ray) in [(24, false), (24, true), (48, false), (48, true)] {
        let path = dir.path().join(format!("{frames}-{blu_ray}.mts"));
        crate::gst_test_media::run_to_eos(&format!(
            "videotestsrc num-buffers={frames} ! video/x-raw,width=64,height=48,framerate=24/1,format=I420 \
             ! x264enc speed-preset=ultrafast key-int-max=24 ! h264parse \
             ! mpegtsmux m2ts-mode={blu_ray} ! filesink location=\"{}\"",
            path.display()
        ));
        let probed =
            crate::probe(&path).unwrap_or_else(|err| panic!("probe {}: {err}", path.display()));
        assert_eq!(
            (probed.frame_rate, probed.frame_count),
            (fps, frames),
            "{}",
            path.display()
        );
        let mut decoder =
            crate::GstDecoder::open(&path, OutputMode::Cpu).expect("the decoder opens");
        let last = decoder
            .frame_at(RationalTime::new(frames - 1, fps))
            .expect("the last frame decodes")
            .expect("the stream holds its last frame");
        assert_eq!(
            aria_core::resample(last.pts, fps).value,
            frames - 1,
            "{}",
            path.display()
        );
    }
}
