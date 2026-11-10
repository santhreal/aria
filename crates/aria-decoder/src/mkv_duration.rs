//! Duration recovery for Matroska and WebM files whose Segment Info states
//! none.
//!
//! A browser's `MediaRecorder`, and a recorder stopped before it finalized its
//! file, write no `Duration` element. GStreamer's Matroska demuxer then states
//! a length estimated from the clusters it read first, which ends before the
//! last cluster. This module reads the Segment Info and, when it states no
//! duration, the blocks of the last cluster: the end of the latest block is
//! the duration.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use aria_core::{Rational, RationalTime};

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const DEFAULT_DURATION: u32 = 0x23_E383;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const BLOCK_DURATION: u32 = 0x9B;

/// Cluster children that hold no block: Void, CRC-32, Position, PrevSize,
/// SilentTracks, EncryptedBlock.
const OTHER_CLUSTER_CHILDREN: [u32; 6] = [0xEC, 0xBF, 0xA7, 0xAB, 0x5854, 0xAF];

/// Segment children that end a cluster of unknown size: Cluster, Cues, Tags,
/// Chapters, Attachments, SeekHead, Info, Tracks.
const SEGMENT_CHILDREN: [u32; 8] = [
    CLUSTER,
    0x1C53_BB6B,
    0x1254_C367,
    0x1043_A770,
    0x1941_A469,
    0x114D_9B74,
    INFO,
    TRACKS,
];

/// Nanoseconds per Matroska tick when the Segment Info states no scale.
const DEFAULT_SCALE_NS: u64 = 1_000_000;

/// Largest Info or Tracks element read whole.
const MAX_HEAD_ELEMENT: u64 = 1 << 20;

/// Segment children read before the first cluster.
const MAX_HEAD_ELEMENTS: usize = 64;

/// The first tail of the file searched for the last cluster, doubled until the
/// last cluster is in it or [`MAX_TAIL`] is read.
const FIRST_TAIL: u64 = 1 << 20;
const MAX_TAIL: u64 = 64 << 20;

const NS_PER_SEC: i32 = 1_000_000_000;

/// The duration of the Matroska or WebM file at `path`, in nanoseconds, read
/// from the end of its last block. `None` when the file is not Matroska,
/// states a duration, or holds no complete cluster in its last [`MAX_TAIL`]
/// bytes.
pub(crate) fn duration_from_file(path: &Path) -> Option<RationalTime> {
    duration_from_reader(std::fs::File::open(path).ok()?)
}

/// [`duration_from_file`] over any seekable byte source.
fn duration_from_reader<R: Read + Seek>(mut source: R) -> Option<RationalTime> {
    let len = source.seek(SeekFrom::End(0)).ok()?;
    let head = Head::read(&mut source, len)?;
    let mut window = FIRST_TAIL.min(len);
    loop {
        let start = len - window;
        let bytes = read_at(&mut source, start, window)?;
        let mut before = bytes.len();
        while let Some(at) = last_cluster_id(&bytes[..before]) {
            if let Some(end) = cluster_end(&bytes[at..], &head) {
                let ns = i64::try_from(end).ok().filter(|ns| *ns > 0)?;
                return Some(RationalTime::new(ns, Rational::new(NS_PER_SEC, 1)));
            }
            before = at;
        }
        if window == len || window >= MAX_TAIL {
            return None;
        }
        window = (window * 2).min(len).min(MAX_TAIL);
    }
}

/// What the Segment states before its first cluster.
#[derive(Debug)]
struct Head {
    /// Nanoseconds per tick.
    scale: u64,
    /// Each track's stated frame duration, in nanoseconds.
    defaults: HashMap<u64, u64>,
}

