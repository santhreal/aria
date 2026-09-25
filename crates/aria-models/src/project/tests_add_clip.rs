//! `add_clip` cases.

use super::*;
use crate::clip::Shape;

#[test]
fn bins_survive_a_save_and_load() {
    let mut project = Project::new("bins", R24);
    let top = project.add_bin("Footage", None).unwrap();
    let inner = project.add_bin("Day 1", Some(top)).unwrap();
    let media = project.add_media(sample_media(R24, 100));
    project.move_media_to_bin(media, Some(inner)).unwrap();
    let json = serde_json::to_string(&project).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bin_count(), 2);
    assert_eq!(back.bin(inner).unwrap().parent, Some(top));
    assert_eq!(back.media(media).unwrap().bin, Some(inner));

    // A pool with no bins says nothing about them on disk, so an older file
    // loads unchanged.
    let flat = Project::new("flat", R24);
    let json = serde_json::to_string(&flat).unwrap();
    assert!(!json.contains("bins"), "an empty pool writes no bins key");
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bin_count(), 0);
}

#[test]
fn is_media_referenced_reflects_clip_usage() {
    let (mut project, media_id, track) = project_with_media(100);
    assert!(!project.is_media_referenced(media_id));

    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    assert!(project.is_media_referenced(media_id));

    project.remove_clip(clip);
    assert!(!project.is_media_referenced(media_id));
}

#[test]
fn remove_media_succeeds_when_unreferenced() {
    let mut project = Project::new("test", R24);
    let id = project.add_media(sample_media(R24, 10));
    let removed = project.remove_media(id).unwrap();
    assert_eq!(removed.duration.value, 10);
    assert_eq!(project.media_count(), 0);
}

// --- add_clip ---------------------------------------------------------

#[test]
fn add_clip_places_media_with_rate_conform() {
    let mut project = Project::new("test", R24);
    let media_id = project.add_media(sample_media(R30, 1000));
    let track = project.add_track(TrackKind::Video, "V1");

    let clip = project
        .add_clip(track, media_id, tr_at(0, 120, R30), rt(10))
        .unwrap();

    let placed = project.clip(clip).unwrap();
    assert_eq!(placed.start().value, 10);
    assert_eq!(placed.timeline.duration.value, 96);
    assert_eq!(placed.source_range(), Some(tr_at(0, 120, R30)));
}

#[test]
fn add_clip_rejects_unknown_track_and_media() {
    let (mut project, media_id, _) = project_with_media(100);
    let missing_track = TrackId::next();
    let missing_media = MediaId::next();

    assert_eq!(
        project.add_clip(missing_track, media_id, tr(0, 10), rt(0)),
        Err(ModelError::UnknownTrack(missing_track))
    );
    let track = project.add_track(TrackKind::Video, "V2");
    assert_eq!(
        project.add_clip(track, missing_media, tr(0, 10), rt(0)),
        Err(ModelError::UnknownMedia(missing_media))
    );
}

#[test]
fn add_clip_rejects_empty_source_and_out_of_bounds() {
    let (mut project, media_id, track) = project_with_media(100);

    assert_eq!(
        project.add_clip(track, media_id, tr(0, 0), rt(0)),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(
        project.add_clip(track, media_id, tr(-1, 10), rt(0)),
        Err(ModelError::SourceOutOfBounds)
    );
    assert_eq!(
        project.add_clip(track, media_id, tr(95, 10), rt(0)),
        Err(ModelError::SourceOutOfBounds)
    );
}

#[test]
fn add_clip_rejects_rate_mismatches() {
    let (mut project, media_id, track) = project_with_media(100);

    assert_eq!(
        project.add_clip(track, media_id, tr_at(0, 10, R30), rt(0)),
        Err(ModelError::RateMismatch {
            expected: R24,
            got: R30,
        })
    );
    let bad_start = RationalTime::new(0, R30);
    assert_eq!(
        project.add_clip(track, media_id, tr(0, 10), bad_start),
        Err(ModelError::RateMismatch {
            expected: R24,
            got: R30,
        })
    );
}

#[test]
fn add_clip_rejects_overlap() {
    let (mut project, media_id, track) = project_with_media(1000);
    project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    assert_eq!(
        project.add_clip(track, media_id, tr(0, 50), rt(50)),
        Err(ModelError::Overlap(track))
    );
}

// --- add_generated ----------------------------------------------------

#[test]
fn add_generated_without_media() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Text, "Titles");
    let clip = project
        .add_generated(track, Generator::text("Hi"), tr(0, 24))
        .unwrap();

    assert!(project.clip(clip).unwrap().is_generated());
    assert_eq!(project.media_count(), 0);
}

