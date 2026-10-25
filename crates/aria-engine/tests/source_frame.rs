//! `media_frame_fit`: a picture of a pool source with nothing on the timeline.
//!
//! What this defends: the pool can show a file before it is placed, the scrub
//! tick is clamped instead of failing at the tail, an audio-only source is
//! refused with a sentence rather than a render error, and an unknown id is
//! named. What it does not catch: pixel colour — the fixtures here are only
//! guaranteed to decode, not to hold a known value.

mod common;

use aria_engine::{Engine, EngineConfig, EngineError};
use aria_models::{MediaId, MediaSource, Project, Rational};
use common::{image_asset, import_asset, small_video_asset, temp_engine};

#[test]
fn renders_a_pool_video_with_an_empty_timeline() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    assert_eq!(engine.project().timeline().track_count(), 0);
    // The import is itself unsaved work, so the claim below is that the render
    // changes nothing — not that the session is clean.
    let revision = engine.revision();
    assert!(engine.is_dirty(), "an imported file is unsaved work");

    let frame = engine
        .media_frame_fit(media, 0, 160, 90)
        .expect("source frame");

    assert!(frame.width <= 160 && frame.height <= 90, "fit is honored");
    assert_eq!(
        frame.pixels.len(),
        usize::try_from(frame.width * frame.height * 4).unwrap()
    );
    // The project is untouched: a preview is not an edit.
    assert_eq!(engine.project().timeline().track_count(), 0);
    assert_eq!(engine.revision(), revision, "a preview recorded a mutation");
}

#[test]
fn renders_a_still_at_its_own_aspect() {
    let Some(path) = image_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    let (width, height) = {
        let source = engine.project().media(media).expect("media");
        (source.width, source.height)
    };

    let frame = engine
        .media_frame_fit(media, 0, 4096, 4096)
        .expect("source frame");

    // Below the fit bound nothing is scaled, so the still is not letterboxed
    // into a derived 16:9 canvas.
    assert_eq!((frame.width, frame.height), (width, height));
}

#[test]
fn a_scrub_past_the_end_shows_the_last_frame() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);

    let end = engine.project().media(media).expect("media").duration.value;
    let past = engine
        .media_frame_fit(media, end + 5_000, 160, 90)
        .expect("clamped to the tail");
    let last = engine
        .media_frame_fit(media, end - 1, 160, 90)
        .expect("tail frame");

    assert_eq!(past.pixels, last.pixels);
}

#[test]
fn a_negative_tick_shows_the_first_frame() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);

    let before = engine
        .media_frame_fit(media, -12, 160, 90)
        .expect("clamped to the head");
    let first = engine.media_frame_fit(media, 0, 160, 90).expect("head");

    assert_eq!(before.pixels, first.pixels);
}

#[test]
fn an_unknown_source_is_named() {
    let (_dir, mut engine) = temp_engine();

    let err = engine
        .media_frame_fit(MediaId::from_raw(9_999), 0, 160, 90)
        .expect_err("no such source");

    assert!(
        matches!(err, EngineError::Model(_)),
        "unknown media should be a model error, got {err:?}"
    );
    assert!(err.to_string().contains("9999") || err.to_string().contains("9_999"));
}

#[test]
fn an_audio_only_source_is_refused() {
    // A probe reports an audio file with zero dimensions; build that shape
    // directly rather than shipping an audio fixture for one refusal.
    let mut project = Project::new("audio", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/pool/music.wav",
        0,
        0,
        Rational::new(48_000, 1),
        480,
        true,
    ));
    let mut engine = Engine::with_project(EngineConfig { undo_limit: 4 }, project).expect("engine");

    let err = engine
        .media_frame_fit(media, 0, 160, 90)
        .expect_err("audio has no picture");

    assert!(
        matches!(&err, EngineError::Refused(msg) if msg.contains("audio-only")),
        "expected a refusal sentence, got {err:?}"
    );
}
