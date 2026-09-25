//! Tests for [`intents`].
//!
//! Declared from `intents.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;
use aria_models::MediaSource;

const FPS: Rational = Rational::FPS_30;

#[test]
fn frame_tick_rounds_to_nearest_frame_and_clamps() {
    assert_eq!(frame_tick(0.0, FPS), 0);
    assert_eq!(frame_tick(-2.0, FPS), 0);
    assert_eq!(frame_tick(1.0, FPS), 30);
    assert_eq!(frame_tick(0.016, FPS), 0); // 0.48 frames rounds down
    assert_eq!(frame_tick(0.017, FPS), 1); // 0.51 frames rounds up
}

#[test]
fn speed_maps_to_milli_rational_and_clamps() {
    assert_eq!(speed_to_rational(2.0), Rational::new(2000, 1000));
    assert_eq!(speed_to_rational(0.3333), Rational::new(333, 1000));
    assert_eq!(speed_to_rational(0.0), Rational::new(50, 1000));
    assert_eq!(speed_to_rational(500.0), Rational::new(100_000, 1000));
}

#[test]
fn extract_audio_delegates_once_and_rejects_a_second_request() {
    let mut project = Project::new("extract", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/extract-mobile.mp4",
        1920,
        1080,
        FPS,
        300,
        true,
    ));
    let main = project.add_track(TrackKind::Video, "Main");
    let video = project
        .add_clip(
            main,
            media,
            TimeRange::at_rate(30, 90, FPS),
            RationalTime::new(60, FPS),
        )
        .unwrap();
    let mut engine =
        Engine::with_project(aria_engine::EngineConfig::default(), project).expect("engine");

    let first = run(&mut engine, Intent::ExtractAudio { clip: video }).unwrap();
    assert!(first["clip"].is_u64());
    assert!(first["track"].is_u64());
    assert_eq!(engine.project().timeline().clip_count(), 2);
    assert!(!engine.project().timeline().carries_own_audio(video));

    let before = serde_json::to_vec(engine.project()).unwrap();
    let error = run(&mut engine, Intent::ExtractAudio { clip: video })
        .expect_err("already-extracted audio must be strict");
    assert!(error.to_string().contains("already has extracted audio"));
    assert_eq!(serde_json::to_vec(engine.project()).unwrap(), before);

    assert!(engine.undo(), "the first extraction is one history entry");
    assert_eq!(engine.project().timeline().clip_count(), 1);
    assert!(engine.project().timeline().carries_own_audio(video));
    assert!(!engine.undo(), "the rejected retry records no history");
}

fn project_with_main() -> (Project, TrackId) {
    let mut project = Project::new("t", FPS);
    let main = project.add_track(TrackKind::Video, "Main");
    (project, main)
}

#[test]
fn nearest_boundary_snaps_between_clips() {
    let (mut project, main) = project_with_main();
    let media = project.add_media(MediaSource::new("/tmp/a.mp4", 640, 360, FPS, 300, false));
    project
        .add_clip(
            main,
            media,
            TimeRange::at_rate(0, 60, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap();
    project
        .add_clip(
            main,
            media,
            TimeRange::at_rate(0, 60, FPS),
            RationalTime::new(60, FPS),
        )
        .unwrap();

    assert_eq!(nearest_main_boundary(&project, main, 0), 0);
    assert_eq!(nearest_main_boundary(&project, main, 25), 0);
    assert_eq!(nearest_main_boundary(&project, main, 45), 60);
    assert_eq!(nearest_main_boundary(&project, main, 100), 120);
    assert_eq!(nearest_main_boundary(&project, main, 10_000), 120);
}

#[test]
fn host_lane_prefers_free_existing_lane_of_kind() {
    let (mut project, _main) = project_with_main();
    let text = project.add_track(TrackKind::Text, "T1");
    project
        .add_generated(
            text,
            Generator::text("busy"),
            TimeRange::at_rate(0, 90, FPS),
        )
        .unwrap();

    // Overlapping span → a new lane below the overlay block.
    let plan = host_lane_plan(
        &project,
        TrackKind::Text,
        TimeRange::at_rate(30, 30, FPS),
        None,
    )
    .unwrap();
    let text_pos = project.timeline().order().iter().position(|t| *t == text);
    assert_eq!(plan, HostLanePlan::Create { index: text_pos });

    // A free span reuses the lane.
    let plan = host_lane_plan(
        &project,
        TrackKind::Text,
        TimeRange::at_rate(90, 30, FPS),
        None,
    )
    .unwrap();
    assert_eq!(plan, HostLanePlan::Existing(text));
}

#[test]
fn new_video_lane_goes_directly_above_main() {
    let (project, main) = project_with_main();
    let plan = host_lane_plan(
        &project,
        TrackKind::Video,
        TimeRange::at_rate(0, 30, FPS),
        None,
    )
    .unwrap();
    let main_pos = project
        .timeline()
        .order()
        .iter()
        .position(|t| *t == main)
        .unwrap();
    assert_eq!(
        plan,
        HostLanePlan::Create {
            index: Some(main_pos + 1)
        }
    );
}

#[test]
fn new_audio_lane_appends_and_sinks_to_the_floor() {
    let (project, _main) = project_with_main();
    let plan = host_lane_plan(
        &project,
        TrackKind::Audio,
        TimeRange::at_rate(0, 30, FPS),
        None,
    )
    .unwrap();
    assert_eq!(plan, HostLanePlan::Create { index: None });
}

#[test]
fn intent_wire_format_roundtrips() {
    let json = r#"{"intent":"add_text","text":"Hi","at_seconds":1.5}"#;
    let intent: Intent = serde_json::from_str(json).unwrap();
    match &intent {
        Intent::AddText {
            text,
            at_seconds,
            duration_seconds,
        } => {
            assert_eq!(text, "Hi");
            assert_eq!(*at_seconds, 1.5);
            assert_eq!(*duration_seconds, 3.0, "duration defaults to 3s");
        }
        other => panic!("unexpected intent {other:?}"),
    }

    let json = r#"{"intent":"ripple_trim_main","clip":7,"edge":"leading","delta_seconds":-0.5}"#;
    let intent: Intent = serde_json::from_str(json).unwrap();
    assert!(matches!(
        intent,
        Intent::RippleTrimMain {
            edge: TrimEdge::Leading,
            ..
        }
    ));

    let json = r#"{"intent":"move_lane","clip":3,"start_seconds":2.0}"#;
    let intent: Intent = serde_json::from_str(json).unwrap();
    assert!(matches!(intent, Intent::MoveLane { track: None, .. }));
}
