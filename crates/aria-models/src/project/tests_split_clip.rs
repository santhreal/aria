//! `split_clip` cases.

use super::*;

// --- remove_clip ------------------------------------------------------

#[test]
fn remove_clip_returns_clip_and_leaves_gap() {
    let (mut project, media_id, track) = project_with_media(200);
    let a = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let b = project
        .add_clip(track, media_id, tr(50, 50), rt(100))
        .unwrap();

    let removed = project.remove_clip(a).unwrap();
    assert_eq!(removed.id, a);
    assert!(project.clip(a).is_none());
    assert_eq!(project.clip(b).unwrap().start().value, 100);
    assert_eq!(project.timeline().clip_count(), 1);
}

#[test]
fn remove_clip_unknown_returns_none() {
    let (mut project, _, _) = project_with_media(10);
    let missing = ClipId::next();
    assert!(project.remove_clip(missing).is_none());
}

// --- split_clip -------------------------------------------------------

#[test]
fn split_clip_divides_media_and_generated() {
    let (mut project, media_id, track) = project_with_media(500);
    let media_clip = project
        .add_clip(track, media_id, tr(100, 100), rt(0))
        .unwrap();
    let right = project.split_clip(media_clip, rt(40)).unwrap();
    assert_eq!(project.clip(media_clip).unwrap().timeline, tr(0, 40));
    assert_eq!(project.clip(right).unwrap().timeline, tr(40, 60));

    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let generated = project
        .add_generated(fx, Generator::Adjustment, tr(200, 100))
        .unwrap();
    let generated_right = project.split_clip(generated, rt(250)).unwrap();
    assert_eq!(project.clip(generated).unwrap().timeline, tr(200, 50));
    assert_eq!(project.clip(generated_right).unwrap().timeline, tr(250, 50));
}

