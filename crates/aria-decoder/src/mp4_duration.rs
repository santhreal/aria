//! Duration recovery for **fragmented MP4** files, where the demuxer reports
//! none.
//!
//! Downloaded/streamed MP4s (Pexels, CDN output, screen recorders) are often
//! *fragmented*: the `moov` declares `duration = 0` and the real timing lives
//! in per-fragment `moof` boxes. Media Foundation's MPEG-4 source does not sum
//! fragments, so `MF_PD_DURATION` is simply absent and the probe would report
//! a zero-length source — which the editor rejects at clip-add time.
//!
//! This module walks the container's *metadata* boxes directly (seeking past
//! `mdat` payloads, so I/O stays a few KB per fragment) and reconstructs the
//! presentation length:
//!
//! - `moov/mvhd` movie duration (authoritative for plain MP4s),
//! - per track: `trak/tkhd` id + `trak/mdia/mdhd` timescale,
//! - `moov/mvex/trex` default sample durations,
//! - per fragment: `moof/traf` (`tfhd` defaults, `tfdt` base decode time,
//!   `trun` sample runs).
//!
//! The result is the **max end time across tracks**, converted to the same
//! 100-ns tick rate Media Foundation uses, so callers can treat it exactly
//! like an `MF_PD_DURATION` value. Any structural surprise yields `None` —
//! this is a best-effort fallback, never an error source.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use aria_core::{Rational, RationalTime};

/// 100-ns ticks per second (Media Foundation's time base).
const HNS_PER_SEC: i64 = 10_000_000;

/// Largest metadata box we are willing to load into memory. `moov`/`moof`
/// boxes are KBs in practice; anything past this is treated as malformed.
const MAX_BOX_BYTES: u64 = 64 * 1024 * 1024;

/// Best-effort duration of the MP4 file at `path`, on the 100-ns time base.
///
/// `None` when the file isn't an MP4, declares no duration anywhere, or is
/// malformed. Intended as a fallback when the platform demuxer reports no
/// duration (fragmented MP4).
pub(crate) fn duration_from_file(path: &Path) -> Option<RationalTime> {
    let file = File::open(path).ok()?;
    duration_from_reader(file)
}

/// [`duration_from_file`] over any seekable byte source (testable in memory).
fn duration_from_reader<R: Read + Seek>(mut source: R) -> Option<RationalTime> {
    let mut state = Tracks::default();
    let len = source.seek(SeekFrom::End(0)).ok()?;
    source.seek(SeekFrom::Start(0)).ok()?;

    let mut pos: u64 = 0;
    let mut saw_mp4_box = false;
    while pos + 8 <= len {
        source.seek(SeekFrom::Start(pos)).ok()?;
        let mut header = [0u8; 8];
        source.read_exact(&mut header).ok()?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let kind = [header[4], header[5], header[6], header[7]];

        let (header_len, box_size) = match size32 {
            0 => (8u64, len - pos),
            1 => {
                let mut large = [0u8; 8];
                source.read_exact(&mut large).ok()?;
                (16u64, u64::from_be_bytes(large))
            }
            n => (8u64, n),
        };
        if box_size < header_len || pos + box_size > len {
            return None;
        }

        match &kind {
            b"moov" | b"moof" => {
                let body_len = box_size - header_len;
                if body_len > MAX_BOX_BYTES {
                    return None;
                }
                let mut body = vec![0u8; body_len as usize];
                source.read_exact(&mut body).ok()?;
                if &kind == b"moov" {
                    state.parse_moov(&body)?;
                } else {
                    state.parse_moof(&body)?;
                }
                saw_mp4_box = true;
            }
            b"ftyp" | b"styp" | b"mdat" | b"free" | b"skip" | b"sidx" | b"uuid" | b"wide"
            | b"pdin" | b"emsg" | b"meta" | b"mfra" => {
                saw_mp4_box = true;
            }
            // An unknown top-level box before any recognized one means this
            // probably isn't an MP4 at all; bail rather than misread it.
            _ if !saw_mp4_box => return None,
            _ => {}
        }
        pos += box_size;
    }

    state.duration_hns().map(|hns| {
        RationalTime::new(
            hns,
            Rational::new(i32::try_from(HNS_PER_SEC).expect("HNS_PER_SEC fits i32"), 1),
        )
    })
}