impl Head {
    /// The Segment head of `source`: `None` when it is not Matroska or its
    /// Info states a duration, which the demuxer reads.
    fn read<R: Read + Seek>(source: &mut R, len: u64) -> Option<Self> {
        let (id, size, body) = header_at(source, 0)?;
        if id != EBML {
            return None;
        }
        let (id, _, mut at) = header_at(source, body.checked_add(size?)?)?;
        if id != SEGMENT {
            return None;
        }
        let mut head = Self {
            scale: DEFAULT_SCALE_NS,
            defaults: HashMap::new(),
        };
        for _ in 0..MAX_HEAD_ELEMENTS {
            if at >= len {
                break;
            }
            let (id, size, body) = header_at(source, at)?;
            if id == CLUSTER {
                break;
            }
            let size = size?;
            if id == INFO || id == TRACKS {
                if size > MAX_HEAD_ELEMENT {
                    return None;
                }
                let bytes = read_at(source, body, size)?;
                if id == INFO && head.info(&bytes)? {
                    return None;
                }
                if id == TRACKS {
                    head.tracks(&bytes);
                }
            }
            at = body.checked_add(size)?;
        }
        Some(head)
    }

    /// Read the scale from `info`. `Some(true)` when it states a duration.
    fn info(&mut self, info: &[u8]) -> Option<bool> {
        let mut stated = false;
        for (id, payload) in children(info) {
            match id {
                TIMESTAMP_SCALE => self.scale = uint(payload).filter(|scale| *scale > 0)?,
                DURATION => stated = float(payload).is_some_and(|ticks| ticks > 0.0),
                _ => {}
            }
        }
        Some(stated)
    }

    /// Read each track's stated frame duration from `tracks`.
    fn tracks(&mut self, tracks: &[u8]) {
        for (_, entry) in children(tracks).filter(|(id, _)| *id == TRACK_ENTRY) {
            let (mut number, mut duration) = (None, None);
            for (id, payload) in children(entry) {
                match id {
                    TRACK_NUMBER => number = uint(payload),
                    DEFAULT_DURATION => duration = uint(payload),
                    _ => {}
                }
            }
            if let (Some(number), Some(duration)) = (number, duration) {
                self.defaults.insert(number, duration);
            }
        }
    }
}

/// One block: its track, its start in ticks from the cluster's, how many
/// frames it laces, and the duration it states.
struct Block {
    track: u64,
    offset: i64,
    frames: u64,
    duration: Option<u64>,
}

impl Block {
    fn parse(payload: &[u8], duration: Option<u64>) -> Option<Self> {
        let (track, at) = vint(payload, false)?;
        let offset = i16::from_be_bytes([*payload.get(at)?, *payload.get(at + 1)?]);
        let flags = *payload.get(at + 2)?;
        let frames = if flags & 0x06 == 0 {
            1
        } else {
            u64::from(*payload.get(at + 3)?) + 1
        };
        Some(Self {
            track,
            offset: i64::from(offset),
            frames,
            duration,
        })
    }
}

/// The end, in nanoseconds, of the latest block of the cluster at the start of
/// `bytes`. `None` when `bytes` does not start a cluster that holds a
/// timestamp and a complete block, or holds an element no cluster holds. A
/// block cut off by the end of the file is not counted.
fn cluster_end(bytes: &[u8], head: &Head) -> Option<u64> {
    let (_, size, body) = header(bytes, 0)?;
    let end = match size {
        Some(size) => body.saturating_add(usize::try_from(size).ok()?),
        None => bytes.len(),
    }
    .min(bytes.len());
    let bytes = &bytes[..end];
    let (mut base, mut blocks) = (None, Vec::new());
    let mut at = body;
    while let Some((id, size, payload_at)) = header(bytes, at) {
        if SEGMENT_CHILDREN.contains(&id) {
            break;
        }
        // An element of unknown size holds no block.
        let size = usize::try_from(size?).ok()?;
        let Some(payload) = payload_at
            .checked_add(size)
            .and_then(|end| bytes.get(payload_at..end))
        else {
            // The file ends inside this element.
            break;
        };
        match id {
            CLUSTER_TIMESTAMP => base = Some(uint(payload)?),
            SIMPLE_BLOCK => blocks.push(Block::parse(payload, None)?),
            BLOCK_GROUP => {
                let (mut block, mut duration) = (None, None);
                for (id, child) in children(payload) {
                    match id {
                        BLOCK => block = Some(child),
                        BLOCK_DURATION => duration = uint(child),
                        _ => {}
                    }
                }
                blocks.push(Block::parse(block?, duration)?);
            }
            id if OTHER_CLUSTER_CHILDREN.contains(&id) => {}
            _ => return None,
        }
        at = payload_at + payload.len();
    }
    latest_end(i64::try_from(base?).ok()?, &blocks, head)
}

