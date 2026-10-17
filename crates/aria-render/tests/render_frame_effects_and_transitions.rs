//! `render_frame` tests covering effects and transitions. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `render_frame` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn pixelate_effect_blockifies_a_solid_fill() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [200, 100, 50, 255],
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project.add_effect(clip, "pixelate").unwrap();
    project.set_effect_param(clip, 0, 0, 24.0).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");
    // Blocky pixelate on a uniform fill still renders; center stays the fill color.
    let center = image.pixel(960, 540);
    assert!(center[0] > 180 && center[1] > 80);
}

#[test]
fn crossfade_transition_blends_two_solids_at_midpoint() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 24, FPS_24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            TimeRange::at_rate(24, 24, FPS_24),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    let px = image.pixel(960, 540);
    // Mid blend of red + blue → purple-ish, not pure red or blue.
    assert!(
        px[0] > 80 && px[2] > 80,
        "midpoint should blend channels, got {px:?}"
    );
    assert!(px[0] < 240 && px[2] < 240);
}

#[test]
fn saturation_minus_one_desaturates_red_solid() {
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let tol = pixel_tol(&gpu);

    let (mut project, clip) = red_solid_project();
    project
        .set_clip_adjustments(
            clip,
            ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    let image = renderer.render_frame(&project, rt(0)).expect("render");
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let expect = grade_ref_u8([255, 0, 0, 255], grade);
    let top_left = image.pixel(0, 0);
    assert_px_close(top_left, expect, tol, "desaturated red solid");
}

#[test]
fn transition_with_effected_clip_renders_without_panic() {
    // Regression: packing skipped nested transition-side effect chains, so
    // realizing a transition whose clips carry effects indexed past the
    // instance store and panicked.
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 24, FPS_24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            TimeRange::at_rate(24, 24, FPS_24),
        )
        .unwrap();
    project.add_effect(left, "gaussian_blur").unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    // Midpoint of the transition window [12, 36).
    let frame = renderer
        .render_frame_fit(&project, rt(24), 64, 64)
        .expect("render transition with effected clip");
    assert!(frame.width > 0 && frame.height > 0);
}

#[test]
fn the_first_frame_of_a_crossfade_reads_as_its_outgoing_clip_with_effects() {
    // Regression: the sides of a transition drew their base content only, so
    // a clip's effect chain vanished for the length of its transition. A
    // crossfade at progress 0 is its outgoing side, so the first frame of the
    // window has to read as the clip did before it.
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let red = [255, 0, 0, 255];
    let left = project
        .add_generated(
            track,
            Generator::SolidColor { rgba: red },
            TimeRange::at_rate(0, 24, FPS_24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            TimeRange::at_rate(24, 24, FPS_24),
        )
        .unwrap();
    project.add_effect(left, "duotone").unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    // The transition window is [12, 36).
    let before = renderer
        .render_frame_fit(&project, rt(6), 64, 64)
        .expect("render before the transition")
        .pixel(32, 32);
    let first = renderer
        .render_frame_fit(&project, rt(12), 64, 64)
        .expect("render the first transition frame")
        .pixel(32, 32);
    assert!(
        before
            .iter()
            .zip(red)
            .any(|(&got, plain)| got.abs_diff(plain) > 40),
        "the duotone must show before the transition, got {before:?}"
    );
    assert_near(first, before, 12, "first frame of the crossfade");
}

#[test]
fn mono_filter_at_zero_intensity_matches_no_filter() {
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let tol = pixel_tol(&gpu);

    let (mut project, clip) = red_solid_project();
    project
        .set_clip_filter(
            clip,
            Some(Filter {
                id: "mono".into(),
                intensity: 0.0.into(),
            }),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    let graded = renderer.render_frame(&project, rt(0)).expect("render");

    let (project, _) = red_solid_project();
    let baseline = renderer.render_frame(&project, rt(0)).expect("render");

    let top_left = graded.pixel(0, 0);
    let baseline_px = baseline.pixel(0, 0);
    assert_px_close(top_left, baseline_px, tol, "mono filter intensity 0");
}

#[test]
fn multiply_red_solid_over_green_solid_is_black() {
    let mut project = Project::new("p", FPS_24);
    let bottom = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            bottom,
            Generator::SolidColor {
                rgba: [0, 255, 0, 255],
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    let top = project.add_track(TrackKind::Sticker, "S2");
    let red = project
        .add_generated(
            top,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project.set_blend_mode(red, BlendMode::Multiply).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");
    let center = image.pixel(960, 540);
    assert!(
        center[0] < 16 && center[1] < 16 && center[2] < 16 && center[3] > 240,
        "multiply red over green should be ~black, got {center:?}"
    );
}

#[test]
fn motion_blur_export_smears_animated_solid() {
    // Small solid sweeping left→right over 2 ticks; with blur at the midpoint,
    // intermediate x positions light up that blur-off leaves dark.
    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 255, 255, 255],
            },
            TimeRange::at_rate(0, 24, FPS_24),
        )
        .unwrap();
    // Shrink so the solid is a bar, not full-canvas (1080×1080 → ~108px).
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: 0.1.into(),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(0),
            ParamValue::Vec2([-0.35, 0.0]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(2),
            ParamValue::Vec2([0.35, 0.0]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_motion_blur(
            clip,
            MotionBlur {
                enabled: true,
                shutter_deg: 360.0,
                samples: 8,
            },
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    // Preview path (default): no blur.
    let off = renderer.render_frame(&project, rt(1)).expect("preview");

    renderer.set_apply_motion_blur(true);
    let on = renderer.render_frame(&project, rt(1)).expect("export blur");

    assert_ne!(
        off.pixel(400, 540),
        on.pixel(400, 540),
        "blur-on export frame must differ from blur-off preview at smear probe"
    );

    // At tick 1 the sharp bar sits near canvas center (x≈540). With a full-
    // frame shutter the smear covers toward the left keyframe — probe there.
    let left = on.pixel(400, 540);
    let left_off = off.pixel(400, 540);
    assert!(
        left[0] > left_off[0],
        "blur should leave coverage left of the sharp mid position: on={left:?} off={left_off:?}"
    );
}
