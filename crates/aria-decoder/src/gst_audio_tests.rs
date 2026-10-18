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

use super::GstAudioReader;

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
