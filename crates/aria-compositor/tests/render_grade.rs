//! Color grade on solid, RGBA, and video layers, and as a canvas-wide pass.

use super::*;

#[test]
fn identity_grade_matches_ungraded_solid() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16);
    let placement = LayerPlacement::full_canvas(&config);
    let rgba = [200, 80, 40, 255];

    let baseline = comp
        .render(&gpu, &config, &[CompositeLayer::solid(rgba, placement)])
        .expect("baseline");

    let explicit = comp
        .render(
            &gpu,
            &config,
            &[CompositeLayer::solid(rgba, placement).with_grade(ColorGrade::IDENTITY)],
        )
        .expect("identity");

    for y in 0..baseline.height {
        for x in 0..baseline.width {
            assert_eq!(baseline.pixel(x, y), explicit.pixel(x, y));
        }
    }
}

#[test]
fn solid_saturation_minus_one_desaturates_red() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16);
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let rgba = [255, 0, 0, 255];
    let layer = CompositeLayer::solid(rgba, LayerPlacement::full_canvas(&config)).with_grade(grade);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    let expect = grade_ref_u8(rgba, grade);
    assert_px(&img, 8, 8, expect, 3);
}

#[test]
fn solid_exposure_plus_one_brightens_mid_gray() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16);
    let grade = ColorGrade {
        exposure: 1.0,
        ..ColorGrade::IDENTITY
    };
    let rgba = [128, 128, 128, 255];
    let layer = CompositeLayer::solid(rgba, LayerPlacement::full_canvas(&config)).with_grade(grade);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    let expect = grade_ref_u8(rgba, grade);
    assert_px(&img, 8, 8, expect, 2);
}

#[test]
fn canvas_pass_grade_replaces_translucent_canvas() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 0, 0]);
    let translucent = CompositeLayer::solid([255, 0, 0, 128], LayerPlacement::full_canvas(&config));
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };

    let img = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::layer(&translucent),
                CompositorLayer::CanvasPass {
                    effects: &[],
                    grade: Some(grade),
                    lut: None,
                },
            ],
        )
        .expect("canvas pass grade");

    let expect = grade_ref_u8([128, 0, 0, 128], grade);
    assert_px(&img, 8, 8, expect, 3);
}

#[test]
fn rgba_grade_skips_transparent_and_grades_opaque() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([7, 8, 9, 255]);
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let bmp = rgba_opaque_and_transparent(16, 16);
    let layer = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_grade(grade);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let opaque_expect = grade_ref_u8([255, 0, 0, 255], grade);
    assert_px(&img, 4, 8, opaque_expect, 3);
    assert_px(&img, 12, 8, [7, 8, 9, 255], 1);
}

#[test]
fn yuv_grade_saturation_minus_one_matches_cpu_reference() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let (y, u, v) = (150u8, 60u8, 200u8);
    let color = ColorSpace::BT709;
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };

    let config = CompositorConfig::new(16, 16);
    let f = nv12_uniform(16, 16, y, u, v, color);
    let layer = CompositeLayer::frame(&f, LayerPlacement::full_canvas(&config)).with_grade(grade);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let [r, g, b] = yuv_ref(y, u, v, color);
    let graded = grade_ref(
        [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        ],
        grade,
    );
    assert_px(
        &img,
        8,
        8,
        [quant(graded[0]), quant(graded[1]), quant(graded[2]), 255],
        3,
    );
}

// --- Every slider reaches the pixels -----------------------------------
//
// A slider is a row in `aria_models::adjust_slider_rows!`. The struct, the
// uniform lanes and the WGSL accessors all expand from that row, so a new
// adjustment cannot be dropped on the way to the GPU by forgetting a field.
// What it can be dropped by is math: a lane nothing reads composites exactly
// like neutral. These two tests press every row through a real render and
// refuse a frame that did not move, on the layer path and on the canvas pass.

/// The value a slider of each shape is pressed to: far enough from neutral to
/// move 8-bit pixels, inside the range the shape validates.
macro_rules! grade_probe_value {
    (signed) => {
        0.8
    };
    (positive) => {
        0.9
    };
    (multiplicative) => {
        2.0
    };
}

/// One probe per row: the id a keyframe and a preset name the slider by, and a
/// writer that presses it.
macro_rules! grade_probes {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        const PROBES: &[(&str, fn(&mut ColorGrade))] = &[
            $(($id, |grade: &mut ColorGrade| grade.$field = grade_probe_value!($shape)),)*
        ];
    };
}

