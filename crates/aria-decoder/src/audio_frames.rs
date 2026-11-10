//! Duration of an elementary audio stream, an MPEG audio file (`.mp3`) with no
//! Xing, Info, or VBRI header or an ADTS AAC file (`.aac`), read by counting
//! its frames.
//!
//! Neither states its length: an encoder writing to a stream, and a recorder
//! writing as it records, leave out the MP3 header that states the frame
//! count, and ADTS has none. GStreamer's parser then states no duration when
//! the probe prerolls, and the import failed. Each frame header states the
//! frame's length and sample count, so counting the frames from the first to
//! the last gives the exact length at any bitrate.

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use aria_core::{Rational, RationalTime};

/// How far past the start, or past an ID3v2 tag, the first frame may sit, and
/// how far past a damaged stretch the next frame is searched for.
const SEARCH: usize = 64 << 10;

/// Frames in a row whose headers agree before a sync word is taken for a
/// frame: a sync word inside a frame body or a tag rarely starts three.
const CONFIRM: usize = 3;

/// The longest frame: an ADTS frame states at most 8191 bytes.
const MAX_FRAME: usize = 8191;

/// The bytes read of each frame header: an ADTS header is 7 bytes long, an
/// MPEG audio header 4.
const HEADER: usize = 7;

/// ADTS sample rates by sampling frequency index.
const ADTS_RATES: [u32; 13] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350,
];

