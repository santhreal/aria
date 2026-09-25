//! Tests for lift, gamma, and gain grading controls at resolve time.
//!
//! Closes the defect where color grading was restricted to a single filter
//! intensity slider, lacking standard 3-way color grading controls (lift/gamma/gain).
//! Asserts the resolved output values, per-channel curves, neutral no-op guarantees,
//! and keyframed interpolation across ticks.

use super::tests::*;
use aria_compositor::ColorGrade;
use aria_models::{
    ColorAdjustments, Easing, GAIN_NEUTRAL, GAMMA_NEUTRAL, LIFT_NEUTRAL, LookParam, Project,
};

#[test]
fn a_lift_raises_the_floor_without_clipping_the_top() {
    let mut project = Project::new("lift_test", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();

    let mut adjust = ColorAdjustments::default();
    adjust.lift_r = Param::Constant(0.25);
    adjust.lift_g = Param::Constant(0.10);
    adjust.lift_b = Param::Constant(0.0);
    project.set_clip_adjustments(clip, adjust).unwrap();

    let scene = resolve(&project, rt(0)).unwrap();
    let layer = &scene.layers[0];
    let grade = layer.color_grade.expect("color grade resolved");

    // Floor raised: input black [0, 0, 0] -> [0.25, 0.10, 0.0]
    let black_graded = grade.apply([0.0, 0.0, 0.0]);
    approx(black_graded[0], 0.25);
    approx(black_graded[1], 0.10);
    approx(black_graded[2], 0.0);

    // Top not clipped: input white [1, 1, 1] -> [1.0, 1.0, 1.0]
    let white_graded = grade.apply([1.0, 1.0, 1.0]);
    approx(white_graded[0], 1.0);
    approx(white_graded[1], 1.0);
    approx(white_graded[2], 1.0);

    // Midtone: input 0.5 -> 0.5 + 0.25 * (1 - 0.5) = 0.625
    let mid_graded = grade.apply([0.5, 0.5, 0.5]);
    approx(mid_graded[0], 0.625);
    approx(mid_graded[1], 0.55);
    approx(mid_graded[2], 0.5);
}

#[test]
fn a_gain_scales_the_top() {
    let mut project = Project::new("gain_test", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();

    let mut adjust = ColorAdjustments::default();
    adjust.gain_r = Param::Constant(1.5);
    adjust.gain_g = Param::Constant(0.8);
    adjust.gain_b = Param::Constant(1.0);
    project.set_clip_adjustments(clip, adjust).unwrap();

    let scene = resolve(&project, rt(0)).unwrap();
    let layer = &scene.layers[0];
    let grade = layer.color_grade.expect("color grade resolved");

    // Black stays 0: 0 * gain = 0
    let black_graded = grade.apply([0.0, 0.0, 0.0]);
    approx(black_graded[0], 0.0);
    approx(black_graded[1], 0.0);
    approx(black_graded[2], 0.0);

    // Midtone scaled: 0.5 * 1.5 = 0.75, 0.5 * 0.8 = 0.4
    let mid_graded = grade.apply([0.5, 0.5, 0.5]);
    approx(mid_graded[0], 0.75);
    approx(mid_graded[1], 0.40);
    approx(mid_graded[2], 0.50);

    // Top scaled: 0.6 * 1.5 = 0.9, 1.0 * 0.8 = 0.8
    let high_graded = grade.apply([0.6, 1.0, 1.0]);
    approx(high_graded[0], 0.90);
    approx(high_graded[1], 0.80);
    approx(high_graded[2], 1.00);
}

#[test]
fn a_gamma_bends_the_mid() {
    let mut project = Project::new("gamma_test", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();

    let mut adjust = ColorAdjustments::default();
    adjust.gamma_r = Param::Constant(2.0);
    adjust.gamma_g = Param::Constant(0.5);
    adjust.gamma_b = Param::Constant(1.0);
    project.set_clip_adjustments(clip, adjust).unwrap();

    let scene = resolve(&project, rt(0)).unwrap();
    let layer = &scene.layers[0];
    let grade = layer.color_grade.expect("color grade resolved");

    // Floor and top unchanged by gamma
    let black_graded = grade.apply([0.0, 0.0, 0.0]);
    approx(black_graded[0], 0.0);
    approx(black_graded[1], 0.0);
    approx(black_graded[2], 0.0);

    let white_graded = grade.apply([1.0, 1.0, 1.0]);
    approx(white_graded[0], 1.0);
    approx(white_graded[1], 1.0);
    approx(white_graded[2], 1.0);

    // Midtone bent: 0.5^(1/2) = sqrt(0.5) ≈ 0.7071, 0.5^(1/0.5) = 0.5^2 = 0.25, 0.5^1 = 0.5
    let mid_graded = grade.apply([0.5, 0.5, 0.5]);
    approx(mid_graded[0], (0.5f32).sqrt());
    approx(mid_graded[1], 0.25);
    approx(mid_graded[2], 0.50);
}

#[test]
fn neutral_values_are_a_noop_byte_for_byte() {
    let mut project = Project::new("neutral_test", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();

    // Default adjustments are all neutral
    let default_adj = ColorAdjustments::default();
    assert!(default_adj.is_neutral());
    project.set_clip_adjustments(clip, default_adj).unwrap();

    let scene = resolve(&project, rt(0)).unwrap();
    let layer = &scene.layers[0];
    // Identity fast-path drops the grade entirely
    assert_eq!(layer.color_grade, None);

    // Identity grade apply is byte-for-byte exact no-op
    let test_pixels = [
        [0.0, 0.0, 0.0],
        [0.25, 0.5, 0.75],
        [1.0, 1.0, 1.0],
        [0.123, 0.456, 0.789],
    ];
    for p in test_pixels {
        let result = ColorGrade::IDENTITY.apply(p);
        assert_eq!(result, p);
    }
}

#[test]
fn each_control_reset_returns_the_neutral() {
    assert_eq!(LIFT_NEUTRAL, 0.0);
    assert_eq!(GAMMA_NEUTRAL, 1.0);
    assert_eq!(GAIN_NEUTRAL, 1.0);

    let mut adjust = ColorAdjustments {
        lift_r: 0.5.into(),
        lift_g: (-0.3).into(),
        lift_b: 0.2.into(),
        lift_master: 0.1.into(),
        gamma_r: 2.5.into(),
        gamma_g: 0.4.into(),
        gamma_b: 1.8.into(),
        gamma_master: 1.2.into(),
        gain_r: 1.9.into(),
        gain_g: 0.7.into(),
        gain_b: 1.3.into(),
        gain_master: 1.5.into(),
        ..Default::default()
    };
    assert!(!adjust.is_neutral());

    // Reset Lift
    adjust.lift_r = LIFT_NEUTRAL.into();
    adjust.lift_g = LIFT_NEUTRAL.into();
    adjust.lift_b = LIFT_NEUTRAL.into();
    adjust.lift_master = LIFT_NEUTRAL.into();
    assert_eq!(adjust.lift_r.constant(), Some(0.0));
    assert_eq!(adjust.lift_master.constant(), Some(0.0));

    // Reset Gamma
    adjust.gamma_r = GAMMA_NEUTRAL.into();
    adjust.gamma_g = GAMMA_NEUTRAL.into();
    adjust.gamma_b = GAMMA_NEUTRAL.into();
    adjust.gamma_master = GAMMA_NEUTRAL.into();
    assert_eq!(adjust.gamma_r.constant(), Some(1.0));
    assert_eq!(adjust.gamma_master.constant(), Some(1.0));

    // Reset Gain
    adjust.gain_r = GAIN_NEUTRAL.into();
    adjust.gain_g = GAIN_NEUTRAL.into();
    adjust.gain_b = GAIN_NEUTRAL.into();
    adjust.gain_master = GAIN_NEUTRAL.into();
    assert_eq!(adjust.gain_r.constant(), Some(1.0));
    assert_eq!(adjust.gain_master.constant(), Some(1.0));

    // Once all reset to documented neutrals, adjustments is neutral again
    assert!(adjust.is_neutral());
}

#[test]
fn a_keyframed_lift_changes_the_frame_between_two_ticks() {
    let mut project = Project::new("keyframe_test", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();

    let param = ClipParam::Look {
        param: LookParam::AdjustLiftMaster,
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
            rt(24),
            ParamValue::Scalar(0.5),
            Easing::Linear,
            None,
        )
        .unwrap();

    // At tick 0: neutral (None or 0.0 lift)
    let scene_0 = resolve(&project, rt(0)).unwrap();
    let layer_0 = &scene_0.layers[0];
    assert!(layer_0.color_grade.is_none());

    // At tick 12: halfway interpolated lift (0.25)
    let scene_12 = resolve(&project, rt(12)).unwrap();
    let layer_12 = &scene_12.layers[0];
    let grade_12 = layer_12.color_grade.expect("grade at tick 12");
    approx(grade_12.lift_master, 0.25);
    let out_12 = grade_12.apply([0.0, 0.0, 0.0]);
    approx(out_12[0], 0.25);

    // At tick 24: full keyframed lift (0.50)
    let scene_24 = resolve(&project, rt(24)).unwrap();
    let layer_24 = &scene_24.layers[0];
    let grade_24 = layer_24.color_grade.expect("grade at tick 24");
    approx(grade_24.lift_master, 0.50);
    let out_24 = grade_24.apply([0.0, 0.0, 0.0]);
    approx(out_24[0], 0.50);

    // The output changes strictly between the two ticks
    assert_ne!(out_12, out_24);
}
