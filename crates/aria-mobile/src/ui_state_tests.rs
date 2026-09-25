//! Tests for [`ui_state`].
//!
//! Declared from `ui_state.rs` with `#[path]`, so `use super::*` still means
//! that module and nothing about these cases changed in the move.

use super::*;
use aria_models::{
    ClipParam, ClipTransform, Easing, MediaSource, ParamValue, RationalTime, TimeRange,
};

const FPS: Rational = Rational::FPS_30;

fn tr(start: i64, len: i64) -> TimeRange {
    TimeRange::at_rate(start, len, FPS)
}

/// A project with the full lane menagerie:
/// video overlay (v2) over main (v1), text + effect lanes, two audio lanes.
fn full_project() -> Project {
    let mut project = Project::new("demo", FPS);
    let main = project.add_track(TrackKind::Video, "Main");
    let media = project.add_media(MediaSource::new(
        "/tmp/beach.mp4",
        1920,
        1080,
        FPS,
        300,
        true,
    ));
    project
        .add_clip(
            main,
            media,
            TimeRange::at_rate(30, 90, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap();

    let overlay = project.add_track(TrackKind::Video, "V2");
    project
        .add_clip(
            overlay,
            media,
            TimeRange::at_rate(0, 60, FPS),
            RationalTime::new(15, FPS),
        )
        .unwrap();

    let text = project.add_track(TrackKind::Text, "T1");
    project
        .add_generated(text, Generator::text("Hello"), tr(10, 45))
        .unwrap();

    let effect = project.add_track(TrackKind::Effect, "FX");
    project
        .add_generated(effect, Generator::Effect, tr(0, 30))
        .unwrap();

    let audio1 = project.add_track(TrackKind::Audio, "A1");
    let audio2 = project.add_track(TrackKind::Audio, "A2");
    project
        .add_clip(
            audio1,
            media,
            TimeRange::at_rate(0, 120, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap();
    let _ = audio2;
    project
}

#[test]
fn lane_rows_follow_the_ui_convention() {
    let project = full_project();
    let rows = lane_rows(&project);
    let timeline = project.timeline();
    let kinds: Vec<&str> = rows
        .iter()
        .map(|id| timeline.track(*id).unwrap().kind.id())
        .collect();
    // Overlay video on top, then main, then text+effect (text lanes
    // always occupy the top of the compositing stack, so reversed they
    // lead the overlay block), then audio floor.
    assert_eq!(
        kinds,
        vec!["video", "video", "text", "effect", "audio", "audio"]
    );

    let main = main_track(&project).unwrap();
    assert_eq!(rows[1], main, "second row is the main track");

    // Audio rows are in stack order (first-added on top).
    let names: Vec<&str> = rows
        .iter()
        .map(|id| timeline.track(*id).unwrap().name.as_str())
        .collect();
    assert_eq!(names[4], "A1");
    assert_eq!(names[5], "A2");
}

#[test]
fn build_reports_durations_canvas_and_flags() {
    let project = full_project();
    let state = UiState::build(
        &project,
        SessionMeta {
            revision: 9,
            dirty: true,
            can_undo: true,
            can_redo: false,
        },
    );

    assert_eq!(state.revision, 9);
    assert!(state.dirty && state.can_undo && !state.can_redo);
    assert_eq!(state.name, "demo");
    assert_eq!(state.fps, FPS);
    // Main clip runs 0..3s; audio runs 0..4s → timeline 4s.
    assert!((state.main_duration_seconds - 3.0).abs() < 1e-9);
    assert!((state.duration_seconds - 4.0).abs() < 1e-9);
    // Auto canvas follows the 1920×1080 footage.
    assert_eq!((state.canvas.width, state.canvas.height), (1920, 1080));

    let main_lane = state.lanes.iter().find(|l| l.is_main).unwrap();
    assert_eq!(main_lane.clips.len(), 1);
    let clip = &main_lane.clips[0];
    assert_eq!(clip.kind, "video");
    assert_eq!(clip.label, "beach");
    assert!((clip.trim_in_seconds.unwrap() - 1.0).abs() < 1e-9);
    assert!((clip.source_duration_seconds.unwrap() - 10.0).abs() < 1e-9);
    assert!(clip.has_audio);
    assert_eq!(clip.speed, 1.0);
    assert_eq!(clip.volume, 1.0);
    let t = clip.transform.as_ref().unwrap();
    assert_eq!((t.pos_x, t.pos_y, t.scale, t.opacity), (0.5, 0.5, 1.0, 1.0));
}

#[test]
fn build_surfaces_text_content_and_style() {
    let project = full_project();
    let state = UiState::build(&project, SessionMeta::default());
    let text_lane = state.lanes.iter().find(|l| l.kind == "text").unwrap();
    let clip = &text_lane.clips[0];
    assert_eq!(clip.kind, "text");
    assert_eq!(clip.text.as_deref(), Some("Hello"));
    assert_eq!(clip.label, "Hello");
    assert!(clip.text_style.is_some());
    assert!((clip.start_seconds - 10.0 / 30.0).abs() < 1e-9);
    assert!((clip.length_seconds - 1.5).abs() < 1e-9);
}

#[test]
fn build_surfaces_transform_keyframes_in_seconds() {
    let mut project = Project::new("kf", FPS);
    let sticker = project.add_track(TrackKind::Sticker, "S");
    let id = project
        .add_generated(
            sticker,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 90),
        )
        .unwrap();
    project
        .set_param_keyframe(
            id,
            ClipParam::Scale,
            RationalTime::new(30, FPS),
            ParamValue::Scalar(2.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            id,
            ClipParam::Opacity,
            RationalTime::new(60, FPS),
            ParamValue::Scalar(0.5),
            Easing::Linear,
            None,
        )
        .unwrap();

    let state = UiState::build(&project, SessionMeta::default());
    let clip = &state.lanes[0].clips[0];
    assert_eq!(clip.keyframe_seconds, vec![1.0, 2.0]);
    assert_eq!(clip.rgba, Some([255, 0, 0, 255]));
    assert_eq!(clip.kind, "solid");
}

#[test]
fn empty_project_has_no_lanes_and_zero_duration() {
    let project = Project::new("empty", FPS);
    let state = UiState::build(&project, SessionMeta::default());
    assert!(state.lanes.is_empty());
    assert_eq!(state.duration_seconds, 0.0);
    assert_eq!(state.main_duration_seconds, 0.0);
    assert!(main_track(&project).is_none());
    // The JSON tree is parseable and carries the envelope fields.
    let json: serde_json::Value = serde_json::from_str(&state.to_json()).unwrap();
    assert_eq!(json["name"], "empty");
    assert_eq!(json["fps"]["num"], 30);
}

#[test]
fn transform_position_converts_to_ui_space() {
    let mut project = Project::new("pos", FPS);
    let sticker = project.add_track(TrackKind::Sticker, "S");
    let id = project
        .add_generated(
            sticker,
            Generator::SolidColor {
                rgba: [1, 2, 3, 255],
            },
            tr(0, 30),
        )
        .unwrap();
    project
        .set_transform(
            id,
            ClipTransform {
                position: [0.25, -0.10],
                anchor_point: [0.5, 0.5],
                scale: 0.5.into(),
                rotation: 90.0,
                opacity: 0.8,
            },
            None,
        )
        .unwrap();

    let state = UiState::build(&project, SessionMeta::default());
    let t = state.lanes[0].clips[0].transform.as_ref().unwrap();
    assert!((t.pos_x - 0.75).abs() < 1e-6);
    assert!((t.pos_y - 0.40).abs() < 1e-6);
    assert_eq!(t.scale, 0.5);
    assert_eq!(t.rotation_degrees, 90.0);
    assert!((t.opacity - 0.8).abs() < 1e-6);
}