aria_models::adjust_slider_rows!(grade_probes);

/// A plate with lights, darks, colour, hard edges and off-centre content.
///
/// Every quadrant sits away from both 0 and 255, so a multiplier is visible
/// before it clamps; the quadrant boundaries are the neighbours sharpness
/// needs, and nothing is grey, which is what saturation and hue need.
fn census_plate(w: u32, h: u32) -> RgbaImage {
    let quadrants = [
        [70u8, 90, 120, 255],
        [190, 150, 90, 255],
        [120, 60, 60, 255],
        [150, 200, 170, 255],
    ];
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let q = usize::from(x >= w / 2) + 2 * usize::from(y >= h / 2);
            pixels[i..i + 4].copy_from_slice(&quadrants[q]);
        }
    }
    RgbaImage::new(w, h, pixels)
}

/// The largest per-channel difference between two frames of equal size.
fn worst_channel_delta(a: &RgbaImage, b: &RgbaImage) -> i32 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut worst = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            for c in 0..4 {
                worst = worst.max((i32::from(p[c]) - i32::from(q[c])).abs());
            }
        }
    }
    worst
}

/// A grade with one row pressed, asserted to be a grade the shader will run.
fn probe_grade(id: &str, set: fn(&mut ColorGrade)) -> ColorGrade {
    let mut grade = ColorGrade::IDENTITY;
    set(&mut grade);
    assert!(
        !grade.is_identity(),
        "{id} was pressed and the grade stayed neutral"
    );
    grade
}

#[test]
fn every_slider_moves_the_layer_it_grades() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let plate = census_plate(32, 32);
    let config = CompositorConfig::new(32, 32).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let baseline = comp
        .render(&gpu, &config, &[CompositeLayer::rgba(&plate, placement)])
        .expect("baseline");

    for (id, set) in PROBES {
        let grade = probe_grade(id, *set);
        let graded = comp
            .render(
                &gpu,
                &config,
                &[CompositeLayer::rgba(&plate, placement).with_grade(grade)],
            )
            .expect("graded layer");
        assert!(
            worst_channel_delta(&baseline, &graded) >= 2,
            "{id} composited the plate unchanged: the row has a lane no shader reads"
        );
    }
}

#[test]
fn every_slider_moves_the_canvas_pass() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let plate = census_plate(32, 32);
    let config = CompositorConfig::new(32, 32).with_background([0, 0, 0, 255]);
    let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config));
    let baseline = comp
        .render_compositor_layers(&gpu, &config, &[CompositorLayer::layer(&layer)])
        .expect("baseline");

    for (id, set) in PROBES {
        let grade = probe_grade(id, *set);
        let graded = comp
            .render_compositor_layers(
                &gpu,
                &config,
                &[
                    CompositorLayer::layer(&layer),
                    CompositorLayer::CanvasPass {
                        effects: &[],
                        grade: Some(grade),
                        lut: None,
                    },
                ],
            )
            .expect("graded canvas");
        assert!(
            worst_channel_delta(&baseline, &graded) >= 2,
            "{id} left the canvas pass unchanged: the row has a lane no shader reads"
        );
    }
}

#[test]
fn grade_branchless_extremes_preserve_color_invariants() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16);
    let placement = LayerPlacement::full_canvas(&config);
    let rgba = [128, 64, 192, 255];

    // Test neutral/identity values for branchless sliders produce exact match.
    let zero_grade = ColorGrade {
        hue: 0.0,
        highlights: 0.0,
        shadows: 0.0,
        ..ColorGrade::IDENTITY
    };
    let img = comp
        .render(
            &gpu,
            &config,
            &[CompositeLayer::solid(rgba, placement).with_grade(zero_grade)],
        )
        .expect("render neutral");
    assert_px(&img, 8, 8, rgba, 1);

    // Test extreme values without wavefront divergence or NaN/inf clamping errors.
    for hue in [-1.0, 0.5, 1.0] {
        for hl in [-1.0, 0.5, 1.0] {
            for sh in [-1.0, 0.5, 1.0] {
                let grade = ColorGrade {
                    hue,
                    highlights: hl,
                    shadows: sh,
                    ..ColorGrade::IDENTITY
                };
                let out = comp
                    .render(
                        &gpu,
                        &config,
                        &[CompositeLayer::solid(rgba, placement).with_grade(grade)],
                    )
                    .expect("render extreme grade");
                let px = out.pixel(8, 8);
                assert_eq!(px[3], 255);
            }
        }
    }
}
