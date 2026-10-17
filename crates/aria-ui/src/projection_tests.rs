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

/// A clip with no label of its own is drawn in the label `docs/spec/look.md`
/// gives its kind: Violet picture, Forest sound, Mango generators, Tan nested
/// sequences, Lavender adjustments.
///
/// The class this closes: a clip kind drawn in a colour the look chapter does
/// not give it, such as every unlabelled clip falling back to one hue. Not
/// caught: the hue values themselves, which `theme_tests` pins.
#[test]
fn an_unlabelled_clip_takes_the_label_of_its_kind() {
    use aria_models::{Generator, LabelColor, Sequence};

    let (mut project, picture, _, sound) = sample_project();
    let span = TimeRange::at_rate(0, 24, Rational::FPS_24);
    let solid = Generator::SolidColor { rgba: [0; 4] };
    let graphics = project.add_track(solid.spec().lane, "G1");
    let generated = project.add_generated(graphics, solid, span).unwrap();
    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let adjustment = project
        .add_generated(fx, Generator::Adjustment, span)
        .unwrap();
    let inner = project.add_sequence(Sequence::new("Inner", Rational::FPS_24));
    let nests = project.add_track(TrackKind::Video, "V2");
    let nested = project
        .add_sequence_clip(nests, inner, span, RationalTime::new(0, Rational::FPS_24))
        .unwrap();

    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    let drawn = |id: ClipId| {
        let clip = store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.id == id.raw())
            .expect("the clip is projected");
        assert_eq!(clip.label, None, "{id:?} carries no label of its own");
        clip.drawn_label()
    };
    assert_eq!(drawn(picture), LabelColor::Violet);
    assert_eq!(drawn(sound), LabelColor::Forest);
    assert_eq!(drawn(generated), LabelColor::Mango);
    assert_eq!(drawn(adjustment), LabelColor::Lavender);
    assert_eq!(drawn(nested), LabelColor::Tan);

    project
        .set_clip_label(sound, Some(LabelColor::Rose))
        .unwrap();
    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    let relabelled = store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.id == sound.raw())
        .unwrap();
    assert_eq!(
        relabelled.drawn_label(),
        LabelColor::Rose,
        "the operator's label wins"
    );
}

/// The badge text a clip projects while a plugin behind it is gone. Closes the
/// class "a clip whose plugin is missing, failed, or disabled draws no badge,
/// or a badge for a part that still has its plugin": every part kind the
/// badge reads (clip type, effect, audio effect, transition) and the order
/// between them. Uses only ids no catalog lists, so nothing registers into the
/// process-global catalogs. Does not cover the paint; the timeline census does.
#[test]
fn a_clip_projects_the_first_part_whose_plugin_is_missing() {
    use aria_models::{AudioEffectInstance, EffectInstance, Generator};

    let rate = Rational::FPS_24;
    let mut project = Project::new("Demo", rate);
    let lane = project.add_track(TrackKind::Sticker, "S1");
    let solid = |project: &mut Project, start| {
        project
            .add_generated(
                lane,
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                TimeRange::at_rate(start, 24, rate),
            )
            .unwrap()
    };
    let left = solid(&mut project, 0);
    let right = solid(&mut project, 24);
    project.add_transition(left, "crossfade").unwrap();
    let badges = |project: &Project| -> Vec<Option<String>> {
        let store = project_to_ui(project, RationalTime::new(0, rate), &[]);
        [left, right]
            .iter()
            .map(|id| {
                store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .find(|clip| clip.id == id.raw())
                    .unwrap()
                    .missing_plugin
                    .as_ref()
                    .map(ToString::to_string)
            })
            .collect()
    };
    assert_eq!(badges(&project), [None, None], "every part has its plugin");

    // An audio lane's crossfades are built in, not plugins.
    for audio in aria_models::AudioTransition::ALL {
        let mut doc = serde_json::to_value(&project).unwrap();
        doc["timeline"]["tracks"][0][1]["transitions"][0]["transition_id"] =
            serde_json::json!(audio.id());
        let built_in: Project = serde_json::from_value(doc).unwrap();
        assert_eq!(
            badges(&built_in),
            [None, None],
            "{} is built in",
            audio.id()
        );
    }

    // A transition id saved on another machine, loaded where no plugin lists it.
    let mut doc = serde_json::to_value(&project).unwrap();
    doc["timeline"]["tracks"][0][1]["transitions"][0]["transition_id"] =
        serde_json::json!("com.example.gone.wipe");
    let mut project: Project = serde_json::from_value(doc).unwrap();
    let missing = |id: &str| Some(format!("Missing plugin: {id}"));
    assert_eq!(
        badges(&project),
        [missing("com.example.gone.wipe"), None],
        "the transition belongs to the clip on its left"
    );

    let clip = project.timeline_mut().clip_mut(left).unwrap();
    clip.audio_effects = vec![AudioEffectInstance::new("com.example.gone.afx")];
    assert_eq!(badges(&project)[0], missing("com.example.gone.afx"));
    let clip = project.timeline_mut().clip_mut(left).unwrap();
    clip.effects = vec![EffectInstance::new("com.example.gone.fx")];
    assert_eq!(badges(&project)[0], missing("com.example.gone.fx"));

    project.timeline_mut().clip_mut(right).unwrap().clip_type =
        Some("com.example.gone.type".into());
    assert_eq!(badges(&project)[1], missing("com.example.gone.type"));
}
