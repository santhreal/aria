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
    assert!(store.sequence.tracks[0].enabled);
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
    // Each clip placing the gone file reads its name and its offline state
    // from the pool entry.
    let clips: Vec<_> = store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .collect();
    assert_eq!(clips.len(), 3);
    for clip in clips {
        assert_eq!(clip.name.as_ref(), "clip");
        assert_eq!(
            clip.offline.as_ref().map(|name| name.as_ref()),
            Some("clip.mp4")
        );
    }
}

/// A clip placing a present file is not drawn offline, and each clip placing a
/// gone one is, so the offline state follows each clip's own pool entry rather
/// than one answer for the whole pool.
#[test]
fn project_to_ui_draws_offline_only_where_the_file_is_gone() {
    let path = crate::scratch_pool::scratch("projection-offline").join("present.mp4");
    std::fs::write(&path, b"present").expect("create the present media");
    let (mut project, a, b, c) = sample_project();
    let rate = Rational::FPS_24;
    let lane = project.add_track(TrackKind::Video, "V2");
    let media = project.add_media(MediaSource::new(path.clone(), 1920, 1080, rate, 240, true));
    let here = project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 48, rate),
            RationalTime::new(0, rate),
        )
        .expect("a clip of the present file");
    let store = project_to_ui(&project, RationalTime::new(0, rate), &[]);
    let drawn: std::collections::HashMap<u64, Option<&str>> = store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| (clip.id, clip.offline.as_ref().map(|name| name.as_ref())))
        .collect();
    let gone = Some("clip.mp4");
    assert_eq!(
        drawn,
        std::collections::HashMap::from([
            (a.raw(), gone),
            (b.raw(), gone),
            (c.raw(), gone),
            (here.raw(), None),
        ])
    );
    std::fs::remove_file(path).expect("remove the present media");
}

/// Each linked clip draws the frames it sits from its partner, as the model
/// measures them, and a linked pair placing the file at the same tick draws
/// none. The fixture links the two clips that start at 0 and a slipped pair:
/// the video clip at frame 48 and an audio clip of the same file at frame 60.
#[test]
fn project_to_ui_draws_the_drift_of_each_linked_clip() {
    let (mut project, a, b, c) = sample_project();
    let rate = Rational::FPS_24;
    let media = project
        .timeline()
        .clip(a)
        .and_then(|clip| clip.media())
        .expect("the sample clips place one file");
    let lane = project.add_track(TrackKind::Audio, "A2");
    let late = project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 48, rate),
            RationalTime::new(60, rate),
        )
        .expect("an audio clip of the same file");
    project.link_clips(&[a, c]).expect("link the aligned pair");
    project
        .link_clips(&[b, late])
        .expect("link the slipped pair");
    let store = project_to_ui(&project, RationalTime::new(0, rate), &[]);
    let drawn: std::collections::HashMap<u64, Option<i64>> = store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| (clip.id, clip.drift))
        .collect();
    assert_eq!(
        drawn,
        std::collections::HashMap::from([
            (a.raw(), None),
            (c.raw(), None),
            (b.raw(), Some(-12)),
            (late.raw(), Some(12)),
        ])
    );
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
/// gives its kind: Teal picture, Forest sound, Mango generators, Tan nested
/// sequences, Yellow adjustments.
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
    assert_eq!(drawn(picture), LabelColor::Teal);
    assert_eq!(drawn(sound), LabelColor::Forest);
    assert_eq!(drawn(generated), LabelColor::Mango);
    assert_eq!(drawn(adjustment), LabelColor::Yellow);
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
        "the set label wins"
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

/// WHY: the keyframe strip on a clip bar draws at clip-local ticks. A clip that
/// does not start at zero shows whether the union is offset by the clip start.
/// Two properties whose keys interleave (Opacity at 6 and 30, Position at 6
/// and 18) leave the union unsorted and duplicated whichever is visited first,
/// so it shows the union is sorted and deduped.
/// Does not cover a sequence-timed clip (`ParamTime::Sequence`).
#[test]
fn the_strip_lists_each_keyed_tick_once_relative_to_the_clip_start() {
    let (mut project, _, later, _) = sample_project();
    let at = |tick: i64| RationalTime::new(48 + tick, Rational::FPS_24);
    let keys = [
        (ClipParam::Opacity, 30, ParamValue::Scalar(0.5)),
        (ClipParam::Opacity, 6, ParamValue::Scalar(1.0)),
        (ClipParam::Position, 18, ParamValue::Vec2([0.3, 0.4])),
        (ClipParam::Position, 6, ParamValue::Vec2([0.1, 0.2])),
    ];
    for (param, tick, value) in keys {
        project
            .set_param_keyframe(
                later,
                param,
                at(tick),
                value,
                aria_models::Easing::Linear,
                None,
            )
            .unwrap();
    }
    let store = project_to_ui(&project, RationalTime::new(0, Rational::FPS_24), &[]);
    let clip = store
        .sequence
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == later.raw())
        .unwrap();
    assert_eq!(clip.timeline_start_ticks, 48);
    assert_eq!(clip.keyframe_ticks, vec![6, 18, 30]);
}