/// The latest end among `blocks` of a cluster at `base` ticks, in
/// nanoseconds. A block without a stated duration lasts its track's stated
/// frame duration, else the shortest step between that track's blocks in the
/// cluster, per laced frame.
fn latest_end(base: i64, blocks: &[Block], head: &Head) -> Option<u64> {
    let scale = i128::from(head.scale);
    let mut starts: HashMap<u64, Vec<i64>> = HashMap::new();
    for block in blocks {
        starts
            .entry(block.track)
            .or_default()
            .push(base + block.offset);
    }
    let steps: HashMap<u64, i128> = starts
        .into_iter()
        .filter_map(|(track, mut starts)| {
            starts.sort_unstable();
            starts.dedup();
            let ticks = starts.windows(2).map(|pair| pair[1] - pair[0]).min()?;
            Some((track, i128::from(ticks) * scale))
        })
        .collect();
    blocks
        .iter()
        .map(|block| {
            let start = i128::from(base + block.offset) * scale;
            let frames = i128::from(block.frames);
            let length = match block.duration {
                Some(ticks) => i128::from(ticks) * scale,
                None => head
                    .defaults
                    .get(&block.track)
                    .map(|ns| i128::from(*ns))
                    .or_else(|| steps.get(&block.track).copied())
                    .map_or(0, |ns| ns * frames),
            };
            start + length
        })
        .max()
        .and_then(|end| u64::try_from(end).ok())
}

/// The offset of the last cluster ID in `bytes`.
fn last_cluster_id(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .rposition(|window| window == CLUSTER.to_be_bytes())
}

/// The elements in `bytes`, in order, up to the first that does not fit.
fn children(bytes: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let (id, size, body) = header(bytes, at)?;
        let end = body.checked_add(usize::try_from(size?).ok()?)?;
        let payload = bytes.get(body..end)?;
        at = end;
        Some((id, payload))
    })
}

/// The ID, payload size and payload offset of the element at `bytes[at..]`.
/// The size is `None` when the element states an unknown size.
fn header(bytes: &[u8], at: usize) -> Option<(u32, Option<u64>, usize)> {
    let (id, id_len) = vint(bytes.get(at..)?, true)?;
    let (size, size_len) = vint(bytes.get(at + id_len..)?, false)?;
    let unknown = size == (1_u64 << (7 * size_len)) - 1;
    Some((
        u32::try_from(id).ok()?,
        (!unknown).then_some(size),
        at + id_len + size_len,
    ))
}

/// [`header`] for the element at `at` in `source`.
fn header_at<R: Read + Seek>(source: &mut R, at: u64) -> Option<(u32, Option<u64>, u64)> {
    source.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = Vec::with_capacity(12);
    source.take(12).read_to_end(&mut bytes).ok()?;
    let (id, size, body) = header(&bytes, 0)?;
    Some((id, size, at + body as u64))
}

/// `len` bytes of `source` from `at`.
pub(crate) fn read_at<R: Read + Seek>(source: &mut R, at: u64, len: u64) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = vec![0; usize::try_from(len).ok()?];
    source.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// The EBML variable-length integer at the start of `bytes` and its length in
/// bytes: with its length marker for an element ID (`id`), without it for a
/// size or a track number.
fn vint(bytes: &[u8], id: bool) -> Option<(u64, usize)> {
    let first = *bytes.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > if id { 4 } else { 8 } {
        return None;
    }
    let lead = if id {
        u64::from(first)
    } else {
        u64::from(first) & (0xFF >> len)
    };
    let value = bytes
        .get(1..len)?
        .iter()
        .fold(lead, |value, byte| (value << 8) | u64::from(*byte));
    Some((value, len))
}

/// The unsigned integer `payload` holds, big-endian, at most eight bytes.
fn uint(payload: &[u8]) -> Option<u64> {
    (payload.len() <= 8).then(|| {
        payload
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte))
    })
}

/// The float `payload` holds: four or eight bytes, big-endian.
fn float(payload: &[u8]) -> Option<f64> {
    match payload.len() {
        4 => Some(f64::from(f32::from_be_bytes(payload.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(payload.try_into().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "mkv_duration_tests.rs"]
mod tests;
