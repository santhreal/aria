//! Duration of an MPEG transport stream (`.ts`, `.mts`, `.m2ts`), read from
//! the presentation timestamps of its first video stream.
//!
//! A transport stream states no duration. GStreamer's demuxer estimates one
//! from the clock references: a 60 s Blu-ray-style stream (192-byte packets,
//! as AVCHD cameras write them) read as 60.29 s, and a 1 s stream as no length
//! at all. This module reads the earliest timestamp near the start of the
//! file and the latest near its end, and adds one frame.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use aria_core::{Rational, RationalTime};

use crate::mkv_duration::read_at;

const SYNC: u8 = 0x47;
const PACKET: usize = 188;

/// Presentation timestamps tick at 90 kHz and wrap at 2^33.
const PTS_RATE: i32 = 90_000;
const PTS_WRAP: i64 = 1 << 33;

/// How much of the start of the file holds the stream tables and the first
/// timestamps.
const HEAD: u64 = 2 << 20;

/// The first tail of the file searched for timestamps, doubled until one is
/// found or [`MAX_TAIL`] is read.
const FIRST_TAIL: u64 = 1 << 20;
const MAX_TAIL: u64 = 64 << 20;

/// How many timestamps the tail holds before its latest is the stream's.
/// A frame is decoded at most 16 frames before the frames shown ahead of it
/// (the H.264 and HEVC reorder limit), so the last 17 frames decoded hold the
/// last frame shown.
const TAIL_STAMPS: usize = 17;

/// Program map stream types that carry pictures: MPEG-1 and MPEG-2 video,
/// MPEG-4 Part 2, H.264, HEVC, VC-1.
const VIDEO_TYPES: [u8; 6] = [0x01, 0x02, 0x10, 0x1B, 0x24, 0xEA];

/// The duration of the transport stream at `path`, at 90 kHz. `None` when the
/// file is not a transport stream, states no stream in its program map, or
/// its timestamps are not found.
pub(crate) fn duration_from_file(path: &Path) -> Option<RationalTime> {
    duration_from_reader(std::fs::File::open(path).ok()?)
}

/// [`duration_from_file`] over any seekable byte source.
fn duration_from_reader<R: Read + Seek>(mut source: R) -> Option<RationalTime> {
    let len = source.seek(SeekFrom::End(0)).ok()?;
    // Four packets tell a transport stream from any other file.
    let stride = stride(&read_at(
        &mut source,
        0,
        (4 * (PACKET as u64 + 4)).min(len),
    )?)?;
    let head = read_at(&mut source, 0, HEAD.min(len))?;
    let pid = stream_pid(&head, stride)?;
    let stamps = timestamps(&head, stride, pid);
    // A timestamp more than half the clock behind the first one read has
    // wrapped.
    let anchor = *stamps.first()?;
    let unwrap = |pts: i64| {
        if pts < anchor - PTS_WRAP / 2 {
            pts + PTS_WRAP
        } else {
            pts
        }
    };
    let first = stamps.into_iter().map(unwrap).min()?;
    let mut window = FIRST_TAIL.min(len);
    loop {
        let tail = read_at(&mut source, len - window, window)?;
        let mut stamps: Vec<i64> = timestamps(&tail, stride, pid)
            .into_iter()
            .map(unwrap)
            .collect();
        let exhausted = window == len || window >= MAX_TAIL;
        if stamps.len() >= TAIL_STAMPS || (exhausted && !stamps.is_empty()) {
            stamps.sort_unstable();
            stamps.dedup();
            let last = *stamps.last()?;
            let step = stamps
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .min()
                .unwrap_or(0);
            let ticks = last - first + step;
            return (ticks > 0).then(|| RationalTime::new(ticks, Rational::new(PTS_RATE, 1)));
        }
        if exhausted {
            return None;
        }
        window = (window * 2).min(len).min(MAX_TAIL);
    }
}

/// The packet stride of the stream starting `head`: 188 bytes, or 192 for a
/// stream that prefixes each packet with a 4-byte timecode.
fn stride(head: &[u8]) -> Option<usize> {
    [(PACKET, 0), (PACKET + 4, 4)]
        .into_iter()
        .find(|(stride, prefix)| (0..4).all(|k| head.get(prefix + k * stride) == Some(&SYNC)))
        .map(|(stride, _)| stride)
}