/// A project file can list sequence markers in any order, and the ruler stops
/// at the first marker past the viewport, so the projection sorts them.
///
/// WHY: the sort ran only when a clip held markers; a file with out-of-order
/// sequence markers and no clip markers projected them unsorted. What this
/// does not catch: a consumer that re-sorts or filters `markers` itself.
#[test]
fn project_to_ui_sorts_sequence_markers_a_file_lists_out_of_order() {
    let (mut project, _, _, _) = sample_project();
    for (tick, name) in [(40, "late"), (8, "early")] {
        project
            .timeline_mut()
            .add_marker(aria_models::Marker::new(
                RationalTime::new(tick, Rational::FPS_24),
                name,
                aria_models::MarkerColor::Teal,
            ))
            .unwrap();
    }
    let mut doc: serde_json::Value =
        serde_json::from_str(&project.save_to_string().unwrap()).unwrap();
    let listed = doc["timeline"]["markers"].as_array_mut().unwrap();
    assert_eq!(listed.len(), 2);
    listed.reverse();
    let reloaded = Project::load_from_string(&doc.to_string()).unwrap();
    assert_eq!(reloaded.timeline().markers()[0].tick.value, 40);

    let store = project_to_ui(&reloaded, RationalTime::new(0, Rational::FPS_24), &[]);
    let ticks: Vec<i64> = store.sequence.markers.iter().map(|m| m.tick).collect();
    assert_eq!(ticks, vec![8, 40]);
}

/// Every projected clip carries the capability flags the model derives for
/// that clip alone, while the projection reads the link set once for all of
/// them. Closes the class where the projection reads that set wrong or not at
/// all: a video clip whose sound was separated to an audio lane offers extract
/// audio, or one whose sound was not loses it. The fixture links a video clip
/// to an audio-lane partner and two video clips to each other, and keeps one
/// video clip unlinked; unlinking the separated clip gives its sound back. A
/// solid on a sticker lane, which carries no sound, holds no insert chain
/// while every media clip holds one.
#[test]
fn every_projected_clip_carries_the_capabilities_the_model_derives() {
    use aria_models::{AudioInsertTarget, ClipAction, ClipCapabilities, Generator, TrackId};
    let rate = Rational::FPS_24;
    let mut project = Project::new("Caps", rate);
    let media = project.add_media(MediaSource::new("take.mp4", 1920, 1080, rate, 240, true));
    let source = TimeRange::at_rate(0, 48, rate);
    let start = RationalTime::new(0, rate);
    let place = |project: &mut Project, kind: TrackKind, name: &str| {
        let lane = project.add_track(kind, name);
        project
            .add_clip(lane, media, source, start)
            .expect("a clip on an empty lane")
    };
    let picture = place(&mut project, TrackKind::Video, "V1");
    let sound = place(&mut project, TrackKind::Audio, "A1");
    let left = place(&mut project, TrackKind::Video, "V2");
    let right = place(&mut project, TrackKind::Video, "V3");
    let alone = place(&mut project, TrackKind::Video, "V4");
    project
        .link_clips(&[picture, sound])
        .expect("link picture and sound");
    project
        .link_clips(&[left, right])
        .expect("link two pictures");
    let stickers = project.add_track(TrackKind::Sticker, "S1");
    let solid = project
        .add_generated(
            stickers,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            source,
        )
        .expect("a solid on an empty lane");

    let extractable = |project: &Project| -> Vec<u64> {
        let store = project_to_ui(project, start, &[]);
        let timeline = project.timeline();
        let mut ids = Vec::new();
        for lane in &store.sequence.tracks {
            let track = timeline
                .track(TrackId::from_raw(lane.id))
                .expect("a projected lane");
            for ui in lane.clips.iter() {
                let clip = timeline
                    .clip(ClipId::from_raw(ui.id))
                    .expect("a projected clip");
                let caps = ClipCapabilities::for_clip(project, clip, track.kind);
                let drawn = [
                    ui.can_split,
                    ui.can_reverse,
                    ui.can_extract_audio,
                    ui.can_ripple_delete,
                    ui.can_freeze_frame,
                    ui.can_stabilize,
                ];
                let derived = [
                    ClipAction::Split,
                    ClipAction::Reverse,
                    ClipAction::ExtractAudio,
                    ClipAction::RippleDelete,
                    ClipAction::FreezeFrame,
                    ClipAction::Stabilize,
                ]
                .map(|action| caps.allows(action));
                assert_eq!(drawn, derived, "{} on {}", ui.name, lane.name);
                assert_eq!(
                    ui.can_hold_inserts,
                    project
                        .insert_target_refusal(AudioInsertTarget::Clip(clip.id))
                        .is_ok(),
                    "{} on {} holds inserts",
                    ui.name,
                    lane.name
                );
                assert_eq!(ui.can_hold_inserts, clip.id != solid, "{}", ui.name);
                if ui.can_extract_audio {
                    ids.push(ui.id);
                }
            }
        }
        ids.sort_unstable();
        ids
    };
    let mut want = [left, right, alone].map(ClipId::raw);
    want.sort_unstable();
    assert_eq!(
        extractable(&project),
        want,
        "the separated picture does not extract its sound"
    );
    project
        .unlink_clips(&[picture])
        .expect("unlink the picture");
    let mut want = [picture, left, right, alone].map(ClipId::raw);
    want.sort_unstable();
    assert_eq!(
        extractable(&project),
        want,
        "an unlinked picture extracts its sound again"
    );
}
