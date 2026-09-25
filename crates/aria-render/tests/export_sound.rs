//! A sound export writes the mix to a file with no picture in it.
//!
//! The class this closes: a sound intermediate that is not the sound. It has
//! three ways to be wrong and all of them look like success. The file can be the
//! wrong length, so the clip it stands in for drifts; it can be silence, because
//! nothing checked that a mix reached the sink; or it can be published over a
//! good file after the export was abandoned. So every assertion here is about
//! the bytes on disk: the header the samples are described by, how many there
//! are, that they are audible, and that a cancelled export leaves what was there
//! before untouched.
//!
//! The source is written by the same sink the export writes with, so no fixture
//! file is checked in and the decode side is exercised against a real container.

use std::path::Path;

use aria_core::{AudioEncoderConfig, RationalTime};
use aria_models::{MediaSource, Project, Rational, TimeRange, TrackKind};
use aria_render::{EmptyExport, ExportSettings, RenderError, export_audio_to_file_observed};

/// Audio-only media counts milliseconds, the same convention the probe reports.
const MS: Rational = Rational::new(1000, 1);
const RATE: u32 = 48_000;
const CHANNELS: usize = 2;

/// Write `millis` of a loud 440 Hz tone, in the container the export writes.
fn write_tone(path: &Path, millis: i64) {
    let mut sink = aria_encoder::open_audio_encoder(path, AudioEncoderConfig::new(RATE, 2))
        .expect("a sound sink");
    let frames = millis * RATE as i64 / 1000;
    let mut block = vec![0.0f32; CHANNELS * RATE as usize / 100];
    let per_block = (block.len() / CHANNELS) as i64;
    let mut written = 0;
    while written < frames {
        let count = (frames - written).min(per_block);
        block.truncate(count as usize * CHANNELS);
        for frame in 0..count {
            let t = (written + frame) as f32 / RATE as f32;
            let sample = (std::f32::consts::TAU * 440.0 * t).sin() * 0.8;
            for channel in 0..CHANNELS {
                block[frame as usize * CHANNELS + channel] = sample;
            }
        }
        sink.push_audio(
            &block,
            RationalTime::new(written, Rational::new(RATE as i32, 1)),
        )
        .expect("a block");
        written += count;
    }
    sink.finish().expect("finish the source");
}

/// A project holding one second of that tone on an audio lane.
fn project_with_tone(path: &Path) -> Project {
    write_tone(path, 1000);
    let mut project = Project::new("sound", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(path, 0, 0, MS, 1000, true));
    let lane = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 1000, MS),
            RationalTime::new(0, Rational::FPS_24),
        )
        .expect("the clip");
    project
}

fn samples_of(bytes: &[u8]) -> Vec<i16> {
    bytes[44..]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[test]
fn a_sound_export_writes_the_span_it_covers_and_it_is_audible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = project_with_tone(&dir.path().join("song.wav"));
    let out = dir.path().join("bake.wav");

    let frames = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |_, _| true,
    )
    .expect("a sound export");

    assert_eq!(frames, 24, "one second at 24 fps");
    let bytes = std::fs::read(&out).expect("the exported file");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().expect("rate")),
        RATE
    );
    // One second of stereo 16-bit at 48 kHz, and the header agrees with it.
    let data = RATE as usize * CHANNELS * 2;
    assert_eq!(bytes.len(), 44 + data, "the file is the span it covers");
    assert_eq!(
        u32::from_le_bytes(bytes[40..44].try_into().expect("data size")) as usize,
        data,
        "the header counts the samples that are there"
    );

    let samples = samples_of(&bytes);
    let loudest = samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    assert!(
        loudest > 8_000,
        "the export wrote silence: the mix never reached the sink (peak {loudest})"
    );
}

/// A muted lane is nothing to render. With no picture to carry, a silent file
/// holds nothing at all, so the refusal names the fader rather than writing it.
#[test]
fn a_silent_sequence_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = project_with_tone(&dir.path().join("song.wav"));
    let lane = project
        .timeline()
        .tracks_ordered()
        .map(|track| track.id)
        .next()
        .expect("the lane");
    project
        .timeline_mut()
        .track_mut(lane)
        .expect("the lane")
        .muted = true;

    let out = dir.path().join("bake.wav");
    let err = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |_, _| true,
    )
    .expect_err("silence is not exportable");
    assert!(
        matches!(err, RenderError::Empty(EmptyExport::Silence)),
        "the refusal must name silence: {err}"
    );
    assert!(!out.exists(), "a refusal wrote a file anyway");
}

/// Publication is transactional, so abandoning an export leaves whatever was at
/// the destination exactly as it was. An intermediate is overwritten by the next
/// render of the same clip, and a half-written one that replaced a good file
/// would be decoded as the clip.
#[test]
fn a_cancelled_export_leaves_the_destination_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = project_with_tone(&dir.path().join("song.wav"));
    let out = dir.path().join("bake.wav");
    std::fs::write(&out, b"the render that is already there").expect("seed the destination");

    let err = export_audio_to_file_observed(
        &project,
        &out,
        ExportSettings::for_project(&project),
        &mut |done, _| done == 0,
    )
    .expect_err("cancelled");
    assert!(matches!(err, RenderError::Cancelled), "{err}");
    assert_eq!(
        std::fs::read(&out).expect("the old file"),
        b"the render that is already there",
        "the destination was replaced by an abandoned export"
    );
}