/// Bit rates in kbit/s by bitrate index 1 to 14.
const MPEG1_LAYER1: [u32; 14] = [
    32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const MPEG1_LAYER2: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const MPEG1_LAYER3: [u32; 14] = [
    32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const MPEG2_LAYER1: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const MPEG2_LAYER23: [u32; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// One frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    /// The version, layer, and sample rate bits of an MPEG audio frame, or
    /// the version, profile, and sample rate bits of an ADTS frame: every
    /// frame of a stream shares them.
    stream: (u8, u8),
    rate: u32,
    samples: u32,
    length: usize,
    /// Where a Xing or Info header would start, for a Layer III frame.
    xing_at: Option<usize>,
}

/// The frame whose header starts `bytes`, `None` for no valid header.
fn header(bytes: [u8; HEADER]) -> Option<Frame> {
    mpeg_header(bytes).or_else(|| adts_header(bytes))
}

/// The MPEG audio frame whose header starts `bytes`. A free-format frame
/// states no length and is not one.
fn mpeg_header(bytes: [u8; HEADER]) -> Option<Frame> {
    if bytes[0] != 0xFF || bytes[1] & 0xE0 != 0xE0 {
        return None;
    }
    // Version: 0 is MPEG-2.5, 1 is reserved, 2 is MPEG-2, 3 is MPEG-1.
    // Layer: 1 is III, 2 is II, 3 is I, 0 is reserved.
    let version = (bytes[1] >> 3) & 3;
    let layer = (bytes[1] >> 1) & 3;
    let bitrate = usize::from(bytes[2] >> 4);
    let rate_index = usize::from((bytes[2] >> 2) & 3);
    if version == 1 || layer == 0 || bitrate == 0 || bitrate == 15 || rate_index == 3 {
        return None;
    }
    let mpeg1 = version == 3;
    let table = match (mpeg1, layer) {
        (true, 3) => &MPEG1_LAYER1,
        (true, 2) => &MPEG1_LAYER2,
        (true, _) => &MPEG1_LAYER3,
        (false, 3) => &MPEG2_LAYER1,
        (false, _) => &MPEG2_LAYER23,
    };
    let bits = table[bitrate - 1] * 1000;
    let rate = [44_100, 48_000, 32_000][rate_index] >> (3 - version.max(1));
    let padding = u32::from((bytes[2] >> 1) & 1);
    let (samples, length) = match layer {
        3 => (384, (12 * bits / rate + padding) * 4),
        2 => (1152, 144 * bits / rate + padding),
        _ if mpeg1 => (1152, 144 * bits / rate + padding),
        _ => (576, 72 * bits / rate + padding),
    };
    let mono = bytes[3] >> 6 == 3;
    let side_info = match (mpeg1, mono) {
        (true, false) => 32,
        (true, true) | (false, false) => 17,
        (false, true) => 9,
    };
    let crc = if bytes[1] & 1 == 0 { 2 } else { 0 };
    Some(Frame {
        stream: (bytes[1] & 0xFE, bytes[2] & 0x0C),
        rate,
        samples,
        length: length as usize,
        xing_at: (layer == 1).then_some(4 + crc + side_info),
    })
}

/// The ADTS frame whose header starts `bytes`: 1024 samples for each raw data
/// block it holds.
fn adts_header(bytes: [u8; HEADER]) -> Option<Frame> {
    // The sync word, then the layer bits, which are zero.
    if bytes[0] != 0xFF || bytes[1] & 0xF6 != 0xF0 {
        return None;
    }
    let rate = *ADTS_RATES.get(usize::from((bytes[2] >> 2) & 0x0F))?;
    let length = (usize::from(bytes[3] & 0x03) << 11)
        | (usize::from(bytes[4]) << 3)
        | usize::from(bytes[5] >> 5);
    if length < HEADER {
        return None;
    }
    Some(Frame {
        stream: (bytes[1] & 0xF8, bytes[2] & 0xFC),
        rate,
        samples: 1024 * (u32::from(bytes[6] & 0x03) + 1),
        length,
        xing_at: None,
    })
}

/// The frame starting `bytes` when [`CONFIRM`] frames in a row start there,
/// of `stream` when one is given. Frames that run to the end of `bytes` count
/// when `bytes` ends the file.
fn confirmed(bytes: &[u8], stream: Option<(u8, u8)>, ends_file: bool) -> Option<Frame> {
    let first = header(bytes.get(..HEADER)?.try_into().ok()?)?;
    if stream.is_some_and(|stream| stream != first.stream) {
        return None;
    }
    let mut at = 0;
    for _ in 0..CONFIRM {
        match bytes.get(at..at + HEADER) {
            Some(next) => {
                let frame = header(next.try_into().ok()?)?;
                if frame.stream != first.stream {
                    return None;
                }
                at += frame.length;
            }
            None => return (ends_file && at == bytes.len()).then_some(first),
        }
    }
    Some(first)
}

/// Whether `frame`, whose bytes start `bytes`, is a Xing, Info, or VBRI
/// header: a frame of silence that states the stream's frame count, which
/// GStreamer's parser reads.
fn states_its_length(bytes: &[u8], frame: Frame) -> bool {
    let tag = |at: usize| bytes.get(at..at + 4);
    frame
        .xing_at
        .is_some_and(|at| matches!(tag(at), Some(b"Xing" | b"Info")))
        || matches!(tag(36), Some(b"VBRI"))
}

/// The duration of the MP3 or ADTS file at `path`, at its sample rate. `None`
/// when it starts no frame, when its first frame states the frame count, or
/// when it is unreadable.
pub(crate) fn duration_from_file(path: &Path) -> Option<RationalTime> {
    duration_from_reader(std::fs::File::open(path).ok()?)
}

/// `len` bytes of `source` from `at`, fewer at its end.
fn window<R: Read + Seek>(source: &mut BufReader<R>, at: u64, len: usize) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = Vec::with_capacity(len);
    source
        .by_ref()
        .take(len as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// The first frame [`confirmed`] in `source` within [`SEARCH`] bytes of `at`,
/// where it starts, and the bytes read from there.
fn find<R: Read + Seek>(
    source: &mut BufReader<R>,
    at: u64,
    len: u64,
    stream: Option<(u8, u8)>,
) -> Option<(u64, Frame, Vec<u8>)> {
    let mut bytes = window(source, at, SEARCH + CONFIRM * MAX_FRAME)?;
    let ends_file = at + bytes.len() as u64 == len;
    let (start, frame) = (0..SEARCH.min(bytes.len()))
        .find_map(|start| Some((start, confirmed(&bytes[start..], stream, ends_file)?)))?;
    bytes.drain(..start);
    Some((at + start as u64, frame, bytes))
}

/// [`duration_from_file`] over any seekable byte source.
fn duration_from_reader<R: Read + Seek>(source: R) -> Option<RationalTime> {
    let mut source = BufReader::with_capacity(1 << 16, source);
    let len = source.seek(SeekFrom::End(0)).ok()?;
    let tag = window(&mut source, 0, 10)?;
    // An ID3v2 tag: "ID3", version, flags, then its size in four 7-bit bytes,
    // and a 10-byte footer when flag 0x10 is set.
    let start = match tag.as_slice() {
        [b'I', b'D', b'3', _, _, flags, size @ ..] if size.len() == 4 => {
            let size = size
                .iter()
                .fold(0_u64, |size, byte| (size << 7) | u64::from(byte & 0x7F));
            10 + size + if flags & 0x10 != 0 { 10 } else { 0 }
        }
        _ => 0,
    };
    let (mut at, first, bytes) = find(&mut source, start, len, None)?;
    if states_its_length(&bytes, first) {
        return None;
    }
    let mut samples: i64 = 0;
    source.seek(SeekFrom::Start(at)).ok()?;
    loop {
        let mut bytes = [0; HEADER];
        let frame = match source.read_exact(&mut bytes) {
            Ok(()) => header(bytes).filter(|frame| frame.stream == first.stream),
            Err(_) => break,
        };
        let frame = match frame {
            Some(frame) => frame,
            // A damaged stretch: the next run of frames within reach, else
            // the end of the stream (an ID3v1 or APE tag, or trailing bytes).
            None => match find(&mut source, at + 1, len, Some(first.stream)) {
                Some((next, _, _)) => {
                    at = next;
                    source.seek(SeekFrom::Start(at)).ok()?;
                    continue;
                }
                None => break,
            },
        };
        // A frame cut off by the end of the file decodes to nothing.
        if at + frame.length as u64 > len {
            break;
        }
        samples += i64::from(frame.samples);
        at += frame.length as u64;
        source.seek_relative((frame.length - HEADER) as i64).ok()?;
    }
    (samples > 0).then(|| RationalTime::new(samples, Rational::new(first.rate as i32, 1)))
}

#[cfg(test)]
#[path = "audio_frames_tests.rs"]
mod tests;
