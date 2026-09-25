//! Defect class: Retimed paste missing from command registry and engine actions.
//!
//! A paste variant places a copy retimed to a target duration (or a chosen rate),
//! as one atomic undo entry. The pasted clip reports the new speed through the same
//! speed fields the inspector reads. Out-of-bounds targets and non-positive durations
//! are refused with named errors.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{ClipId, MediaId, MediaSource, Project, Rational, TrackId, TrackKind};
use common::{add_track, rt, tr};

fn engine_with_media(media_duration: i64) -> (tempfile::TempDir, Engine, MediaId, TrackId, ClipId) {
    let dir = tempfile::tempdir().expect("tempdir");
    // Pool and source clip are fixture setup, not the thing under test, so they
    // are built on a project and the engine opened around it — a live engine
    // exposes no mutable project, because every change to one is a command.
    let mut project = Project::new("paste retimed", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/media/take.mp4",
        1920,
        1080,
        Rational::FPS_24,
        media_duration,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(track, media, tr(0, 100), rt(0))
        .expect("add source clip");
    let engine = Engine::with_project(aria_engine::EngineConfig { undo_limit: 32 }, project)
        .expect("engine");
    (dir, engine, media, track, clip)
}

#[test]
fn paste_retimed_with_target_duration_derives_speed_and_undoes_in_one_step() {
    let (_dir, mut engine, _media, track, source_clip) = engine_with_media(1000);

    // Source clip is 100 ticks @ 24fps (1.0x speed).
    // Paste retimed to target duration 50 ticks (should derive 2.0x speed).
    let outcome = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: Some(rt(50)),
            target_speed: None,
        }))
        .expect("paste retimed");

    let pasted_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let clip = engine.project().clip(pasted_id).expect("pasted clip");
    assert_eq!(clip.timeline, tr(200, 50));
    assert_eq!(clip.speed, Rational::new(2, 1));
    // Inspector speed accessor verification:
    let inspector_speed = clip.speed.num as f32 / clip.speed.den as f32;
    assert_eq!(inspector_speed, 2.0);

    // One undo completely removes the pasted clip.
    assert!(engine.undo(), "undo");
    assert!(engine.project().clip(pasted_id).is_none());

    // Redo restores the retimed duplicate with its exact speed and timeline range.
    assert!(engine.redo(), "redo");
    let restored = engine.project().clip(pasted_id).expect("restored clip");
    assert_eq!(restored.timeline, tr(200, 50));
    assert_eq!(restored.speed, Rational::new(2, 1));
}

#[test]
fn paste_retimed_with_target_speed_derives_duration_and_reads_back_in_inspector() {
    let (_dir, mut engine, _media, track, source_clip) = engine_with_media(1000);

    // Paste with explicit target speed 0.5x (1/2), occupying 200 ticks.
    let outcome = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(100),
            target_duration: None,
            target_speed: Some(Rational::new(1, 2)),
        }))
        .expect("paste retimed with speed");

    let pasted_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let clip = engine.project().clip(pasted_id).expect("pasted clip");
    assert_eq!(clip.timeline, tr(100, 200));
    assert_eq!(clip.speed, Rational::new(1, 2));
    let inspector_speed = clip.speed.num as f32 / clip.speed.den as f32;
    assert_eq!(inspector_speed, 0.5);
}

#[test]
fn paste_retimed_refuses_out_of_bounds_and_invalid_targets_with_named_errors() {
    let (_dir, mut engine, _media, track, source_clip) = engine_with_media(1000);

    // 1. Zero target duration
    let err_zero_dur = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: Some(rt(0)),
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        err_zero_dur
            .to_string()
            .contains("target duration must be positive"),
        "expected positive duration error, got: {err_zero_dur}"
    );

    // 2. Negative target duration
    let err_neg_dur = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: Some(rt(-50)),
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        err_neg_dur
            .to_string()
            .contains("target duration must be positive"),
        "expected positive duration error, got: {err_neg_dur}"
    );

    // 3. Speed below minimum (0.05)
    let err_too_slow = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: None,
            target_speed: Some(Rational::new(1, 100)), // 0.01x
        }))
        .unwrap_err();
    assert!(
        err_too_slow.to_string().contains("outside documented min"),
        "expected min speed error, got: {err_too_slow}"
    );

    // 4. Speed above maximum (100.0)
    let err_too_fast = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: None,
            target_speed: Some(Rational::new(200, 1)), // 200.0x
        }))
        .unwrap_err();
    assert!(
        err_too_fast.to_string().contains("outside documented min"),
        "expected max speed error, got: {err_too_fast}"
    );

    // 5. Target duration that implies speed below min (100 ticks stretched to 10000 ticks = 0.01x)
    let err_implied_slow = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: Some(rt(10000)),
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        err_implied_slow
            .to_string()
            .contains("outside documented min"),
        "expected implied min speed error, got: {err_implied_slow}"
    );

    // 6. Missing both duration and speed
    let err_missing_both = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: None,
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        err_missing_both
            .to_string()
            .contains("requires a target duration or target speed"),
        "expected missing target error, got: {err_missing_both}"
    );

    // 7. Generated clip rejected
    let sticker_track = add_track(&mut engine, TrackKind::Sticker, "Stickers");
    let generated = engine
        .apply(Command::Edit(EditCommand::AddGenerated {
            track: sticker_track,
            generator: aria_models::Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            timeline: tr(0, 50),
        }))
        .expect("add generated");
    let gen_id = match generated {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };
    let err_gen = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: gen_id,
            to_track: sticker_track,
            start: rt(100),
            target_duration: Some(rt(25)),
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        err_gen
            .to_string()
            .contains("speed requires a media-backed clip"),
        "expected media-backed requirement error, got: {err_gen}"
    );

    // 8. Overlap on destination track
    let err_overlap = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(50), // overlaps [0, 100)
            target_duration: Some(rt(100)),
            target_speed: None,
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_overlap,
            aria_engine::EngineError::Model(aria_models::ModelError::Overlap(_))
        ),
        "expected overlap error, got: {err_overlap}"
    );
}

#[test]
fn paste_retimed_roundtrips_save_and_load() {
    let (_dir, mut engine, _media, track, source_clip) = engine_with_media(1000);

    let outcome = engine
        .apply(Command::Edit(EditCommand::PasteRetimed {
            clip: source_clip,
            to_track: track,
            start: rt(200),
            target_duration: Some(rt(40)),
            target_speed: None,
        }))
        .expect("paste retimed");

    let pasted_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let serialized = engine.project().save_to_string().expect("save");
    let loaded = Project::load_from_string(&serialized).expect("load");

    let clip = loaded
        .clip(pasted_id)
        .expect("pasted clip in loaded project");
    assert_eq!(clip.timeline, tr(200, 40));
    assert_eq!(clip.speed, Rational::new(5, 2)); // 100/40 = 5/2 = 2.5x
    assert_eq!(clip.speed.num as f32 / clip.speed.den as f32, 2.5);
}