/// The 188-byte packets in `bytes`, from the first offset whose next four
/// strides all hold the sync byte.
fn packets(bytes: &[u8], stride: usize) -> impl Iterator<Item = &[u8]> {
    let start = (0..stride.min(bytes.len()))
        .find(|at| (0..4).all(|k| bytes.get(at + k * stride) == Some(&SYNC)));
    start
        .into_iter()
        .flat_map(move |start| (start..bytes.len()).step_by(stride))
        .map_while(move |at| bytes.get(at..at + PACKET))
        .filter(|packet| packet[0] == SYNC)
}

/// A packet's PID, whether it starts a payload unit, and its payload.
fn split(packet: &[u8]) -> Option<(u16, bool, &[u8])> {
    let pid = (u16::from(packet[1] & 0x1F) << 8) | u16::from(packet[2]);
    let unit_start = packet[1] & 0x40 != 0;
    let control = (packet[3] >> 4) & 0x3;
    let payload_at = match control {
        1 => 4,
        3 => 5 + usize::from(packet[4]),
        _ => return None,
    };
    Some((pid, unit_start, packet.get(payload_at..)?))
}

/// The PID of the first video stream the program map of `head` lists, else
/// of its first stream.
fn stream_pid(head: &[u8], stride: usize) -> Option<u16> {
    let table = |pid: u16, id: u8| {
        packets(head, stride).find_map(move |packet| {
            let (at, unit_start, payload) = split(packet)?;
            if at != pid || !unit_start {
                return None;
            }
            let section = payload.get(1 + usize::from(*payload.first()?)..)?;
            if *section.first()? != id {
                return None;
            }
            let length = usize::from(
                (u16::from(*section.get(1)? & 0x0F) << 8) | u16::from(*section.get(2)?),
            );
            // The section, without its 4-byte CRC.
            section.get(..(3 + length).checked_sub(4)?)
        })
    };
    let pat = table(0, 0x00)?;
    let pmt_pid = pat.get(8..)?.chunks_exact(4).find_map(|entry| {
        let program = (u16::from(entry[0]) << 8) | u16::from(entry[1]);
        (program != 0).then(|| (u16::from(entry[2] & 0x1F) << 8) | u16::from(entry[3]))
    })?;
    let pmt = table(pmt_pid, 0x02)?;
    let info = usize::from((u16::from(*pmt.get(10)? & 0x0F) << 8) | u16::from(*pmt.get(11)?));
    let mut streams = Vec::new();
    let mut at = 12 + info;
    while let Some(entry) = pmt.get(at..at + 5) {
        let pid = (u16::from(entry[1] & 0x1F) << 8) | u16::from(entry[2]);
        streams.push((entry[0], pid));
        at += 5 + usize::from((u16::from(entry[3] & 0x0F) << 8) | u16::from(entry[4]));
    }
    streams
        .iter()
        .find(|(kind, _)| VIDEO_TYPES.contains(kind))
        .or_else(|| streams.first())
        .map(|(_, pid)| *pid)
}

/// The presentation timestamps of the PES packets of `pid` that start in
/// `bytes`.
fn timestamps(bytes: &[u8], stride: usize, pid: u16) -> Vec<i64> {
    packets(bytes, stride)
        .filter_map(|packet| {
            let (at, unit_start, pes) = split(packet)?;
            if at != pid || !unit_start || pes.get(..3)? != [0, 0, 1] {
                return None;
            }
            if *pes.get(7)? & 0x80 == 0 {
                return None;
            }
            let pts = pes.get(9..14)?;
            Some(
                (i64::from((pts[0] >> 1) & 0x07) << 30)
                    | (i64::from(pts[1]) << 22)
                    | (i64::from(pts[2] >> 1) << 15)
                    | (i64::from(pts[3]) << 7)
                    | i64::from(pts[4] >> 1),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "ts_duration_tests.rs"]
mod tests;
