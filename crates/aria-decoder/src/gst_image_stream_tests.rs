//! WHY: the probe kept a stream only when its caps named `video/` or
//! `audio/`, and a demuxer exposes a Motion JPEG or PNG stream as `image/jpeg`
//! or `image/png`. A camera's Motion JPEG AVI with sound imported as a sound
//! clip with no picture, and one without sound failed to import. These tests
//! close that class at the stream choice: every picture codec coded one image
//! at a time, in every container that holds it, with and without sound,
//! probes at its frame size, rate and length, is not sound alone, and decodes
//! every frame. They do not cover JPEG 2000, which this host has no encoder
//! for, or an album cover, which a demuxer states as a tag and not a stream.

use std::path::PathBuf;

use aria_core::{Rational, VideoDecoder};

use super::*;
use crate::GstDecoder;
use crate::gst_test_media::run_to_eos;

/// The encoders that code one picture per frame, and the containers that
/// hold each one.
const CODED_ONE_AT_A_TIME: [(&str, &[(&str, &str)]); 2] = [
    (
        "jpegenc",
        &[("qtmux", "mov"), ("avimux", "avi"), ("matroskamux", "mkv")],
    ),
    ("pngenc", &[("qtmux", "mov")]),
];

/// 48 frames of 64x48 at 24 fps coded by `encoder` into `muxer`, with 2 s of
/// 48 kHz PCM when `sound` holds.
fn picture_file(dir: &Path, encoder: &str, muxer: &str, ext: &str, sound: bool) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("{encoder}-{muxer}-{sound}.{ext}"));
    let mut desc = format!(
        "videotestsrc num-buffers=48 ! video/x-raw,width=64,height=48,framerate=24/1 ! \
         {encoder} ! queue ! {muxer} name=mux ! filesink location=\"{}\"",
        path.display()
    );
    if sound {
        desc.push_str(
            " audiotestsrc num-buffers=96 samplesperbuffer=1000 ! \
             audio/x-raw,format=S16LE,rate=48000,channels=2 ! queue ! mux.",
        );
    }
    run_to_eos(&desc);
    path
}

#[test]
fn a_picture_coded_one_image_at_a_time_probes_and_decodes_as_video() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for (encoder, muxers) in CODED_ONE_AT_A_TIME {
        for &(muxer, ext) in muxers {
            for sound in [false, true] {
                let file = picture_file(dir.path(), encoder, muxer, ext, sound);
                let case = format!("{encoder} in {muxer}, sound {sound}");

                let probed = probe(&file).unwrap_or_else(|e| panic!("{case}: probe: {e}"));
                assert_eq!((probed.width, probed.height), (64, 48), "{case}");
                assert_eq!(probed.frame_rate, Rational::new(24, 1), "{case}");
                assert_eq!(probed.frame_count, 48, "{case}");
                assert_eq!(probed.has_audio, sound, "{case}");
                assert!(probed.video_codec.is_some(), "{case}: the codec is named");
                assert!(!is_sound_alone(&file), "{case}: not sound alone");

                let mut decoder = GstDecoder::open(&file, OutputMode::Cpu)
                    .unwrap_or_else(|e| panic!("{case}: open: {e}"));
                let mut frames = 0;
                while let Some(frame) = decoder.next_frame().expect("the fixture decodes") {
                    assert_eq!(frame.coded_size, (64, 48), "{case}: frame {frames}");
                    frames += 1;
                }
                assert_eq!(frames, 48, "{case}: every frame decodes");
            }
        }
    }
}
