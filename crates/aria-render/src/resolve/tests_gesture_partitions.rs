//! `gesture_partitions` cases.

use super::*;

#[test]
fn gesture_partitions_fall_back_when_canvas_pass_sits_above_clip() {
    let mut project = Project::new("p", FPS_24);
    let base = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            base,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    let adjustment = project.add_track(TrackKind::Adjustment, "A1");
    let bar = project
        .add_generated(adjustment, Generator::Adjustment, tr(0, 100))
        .unwrap();
    project
        .set_clip_adjustments(
            bar,
            ColorAdjustments {
                exposure: 1.0.into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();

    let partitions = resolve_gesture_partitions(&project, rt(5), clip).unwrap();
    assert!(partitions.is_none());
}

#[test]
fn abutting_clips_crossfade_at_midpoint() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(24, 24),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();

    // Cut at 24; window [12, 36); midpoint tick 24 → progress 0.5.
    let scene = resolve(&project, rt(24)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    match &scene.layers[0].source {
        LayerSource::Transition {
            transition_id,
            progress,
            ..
        } => {
            assert_eq!(transition_id, "crossfade");
            approx(*progress, 0.5);
        }
        other => panic!("expected transition layer, got {other:?}"),
    }
}
#[test]
fn transition_alignment_shifts_resolved_blend_window() {
    use aria_models::TransitionAlignment;

    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 48),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(48, 48),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();

    for (align, before_tick, mid_tick, after_tick) in [
        (TransitionAlignment::Center, 35, 48, 60),
        (TransitionAlignment::Start, 47, 60, 72),
        (TransitionAlignment::End, 23, 36, 48),
    ] {
        project.set_transition_alignment(left, align).unwrap();
        let before = resolve(&project, rt(before_tick)).unwrap();
        assert!(
            !matches!(before.layers[0].source, LayerSource::Transition { .. }),
            "{align:?} should not blend at tick {before_tick}"
        );
        let mid = resolve(&project, rt(mid_tick)).unwrap();
        match &mid.layers[0].source {
            LayerSource::Transition { progress, .. } => approx(*progress, 0.5),
            other => panic!("{align:?}: expected transition at {mid_tick}, got {other:?}"),
        }
        let after = resolve(&project, rt(after_tick)).unwrap();
        assert!(
            !matches!(after.layers[0].source, LayerSource::Transition { .. }),
            "{align:?} should not blend at tick {after_tick}"
        );
    }
}

#[test]
fn transition_between_canvas_passes_falls_back_to_plain_clip() {
    // Legacy project files may carry transitions on effect/filter/
    // adjustment lanes (the model now rejects creating them). Both sides
    // resolve to CanvasPass, which the renderer can't nest — the resolver
    // must skip the transition and resolve the track normally, not error.
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Adjustment, "A1");
    let left = project
        .add_generated(track, Generator::Adjustment, tr(0, 24))
        .unwrap();
    let right = project
        .add_generated(track, Generator::Adjustment, tr(24, 24))
        .unwrap();
    for clip in [left, right] {
        project
            .set_clip_adjustments(
                clip,
                ColorAdjustments {
                    exposure: 1.0.into(),
                    ..ColorAdjustments::default()
                },
            )
            .unwrap();
    }
    // Inject the transition through the persistence path, the way a
    // project saved before the model-level guard would carry it.
    let mut doc = serde_json::to_value(&project).unwrap();
    doc["timeline"]["tracks"][0][1]["transitions"] = serde_json::json!([{
        "left": left.raw(),
        "right": right.raw(),
        "transition_id": "crossfade",
        "duration": 24,
    }]);
    let project: Project = serde_json::from_value(doc).unwrap();

    // Midpoint of the window [12, 36): no Transition layer, just the
    // plain canvas pass of the clip under the playhead.
    let scene = resolve(&project, rt(24)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(matches!(scene.layers[0].source, LayerSource::CanvasPass));
}

#[test]
fn outside_transition_window_is_single_clip() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(24, 24),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();

    let scene = resolve(&project, rt(0)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(matches!(scene.layers[0].source, LayerSource::Solid(_)));
}

#[test]
fn gesture_partitions_split_stack_and_apply_identity_to_sprite() {
    let mut project = Project::new("p", FPS_24);
    let media_a = project.add_media(video(1920, 1080));
    let media_b = project.add_media(video(1280, 720));
    let bottom_track = project.add_track(TrackKind::Video, "V1");
    let top_track = project.add_track(TrackKind::Video, "V2");
    let bottom = project
        .add_clip(bottom_track, media_a, tr(0, 100), rt(0))
        .unwrap();
    let top = project
        .add_clip(top_track, media_b, tr(0, 100), rt(0))
        .unwrap();

    let scene = resolve_with(
        &project,
        rt(5),
        ResolveOverrides {
            transform: Some((top, GESTURE_IDENTITY_TRANSFORM)),
            ..ResolveOverrides::default()
        },
    )
    .unwrap();
    let layer_allocation = scene.layers.as_ptr();
    let parts = partition_gesture_scene(scene, top).expect("top clip should partition");
    assert_eq!(
        parts.below.layers.as_ptr(),
        layer_allocation,
        "partitioning must retain the original layer allocation"
    );
    assert_eq!(parts.below.layers.len(), 1);
    assert_eq!(parts.below.layers[0].clip, Some(bottom));
    assert_eq!(parts.sprite.layers.len(), 1);
    assert_eq!(parts.sprite.layers[0].clip, Some(top));
    assert!(parts.above.layers.is_empty());
    assert_eq!(parts.below.background[3], 255);
    assert_eq!(parts.sprite.background, [0, 0, 0, 0]);
    assert_eq!(parts.above.background, [0, 0, 0, 0]);
    approx2(parts.sprite.layers[0].center, [960.0, 540.0]);
    match parts.sprite.layers[0].size {
        SizeSpec::Fixed(size) => {
            // 1280×720 aspect-fits into 1920×1080, then × identity scale 1.
            approx(size[0], 1920.0);
            approx(size[1], 1080.0);
        }
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn gesture_partitions_return_none_inside_transition_window() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(24, 24),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();

    assert!(
        resolve_gesture_partitions(&project, rt(12), left)
            .unwrap()
            .is_none()
    );
}

#[test]
fn video_clip_blend_mode_reaches_scene_layer() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project.set_blend_mode(clip, BlendMode::Multiply).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert_eq!(scene.layers[0].blend_mode, BlendMode::Multiply);
}

#[test]
fn text_generator_blend_mode_reaches_scene_layer() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "Hi".into(),
                style: ModelTextStyle::default(),
            },
            tr(0, 100),
        )
        .unwrap();
    project.set_blend_mode(clip, BlendMode::Screen).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(matches!(scene.layers[0].source, LayerSource::Text { .. }));
    assert_eq!(scene.layers[0].blend_mode, BlendMode::Screen);
}

