//! Defect class: Subclip creation, pool integration, bounded placement, and history.
//!
//! A subclip is created from a source in/out, appears in the media pool as its own
//! item, and edits as a bounded clip whose source window may not exceed the parent's.
//! Undoing subclip creation removes it from the pool; redo restores it. Out-of-bounds
//! windows are refused with named model errors.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine};
use aria_models::{BinId, MediaId, MediaSource, ModelError, Project, Rational, TrackId, TrackKind};
use common::{rt, tr};

fn engine_with_parent_media(duration: i64) -> (tempfile::TempDir, Engine, MediaId, TrackId, BinId) {
    let dir = tempfile::tempdir().expect("tempdir");
    // The pool state is fixture setup, not the thing under test, so it is built
    // on a project and the engine is opened around it — the engine exposes no
    // mutable project, because every change to a live one is a command.
    let mut project = Project::new("subclip", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/media/long_take.mp4",
        1920,
        1080,
        Rational::FPS_24,
        duration,
        true,
    ));
    let bin = project.add_bin("Selects", None).expect("add bin");
    let track = project.add_track(TrackKind::Video, "V1");
    let engine = Engine::with_project(aria_engine::EngineConfig { undo_limit: 32 }, project)
        .expect("engine");
    (dir, engine, media, track, bin)
}

#[test]
fn create_subclip_appears_in_pool_and_undo_redo_cycles() {
    let (_dir, mut engine, parent, _track, bin) = engine_with_parent_media(1000);

    // Create a subclip for source window [100, 400) (duration 300 ticks) inside bin.
    engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Take 1 Dialog".to_string(),
            source: tr(100, 300),
            bin: Some(bin),
        }))
        .expect("create subclip");

    let subclip = engine
        .project()
        .media_iter()
        .find(|m| m.is_subclip())
        .expect("subclip present in pool")
        .clone();

    assert_eq!(subclip.parent, Some(parent));
    assert_eq!(subclip.source_range, Some(tr(100, 300)));
    assert_eq!(subclip.duration, rt(300));
    assert_eq!(subclip.display_name(), "Take 1 Dialog");
    assert_eq!(subclip.bin, Some(bin));
    assert_eq!(subclip.frame_rate, Rational::FPS_24);

    let subclip_id = subclip.id;

    // Undo subclip creation removes it from the pool.
    assert!(engine.undo(), "undo");
    assert!(engine.project().media(subclip_id).is_none());

    // Redo restores the subclip in the pool.
    assert!(engine.redo(), "redo");
    let restored = engine
        .project()
        .media(subclip_id)
        .expect("restored subclip");
    assert_eq!(restored.parent, Some(parent));
    assert_eq!(restored.source_range, Some(tr(100, 300)));
    assert_eq!(restored.duration, rt(300));
    assert_eq!(restored.display_name(), "Take 1 Dialog");
}

#[test]
fn create_subclip_refuses_out_of_bounds_and_empty_ranges_with_named_rules() {
    let (_dir, mut engine, parent, _track, bin) = engine_with_parent_media(1000);

    // 1. Out-of-bounds: window reaches past parent end (800 + 300 = 1100 > 1000)
    let err_oob = engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Past End".to_string(),
            source: tr(800, 300),
            bin: Some(bin),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_oob,
            aria_engine::EngineError::Model(ModelError::SourceOutOfBounds)
        ),
        "expected SourceOutOfBounds, got: {err_oob}"
    );

    // 2. Out-of-bounds: negative start
    let err_neg = engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Negative Start".to_string(),
            source: tr(-10, 200),
            bin: Some(bin),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_neg,
            aria_engine::EngineError::Model(ModelError::SourceOutOfBounds)
        ),
        "expected SourceOutOfBounds, got: {err_neg}"
    );

    // 3. Empty range
    let err_empty = engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Empty".to_string(),
            source: tr(100, 0),
            bin: Some(bin),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_empty,
            aria_engine::EngineError::Model(ModelError::InvalidRange)
        ),
        "expected InvalidRange, got: {err_empty}"
    );

    // 4. Unknown parent media
    let missing_media = MediaId::next();
    let err_missing = engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent: missing_media,
            name: "Missing Parent".to_string(),
            source: tr(0, 100),
            bin: Some(bin),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_missing,
            aria_engine::EngineError::Model(ModelError::UnknownMedia(_))
        ),
        "expected UnknownMedia, got: {err_missing}"
    );

    // 5. Unknown bin
    let missing_bin = BinId::next();
    let err_bin = engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Missing Bin".to_string(),
            source: tr(0, 100),
            bin: Some(missing_bin),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_bin,
            aria_engine::EngineError::Model(ModelError::UnknownBin(_))
        ),
        "expected UnknownBin, got: {err_bin}"
    );
}

