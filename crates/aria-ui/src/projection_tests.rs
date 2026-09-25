//! Tests for [`super`].
//!
//! Moved out of `projection.rs` to stay under the file ceiling. The
//! module is unchanged; only its location moved.
use aria_models::{
    ClipId, ClipParam, MediaSource, ParamValue, Project, Rational, RationalTime, TimeRange,
    TrackKind,
};

use super::project_to_ui;
use crate::types::UiTrackKind;

fn sample_project() -> (Project, ClipId, ClipId, ClipId) {
    let mut project = Project::new("Demo", Rational::FPS_24);
    let video = project.add_track(TrackKind::Video, "V1");
    let audio = project.add_track(TrackKind::Audio, "A1");
    let media = project.add_media(MediaSource::new(
        "clip.mp4",
        1920,
        1080,
        Rational::FPS_24,
        240,
        true,
    ));
    let source = TimeRange::at_rate(0, 48, Rational::FPS_24);
    let start = RationalTime::new(0, Rational::FPS_24);
    let later = RationalTime::new(48, Rational::FPS_24);
    let a = project.add_clip(video, media, source, start).unwrap();
    let b = project.add_clip(video, media, source, later).unwrap();
    let c = project.add_clip(audio, media, source, start).unwrap();
    (project, a, b, c)
}

#[test]
fn project_to_ui_preserves_track_and_clip_counts() {
    let (project, _, _, _) = sample_project();
    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    assert_eq!(store.sequence.tracks.len(), 2);
    let clip_count: usize = store.sequence.tracks.iter().map(|t| t.clips.len()).sum();
    assert_eq!(clip_count, 3);
    assert_eq!(store.project_name, "Demo");
    assert_eq!(store.sequence.duration_ticks, 96);
    assert_eq!(store.media.len(), 1);
    assert_eq!(store.media[0].width, 1920);
    assert_eq!(store.sequence.tracks[0].enabled, true);
    let first_clip = &store
        .sequence
        .tracks
        .iter()
        .find(|t| t.kind == UiTrackKind::Video)
        .unwrap()
        .clips[0];
    assert_eq!(first_clip.timeline_start_ticks, 0);
    assert_eq!(first_clip.timeline_end_ticks, 48);
    assert_eq!(first_clip.duration_label, "00:00:02:00");
}

#[test]
fn project_to_ui_marks_selected_clip() {
    let (project, first, _, _) = sample_project();
    let store = project_to_ui(&project, RationalTime::new(12, Rational::FPS_24), &[first]);
    assert_eq!(store.selection, vec![first.raw()]);
    let selected: Vec<_> = store
        .sequence
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .filter(|c| c.selected)
        .map(|c| c.id)
        .collect();
    assert_eq!(selected, vec![first.raw()]);
    assert_eq!(store.playhead_ticks, 12);
}

#[test]
fn project_to_ui_flags_missing_media() {
    let (project, _, _, _) = sample_project();
    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    assert!(store.media[0].missing);
    assert_eq!(store.media[0].path, "clip.mp4");
}

#[test]
fn repeated_projection_reuses_missing_media_status() {
    let path = crate::scratch_pool::scratch("projection-media-status").join("cached.mp4");
    let _ = std::fs::remove_file(&path);
    let mut project = Project::new("Status", Rational::FPS_24);
    project.add_media(MediaSource::new(
        path.clone(),
        1920,
        1080,
        Rational::FPS_24,
        24,
        false,
    ));

    let first = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    assert!(first.media[0].missing);

    std::fs::write(&path, b"arrived").expect("create media after the cached probe");
    let second = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    assert!(
        second.media[0].missing,
        "a repeated projection bypassed the cached filesystem status"
    );
    std::fs::remove_file(path).expect("remove status fixture");
}

#[test]
fn project_to_ui_projects_clip_audio() {
    let (mut project, first, _, _) = sample_project();
    project
        .set_clip_audio(
            first,
            Some(0.5),
            RationalTime::new(6, Rational::FPS_24),
            RationalTime::new(12, Rational::FPS_24),
        )
        .unwrap();
    project
        .set_param_constant(first, ClipParam::Pan, ParamValue::Scalar(-0.25))
        .unwrap();
    project
        .set_clip_speed(first, Rational::new(1, 1), true)
        .unwrap();
    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[first]);
    let clip = store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.id == first.raw())
        .expect("selected clip");
    assert!((clip.volume - 0.5).abs() < 1e-6);
    assert!((clip.pan + 0.25).abs() < 1e-6);
    assert_eq!(clip.fade_in_ticks, 6);
    assert_eq!(clip.fade_out_ticks, 12);
    assert!(clip.reversed);
    assert_eq!(clip.speed_num, 1);
    assert_eq!(clip.speed_den, 1);
}