#[test]
fn split_clip_preserves_full_property_snapshot_and_rebases_tail_curves() {
    let (mut project, media_id, track) = project_with_media(1_000);
    let clip_id = project
        .add_clip(track, media_id, tr(17, 180), rt(10))
        .unwrap();
    project
        .set_clip_speed(clip_id, Rational::new(3, 2), true)
        .unwrap();
    let link = crate::LinkId::next();

    {
        let clip = project.timeline_mut().clip_mut(clip_id).unwrap();
        clip.link = Some(link);
        clip.transform.position.set_keyframe(
            0,
            [0.1, -0.2],
            Easing::Bezier {
                points: [0.2, 0.8, 0.7, 0.1],
            },
        );
        clip.transform
            .position
            .set_keyframe(100, [0.7, 0.4], Easing::EaseOut);
        clip.transform.anchor_point = Param::Constant([0.25, 0.75]);
        clip.transform
            .scale
            .set_keyframe(0, 0.8.into(), Easing::EaseIn);
        clip.transform
            .scale
            .set_keyframe(100, 1.6.into(), Easing::EaseInOut);
        clip.transform.rotation = Param::Constant(27.5);
        clip.transform.opacity = Param::Constant(0.65);

        // A normalized, non-default representation whose value is flat
        // keeps this property-focused test on the exact constant-rate
        // source path while proving the ramp field survives.
        clip.speed_curve.set_keyframe(0, 1.0, Easing::Linear);
        clip.speed_curve
            .set_keyframe(crate::SPEED_CURVE_SCALE, 1.0, Easing::Linear);
        clip.preserve_pitch = false;

        clip.volume.set_keyframe(0, 0.2, Easing::EaseIn);
        clip.volume.set_keyframe(100, 1.4, Easing::EaseOut);
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
            .set_param_keyframe(0, 100, 12.0, Easing::EaseOut)
            .unwrap();
        clip.effects = vec![effect];
        clip.beats = vec![19, 31, 47, 139];

        clip.mask = Some(Mask {
            kind: crate::look::MaskKind::Heart,
            feather: 0.35.into(),
            invert: true,
            ..Mask::new(crate::look::MaskKind::Heart)
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
        clip.lut = Some(Lut {
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
        clip.audio_role = Some(AudioRole::Voiceover);
        clip.replaceable = Some(
            Replaceable::new(7)
                .with_accepts(SlotMedia::VideoOnly)
                .with_label("Hero"),
        );
        clip.text_editable = true;
    }

    let before = project.clip(clip_id).unwrap().clone();
    let (expected_left_curve, expected_right_curve) =
        split_speed_curve(&before.speed_curve, 0.5).unwrap();
    let right_id = project.split_clip(clip_id, rt(70)).unwrap();

    let mut expected_left = before.clone();
    expected_left.timeline = tr(10, 60);
    expected_left.content = ClipSource::Media {
        media: media_id,
        source: tr(107, 90),
    };
    expected_left.fade_out = 0;
    expected_left.animation_out = None;
    expected_left.speed_curve = expected_left_curve;
    assert_eq!(
        project.clip(clip_id).unwrap(),
        &expected_left,
        "the head differs only in its split geometry and right-edge properties"
    );

    let mut expected_right = before.clone();
    expected_right.id = right_id;
    expected_right.timeline = tr(70, 60);
    expected_right.content = ClipSource::Media {
        media: media_id,
        source: tr(17, 90),
    };
    expected_right.link = None;
    expected_right.shift_timeline_params(-60).unwrap();
    expected_right.speed_curve = expected_right_curve;
    expected_right.fade_in = 0;
    expected_right.animation_in = None;
    assert_eq!(
        project.clip(right_id).unwrap(),
        &expected_right,
        "normalizing only split-specific fields makes the full snapshots equal"
    );

    let right = project.clip(right_id).unwrap();
    for tail_tick in [0, 17, 59] {
        assert_eq!(
            right.transform.sample(tail_tick),
            before.transform.sample(60 + tail_tick)
        );
        assert_eq!(
            right.volume.sample(tail_tick),
            before.volume.sample(60 + tail_tick)
        );
        assert_eq!(
            right.effects[0].sample_param("radius", tail_tick as f64),
            before.effects[0].sample_param("radius", (60 + tail_tick) as f64)
        );
    }
    assert_eq!(project.clip(clip_id).unwrap().link, Some(link));
    assert_eq!(
        right.link, None,
        "callers remain responsible for relinking tails"
    );
}

#[test]
fn split_clip_uses_forward_constant_speed_source_boundary() {
    let (mut project, media_id, track) = project_with_media(1_000);
    let clip_id = project
        .add_clip(track, media_id, tr(100, 120), rt(10))
        .unwrap();
    project
        .set_clip_speed(clip_id, Rational::new(3, 2), false)
        .unwrap();
    let before = project.clip(clip_id).unwrap().clone();

    let right_id = project.split_clip(clip_id, rt(42)).unwrap();
    let left = project.clip(clip_id).unwrap();
    let right = project.clip(right_id).unwrap();
    assert_eq!(left.timeline, tr(10, 32));
    assert_eq!(right.timeline, tr(42, 48));
    assert_eq!(left.source_range(), Some(tr(100, 48)));
    assert_eq!(right.source_range(), Some(tr(148, 72)));

    for tick in [10, 25, 41] {
        assert_eq!(
            left.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
    for tick in [42, 55, 75, 89] {
        assert_eq!(
            right.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
}

#[test]
fn split_clip_uses_reversed_constant_speed_source_boundary() {
    let (mut project, media_id, track) = project_with_media(1_000);
    let clip_id = project
        .add_clip(track, media_id, tr(100, 120), rt(10))
        .unwrap();
    project
        .set_clip_speed(clip_id, Rational::new(3, 2), true)
        .unwrap();
    let before = project.clip(clip_id).unwrap().clone();

    let right_id = project.split_clip(clip_id, rt(42)).unwrap();
    let left = project.clip(clip_id).unwrap();
    let right = project.clip(right_id).unwrap();
    assert_eq!(left.source_range(), Some(tr(172, 48)));
    assert_eq!(right.source_range(), Some(tr(100, 72)));

    for tick in [10, 25, 41] {
        assert_eq!(
            left.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
    for tick in [42, 55, 75, 89] {
        assert_eq!(
            right.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
}

#[test]
fn split_clip_segments_speed_curve_and_preserves_source_mapping() {
    let (mut project, media_id, track) = project_with_media(1_000);
    let clip_id = project
        .add_clip(track, media_id, tr(100, 200), rt(0))
        .unwrap();
    let mut curve = Param::Constant(1.0);
    curve.set_keyframe(0, 1.0, Easing::Linear);
    curve.set_keyframe(crate::SPEED_CURVE_SCALE, 3.0, Easing::Linear);
    project.set_clip_speed_curve(clip_id, Some(curve)).unwrap();
    let before = project.clip(clip_id).unwrap().clone();
    assert_eq!(before.timeline, tr(0, 100));

    let right_id = project.split_clip(clip_id, rt(50)).unwrap();
    let left = project.clip(clip_id).unwrap();
    let right = project.clip(right_id).unwrap();
    assert_eq!(left.source_range(), Some(tr(100, 75)));
    assert_eq!(right.source_range(), Some(tr(175, 125)));
    assert_eq!(
        left.source_range().unwrap().end_tick(),
        right.source_range().unwrap().start.value,
        "forward source windows stay contiguous at the split"
    );
    assert_eq!(
        left.speed_curve.sample_at(500.0),
        before.speed_curve.sample_at(250.0)
    );
    assert_eq!(
        right.speed_curve.sample_at(500.0),
        before.speed_curve.sample_at(750.0)
    );

    for tick in [0, 10, 25, 49] {
        assert_eq!(
            left.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
    for tick in [50, 60, 75, 99] {
        assert_eq!(
            right.source_time_at(rt(tick)),
            before.source_time_at(rt(tick))
        );
    }
    assert_eq!(
        right.source_time_at(rt(50)).unwrap(),
        before.source_time_at(rt(50)).unwrap(),
        "the first tail frame is the original frame at the boundary"
    );
}

#[test]
fn split_clip_rejects_a_cut_inside_one_source_frame() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip_id = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip_id, Rational::new(1, 2), false)
        .unwrap();
    let before = project.clip(clip_id).unwrap().clone();

    assert_eq!(
        project.split_clip(clip_id, rt(1)),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(project.clip(clip_id), Some(&before));
    assert_eq!(project.timeline().clip_count(), 1);
}

#[test]
fn split_after_reload_allocates_a_fresh_tail_id() {
    // Regression for the cross-session id-collision bug: a clip persisted
    // in one session must not have its id re-handed to a clip created in
    // the next. Round-tripping through JSON (as loading a project does)
    // reseeds the id allocator, so the split's tail id is distinct from
    // every id already in the project.
    let (mut project, media_id, track) = project_with_media(500);
    let persisted = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    let json = serde_json::to_string(&project).unwrap();
    let mut reloaded: Project = serde_json::from_str(&json).unwrap();

    let tail = reloaded.split_clip(persisted, rt(40)).unwrap();
    assert_ne!(tail, persisted, "tail must not reuse the persisted clip id");
    assert_eq!(reloaded.timeline().clip_count(), 2);
    // Both halves are addressable and distinct — the overwrite that used
    // to drop the left half can no longer happen.
    assert_eq!(reloaded.clip(persisted).unwrap().timeline, tr(0, 40));
    assert_eq!(reloaded.clip(tail).unwrap().timeline, tr(40, 60));
}

#[test]
fn split_clip_rejects_at_boundaries() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(10))
        .unwrap();
    assert_eq!(
        project.split_clip(clip, rt(10)),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(
        project.split_clip(clip, rt(110)),
        Err(ModelError::InvalidRange)
    );
}

#[test]
fn split_clip_fails_closed_when_time_anchored_state_cannot_continue() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip_id = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    project.timeline_mut().clip_mut(clip_id).unwrap().fade_in = 40;
    let before = project.clip(clip_id).unwrap().clone();
    assert!(matches!(
        project.split_clip(clip_id, rt(20)),
        Err(ModelError::InvalidParam(message)) if message.contains("fade-in")
    ));
    assert_eq!(project.clip(clip_id), Some(&before));
    assert_eq!(project.timeline().clip_count(), 1);

    let clip = project.timeline_mut().clip_mut(clip_id).unwrap();
    clip.fade_in = 0;
    clip.animation_combo = Some(AnimationRef::new("pulse"));
    let before = project.clip(clip_id).unwrap().clone();
    assert!(matches!(
        project.split_clip(clip_id, rt(25)),
        Err(ModelError::InvalidParam(message)) if message.contains("phase boundary")
    ));
    assert_eq!(project.clip(clip_id), Some(&before));
    assert_eq!(project.timeline().clip_count(), 1);
}

#[test]
fn split_clip_rejects_unknown_clip() {
    let (mut project, _, _) = project_with_media(10);
    let unknown = ClipId::from_raw(999);
    assert_eq!(
        project.split_clip(unknown, rt(5)),
        Err(ModelError::UnknownClip(unknown))
    );
}
