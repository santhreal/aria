//! WHY: a probe that plugs a decoder loads the decoder's plugin, and the CUDA
//! one takes about 1.5 s to load while GStreamer loads nothing else. Every
//! import paid it, or waited for the decode device warm-up paying it, before
//! the editor window opened on media named on the command line. These tests
//! close that class at the probe: a probe of any checked-in media loads no
//! plugin that registers a decoder, and what it reads at the stream caps is
//! what the decoder opens with. They do not catch a decoder plugin loaded by a
//! format no checked-in fixture has.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gstreamer as gst;

use super::*;
use crate::gst_audio::GstAudioReader;
use crate::gst_test_media::h264_aac_mp4;

/// Every checked-in fixture the stream probe reads: each file in the fixture
/// directories that is not a still image.
fn stream_fixtures() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<PathBuf> = [
        manifest.join("../../tests/fixtures"),
        manifest.join("tests/fixtures"),
    ]
    .iter()
    .flat_map(|dir| {
        std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
            .map(|entry| entry.expect("a fixture directory entry").path())
    })
    .filter(|path| path.is_file() && crate::image::sniff_image(path).is_none())
    .collect();
    found.sort();
    assert!(
        found.len() >= 4,
        "expected the checked-in video and audio fixtures, found {found:?}"
    );
    found
}

/// The loaded plugins that register a codec decoder.
fn loaded_decoder_plugins() -> Vec<String> {
    let registry = gst::Registry::get();
    registry
        .plugins()
        .into_iter()
        .filter(|plugin| plugin.is_loaded())
        .map(|plugin| plugin.plugin_name().to_string())
        .filter(|name| {
            registry
                .features_by_plugin(name)
                .into_iter()
                .any(|feature| {
                    feature
                        .downcast::<gst::ElementFactory>()
                        .ok()
                        .and_then(|factory| {
                            factory
                                .metadata(gst::ELEMENT_METADATA_KLASS)
                                .map(str::to_owned)
                        })
                        .is_some_and(|klass| klass.contains("Decoder") && !klass.contains("Bin"))
                })
        })
        .collect()
}

/// Run in a fresh process by [`a_probe_loads_no_decoder_plugin`]: probe every
/// stream fixture, then fail when a decoder plugin is loaded.
#[test]
#[ignore = "run in a child process by a_probe_loads_no_decoder_plugin"]
fn probe_child() {
    crate::gst::ensure_init().expect("GStreamer initializes");
    assert_eq!(
        loaded_decoder_plugins(),
        Vec::<String>::new(),
        "a decoder plugin was loaded before any probe"
    );
    for path in stream_fixtures() {
        crate::probe(&path).unwrap_or_else(|err| panic!("probe {}: {err}", path.display()));
        assert_eq!(
            loaded_decoder_plugins(),
            Vec::<String>::new(),
            "probing {} loaded a decoder plugin",
            path.display()
        );
    }
}

#[test]
fn a_probe_loads_no_decoder_plugin() {
    const BOUND: Duration = Duration::from_secs(60);
    let ran = crate::test_child::run("gst_probe::tests::probe_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the probing child did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert!(
        status.success() && ran.stdout.contains("1 passed"),
        "the probing child failed\n{}",
        ran.transcript()
    );
}

/// Each stream fixture, and a synthetic MP4 with B-frames, an edit list, and
/// an AAC track, probes to the frame size, rate, and length its decoder opens
/// with, and states audio exactly when its audio track opens.
#[test]
fn every_fixture_probes_as_its_decoder_opens() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut sources = stream_fixtures();
    sources.push(h264_aac_mp4(dir.path(), 2));
    for path in sources {
        let probed =
            crate::probe(&path).unwrap_or_else(|err| panic!("probe {}: {err}", path.display()));
        match crate::GstDecoder::open(&path, OutputMode::Cpu) {
            Ok(decoder) => {
                let opened = MediaProbe::video(decoder.info(), Tracks::default());
                assert_eq!(
                    (
                        probed.width,
                        probed.height,
                        probed.frame_rate,
                        probed.frame_count
                    ),
                    (
                        opened.width,
                        opened.height,
                        opened.frame_rate,
                        opened.frame_count
                    ),
                    "{}: the probe and the decoder disagree",
                    path.display()
                );
                assert!(
                    probed.video_codec.is_some(),
                    "{}: the probe names no video codec",
                    path.display()
                );
            }
            Err(err) => assert!(
                probed.width == 0 && probed.height == 0 && probed.video_codec.is_none(),
                "{}: no video decoder opens ({err}) but the probe states a picture: {probed:?}",
                path.display()
            ),
        }
        let audio = GstAudioReader::open(&path, 48_000, 2).is_ok();
        assert_eq!(
            (probed.has_audio, probed.audio_codec.is_some()),
            (audio, audio),
            "{}: the probe states audio {}, the audio reader {}",
            path.display(),
            probed.has_audio,
            if audio { "opens" } else { "does not open" }
        );
    }
}

