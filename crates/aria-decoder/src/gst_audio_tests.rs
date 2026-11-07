//! WHY: the audio pipeline let `uridecodebin` plug every stream of the file it
//! read, so reading the sound of a video file built a video parser and decoder
//! that decoded frames no sink pulled. With the CUDA decoder installed, each
//! audio open created a GPU context of its own: about 80 to 100 ms per open,
//! two opens per export of a clip with sound, and 1.5 to 2 s for the first
//! open of a process. These tests hold that an audio open plugs no video
//! element on a file with video and audio, and still reads all of its sound.
//!
//! Not caught: a container whose demuxer GStreamer files under a class other
//! than `Demuxer`; its audio would not open, which the open's error reports.

use std::path::{Path, PathBuf};

use aria_core::AudioReader;
use gstreamer as gst;
use gstreamer::prelude::*;

use super::{GstAudioReader, SOUND, STREAM, stream_bit};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// The element class of everything the reader's pipeline holds, bins
/// included.
fn plugged(reader: &GstAudioReader) -> Vec<String> {
    reader
        .pipeline
        .iterate_recurse()
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|element: gst::Element| element.factory())
        .map(|factory| {
            let name = factory.name();
            let klass = factory
                .metadata(gst::ELEMENT_METADATA_KLASS)
                .unwrap_or_default();
            format!("{name} ({klass})")
        })
        .collect()
}

/// Every frame the reader yields from its start.
fn frames(reader: &mut GstAudioReader) -> usize {
    let mut out = vec![0f32; 4096 * 2];
    let mut total = 0;
    loop {
        let read = reader.read(&mut out).expect("read the sound");
        if read == 0 {
            return total;
        }
        total += read;
    }
}

#[test]
fn the_sound_of_a_video_file_opens_with_no_video_element() {
    let mut reader = GstAudioReader::open(&fixture("fixture-640x360-24fps-3s-av.mp4"), 48_000, 2)
        .expect("open the sound of a video file");
    let plugged = plugged(&reader);
    assert!(
        plugged
            .iter()
            .any(|element| element.contains("Decoder/Audio")),
        "no audio decoder in {plugged:?}"
    );
    let video: Vec<_> = plugged
        .iter()
        .filter(|element| element.contains("Video") || element.contains("Image"))
        .collect();
    assert!(video.is_empty(), "the audio open plugged {video:?}");
    // Two seconds of sound at 48 kHz, and the frame the resampler rounds up.
    assert_eq!(frames(&mut reader), 96_001);
}

#[test]
fn a_file_of_sound_alone_reads_all_of_it() {
    let mut reader = GstAudioReader::open(&fixture("fixture-sine-48k-2s.wav"), 48_000, 2)
        .expect("open a sound file");
    assert_eq!(frames(&mut reader), 96_000);
}

/// WHY: a file with no sound exposed its other streams, linked none of them,
/// and failed with GStreamer's "Internal data stream error" and an
/// instruction to install plugins, which no install fixes. Every other
/// platform's reader states "no audio track". A source GStreamer cannot read
/// at all keeps the error it posted: having no sound is decided from the
/// streams the file exposed, not from the error.
///
/// Not caught: a file whose only sound stream has no decoder installed; it
/// exposes an audio stream, so its error stays the install hint. Nor an open
/// whose state change neither fails nor completes within its wait, which no
/// fixture reaches.
#[test]
fn a_file_with_no_sound_is_refused_as_having_no_audio_track() {
    for name in ["fixture-640x360-24fps-3s.mp4", "texture.png"] {
        let err = GstAudioReader::open(&fixture(name), 16_000, 1)
            .expect_err("a file with no sound opened for its sound");
        assert!(
            matches!(&err, aria_core::DecodeError::Unsupported(why) if why == "source has no audio track"),
            "{name}: {err}"
        );
    }
    let err = GstAudioReader::open(Path::new("/dev/null"), 16_000, 1)
        .expect_err("an empty source opened");
    assert!(
        !err.to_string().contains("no audio track"),
        "a source with no streams at all was reported as soundless: {err}"
    );
}

/// A file is soundless only when every stream it exposed was something other
/// than sound; one audio pad among them keeps the open's own error.
#[test]
fn an_audio_pad_counts_as_sound_and_any_other_does_not() {
    crate::gst::ensure_init().expect("init GStreamer");
    let caps = |name: &str| gst::Caps::builder(name).build();
    for (name, bit) in [
        ("audio/x-raw", SOUND),
        ("audio/mpeg", SOUND),
        ("video/x-h264", STREAM),
        ("image/png", STREAM),
        ("application/x-subtitle", STREAM),
    ] {
        assert_eq!(stream_bit(Some(&*caps(name))), bit, "{name}");
    }
    assert_eq!(stream_bit(None), STREAM, "a pad with no caps yet");
}
