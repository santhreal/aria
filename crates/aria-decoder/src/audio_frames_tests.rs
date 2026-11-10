//! WHY: an MP3 with no Xing, Info, or VBRI header, as a streaming encoder or a
//! recorder writes one, and every ADTS AAC file failed to import: GStreamer's
//! parser states no length for them when the probe prerolls. These tests close
//! the class at the probe: such a file lasts exactly the samples of its
//! frames, at any bitrate, layer, version, or block count, past an ID3 tag, a
//! damaged stretch, or a cut-off last frame, and an MP3 whose first frame
//! states the count is left to the parser. Not caught: a free-format MP3,
//! whose frames state no length.

use std::io::Cursor;

use aria_core::AudioReader;

use super::*;
use crate::gst_audio::GstAudioReader;
use crate::gst_test_media::{aac_encoder, run_to_eos};

/// MPEG-1 Layer III at 128 kbit/s, 48 kHz: 384 bytes and 1152 samples.
const L3_128: [u8; 4] = [0xFF, 0xFB, 0x94, 0x44];
/// MPEG-1 Layer III at 160 kbit/s, 48 kHz: 480 bytes.
const L3_160: [u8; 4] = [0xFF, 0xFB, 0xA4, 0x44];
/// MPEG-1 Layer III at 128 kbit/s, 44.1 kHz: 417 bytes, 418 padded.
const L3_44K: [u8; 4] = [0xFF, 0xFB, 0x90, 0x44];
const L3_44K_PADDED: [u8; 4] = [0xFF, 0xFB, 0x92, 0x44];
/// MPEG-2 Layer III at 64 kbit/s, 24 kHz: 192 bytes and 576 samples.
const MPEG2_L3: [u8; 4] = [0xFF, 0xF3, 0x84, 0x44];
/// MPEG-2.5 Layer III at 64 kbit/s, 11.025 kHz: 417 bytes and 576 samples.
const MPEG25_L3: [u8; 4] = [0xFF, 0xE3, 0x80, 0x44];
/// MPEG-1 Layer II at 192 kbit/s, 48 kHz: 576 bytes and 1152 samples.
const L2: [u8; 4] = [0xFF, 0xFD, 0xA4, 0x44];
/// MPEG-1 Layer I at 384 kbit/s, 48 kHz: 384 bytes and 384 samples.
const L1: [u8; 4] = [0xFF, 0xFF, 0xC4, 0x44];

/// An ADTS frame of AAC LC stereo at 48 kHz, `length` bytes long, holding
/// `blocks` raw data blocks.
fn adts_frame(length: usize, blocks: u8) -> Vec<u8> {
    let mut out = vec![
        0xFF,
        0xF1,
        0x4C,
        0x80 | (length >> 11) as u8,
        (length >> 3) as u8,
        ((length & 7) << 5) as u8 | 0x1F,
        0xFC | (blocks - 1),
    ];
    out.resize(length.max(out.len()), 0);
    out
}

/// A frame of `header`, `length` bytes long, its body zero.
fn frame(header: [u8; 4], length: usize) -> Vec<u8> {
    let mut out = header.to_vec();
    out.resize(length, 0);
    out
}

/// `count` frames of `header`, each `length` bytes.
fn frames(header: [u8; 4], length: usize, count: usize) -> Vec<u8> {
    (0..count).flat_map(|_| frame(header, length)).collect()
}

/// The duration of `bytes` as (samples, sample rate).
fn length(bytes: Vec<u8>) -> Option<(i64, i32)> {
    duration_from_reader(Cursor::new(bytes)).map(|duration| (duration.value, duration.rate.num))
}

#[test]
fn every_layer_and_version_counts_its_samples() {
    for (header, bytes, samples, rate) in [
        (L3_128, 384, 1152, 48_000),
        (MPEG2_L3, 192, 576, 24_000),
        (MPEG25_L3, 417, 576, 11_025),
        (L2, 576, 1152, 48_000),
        (L1, 384, 384, 48_000),
    ] {
        assert_eq!(
            length(frames(header, bytes, 100)),
            Some((100 * samples, rate)),
            "{header:02X?}"
        );
    }
}

#[test]
fn adts_frames_count_1024_samples_a_block() {
    let mut bytes: Vec<u8> = (0..40).flat_map(|_| adts_frame(300, 1)).collect();
    bytes.extend((0..20).flat_map(|k| adts_frame(250 + 121 * (k % 2), 2)));
    assert_eq!(length(bytes), Some((40 * 1024 + 20 * 2048, 48_000)));
    // A header stating a frame shorter than itself starts no frame.
    assert_eq!(
        length((0..40).flat_map(|_| adts_frame(6, 1)).collect()),
        None
    );
}

#[test]
fn frames_of_any_bitrate_and_padding_count_alike() {
    let mut bytes = frames(L3_44K_PADDED, 418, 30);
    bytes.extend(frames(L3_44K, 417, 30));
    assert_eq!(length(bytes), Some((60 * 1152, 44_100)));
    let mut bytes = frames(L3_128, 384, 30);
    bytes.extend(frames(L3_160, 480, 30));
    bytes.extend(frames(L3_128, 384, 30));
    assert_eq!(length(bytes), Some((90 * 1152, 48_000)));
}

