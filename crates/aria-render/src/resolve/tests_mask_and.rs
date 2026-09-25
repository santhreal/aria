//! `mask_and` cases.

use super::*;

/// The model's validation cap and the evaluator's vertex-buffer bound
/// must agree, or a valid project could hold a star the renderer clamps.
#[test]
fn mask_and_chroma_key_reach_media_layer() {
    use aria_models::{ChromaKey, Mask, MaskKind};

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_mask(clip, Some(Mask::new(MaskKind::Circle)))
        .unwrap();
    project
        .set_clip_chroma_key(
            clip,
            Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.5.into(),
                shadow: 0.0.into(),
            }),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    let layer = &scene.layers[0];
    let mask = layer.mask.unwrap();
    assert_eq!(mask.kind, MaskKind::Circle);
    approx2(mask.center, [0.0, 0.0]);
    approx2(mask.size, [1.0, 1.0]);
    approx(mask.rotation_rad, 0.0);
    approx(mask.roundness, 0.0);
    assert_eq!(layer.chroma_key.unwrap().rgb, [0, 255, 0]);
    assert!((layer.chroma_key.unwrap().strength - 0.5).abs() < 1e-6);
}

#[test]
fn keyframed_mask_center_samples_per_tick() {
    use aria_models::{LookParam, Mask, MaskKind};

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_mask(clip, Some(Mask::new(MaskKind::Circle)))
        .unwrap();
    let center = ClipParam::Look {
        param: LookParam::MaskCenter,
    };
    project
        .set_param_keyframe(
            clip,
            center,
            rt(0),
            ParamValue::Vec2([0.0, 0.0]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            center,
            rt(40),
            ParamValue::Vec2([0.5, -0.25]),
            Easing::Linear,
            None,
        )
        .unwrap();

    let start = resolve(&project, rt(0)).unwrap();
    approx2(start.layers[0].mask.unwrap().center, [0.0, 0.0]);
    let mid = resolve(&project, rt(20)).unwrap();
    approx2(mid.layers[0].mask.unwrap().center, [0.25, -0.125]);
    let end = resolve(&project, rt(40)).unwrap();
    approx2(end.layers[0].mask.unwrap().center, [0.5, -0.25]);
}

#[test]
fn mask_rotation_degrees_resolve_to_radians() {
    use aria_models::{LookParam, Mask, MaskKind};

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_mask(clip, Some(Mask::new(MaskKind::Rectangle)))
        .unwrap();
    project
        .set_param_constant(
            clip,
            ClipParam::Look {
                param: LookParam::MaskRotation,
            },
            ParamValue::Scalar(90.0),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    approx(
        scene.layers[0].mask.unwrap().rotation_rad,
        std::f32::consts::FRAC_PI_2,
    );
}

#[test]
fn generated_clip_has_no_look_fields() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(scene.layers[0].mask.is_none());
    assert!(scene.layers[0].chroma_key.is_none());
}

#[test]
fn cleared_mask_is_none_on_layer() {
    use aria_models::{Mask, MaskKind};

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_mask(clip, Some(Mask::new(MaskKind::Rectangle)))
        .unwrap();
    project.set_clip_mask(clip, None).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert!(scene.layers[0].mask.is_none());
}

#[test]
fn star_point_cap_matches_the_shapes_crate() {
    assert_eq!(aria_models::MAX_STAR_POINTS, aria_shapes::MAX_STAR_POINTS);
}

#[test]
fn clip_with_gaussian_blur_carries_sampled_radius() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project.add_effect(clip, "gaussian_blur").unwrap();
    project
        .set_effect_param(clip, 0, 0, 12.0)
        .expect("set radius");

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert_eq!(scene.layers[0].effects.len(), 1);
    assert_eq!(scene.layers[0].effects[0].id, "gaussian_blur");
    approx(scene.layers[0].effects[0].params[0], 12.0);
}

#[test]
fn color_overlay_flattens_typed_params_in_catalog_order() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project.add_effect(clip, "color_overlay").unwrap();
    project
        .set_param_constant(
            clip,
            ClipParam::Effect {
                effect: 0,
                param: 0,
            },
            ParamValue::Color([51, 102, 153, 204]),
        )
        .unwrap();
    project
        .set_param_constant(
            clip,
            ClipParam::Effect {
                effect: 0,
                param: 1,
            },
            ParamValue::Vec2([0.25, -0.5]),
        )
        .unwrap();
    project.set_effect_param(clip, 0, 2, 0.75).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let params = &scene.layers[0].effects[0].params;
    // color (4) + offset (2) + amount (1)
    assert_eq!(params.len(), 7);
    approx(params[0], 51.0 / 255.0);
    approx(params[1], 102.0 / 255.0);
    approx(params[2], 153.0 / 255.0);
    approx(params[3], 204.0 / 255.0);
    approx(params[4], 0.25);
    approx(params[5], -0.5);
    approx(params[6], 0.75);
}