#[test]
fn adjustment_lane_blend_mode_stays_normal_on_canvas_pass() {
    let mut project = Project::new("p", FPS_24);
    let base = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            base,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    let adjustment = project.add_track(TrackKind::Adjustment, "A1");
    let bar = project
        .add_generated(adjustment, Generator::Adjustment, tr(0, 100))
        .unwrap();
    project
        .set_clip_adjustments(
            bar,
            ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();
    // Even if the bar carries a blend mode, canvas passes must stay Normal.
    project.set_blend_mode(bar, BlendMode::Multiply).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert_eq!(scene.layers[1].source, LayerSource::CanvasPass);
    assert_eq!(scene.layers[1].blend_mode, BlendMode::Normal);
}

#[test]
fn shadow_blur_scales_to_canvas_pixels_on_tall_canvas() {
    // Auto canvas follows the 3840×2160 media → ch=2160 → px = 2× at scale 1.
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(3840, 2160));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 128]),
                    offset: Param::Constant([4.0, 4.0]),
                    blur: Param::Constant(8.0),
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!((scene.width, scene.height), (3840, 2160));
    assert_eq!(scene.layers.len(), 1);
    let styles = scene.layers[0].styles.as_ref().expect("styles");
    assert_eq!(
        styles.shadow,
        Some(SceneShadow {
            rgba: [0, 0, 0, 128],
            offset: [8.0, 8.0],
            blur: 16.0,
        })
    );
}

#[test]
fn keyframed_glow_radius_samples_per_tick() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                glow: Some(LayerGlow {
                    rgba: Param::Constant([255, 255, 255, 255]),
                    radius: Param::Constant(0.0),
                    intensity: Param::Constant(1.0),
                }),
                ..Default::default()
            },
        )
        .unwrap();
    let radius = ClipParam::Style {
        param: StyleParam::GlowRadius,
    };
    project
        .set_param_keyframe(
            clip,
            radius,
            rt(0),
            ParamValue::Scalar(4.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            radius,
            rt(40),
            ParamValue::Scalar(20.0),
            Easing::Linear,
            None,
        )
        .unwrap();

    // Midpoint of the segment (local tick 20) → radius 12 ref px → 12 canvas px.
    let mid = resolve(&project, rt(20)).unwrap();
    approx(
        mid.layers[0]
            .styles
            .as_ref()
            .unwrap()
            .glow
            .as_ref()
            .unwrap()
            .radius,
        12.0,
    );
    let end = resolve(&project, rt(40)).unwrap();
    approx(
        end.layers[0]
            .styles
            .as_ref()
            .unwrap()
            .glow
            .as_ref()
            .unwrap()
            .radius,
        20.0,
    );
}

#[test]
fn transform_scale_doubles_outline_width() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                outline: Some(LayerOutline {
                    rgba: Param::Constant([255, 255, 255, 255]),
                    width: Param::Constant(4.0),
                }),
                ..Default::default()
            },
        )
        .unwrap();
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: 2.0.into(),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    approx(
        scene.layers[0]
            .styles
            .as_ref()
            .unwrap()
            .outline
            .as_ref()
            .unwrap()
            .width,
        8.0,
    );
}