/// Per-track timing state accumulated across `moov` and every `moof`.
#[derive(Default)]
struct Tracks {
    movie_timescale: u32,
    movie_duration: u64,
    /// `trak/tkhd` track id -> `mdhd` media timescale.
    timescales: HashMap<u32, u32>,
    /// `mvex/trex` per-track default sample duration.
    trex_default: HashMap<u32, u32>,
    /// Current decode time per track, advanced by each fragment.
    time: HashMap<u32, u64>,
    /// Max end time seen per track (media timescale).
    end: HashMap<u32, u64>,
}

impl Tracks {
    fn parse_moov(&mut self, body: &[u8]) -> Option<()> {
        for (kind, child) in BoxIter::new(body) {
            match &kind {
                b"mvhd" => {
                    let version = *child.first()?;
                    if version == 1 {
                        self.movie_timescale = read_u32(child, 20)?;
                        self.movie_duration = read_u64(child, 24)?;
                    } else {
                        self.movie_timescale = read_u32(child, 12)?;
                        self.movie_duration = u64::from(read_u32(child, 16)?);
                    }
                }
                b"trak" => self.parse_trak(child)?,
                b"mvex" => {
                    for (mkind, mchild) in BoxIter::new(child) {
                        if &mkind == b"trex" {
                            let track_id = read_u32(mchild, 4)?;
                            let default_duration = read_u32(mchild, 12)?;
                            self.trex_default.insert(track_id, default_duration);
                        }
                    }
                }
                _ => {}
            }
        }
        Some(())
    }

