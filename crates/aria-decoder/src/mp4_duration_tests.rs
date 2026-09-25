//! MP4 duration probe tests.

use super::*;
use std::io::Cursor;

fn boxed(kind: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend(body);
    out
}

fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: Vec<u8>) -> Vec<u8> {
    let mut inner = vec![version];
    inner.extend_from_slice(&flags.to_be_bytes()[1..]);
    inner.extend(body);
    boxed(kind, inner)
}

fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = vec![0u8; 8]; // creation + modification (v0)
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend(vec![0u8; 80]); // rate/volume/matrix/next_track_id
    full_box(b"mvhd", 0, 0, body)
}

fn trak(track_id: u32, timescale: u32) -> Vec<u8> {
    let mut tkhd_body = vec![0u8; 8];
    tkhd_body.extend_from_slice(&track_id.to_be_bytes());
    tkhd_body.extend(vec![0u8; 68]);
    let tkhd = full_box(b"tkhd", 0, 7, tkhd_body);

    let mut mdhd_body = vec![0u8; 8];
    mdhd_body.extend_from_slice(&timescale.to_be_bytes());
    mdhd_body.extend_from_slice(&0u32.to_be_bytes()); // duration 0 (fragmented)
    mdhd_body.extend(vec![0u8; 4]);
    let mdhd = full_box(b"mdhd", 0, 0, mdhd_body);
    let mdia = boxed(b"mdia", mdhd);

    boxed(b"trak", [tkhd, mdia].concat())
}

fn trex(track_id: u32, default_duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&track_id.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes()); // sample description index
    body.extend_from_slice(&default_duration.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes()); // default size
    body.extend_from_slice(&0u32.to_be_bytes()); // default flags
    full_box(b"trex", 0, 0, body)
}

fn tfdt(base: u64) -> Vec<u8> {
    full_box(b"tfdt", 1, 0, base.to_be_bytes().to_vec())
}

/// A trun with explicit per-sample durations (+ sizes, to exercise stride).
fn trun_with_durations(durations: &[u32]) -> Vec<u8> {
    let flags = 0x000001 | 0x000100 | 0x000200; // data offset + duration + size
    let mut body = Vec::new();
    body.extend_from_slice(&(durations.len() as u32).to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes()); // data_offset
    for &d in durations {
        body.extend_from_slice(&d.to_be_bytes());
        body.extend_from_slice(&100u32.to_be_bytes()); // size
    }
    full_box(b"trun", 0, flags, body)
}

fn trun_defaults(count: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&count.to_be_bytes());
    full_box(b"trun", 0, 0, body)
}

/// tfhd carrying only the track id (defaults come from trex).
fn tfhd_plain(track_id: u32) -> Vec<u8> {
    full_box(b"tfhd", 0, 0, track_id.to_be_bytes().to_vec())
}

fn moof(traf_children: Vec<u8>) -> Vec<u8> {
    boxed(b"moof", boxed(b"traf", traf_children))
}

#[test]
fn plain_mp4_uses_mvhd_duration() {
    let moov = boxed(b"moov", [mvhd(1000, 5000), trak(1, 30000)].concat());
    let file = [boxed(b"ftyp", vec![0; 8]), moov].concat();
    let duration = duration_from_reader(Cursor::new(file)).expect("duration");
    // 5000 / 1000 = 5 s = 50_000_000 hns.
    assert_eq!(duration.value, 50_000_000);
    assert_eq!(duration.rate, Rational::new(10_000_000, 1));
}

#[test]
fn fragmented_mp4_sums_runs_across_fragments() {
    let moov = boxed(
        b"moov",
        [mvhd(1000, 0), trak(1, 30000), boxed(b"mvex", trex(1, 1001))].concat(),
    );
    // Fragment 1: explicit durations 1000 + 1000 + 1001 from base 0.
    let frag1 = moof(
        [
            tfhd_plain(1),
            tfdt(0),
            trun_with_durations(&[1000, 1000, 1001]),
        ]
        .concat(),
    );
    // Fragment 2: 2 samples at the trex default (1001), rebased at 3001.
    let frag2 = moof([tfhd_plain(1), tfdt(3001), trun_defaults(2)].concat());
    let file = [boxed(b"ftyp", vec![0; 8]), moov, frag1, frag2].concat();

    let duration = duration_from_reader(Cursor::new(file)).expect("duration");
    // End = 3001 + 2 * 1001 = 5003 ticks @ 30000 Hz -> 1_667_666 hns.
    assert_eq!(duration.value, 5003 * 10_000_000 / 30000);
}

#[test]
fn tfhd_default_duration_overrides_trex() {
    let moov = boxed(
        b"moov",
        [mvhd(1000, 0), trak(1, 1000), boxed(b"mvex", trex(1, 999))].concat(),
    );
    // tfhd flag 0x08: default_sample_duration = 500 follows track id.
    let mut tfhd_body = 1u32.to_be_bytes().to_vec();
    tfhd_body.extend_from_slice(&500u32.to_be_bytes());
    let tfhd = full_box(b"tfhd", 0, 0x000008, tfhd_body);
    let frag = moof([tfhd, tfdt(0), trun_defaults(4)].concat());
    let file = [moov, frag].concat();

    let duration = duration_from_reader(Cursor::new(file)).expect("duration");
    // 4 * 500 = 2000 ticks @ 1000 Hz -> 2 s.
    assert_eq!(duration.value, 20_000_000);
}

#[test]
fn longest_track_wins() {
    let moov = boxed(
        b"moov",
        [
            mvhd(1000, 0),
            trak(1, 30000),
            trak(2, 48000),
            boxed(b"mvex", [trex(1, 1001), trex(2, 1024)].concat()),
        ]
        .concat(),
    );
    let video = moof([tfhd_plain(1), tfdt(0), trun_defaults(30)].concat()); // ~1.001 s
    let audio = moof([tfhd_plain(2), tfdt(0), trun_defaults(100)].concat()); // ~2.13 s
    let file = [moov, video, audio].concat();

    let duration = duration_from_reader(Cursor::new(file)).expect("duration");
    assert_eq!(duration.value, 100 * 1024 * 10_000_000 / 48000);
}

#[test]
fn non_mp4_and_empty_yield_none() {
    assert!(duration_from_reader(Cursor::new(b"RIFF....WAVE".to_vec())).is_none());
    assert!(duration_from_reader(Cursor::new(Vec::new())).is_none());
    // Valid boxes but no duration anywhere.
    let moov = boxed(b"moov", [mvhd(1000, 0), trak(1, 30000)].concat());
    assert!(duration_from_reader(Cursor::new(moov)).is_none());
}
