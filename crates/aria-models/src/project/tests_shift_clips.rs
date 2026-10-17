//! `shift_clips` cases.

use super::*;

#[test]
fn split_retimed_clip_inherits_speed_and_partitions_source() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip, Rational::new(2, 1), false)
        .unwrap();
    // Timeline [0, 50) over source [0, 100).

    let right = project.split_clip(clip, rt(20)).unwrap();
    let left = project.clip(clip).unwrap();
    let right = project.clip(right).unwrap();
    assert_eq!(left.timeline, tr(0, 20));
    assert_eq!(left.source_range(), Some(tr(0, 40)));
    assert_eq!(right.timeline, tr(20, 30));
    assert_eq!(right.source_range(), Some(tr(40, 60)));
    assert_eq!(right.speed, Rational::new(2, 1));
}

#[test]
fn split_reversed_clip_hands_window_top_to_the_left() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(100, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip, Rational::new(1, 1), true)
        .unwrap();

    let right = project.split_clip(clip, rt(30)).unwrap();
    let left = project.clip(clip).unwrap();
    let right = project.clip(right).unwrap();
    // Left timeline half plays the source top backward; right half the
    // bottom.
    assert_eq!(left.source_range(), Some(tr(170, 30)));
    assert_eq!(right.source_range(), Some(tr(100, 70)));
    assert!(right.reversed);
}

// --- move_clip --------------------------------------------------------

#[test]
fn move_clip_repositions_on_same_track() {
    let (mut project, media_id, track) = project_with_media(200);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();

    project.move_clip(clip, track, rt(80)).unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline, tr(80, 50));
    assert_eq!(project.timeline().track_of(clip), Some(track));
}

#[test]
fn move_clip_rejects_negative_start_and_unknown_track() {
    let (mut project, media_id, track) = project_with_media(200);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let missing = TrackId::next();

    assert_eq!(
        project.move_clip(clip, track, rt(-1)),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(
        project.move_clip(clip, missing, rt(0)),
        Err(ModelError::UnknownTrack(missing))
    );
}

#[test]
fn shift_clips_right_moves_suffix_only() {
    let (mut project, _, track, [a, b, c]) = packed_track();
    let first = project.shift_clips(track, rt(50), rt(30)).unwrap();
    assert_eq!(first, Some(rt(80)));
    assert_eq!(project.clip(a).unwrap().start().value, 0);
    assert_eq!(project.clip(b).unwrap().start().value, 80);
    assert_eq!(project.clip(c).unwrap().start().value, 130);
}

#[test]
fn shift_clips_left_closes_gap() {
    let (mut project, _, track, [a, b, c]) = packed_track();
    project.shift_clips(track, rt(50), rt(30)).unwrap();
    let first = project.shift_clips(track, rt(80), rt(-30)).unwrap();
    assert_eq!(first, Some(rt(50)));
    assert_eq!(project.clip(a).unwrap().start().value, 0);
    assert_eq!(project.clip(b).unwrap().start().value, 50);
    assert_eq!(project.clip(c).unwrap().start().value, 100);
}

#[test]
fn shift_clips_left_rejects_collision_and_negative() {
    let (mut project, _, track, [_, b, _]) = packed_track();
    assert_eq!(
        project.shift_clips(track, rt(50), rt(-10)),
        Err(ModelError::Overlap(track))
    );
    assert_eq!(
        project.shift_clips(track, rt(0), rt(-1)),
        Err(ModelError::InvalidRange)
    );
    // Validation failures must not mutate anything.
    assert_eq!(project.clip(b).unwrap().start().value, 50);
}

#[test]
fn shift_clips_past_content_is_noop() {
    let (mut project, _, track, [a, b, c]) = packed_track();
    assert_eq!(project.shift_clips(track, rt(999), rt(40)).unwrap(), None);
    for (clip, start) in [(a, 0), (b, 50), (c, 100)] {
        assert_eq!(project.clip(clip).unwrap().start().value, start);
    }
}

#[test]
fn shift_clips_rejects_zero_delta_and_unknown_track() {
    let (mut project, _, track, _) = packed_track();
    assert_eq!(
        project.shift_clips(track, rt(0), rt(0)),
        Err(ModelError::InvalidRange)
    );
    let missing = TrackId::next();
    assert_eq!(
        project.shift_clips(missing, rt(0), rt(10)),
        Err(ModelError::UnknownTrack(missing))
    );
}

// --- ripple_delete ----------------------------------------------------

#[test]
fn ripple_delete_shifts_later_clips() {
    let (mut project, media_id, track) = project_with_media(300);
    let a = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let b = project
        .add_clip(track, media_id, tr(50, 50), rt(50))
        .unwrap();
    let c = project
        .add_clip(track, media_id, tr(100, 50), rt(150))
        .unwrap();

    project.ripple_delete(b).unwrap();
    assert!(project.clip(b).is_none());
    assert_eq!(project.clip(a).unwrap().start().value, 0);
    assert_eq!(project.clip(c).unwrap().start().value, 100);
}

// --- timeline accessors -----------------------------------------------

#[test]
fn timeline_mut_allows_direct_timeline_edits() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Audio, "A1");
    assert_eq!(project.timeline_mut().track_count(), 1);
    assert_eq!(
        project.timeline().track(track).unwrap().kind,
        TrackKind::Audio
    );
}