#[test]
fn subclip_placed_on_timeline_edits_as_bounded_clip() {
    let (_dir, mut engine, parent, track, _bin) = engine_with_parent_media(1000);

    // Create subclip for window [200, 500) (duration 300).
    engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Working Piece".to_string(),
            source: tr(200, 300),
            bin: None,
        }))
        .expect("create subclip");

    let subclip_id = engine
        .project()
        .media_iter()
        .find(|m| m.is_subclip())
        .expect("subclip exists")
        .id;

    // Place subclip on timeline: whole subclip window [0, 300) placed at tick 0.
    let outcome = engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media: subclip_id,
            source: tr(0, 300),
            start: rt(0),
        }))
        .expect("place subclip on timeline");

    let clip_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let clip = engine.project().clip(clip_id).expect("clip");
    assert_eq!(clip.timeline, tr(0, 300));
    assert_eq!(clip.source_range(), Some(tr(0, 300)));

    // Trim clip within subclip bounds (shorten to 150 ticks).
    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 150),
        }))
        .expect("trim within bounds");

    let trimmed = engine.project().clip(clip_id).expect("trimmed clip");
    assert_eq!(trimmed.timeline, tr(0, 150));
    assert_eq!(trimmed.source_range(), Some(tr(0, 150)));

    // Slip within subclip bounds (slip window by 50 ticks inside the 300-tick subclip).
    engine
        .apply(Command::Edit(EditCommand::SlipClip {
            clip: clip_id,
            by: 50,
        }))
        .expect("slip within bounds");

    let slipped = engine.project().clip(clip_id).expect("slipped clip");
    assert_eq!(slipped.timeline, tr(0, 150));
    assert_eq!(slipped.source_range(), Some(tr(50, 150)));

    // Trim clip OUTSIDE subclip bounds (e.g. extending to 400 ticks when subclip is 300) is refused.
    let err_trim_oob = engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 400),
        }))
        .unwrap_err();
    assert!(
        matches!(
            err_trim_oob,
            aria_engine::EngineError::Model(ModelError::SourceOutOfBounds)
        ),
        "expected SourceOutOfBounds when trimming past subclip extent, got: {err_trim_oob}"
    );
}

#[test]
fn subclip_roundtrips_save_and_load() {
    let (_dir, mut engine, parent, track, bin) = engine_with_parent_media(1000);

    engine
        .apply(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name: "Clip Selection".to_string(),
            source: tr(50, 250),
            bin: Some(bin),
        }))
        .expect("create subclip");

    let subclip_id = engine
        .project()
        .media_iter()
        .find(|m| m.is_subclip())
        .expect("subclip exists")
        .id;

    let outcome = engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media: subclip_id,
            source: tr(0, 200),
            start: rt(10),
        }))
        .expect("add clip");

    let clip_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };

    let serialized = engine.project().save_to_string().expect("save");
    let loaded = Project::load_from_string(&serialized).expect("load");

    let loaded_subclip = loaded.media(subclip_id).expect("subclip loaded");
    assert_eq!(loaded_subclip.parent, Some(parent));
    assert_eq!(loaded_subclip.source_range, Some(tr(50, 250)));
    assert_eq!(loaded_subclip.duration, rt(250));
    assert_eq!(loaded_subclip.display_name(), "Clip Selection");
    assert_eq!(loaded_subclip.bin, Some(bin));

    let loaded_clip = loaded.clip(clip_id).expect("clip loaded");
    assert_eq!(loaded_clip.timeline, tr(10, 200));
    assert_eq!(loaded_clip.source_range(), Some(tr(0, 200)));
}