#[test]
fn adjustment_lane_resolves_to_canvas_pass_above_lower_layers() {
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

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert!(matches!(scene.layers[0].source, LayerSource::Solid(_)));
    assert_eq!(scene.layers[1].source, LayerSource::CanvasPass);
    assert_eq!(scene.layers[1].clip, Some(bar));
    assert_eq!(scene.layers[1].effects, Vec::<ResolvedPass>::new());
    assert_eq!(
        scene.layers[1].color_grade,
        Some(effective_grade(
            None,
            &ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        ))
    );
}

#[test]
fn every_lane_bar_variant_applies_both_its_effects_and_its_grade() {
    // The three lane-bar generators differ only in what an operator reaches for,
    // so each one carries everything its bar holds. Half of that was dropped:
    // Adjustment and Filter lost the effect stack, Effect lost the grade.
    for (kind, generator) in [
        (TrackKind::Adjustment, Generator::Adjustment),
        (TrackKind::Effect, Generator::Effect),
        (TrackKind::Filter, Generator::Filter),
    ] {
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
        let lane = project.add_track(kind, "L1");
        let bar = project.add_generated(lane, generator, tr(0, 100)).unwrap();
        project.add_effect(bar, "gaussian_blur").unwrap();
        project.set_effect_param(bar, 0, 0, 8.0).unwrap();
        let grade = ColorAdjustments {
            saturation: (-1.0).into(),
            ..ColorAdjustments::default()
        };
        project.set_clip_adjustments(bar, grade.clone()).unwrap();

        let scene = resolve(&project, rt(5)).unwrap();
        assert_eq!(scene.layers.len(), 2, "{kind:?} lane did not add a pass");
        let pass = &scene.layers[1];
        assert_eq!(pass.source, LayerSource::CanvasPass);
        assert_eq!(pass.clip, Some(bar));
        assert_eq!(
            pass.effects.len(),
            1,
            "{kind:?} lane dropped its effect stack"
        );
        assert_eq!(pass.effects[0].id, "gaussian_blur");
        approx(pass.effects[0].params[0], 8.0);
        assert_eq!(
            pass.color_grade,
            Some(effective_grade(None, &grade)),
            "{kind:?} lane dropped its grade"
        );
    }
}

#[test]
fn filter_lane_resolves_to_canvas_pass_grade() {
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
    let filter_track = project.add_track(TrackKind::Filter, "F1");
    let bar = project
        .add_generated(filter_track, Generator::Filter, tr(0, 100))
        .unwrap();
    let filter = Filter {
        id: "mono".into(),
        intensity: 1.0.into(),
    };
    project.set_clip_filter(bar, Some(filter.clone())).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert_eq!(scene.layers[1].source, LayerSource::CanvasPass);
    assert_eq!(
        scene.layers[1].color_grade,
        Some(effective_grade(Some(&filter), &ColorAdjustments::default()))
    );
}

#[test]
fn effect_lane_resolves_to_canvas_pass_effect_chain() {
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
    let effect_track = project.add_track(TrackKind::Effect, "E1");
    let bar = project
        .add_generated(effect_track, Generator::Effect, tr(0, 100))
        .unwrap();
    project.add_effect(bar, "gaussian_blur").unwrap();
    project.set_effect_param(bar, 0, 0, 8.0).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert_eq!(scene.layers[1].source, LayerSource::CanvasPass);
    assert_eq!(scene.layers[1].color_grade, None);
    assert_eq!(scene.layers[1].effects.len(), 1);
    assert_eq!(scene.layers[1].effects[0].id, "gaussian_blur");
    approx(scene.layers[1].effects[0].params[0], 8.0);
}

#[test]
fn identity_or_inactive_lane_passes_are_elided() {
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
    project
        .add_generated(adjustment, Generator::Adjustment, tr(0, 100))
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1, "neutral pass should not render");

    let bar = project
        .add_generated(adjustment, Generator::Adjustment, tr(120, 10))
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
    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1, "out-of-range pass should not render");

    project
        .timeline_mut()
        .track_mut(adjustment)
        .unwrap()
        .enabled = false;
    let scene = resolve(&project, rt(125)).unwrap();
    assert!(
        scene.layers.is_empty(),
        "disabled pass track should not render"
    );
}