#[test]
fn add_generated_rejects_empty_and_wrong_rate() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Video, "V1");

    assert_eq!(
        project.add_generated(track, Generator::Adjustment, tr(0, 0)),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(
        project.add_generated(
            track,
            Generator::shape(Shape::Rectangle, [255, 255, 255, 255]),
            tr_at(0, 10, R30),
        ),
        Err(ModelError::RateMismatch {
            expected: R24,
            got: R30,
        })
    );
}

// --- duplicate_clip ---------------------------------------------------

#[test]
fn duplicate_clip_preserves_full_property_snapshot() {
    let (mut project, media_id, source_track) = project_with_media(1_000);
    let destination_track = project.add_track(TrackKind::Video, "V2");
    let source_id = project
        .add_clip(source_track, media_id, tr(17, 80), rt(10))
        .unwrap();
    let source_link = crate::LinkId::next();

    {
        let clip = project.timeline_mut().clip_mut(source_id).unwrap();
        clip.link = Some(source_link);

        clip.transform.position.set_keyframe(
            0,
            [0.1, -0.2],
            Easing::Bezier {
                points: [0.2, 0.8, 0.7, 0.1],
            },
        );
        clip.transform
            .position
            .set_keyframe(60, [0.7, 0.4], Easing::EaseOut);
        clip.transform.anchor_point = Param::Constant([0.25, 0.75]);
        clip.transform
            .scale
            .set_keyframe(0, 0.8.into(), Easing::EaseIn);
        clip.transform
            .scale
            .set_keyframe(60, 1.6.into(), Easing::EaseInOut);
        clip.transform.rotation = Param::Constant(27.5);
        clip.transform.opacity = Param::Constant(0.65);

        clip.speed = Rational::new(3, 2);
        clip.reversed = true;
        clip.speed_curve.set_keyframe(0, 0.75, Easing::EaseIn);
        clip.speed_curve
            .set_keyframe(crate::SPEED_CURVE_SCALE, 1.5, Easing::EaseOut);
        clip.preserve_pitch = false;

        clip.volume.set_keyframe(0, 0.2, Easing::EaseIn);
        clip.volume.set_keyframe(60, 1.4, Easing::EaseOut);
        clip.fade_in = 9;
        clip.fade_out = 11;
        clip.denoise = true;

        clip.crop = CropRect {
            x: 0.1,
            y: 0.2,
            w: 0.7,
            h: 0.6,
        }
        .into();
        clip.flip_h = true;
        clip.flip_v = true;

        let mut effect = EffectInstance::new("gaussian_blur");
        effect
            .set_param_keyframe(0, 0, 2.0, Easing::EaseIn)
            .unwrap();
        effect
            .set_param_keyframe(0, 60, 12.0, Easing::EaseOut)
            .unwrap();
        clip.effects = vec![effect];
        clip.beats = vec![19, 31, 47];

        clip.mask = Some(Mask {
            kind: MaskKind::Heart,
            feather: 0.35.into(),
            invert: true,
            ..Mask::new(MaskKind::Heart)
        });
        clip.chroma_key = Some(ChromaKey {
            rgb: [3, 240, 17],
            strength: 0.72.into(),
            shadow: 0.18.into(),
        });
        clip.stabilize = Some(StabilizeLevel::MaxSmooth);
        clip.filter = Some(Filter {
            id: "vivid".into(),
            intensity: 0.63.into(),
        });
        clip.lut = Some(crate::look::Lut {
            path: "/tmp/cinematic.cube".into(),
            intensity: 0.71.into(),
        });
        clip.adjust = ColorAdjustments {
            brightness: 0.1.into(),
            contrast: (-0.2).into(),
            saturation: 0.3.into(),
            exposure: (-0.4).into(),
            temperature: 0.5.into(),
            ..Default::default()
        };
        clip.animation_in = Some(AnimationRef::new("zoom_in"));
        clip.animation_out = Some(AnimationRef::new("drop"));
        clip.animation_combo = Some(AnimationRef::new("pulse"));
        clip.audio_role = Some(AudioRole::Voiceover);
        clip.replaceable = Some(
            Replaceable::new(7)
                .with_accepts(SlotMedia::VideoOnly)
                .with_label("Hero"),
        );
        clip.text_editable = true;
    }

    let source_before = project.clip(source_id).unwrap().clone();
    let destination_start = rt(240);
    let duplicate_id = project
        .duplicate_clip(source_id, destination_track, destination_start)
        .unwrap();
    let duplicate = project.clip(duplicate_id).unwrap();

    let mut expected = source_before.clone();
    expected.id = duplicate_id;
    expected.timeline.start = destination_start;
    expected.link = None;
    assert_eq!(
        duplicate, &expected,
        "normalizing only id/start/link makes the full snapshots equal"
    );
    assert_eq!(
        project.clip(source_id).unwrap(),
        &source_before,
        "duplication never mutates the source"
    );
    assert_ne!(duplicate_id, source_id);
    assert_eq!(
        project.timeline().track_of(duplicate_id),
        Some(destination_track)
    );
}