/// The codec names come from the stream caps, not the container: a WAV file's
/// audio is PCM.
#[test]
fn codec_names_come_from_the_stream_caps() {
    let fixture = |name: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    };
    let cases = [
        (
            fixture("fixture-640x360-24fps-3s-av.mp4"),
            Some("H.264 (High Profile)"),
            Some("MPEG-4 AAC"),
        ),
        (
            fixture("fixture-640x360-24fps-3s.mp4"),
            Some("H.264 (High Profile)"),
            None,
        ),
        (
            fixture("fixture-sine-48k-2s.wav"),
            None,
            Some("Raw 16-bit PCM audio"),
        ),
    ];
    for (path, video, audio) in cases {
        let probed =
            crate::probe(&path).unwrap_or_else(|err| panic!("probe {}: {err}", path.display()));
        assert_eq!(
            (probed.video_codec.as_deref(), probed.audio_codec.as_deref()),
            (video, audio),
            "{}",
            path.display()
        );
    }
}

/// An audio-only file probes at millisecond ticks for its whole length.
#[test]
fn an_audio_file_probes_its_length_in_milliseconds() {
    let wav =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fixture-sine-48k-2s.wav");
    let probed = crate::probe(&wav).expect("probe the WAV fixture");
    assert_eq!(
        (
            probed.width,
            probed.height,
            probed.frame_rate,
            probed.frame_count,
            probed.has_audio
        ),
        (0, 0, aria_core::Rational::new(1000, 1), 2000, true)
    );
}

/// A stream whose caps state no frame size is opened with its decoder; caps
/// with a missing, zero, or negative side state none.
#[test]
fn a_stream_without_a_stated_frame_size_reads_it_from_its_decoder() {
    let opengop = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opengop_h264.mp4");
    let decoder = crate::GstDecoder::open(&opengop, OutputMode::Cpu).expect("open the fixture");
    let bare = gst::Caps::builder("video/x-h264").build();
    let info = video_info(&opengop, &bare).expect("the decoder states the frame");
    assert_eq!(
        (info.display_size, info.frame_rate),
        (decoder.info().display_size, decoder.info().frame_rate)
    );
    for caps in [
        gst::Caps::builder("video/x-h264")
            .field("width", 320)
            .build(),
        gst::Caps::builder("video/x-h264")
            .field("width", 0)
            .field("height", 180)
            .build(),
        gst::Caps::builder("video/x-h264")
            .field("width", -320)
            .field("height", 180)
            .build(),
    ] {
        assert!(stated_frame(&caps).is_none(), "{caps} states a frame size");
    }
    let stated = gst::Caps::builder("video/x-h264")
        .field("width", 320)
        .field("height", 180)
        .field("framerate", gst::Fraction::new(0, 1))
        .build();
    let info = stated_frame(&stated).expect("320x180 is a frame size");
    assert_eq!(
        (info.display_size, info.frame_rate),
        ((320, 180), aria_core::Rational::new(30, 1)),
        "a variable rate reads as 30 fps, as a decoded one does"
    );
}

/// A file that is not media fails with the error GStreamer posted for it, and
/// a path that does not exist fails before a pipeline is built.
#[test]
fn a_file_that_is_not_media_fails_with_its_cause() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let text = dir.path().join("notes.mp4");
    std::fs::write(&text, "these are notes, not a movie\n".repeat(64)).expect("write the file");
    let err = crate::probe(&text).expect_err("text is not media");
    assert!(
        err.to_string().contains("GStreamer: "),
        "the error does not state the error GStreamer posted: {err}"
    );
    let gone = dir.path().join("gone.mp4");
    let err = crate::probe(&gone).expect_err("a missing file is not media");
    assert!(err.to_string().contains("couldn't resolve"), "{err}");
}