    fn parse_trak(&mut self, body: &[u8]) -> Option<()> {
        let mut track_id = None;
        let mut timescale = None;
        for (kind, child) in BoxIter::new(body) {
            match &kind {
                b"tkhd" => {
                    let version = *child.first()?;
                    let offset = if version == 1 { 20 } else { 12 };
                    track_id = Some(read_u32(child, offset)?);
                }
                b"mdia" => {
                    for (mkind, mchild) in BoxIter::new(child) {
                        if &mkind == b"mdhd" {
                            let version = *mchild.first()?;
                            let offset = if version == 1 { 16 } else { 12 };
                            timescale = Some(read_u32(mchild, offset)?);
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(id), Some(scale)) = (track_id, timescale) {
            self.timescales.insert(id, scale);
        }
        Some(())
    }

    fn parse_moof(&mut self, body: &[u8]) -> Option<()> {
        for (kind, child) in BoxIter::new(body) {
            if &kind == b"traf" {
                self.parse_traf(child)?;
            }
        }
        Some(())
    }

    fn parse_traf(&mut self, body: &[u8]) -> Option<()> {
        // tfhd is mandatory and precedes the runs; parse children in order so
        // tfdt (when present) rebases the clock before truns advance it.
        let mut track_id = None;
        let mut default_duration = None;
        for (kind, child) in BoxIter::new(body) {
            match &kind {
                b"tfhd" => {
                    let flags = read_u24(child, 1)?;
                    let id = read_u32(child, 4)?;
                    let mut offset = 8usize;
                    if flags & 0x00_0001 != 0 {
                        offset += 8; // base_data_offset
                    }
                    if flags & 0x00_0002 != 0 {
                        offset += 4; // sample_description_index
                    }
                    if flags & 0x00_0008 != 0 {
                        default_duration = Some(read_u32(child, offset)?);
                    }
                    track_id = Some(id);
                    if default_duration.is_none() {
                        default_duration = self.trex_default.get(&id).copied();
                    }
                }
                b"tfdt" => {
                    let id = track_id?;
                    let version = *child.first()?;
                    let base = if version == 1 {
                        read_u64(child, 4)?
                    } else {
                        u64::from(read_u32(child, 4)?)
                    };
                    self.time.insert(id, base);
                }
                b"trun" => {
                    let id = track_id?;
                    let advanced = trun_duration(child, default_duration)?;
                    let t = self.time.entry(id).or_insert(0);
                    *t = t.checked_add(advanced)?;
                    let end = self.end.entry(id).or_insert(0);
                    *end = (*end).max(*t);
                }
                _ => {}
            }
        }
        Some(())
    }

    /// Longest track (or movie header) duration, in 100-ns ticks.
    fn duration_hns(&self) -> Option<i64> {
        let mut best: i64 = 0;
        if self.movie_timescale > 0 {
            best = best.max(to_hns(self.movie_duration, self.movie_timescale)?);
        }
        for (track, end) in &self.end {
            let scale = self.timescales.get(track).copied().unwrap_or(0);
            if scale > 0 {
                best = best.max(to_hns(*end, scale)?);
            }
        }
        (best > 0).then_some(best)
    }
}

/// Total duration of one `trun` box: per-sample durations when present,
/// otherwise `sample_count * default_duration`.
fn trun_duration(body: &[u8], default_duration: Option<u32>) -> Option<u64> {
    let flags = read_u24(body, 1)?;
    let sample_count = read_u32(body, 4)?;
    let mut offset = 8usize;
    if flags & 0x00_0001 != 0 {
        offset += 4; // data_offset
    }
    if flags & 0x00_0004 != 0 {
        offset += 4; // first_sample_flags
    }
    let has_duration = flags & 0x00_0100 != 0;
    if !has_duration {
        return u64::from(sample_count).checked_mul(u64::from(default_duration?));
    }
    // Stride over the optional per-sample fields that follow duration.
    let mut stride = 4usize;
    if flags & 0x00_0200 != 0 {
        stride += 4; // sample_size
    }
    if flags & 0x00_0400 != 0 {
        stride += 4; // sample_flags
    }
    if flags & 0x00_0800 != 0 {
        stride += 4; // sample_composition_time_offset
    }
    let mut total: u64 = 0;
    for i in 0..sample_count as usize {
        let duration = read_u32(body, offset + i * stride)?;
        total = total.checked_add(u64::from(duration))?;
    }
    Some(total)
}

/// `ticks / timescale` seconds, exactly, as 100-ns ticks.
fn to_hns(ticks: u64, timescale: u32) -> Option<i64> {
    let hns = i128::from(ticks) * i128::from(HNS_PER_SEC) / i128::from(timescale);
    i64::try_from(hns).ok()
}

/// Iterate the child boxes of an ISO-BMFF container body as
/// `([u8; 4] fourcc, body)` pairs, stopping at the first malformed header.
struct BoxIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BoxIter<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for BoxIter<'a> {
    type Item = ([u8; 4], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.data.get(self.pos..)?;
        if rest.len() < 8 {
            return None;
        }
        let size32 = u64::from(u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]));
        let kind = [rest[4], rest[5], rest[6], rest[7]];
        let (header_len, size) = match size32 {
            0 => (8u64, rest.len() as u64),
            1 => {
                let large = read_u64(rest, 8)?;
                (16u64, large)
            }
            n => (8u64, n),
        };
        if size < header_len || size > rest.len() as u64 {
            return None;
        }
        let body = &rest[header_len as usize..size as usize];
        self.pos += size as usize;
        Some((kind, body))
    }
}

fn read_u24(data: &[u8], offset: usize) -> Option<u32> {
    let b = data.get(offset..offset + 3)?;
    Some(u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let b = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let b = data.get(offset..offset + 8)?;
    Some(u64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

#[cfg(test)]
#[path = "mp4_duration_tests.rs"]
mod tests;
