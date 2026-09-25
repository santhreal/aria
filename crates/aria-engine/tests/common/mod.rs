//! Shared helpers for `aria-engine` integration tests.

// Each integration test binary includes this module privately, so a helper only
// some binaries call is dead there, and every `pub` in it is unreachable from
// outside that binary by construction. Both are properties of a shared test
// helper, not defects in it.
#![allow(dead_code, unreachable_pub)]

use std::path::{Path, PathBuf};

use aria_commands::{Command, EditCommand, EditOutcome, ProjectCommand};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    ClipId, Generator, MediaId, Rational, RationalTime, TimeRange, TrackId, TrackKind,
};

pub fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-assets/assets")
}

/// A real video file to import.
///
/// The checked-in fixture guarantees the media paths are exercised on every
/// machine; `local-assets` lets a developer point the same tests at richer
/// media (longer, 1080p, other codecs) by dropping files there.
///
/// The fixture is 24 fps because [`rt`] and [`tr`] are: importing a video sets
/// the sequence rate from the file, and placing a clip at another rate is a
/// `RateMismatch` refusal rather than a test.
///
/// Require the decoder to actually open it rather than that the file exists: a
/// platform with no native decoder cannot read it, and a test that skips is
/// better than one asserting on an `Unsupported` error.
pub fn small_video_asset() -> Option<PathBuf> {
    let dir = assets_dir();
    for name in [
        "15531444_1920_1080_24fps.mp4",
        "6137050-hd_1920_1080_24fps.mp4",
    ] {
        let path = dir.join(name);
        if readable_video(&path) {
            return Some(path);
        }
    }
    let local = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "mp4") && readable_video(path));
    local.or_else(|| {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4");
        readable_video(&fixture).then_some(fixture)
    })
}

/// Whether the platform's decoder can open `path` at all. An import probes the
/// file, so a file the decoder cannot open is a file these tests cannot use.
fn readable_video(path: &Path) -> bool {
    path.exists() && aria_decoder::open_video_decoder(path, aria_decoder::OutputMode::Cpu).is_ok()
}

pub fn image_asset() -> Option<PathBuf> {
    let path = assets_dir().join("texture.png");
    path.exists().then_some(path)
}

pub fn temp_engine() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::new(EngineConfig { undo_limit: 32 }).expect("engine");
    (dir, engine)
}

pub fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, Rational::FPS_24)
}

pub fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, Rational::FPS_24)
}

pub fn add_track(engine: &mut Engine, kind: TrackKind, name: &str) -> TrackId {
    match engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind,
            name: name.into(),
            index: None,
            pinned: false,
        }))
        .expect("add track")
    {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => id,
        other => panic!("expected CreatedTrack, got {other:?}"),
    }
}

pub fn import_asset(engine: &mut Engine, path: &Path) -> MediaId {
    match engine
        .apply(Command::Project(ProjectCommand::Import {
            path: path.to_path_buf(),
        }))
        .expect("import")
    {
        ApplyOutcome::Imported { media } => media,
        other => panic!("expected import outcome, got {other:?}"),
    }
}

pub fn remove_media(engine: &mut Engine, media: MediaId) {
    match engine
        .apply(Command::Project(ProjectCommand::RemoveMedia { media }))
        .expect("remove media")
    {
        ApplyOutcome::RemovedMedia { media: removed } => assert_eq!(removed, media),
        other => panic!("expected RemovedMedia, got {other:?}"),
    }
}

pub fn created_clip(outcome: ApplyOutcome) -> ClipId {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

/// Track kind that accepts clips produced by `generator`.
pub fn track_kind_for(generator: &Generator) -> TrackKind {
    TrackKind::for_generator(generator).expect("generator maps to a track kind")
}

pub fn add_track_for_generator(engine: &mut Engine, generator: &Generator, name: &str) -> TrackId {
    add_track(engine, track_kind_for(generator), name)
}

pub fn add_generated(
    engine: &mut Engine,
    track: TrackId,
    generator: Generator,
    timeline: TimeRange,
) -> ClipId {
    created_clip(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator,
                timeline,
            }))
            .expect("add generated"),
    )
}

pub fn add_media_clip(
    engine: &mut Engine,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    start: RationalTime,
) -> ClipId {
    created_clip(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media,
                source,
                start,
            }))
            .expect("add clip"),
    )
}

pub fn export_to(engine: &mut Engine, path: &Path) -> u64 {
    match engine
        .apply(Command::Project(ProjectCommand::Export {
            path: path.to_path_buf(),
            format: Default::default(),
        }))
        .expect("export command")
    {
        ApplyOutcome::Exported { frames } => frames,
        other => panic!("expected Exported, got {other:?}"),
    }
}

pub fn save_project(engine: &mut Engine, path: &Path) {
    match engine
        .apply(Command::Project(ProjectCommand::Save {
            path: path.to_path_buf(),
        }))
        .expect("save")
    {
        ApplyOutcome::Saved => {}
        other => panic!("expected Saved, got {other:?}"),
    }
}

pub fn open_project(engine: &mut Engine, path: &Path) {
    match engine
        .apply(Command::Project(ProjectCommand::Open {
            path: path.to_path_buf(),
        }))
        .expect("open")
    {
        ApplyOutcome::Opened => {}
        other => panic!("expected Opened, got {other:?}"),
    }
}

pub fn load_project(engine: &mut Engine, path: &Path) {
    match engine
        .apply(Command::Project(ProjectCommand::Load {
            path: path.to_path_buf(),
        }))
        .expect("load")
    {
        ApplyOutcome::Loaded => {}
        other => panic!("expected Loaded, got {other:?}"),
    }
}

/// All MP4 assets under `local-assets/assets/`, sorted for deterministic tests.
pub fn video_assets() -> Vec<PathBuf> {
    let dir = assets_dir();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "mp4"))
        .collect();
    paths.sort();
    paths
}
