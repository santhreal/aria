//! An import of a video or sound that states no length fails at the import.
//!
//! WHY: a probe that read a stream and no length for it, as GStreamer reads a
//! raw H.264 or MPEG-2 stream with no container, imported a pool entry zero
//! frames long, and the placement that followed failed with "invalid time
//! range" and no file name. Every decoder's probe reaches the pool through the
//! import, so the cases are probes a registered decoder states: each kind of
//! stream with a length, at zero frames and at one frame. Not caught: a length
//! that is stated wrong rather than missing.

use std::path::Path;
use std::sync::Arc;

mod common;

use aria_commands::{Command, ProjectCommand};
use aria_core::{DecodeError, Rational, VideoDecoder};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};
use aria_engine::{ApplyOutcome, EngineError};

/// A decoder that states `probe` for every file.
struct Stated(MediaProbe);

impl ExternalDecoder for Stated {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        Ok(self.0.clone())
    }

    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        Err(DecodeError::unsupported("this decoder only probes"))
    }
}

/// Each kind of stream that has a length, as `(name, probe)` at `frames`.
fn streams(frames: i64) -> [(&'static str, MediaProbe); 3] {
    let picture = MediaProbe {
        width: 640,
        height: 360,
        frame_rate: Rational::FPS_24,
        frame_count: frames,
        has_audio: false,
        is_image: false,
        video_codec: None,
        audio_codec: None,
    };
    [
        ("picture", picture.clone()),
        (
            "picture and sound",
            MediaProbe {
                has_audio: true,
                ..picture.clone()
            },
        ),
        (
            "sound",
            MediaProbe {
                width: 0,
                height: 0,
                frame_rate: Rational::new(1000, 1),
                has_audio: true,
                ..picture
            },
        ),
    ]
}

/// The import of a file a decoder states `probed` for.
fn import(ext: &str, probed: MediaProbe) -> Result<ApplyOutcome, EngineError> {
    let _registered = register_external_decoder(
        ext,
        &[ext.to_owned()],
        100,
        Arc::new(Stated(probed)) as Arc<dyn ExternalDecoder>,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("take.{ext}"));
    std::fs::write(&path, b"take").expect("write the take");
    let path = dunce::canonicalize(&path).expect("the take exists");
    let (_dir, mut engine) = common::temp_engine();
    engine.apply(Command::Project(ProjectCommand::Import { path }))
}

#[test]
fn a_stream_with_no_length_fails_the_import_and_one_frame_imports() {
    for (index, (kind, probed)) in streams(0).into_iter().enumerate() {
        let err = import(&format!("nolength{index}"), probed)
            .expect_err(&format!("{kind} with no length imports"));
        assert_eq!(
            err.to_string(),
            "failed to open source: the file states no length for its video or sound, as a raw \
             H.264 or MPEG-2 stream with no container does. Remux it into MP4 or Matroska and \
             import that.",
            "{kind}"
        );
    }
    for (index, (kind, probed)) in streams(1).into_iter().enumerate() {
        let outcome = import(&format!("onelength{index}"), probed)
            .unwrap_or_else(|err| panic!("{kind} one frame long: {err}"));
        assert!(
            matches!(outcome, ApplyOutcome::Imported { .. }),
            "{kind}: {outcome:?}"
        );
    }
}
