//! WHY: a WebM or Matroska file with no `Duration` element (a browser's
//! `MediaRecorder` output, a recording stopped before it was finalized)
//! probed to the length GStreamer's demuxer estimated from its first
//! clusters, so a three-second recording imported as a third of a second and
//! the rest of it could not be placed. These tests close the class at the
//! probe: a file with no stated duration lasts to the end of its last block,
//! whatever states the block's length (its own duration, its track's frame
//! duration, or the step between the track's blocks), however the last
//! cluster is sized or cut off, and a file that states its duration is left
//! to the demuxer. Not caught: a last cluster more than `MAX_TAIL` bytes
//! long, which reads as no duration.

use std::io::Cursor;

use aria_core::{Rational, RationalTime, VideoDecoder};

use super::*;
use crate::OutputMode;

/// The ID's bytes without leading zero bytes.
fn id_bytes(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(3);
    bytes[first..].to_vec()
}

/// The element `id` holding `payload`, its size written in eight bytes.
fn element(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    let size = payload.len() as u64 | (1 << 56);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The header of the element `id` of unknown size.
fn unknown(id: u32) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    out
}

fn unsigned(id: u32, value: u64) -> Vec<u8> {
    element(id, &value.to_be_bytes())
}

/// A SimpleBlock of `track` at `offset` ticks with `flags`, then `rest`.
fn block(track: u8, offset: i16, flags: u8, rest: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x80 | track];
    payload.extend_from_slice(&offset.to_be_bytes());
    payload.push(flags);
    payload.extend_from_slice(rest);
    element(SIMPLE_BLOCK, &payload)
}

/// A cluster at `timestamp` ticks holding `children`, of known size.
fn cluster(timestamp: u64, children: &[Vec<u8>]) -> Vec<u8> {
    element(CLUSTER, &cluster_body(timestamp, children))
}

fn cluster_body(timestamp: u64, children: &[Vec<u8>]) -> Vec<u8> {
    let mut body = unsigned(CLUSTER_TIMESTAMP, timestamp);
    body.extend(children.iter().flatten());
    body
}

/// A track entry numbered `number`, stating `default` nanoseconds per frame.
fn track(number: u64, default: Option<u64>) -> Vec<u8> {
    let mut entry = unsigned(TRACK_NUMBER, number);
    if let Some(default) = default {
        entry.extend(unsigned(DEFAULT_DURATION, default));
    }
    element(TRACK_ENTRY, &entry)
}

/// A WebM file: its EBML header, then a Segment of unknown size holding
/// `info`'s children as its Info, `tracks`, then `rest`.
fn webm(info: &[Vec<u8>], tracks: &[Vec<u8>], rest: &[Vec<u8>]) -> Vec<u8> {
    let mut out = element(EBML, &element(0x4282, b"webm"));
    out.extend(unknown(SEGMENT));
    out.extend(element(INFO, &info.concat()));
    out.extend(element(TRACKS, &tracks.concat()));
    out.extend(rest.iter().flatten());
    out
}

/// The duration read from `bytes`, in nanoseconds.
fn read_ns(bytes: Vec<u8>) -> Option<i64> {
    duration_from_reader(Cursor::new(bytes)).map(|duration| {
        assert_eq!(duration.rate, Rational::new(NS_PER_SEC, 1));
        duration.value
    })
}

const FRAME_NS: u64 = 41_666_666;

#[test]
fn a_stated_duration_is_left_to_the_demuxer_and_a_zero_one_is_not_stated() {
    let video = [track(1, Some(FRAME_NS))];
    let clusters = [cluster(0, &[block(1, 0, 0x80, b"x")])];
    let stated = element(DURATION, &3000.0_f64.to_be_bytes());
    assert_eq!(read_ns(webm(&[stated], &video, &clusters)), None);
    let short = element(DURATION, &2.5_f32.to_be_bytes());
    assert_eq!(read_ns(webm(&[short], &video, &clusters)), None);
    let zero = element(DURATION, &0.0_f64.to_be_bytes());
    assert_eq!(
        read_ns(webm(&[zero], &video, &clusters)),
        Some(FRAME_NS as i64)
    );
}

#[test]
fn the_end_is_the_last_block_and_its_track_frame_duration() {
    let video = [track(1, Some(FRAME_NS))];
    let clusters = [
        cluster(0, &[block(1, 0, 0x80, b"k"), block(1, 42, 0, b"p")]),
        cluster(2000, &[block(1, 0, 0x80, b"k"), block(1, 958, 0, b"p")]),
    ];
    assert_eq!(
        read_ns(webm(&[], &video, &clusters)),
        Some(2_958_000_000 + FRAME_NS as i64)
    );
}

#[test]
fn a_block_lasts_its_stated_duration_else_its_track_step_per_laced_frame() {
    // Track 2 states no frame duration: its blocks step 20 ticks, and the
    // last laces three frames. Track 3's block states its own duration.
    let tracks = [track(2, None), track(3, None)];
    let grouped = element(
        BLOCK_GROUP,
        &[
            element(BLOCK, &[0x83, 0, 30, 0, b'g']),
            unsigned(BLOCK_DURATION, 90),
        ]
        .concat(),
    );
    let clusters = [cluster(
        1000,
        &[
            block(2, 0, 0x80, b"a"),
            block(2, 20, 0x80, b"b"),
            block(2, 40, 0x82, &[2, b'c']),
            grouped,
        ],
    )];
    // Track 2 ends at 1040 + 3 * 20; track 3 at 1030 + 90, which is later.
    assert_eq!(read_ns(webm(&[], &tracks, &clusters)), Some(1_120_000_000));
    // Without the stated duration, track 2's laced block ends last.
    let clusters = [cluster(
        1000,
        &[
            block(2, 0, 0x80, b"a"),
            block(2, 20, 0x80, b"b"),
            block(2, 40, 0x82, &[2, b'c']),
        ],
    )];
    assert_eq!(read_ns(webm(&[], &tracks, &clusters)), Some(1_100_000_000));
    // A track with one block and no stated length ends where it starts.
    let clusters = [cluster(1000, &[block(2, 7, 0x80, b"a")])];
    assert_eq!(read_ns(webm(&[], &tracks, &clusters)), Some(1_007_000_000));
}