#[test]
fn tags_before_and_after_the_frames_are_not_counted() {
    // An ID3v2 tag of 576 bytes, its size in 7-bit bytes, with and without a
    // footer. Its body holds a run of frames of another stream, as a tag
    // holding a sound sample does.
    for footer in [false, true] {
        let mut bytes = b"ID3\x04\x00".to_vec();
        bytes.push(if footer { 0x10 } else { 0 });
        bytes.extend([0, 0, 0x04, 0x40]);
        bytes.extend(frames(MPEG2_L3, 192, 3));
        if footer {
            bytes.extend(b"3DI\x04\x00\x10\x00\x00\x04\x40");
        }
        bytes.extend(frames(L3_128, 384, 40));
        // An APE tag and an ID3v1 tag.
        bytes.extend(b"APETAGEX");
        bytes.extend([0; 24]);
        bytes.extend(b"TAG");
        bytes.extend([0xFF; 125]);
        assert_eq!(length(bytes), Some((40 * 1152, 48_000)), "footer {footer}");
    }
}

#[test]
fn a_damaged_stretch_is_skipped_and_a_cut_off_frame_is_not_counted() {
    let mut bytes = frames(L3_128, 384, 50);
    // A stretch holding a lone sync word that starts no run of frames.
    bytes.extend([0; 300]);
    bytes.extend(L3_128);
    bytes.extend([0x55; 700]);
    bytes.extend(frames(L3_128, 384, 50));
    bytes.extend(frame(L3_128, 384)[..200].iter());
    assert_eq!(length(bytes), Some((100 * 1152, 48_000)));
}

#[test]
fn a_frame_that_states_the_count_leaves_the_length_to_the_parser() {
    // Stereo MPEG-1 Layer III: the side information ends 36 bytes in.
    for tag in [&b"Xing"[..], b"Info"] {
        let mut first = frame(L3_128, 384);
        first[36..40].copy_from_slice(tag);
        let bytes = [first, frames(L3_128, 384, 40)].concat();
        assert_eq!(length(bytes), None, "{}", String::from_utf8_lossy(tag));
    }
    // A mono MPEG-2 frame's side information ends 13 bytes in.
    let mono = [0xFF, 0xF3, 0x84, 0xC4];
    let mut first = frame(mono, 192);
    first[13..17].copy_from_slice(b"Xing");
    assert_eq!(length([first, frames(mono, 192, 40)].concat()), None);
    let mut first = frame(L3_128, 384);
    first[36..40].copy_from_slice(b"VBRI");
    assert_eq!(length([first, frames(L3_128, 384, 40)].concat()), None);
}

#[test]
fn a_file_that_starts_no_run_of_frames_has_no_duration() {
    assert_eq!(length(Vec::new()), None);
    assert_eq!(length(b"RIFF\x00\x00\x00\x00WAVEfmt ".repeat(40)), None);
    // A free-format stream states no frame length.
    assert_eq!(length(frames([0xFF, 0xFB, 0x04, 0x44], 384, 40)), None);
    // Two sync words that do not start a third frame.
    assert_eq!(
        length([frame(L3_128, 384), frame(L3_128, 384), vec![7; 400]].concat()),
        None
    );
}

/// An MP3 encoded with no Xing header, at a constant or a variable bitrate,
/// and an ADTS AAC file probe to the sound written and their decoder reads to
/// the end; an MP3 with the header still probes from it.
#[test]
fn a_file_with_no_frame_count_probes_to_its_length() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    // 10 s of 48 kHz stereo pink noise.
    let sound = "audiotestsrc num-buffers=300 samplesperbuffer=1600 wave=pink-noise \
                 ! audio/x-raw,rate=48000,channels=2 ! audioconvert";
    let adts = format!(
        "{} ! aacparse ! audio/mpeg,stream-format=adts",
        aac_encoder()
    );
    for (name, encoder) in [
        ("cbr.mp3", "lamemp3enc target=bitrate cbr=true bitrate=192"),
        ("vbr.mp3", "lamemp3enc target=quality quality=2"),
        ("xing.mp3", "lamemp3enc target=quality quality=2 ! xingmux"),
        ("adts.aac", adts.as_str()),
    ] {
        let path = dir.path().join(name);
        run_to_eos(&format!(
            "{sound} ! {encoder} ! filesink location=\"{}\"",
            path.display()
        ));
        let probed = crate::probe(&path).unwrap_or_else(|err| panic!("{name}: probe: {err}"));
        // The encoder adds its delay and pads the sound to whole frames of
        // 24 ms.
        assert!(
            probed.has_audio && (10_000..=10_150).contains(&probed.frame_count),
            "{name}: {probed:?}"
        );
        let mut reader = GstAudioReader::open(&path, 48_000, 2)
            .unwrap_or_else(|err| panic!("{name}: the sound does not open: {err}"));
        reader.seek_to_frame(9 * 48_000).expect("a seek to 9 s");
        let mut out = vec![0.0; 2 * 4800];
        let mut read = 0;
        while read < 4800 {
            match reader.read(&mut out[2 * read..]).expect("the sound reads") {
                0 => break,
                got => read += got,
            }
        }
        assert_eq!(read, 4800, "{name}: 9 s in, the sound plays on");
    }
}
