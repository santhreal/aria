//! WHY: the import dialogs offered six video and audio containers, so an
//! `.m4a` voice memo, a `.flac` track, or an `.mxf` camera file could not be
//! picked although the GStreamer decoder reads each one. These tests derive
//! the containers from [`container_extensions`] and fail on one with no file
//! to write, or whose file the probe and the decoder do not read at its
//! length, so a container joins the list only with proof it imports. Not
//! caught: a codec other than the one each file is written with.

use aria_core::{Rational, RationalTime, VideoDecoder};

use super::*;
use crate::OutputMode;
use crate::gst_audio::GstAudioReader;
use crate::gst_test_media::{aac_encoder, run_to_eos};

/// 48 frames of 24 fps H.264.
const H264: &str = "videotestsrc num-buffers=48 ! video/x-raw,width=64,height=48,framerate=24/1,format=I420 \
     ! x264enc speed-preset=ultrafast key-int-max=24 ! h264parse";

/// 2 s of 48 kHz stereo sound.
const SOUND: &str =
    "audiotestsrc num-buffers=100 samplesperbuffer=960 ! audio/x-raw,rate=48000,channels=2";

/// The pipeline, up to its file sink, that writes two seconds of `ext`, and
/// whether the file holds a picture. `None` for a container with no pipeline.
fn writer(ext: &str) -> Option<(String, bool)> {
    let video = |mux: &str| Some((format!("{H264} ! {mux}"), true));
    let sound = |chain: &str| Some((format!("{SOUND} ! {chain}"), false));
    match ext {
        "mp4" | "m4v" => video("mp4mux"),
        "mov" => video("qtmux"),
        "mkv" => video("matroskamux"),
        "avi" => video("avimux"),
        // mxfmux indexes a long-GOP stream with a table mxfdemux misreads:
        // its seeks land on the wrong frame, and one past the table aborts
        // the process. An all-intra stream, as AVC-Intra cameras record,
        // indexes and seeks to every frame.
        "mxf" => Some((
            format!(
                "{} ! mxfmux",
                H264.replace("key-int-max=24", "key-int-max=1")
            ),
            true,
        )),
        "3gp" => video("3gppmux"),
        "mts" => video("mpegtsmux"),
        "m2ts" => video("mpegtsmux m2ts-mode=true"),
        "webm" => Some((
            "videotestsrc num-buffers=48 ! video/x-raw,width=64,height=48,framerate=24/1 \
             ! vp8enc deadline=1 ! webmmux"
                .to_owned(),
            true,
        )),
        "wav" => sound("wavenc"),
        "mp3" => sound("audioconvert ! lamemp3enc"),
        "m4a" => sound(&format!("{} ! mp4mux", aac_encoder())),
        "flac" => sound("flacenc"),
        "ogg" => sound("audioconvert ! vorbisenc ! oggmux"),
        "opus" => sound("audioconvert ! opusenc ! oggmux"),
        "aac" => sound(&format!(
            "{} ! aacparse ! audio/mpeg,stream-format=adts",
            aac_encoder()
        )),
        _ => None,
    }
}

/// Every container this build offers probes to the length written, and its
/// decoder reads it to the end: the last frame of a picture, or the sound.
#[test]
fn every_offered_container_imports_at_its_length() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let fps = Rational::new(24, 1);
    for ext in container_extensions() {
        let (pipeline, picture) =
            writer(ext).unwrap_or_else(|| panic!("no file is written for .{ext}"));
        let path = dir.path().join(format!("two-seconds.{ext}"));
        run_to_eos(&format!(
            "{pipeline} ! filesink location=\"{}\"",
            path.display()
        ));
        let probed = crate::probe(&path).unwrap_or_else(|err| panic!(".{ext}: probe: {err}"));
        if picture {
            assert_eq!((probed.frame_rate, probed.frame_count), (fps, 48), ".{ext}");
            let mut decoder = crate::GstDecoder::open(&path, OutputMode::Cpu)
                .unwrap_or_else(|err| panic!(".{ext}: open: {err}"));
            let last = decoder
                .frame_at(RationalTime::new(47, fps))
                .unwrap_or_else(|err| panic!(".{ext}: decode: {err}"))
                .unwrap_or_else(|| panic!(".{ext}: no last frame"));
            assert_eq!(aria_core::resample(last.pts, fps).value, 47, ".{ext}");
        } else {
            // An encoder may pad its first and last packet, by at most one
            // packet of a few dozen milliseconds.
            assert!(probed.width == 0 && probed.has_audio, ".{ext}: {probed:?}");
            assert!(
                (1950..=2100).contains(&probed.frame_count),
                ".{ext}: {} ms",
                probed.frame_count
            );
            GstAudioReader::open(&path, 48_000, 2)
                .unwrap_or_else(|err| panic!(".{ext}: the sound does not open: {err}"));
        }
    }
}

/// A container the import dialogs list once, the portable ones first.
#[test]
fn the_offered_containers_are_listed_once_portable_first() {
    let all = container_extensions();
    assert_eq!(all[..PORTABLE_CONTAINERS.len()], *PORTABLE_CONTAINERS);
    let mut unique = all.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "{all:?}");
}