#[test]
fn ticks_are_read_at_the_stated_timestamp_scale() {
    let info = [unsigned(TIMESTAMP_SCALE, 100_000)];
    let video = [track(1, Some(FRAME_NS))];
    let clusters = [cluster(20_000, &[block(1, 10, 0x80, b"k")])];
    assert_eq!(
        read_ns(webm(&info, &video, &clusters)),
        Some(20_010 * 100_000 + FRAME_NS as i64)
    );
}

#[test]
fn a_cluster_cut_off_inside_a_block_ends_at_its_last_whole_block() {
    let video = [track(1, Some(FRAME_NS))];
    let mut cut = unknown(CLUSTER);
    cut.extend(cluster_body(
        3000,
        &[block(1, 0, 0x80, b"k"), block(1, 42, 0, b"p")],
    ));
    // A block that states 1000 bytes, of which the file holds four.
    let mut partial = block(1, 83, 0, &[0; 1000]);
    partial.truncate(partial.len() - 996);
    cut.extend(partial);
    let file = webm(&[], &video, &[cluster(0, &[block(1, 0, 0x80, b"k")]), cut]);
    assert_eq!(read_ns(file), Some(3_042_000_000 + FRAME_NS as i64));
}

#[test]
fn a_cluster_of_unknown_size_ends_at_the_next_segment_child() {
    let video = [track(1, Some(FRAME_NS))];
    let mut open = unknown(CLUSTER);
    open.extend(cluster_body(500, &[block(1, 125, 0x80, b"k")]));
    let cues = element(0x1C53_BB6B, &[0xBB, 0x80]);
    assert_eq!(
        read_ns(webm(&[], &video, &[open, cues])),
        Some(625_000_000 + FRAME_NS as i64)
    );
}

#[test]
fn a_cluster_id_inside_a_block_is_not_taken_for_a_cluster() {
    let video = [track(1, Some(FRAME_NS))];
    let mut lookalike = CLUSTER.to_be_bytes().to_vec();
    lookalike.extend_from_slice(&[0x81, 0x00, 0xFF, 0xFF]);
    let clusters = [cluster(
        1000,
        &[block(1, 0, 0x80, b"k"), block(1, 500, 0, &lookalike)],
    )];
    assert_eq!(
        read_ns(webm(&[], &video, &clusters)),
        Some(1_500_000_000 + FRAME_NS as i64)
    );
}

#[test]
fn a_last_cluster_longer_than_the_first_tail_is_found() {
    let video = [track(1, Some(FRAME_NS))];
    let big = vec![0_u8; FIRST_TAIL as usize + (FIRST_TAIL as usize) / 2];
    let clusters = [
        cluster(0, &[block(1, 0, 0x80, b"k")]),
        cluster(4000, &[block(1, 0, 0x80, &big), block(1, 42, 0, b"p")]),
    ];
    assert_eq!(
        read_ns(webm(&[], &video, &clusters)),
        Some(4_042_000_000 + FRAME_NS as i64)
    );
}

#[test]
fn a_file_that_is_not_matroska_or_holds_no_block_has_no_duration() {
    let video = [track(1, Some(FRAME_NS))];
    assert_eq!(read_ns(webm(&[], &video, &[])), None);
    assert_eq!(
        read_ns(webm(&[], &video, &[cluster(100, &[])])),
        None,
        "a cluster with no block"
    );
    let mp4 = [&[0, 0, 0, 16][..], b"ftypisom", &[0; 4]].concat();
    assert_eq!(read_ns(mp4), None);
    assert_eq!(read_ns(Vec::new()), None);
    let mut segment = element(EBML, &element(0x4282, b"webm"));
    segment.extend(unknown(0x1654_AE6B));
    assert_eq!(read_ns(segment), None, "a Segment that is not one");
}

/// The probe of a streamed WebM states every frame its decoder decodes, as
/// the probe of the same frames muxed for a seekable file does.
#[test]
fn a_streamed_webm_probes_to_its_last_frame() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for muxed in [false, true] {
        let path = crate::gst_test_media::vp8_webm(dir.path(), muxed);
        let probed =
            crate::probe(&path).unwrap_or_else(|err| panic!("probe {}: {err}", path.display()));
        assert_eq!(
            (probed.frame_rate, probed.frame_count),
            (Rational::new(24, 1), 72),
            "{}",
            path.display()
        );
        let mut decoder =
            crate::GstDecoder::open(&path, OutputMode::Cpu).expect("the decoder opens the file");
        let last = RationalTime::new(71, Rational::new(24, 1));
        let frame = decoder
            .frame_at(last)
            .expect("the last frame decodes")
            .expect("the stream holds its last frame");
        assert_eq!(
            aria_core::resample(frame.pts, Rational::new(24, 1)).value,
            71,
            "{}",
            path.display()
        );
    }
}