// --- Clone ------------------------------------------------------------

#[test]
fn project_clone_is_independent_snapshot() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();

    let mut cloned = project.clone();
    assert_eq!(cloned.clip(clip).unwrap().timeline, tr(0, 50));

    cloned.remove_clip(clip);
    assert!(cloned.clip(clip).is_none());
    assert!(project.clip(clip).is_some());
}

#[test]
fn look_setters_persist_on_a_media_clip() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();

    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    project
        .set_clip_chroma_key(
            clip,
            Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.7.into(),
                shadow: 0.2.into(),
            }),
        )
        .unwrap();
    project
        .set_clip_stabilize(clip, Some(StabilizeLevel::Smooth))
        .unwrap();
    project
        .set_clip_filter(clip, Some(Filter::new("vivid")))
        .unwrap();
    project
        .set_clip_adjustments(
            clip,
            ColorAdjustments {
                brightness: 0.25.into(),
                ..Default::default()
            },
        )
        .unwrap();
    project.set_blend_mode(clip, BlendMode::Multiply).unwrap();
    project
        .set_motion_blur(
            clip,
            MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 8,
            },
        )
        .unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow::default()),
                ..Default::default()
            },
        )
        .unwrap();

    let c = project.clip(clip).unwrap();
    assert_eq!(c.masks[0].kind, MaskKind::Circle);
    assert_eq!(c.chroma_key.as_ref().unwrap().strength, 0.7.into());
    assert_eq!(c.stabilize, Some(StabilizeLevel::Smooth));
    assert_eq!(c.filter.as_ref().unwrap().id, "vivid");
    assert_eq!(c.adjust.brightness, 0.25.into());
    assert_eq!(c.blend_mode, BlendMode::Multiply);
    assert!(c.motion_blur.enabled);
    assert!(c.styles.shadow.is_some());

    // Clearing works and the fields vanish from saves again.
    project.remove_clip_mask(clip, 0).unwrap();
    assert!(project.clip(clip).unwrap().masks.is_empty());
    project
        .set_layer_styles(clip, LayerStyles::default())
        .unwrap();
    assert!(project.clip(clip).unwrap().styles.is_empty());
    project
        .set_motion_blur(clip, MotionBlur::default())
        .unwrap();
    assert!(project.clip(clip).unwrap().motion_blur.is_default());
}

#[test]
fn filter_intensity_keyframes_sample_at_clip_local_ticks() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    project
        .set_clip_filter(clip, Some(Filter::new("vivid")))
        .unwrap();
    let param = ClipParam::Look {
        param: crate::LookParam::FilterIntensity,
    };
    project
        .set_param_keyframe(
            clip,
            param,
            rt(0),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            param,
            rt(10),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();

    let intensity = project
        .clip(clip)
        .unwrap()
        .filter
        .as_ref()
        .unwrap()
        .intensity
        .sample(5);
    assert!((intensity - 0.5).abs() < f32::EPSILON);
}

#[test]
fn adjust_hue_keyframes_through_project_api() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let param = ClipParam::Look {
        param: crate::LookParam::AdjustHue,
    };
    project
        .set_param_keyframe(
            clip,
            param,
            rt(0),
            ParamValue::Scalar(-1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            param,
            rt(10),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    let hue = project.clip(clip).unwrap().adjust.hue.sample(5);
    assert!((hue - 0.0).abs() < f32::EPSILON);

    assert!(
        project
            .set_param_keyframe(
                clip,
                ClipParam::Look {
                    param: crate::LookParam::AdjustSharpness,
                },
                rt(0),
                ParamValue::Scalar(-0.5),
                Easing::Linear,
                None
            )
            .is_err()
    );
}

#[test]
fn mask_geometry_keyframes_through_project_api() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project
        .add_clip(track, media_id, tr(0, 48), rt(100))
        .unwrap();
    project
        .add_clip_mask(clip, Mask::new(MaskKind::Rectangle))
        .unwrap();

    let rotation = ClipParam::Mask {
        mask: 0,
        param: crate::MaskParam::Rotation,
    };
    project
        .set_param_keyframe(
            clip,
            rotation,
            rt(100),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            rotation,
            rt(140),
            ParamValue::Scalar(90.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    assert!((project.clip(clip).unwrap().masks[0].rotation.sample(20) - 45.0).abs() < f32::EPSILON);

    let center = ClipParam::Mask {
        mask: 0,
        param: crate::MaskParam::Center,
    };
    assert!(
        project
            .set_param_keyframe(
                clip,
                center,
                rt(100),
                ParamValue::Scalar(0.5),
                Easing::Linear,
                None
            )
            .is_err()
    );
    project
        .set_param_keyframe(
            clip,
            center,
            rt(100),
            ParamValue::Vec2([0.1, -0.2]),
            Easing::Linear,
            None,
        )
        .unwrap();

    let bare = project
        .add_clip(track, media_id, tr(0, 48), rt(200))
        .unwrap();
    assert!(
        project
            .set_param_keyframe(
                bare,
                rotation,
                rt(200),
                ParamValue::Scalar(15.0),
                Easing::Linear,
                None
            )
            .is_err()
    );
}