#[test]
fn duplicate_clip_clones_generated_content_and_template_flag() {
    let mut project = Project::new("test", R24);
    let source_track = project.add_track(TrackKind::Text, "Titles");
    let destination_track = project.add_track(TrackKind::Text, "Titles 2");
    let source_id = project
        .add_generated(source_track, Generator::text("Animated title"), tr(5, 40))
        .unwrap();
    project.set_text_editable(source_id, true).unwrap();
    project
        .set_param_keyframe(
            source_id,
            ClipParam::Scale,
            rt(5),
            ParamValue::Scalar(0.75),
            Easing::EaseInOut,
            None,
        )
        .unwrap();

    let source_before = project.clip(source_id).unwrap().clone();
    let start = rt(100);
    let duplicate_id = project
        .duplicate_clip(source_id, destination_track, start)
        .unwrap();

    let mut expected = source_before.clone();
    expected.id = duplicate_id;
    expected.timeline.start = start;
    expected.link = None;
    assert_eq!(project.clip(duplicate_id).unwrap(), &expected);
    assert!(project.clip(duplicate_id).unwrap().text_editable);
    assert_eq!(project.clip(source_id).unwrap(), &source_before);
}

#[test]
fn duplicate_clip_unlinks_copy_without_changing_source_group() {
    let (mut project, media_id, source_track) = project_with_media(300);
    let destination_track = project.add_track(TrackKind::Video, "V2");
    let first = project
        .add_clip(source_track, media_id, tr(0, 20), rt(0))
        .unwrap();
    let second = project
        .add_clip(source_track, media_id, tr(20, 20), rt(20))
        .unwrap();
    let group = crate::LinkId::next();
    for id in [first, second] {
        project.timeline_mut().clip_mut(id).unwrap().link = Some(group);
    }

    let duplicate = project
        .duplicate_clip(first, destination_track, rt(100))
        .unwrap();

    assert_eq!(project.clip(first).unwrap().link, Some(group));
    assert_eq!(project.clip(second).unwrap().link, Some(group));
    assert_eq!(project.clip(duplicate).unwrap().link, None);
}

#[test]
fn duplicate_clip_rejections_are_atomic() {
    let (mut project, media_id, source_track) = project_with_media(500);
    let destination_track = project.add_track(TrackKind::Video, "V2");
    let incompatible_track = project.add_track(TrackKind::Text, "Titles");
    let source = project
        .add_clip(source_track, media_id, tr(0, 40), rt(0))
        .unwrap();
    let blocker = project
        .add_clip(destination_track, media_id, tr(40, 40), rt(100))
        .unwrap();
    let source_before = project.clip(source).unwrap().clone();
    let blocker_before = project.clip(blocker).unwrap().clone();
    let clip_count_before = project.timeline().clip_count();
    let project_before = serde_json::to_value(&project).unwrap();

    macro_rules! assert_atomic_rejection {
        ($result:expr, $expected:expr) => {{
            assert_eq!($result, Err($expected));
            assert_eq!(project.timeline().clip_count(), clip_count_before);
            assert_eq!(project.clip(source).unwrap(), &source_before);
            assert_eq!(project.clip(blocker).unwrap(), &blocker_before);
            assert_eq!(
                serde_json::to_value(&project).unwrap(),
                project_before,
                "the complete serialized project remains unchanged"
            );
        }};
    }

    let equivalent_but_noncanonical_rate = Rational::new(48, 2);
    assert_atomic_rejection!(
        project.duplicate_clip(
            source,
            destination_track,
            RationalTime::new(200, equivalent_but_noncanonical_rate)
        ),
        ModelError::RateMismatch {
            expected: R24,
            got: equivalent_but_noncanonical_rate,
        }
    );
    assert_atomic_rejection!(
        project.duplicate_clip(source, destination_track, rt(-1)),
        ModelError::InvalidRange
    );

    let missing_clip = ClipId::next();
    assert_atomic_rejection!(
        project.duplicate_clip(missing_clip, destination_track, rt(200)),
        ModelError::UnknownClip(missing_clip)
    );

    let missing_track = TrackId::next();
    assert_atomic_rejection!(
        project.duplicate_clip(source, missing_track, rt(200)),
        ModelError::UnknownTrack(missing_track)
    );
    assert_atomic_rejection!(
        project.duplicate_clip(source, incompatible_track, rt(200)),
        ModelError::IncompatibleTrackKind {
            track: incompatible_track,
            kind: TrackKind::Text,
        }
    );
    assert_atomic_rejection!(
        project.duplicate_clip(source, destination_track, rt(110)),
        ModelError::Overlap(destination_track)
    );
}
