//! Production freeze-frame command integration, validation, and history.

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AnimationRef, ClipId, Easing, Generator, Keyframe, LinkId, MediaId, MediaSource, Param,
    Project, Rational, RationalTime, TimeRange, TrackId, TrackKind,
};

const FPS: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn engine(project: Project) -> Engine {
    Engine::with_project(EngineConfig { undo_limit: 32 }, project).expect("engine")
}

fn snapshot(project: &Project) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snapshot.aria");
    project.save_to_file(&path).expect("save project snapshot");
    std::fs::read(path).expect("read project snapshot")
}

struct VideoFixture {
    project: Project,
    media: MediaId,
    track: TrackId,
    source: ClipId,
    next: ClipId,
}

fn video_fixture(next_start: i64) -> VideoFixture {
    let mut project = Project::new("freeze command", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/freeze-engine.mp4",
        1920,
        1080,
        FPS,
        1_000,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let source = project
        .add_clip(track, media, tr(100, 50), rt(10))
        .expect("source");
    let next = project
        .add_clip(track, media, tr(300, 20), rt(next_start))
        .expect("next");
    VideoFixture {
        project,
        media,
        track,
        source,
        next,
    }
}

fn apply_freeze(
    engine: &mut Engine,
    clip: ClipId,
    at: RationalTime,
    duration: RationalTime,
) -> ClipId {
    match engine
        .apply(Command::Edit(EditCommand::FreezeFrame {
            clip,
            at,
            duration,
        }))
        .expect("freeze frame")
    {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

fn track_layout(engine: &Engine, track: TrackId) -> Vec<(ClipId, i64, i64)> {
    engine
        .project()
        .timeline()
        .track(track)
        .unwrap()
        .clips_ordered()
        .into_iter()
        .map(|clip| {
            (
                clip.id,
                clip.timeline.start.value,
                clip.timeline.duration.value,
            )
        })
        .collect()
}

fn freeze_command(clip: ClipId, at: RationalTime, duration: RationalTime) -> EditCommand {
    EditCommand::FreezeFrame { clip, at, duration }
}

fn assert_rejected_unchanged(project: Project, command: EditCommand, message_fragment: &str) {
    let mut engine = engine(project);
    let before = snapshot(engine.project());
    let revision = engine.revision();
    let error = engine
        .apply(Command::Edit(command))
        .expect_err("command must reject");
    assert!(
        error.to_string().contains(message_fragment),
        "expected '{message_fragment}' in '{error}'"
    );
    assert_eq!(
        snapshot(engine.project()),
        before,
        "rejected freeze mutated the project"
    );
    assert_eq!(engine.revision(), revision);
    assert!(!engine.can_undo(), "rejected freeze added history");
    assert!(!engine.can_redo());
}

#[path = "freeze_frame_insertion_and_undo.rs"]
mod insertion_and_undo;

#[path = "freeze_frame_atomic_rejections.rs"]
mod atomic_rejections;
