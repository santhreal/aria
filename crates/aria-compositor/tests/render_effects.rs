//! Effect passes on a layer and on the whole canvas: what each one does to a
//! known plate, not whether it dispatches.

use super::*;

#[test]
fn vignette_darkens_corners_vs_center() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let white = CompositeLayer::solid([255, 255, 255, 255], LayerPlacement::full_canvas(&config));
    let baseline = comp.render(&gpu, &config, &[white]).expect("baseline");

    let vignette = PassInstance::new("vignette", vec![0.8]);
    let effects = [vignette];
    let effected =
        CompositeLayer::solid([255, 255, 255, 255], LayerPlacement::full_canvas(&config))
            .with_effects(&effects);
    let dark = comp.render(&gpu, &config, &[effected]).expect("vignette");

    let center = baseline.pixel(32, 32);
    let corner = dark.pixel(2, 2);
    assert!(
        corner[0] < center[0] - 20,
        "corner should darken: {corner:?} vs {center:?}"
    );
}

#[test]
fn gaussian_blur_radius_softens_a_hard_edge() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    // Half white / half black vertical split — blur should mix across the seam.
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for _y in 0..64 {
        for x in 0..64 {
            let v = if x < 32 { 255 } else { 0 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let bmp = RgbaImage::new(64, 64, pixels);
    let sharp = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let sharp_img = comp.render(&gpu, &config, &[sharp]).expect("sharp");

    let blur = PassInstance::new("gaussian_blur", vec![8.0]);
    let effects = [blur];
    let soft =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    let soft_img = comp.render(&gpu, &config, &[soft]).expect("blur");

    let sharp_seam = sharp_img.pixel(31, 32)[0] as i16;
    let soft_seam = soft_img.pixel(31, 32)[0] as i16;
    assert!(
        (soft_seam - sharp_seam).abs() > 10,
        "blur should pull dark ink across the seam: sharp={sharp_seam} soft={soft_seam}"
    );
}

/// The variance of the edge a blurred picture draws along `axis` (0 across
/// columns, 1 across rows), read from the red channel's step: the spread of
/// `|p[i] - p[i + 1]|` about its mean, in texels squared.
fn edge_variance(image: &RgbaImage, axis: usize, len: u32) -> f64 {
    let read = |i: u32| {
        let [x, y] = if axis == 0 { [i, 2] } else { [2, i] };
        f64::from(image.pixel(x, y)[0])
    };
    let steps: Vec<(f64, f64)> = (0..len - 1)
        .map(|i| (f64::from(i) + 0.5, (read(i) - read(i + 1)).abs()))
        .collect();
    let mass: f64 = steps.iter().map(|(_, d)| d).sum();
    let mean = steps.iter().map(|(x, d)| x * d).sum::<f64>() / mass;
    steps
        .iter()
        .map(|(x, d)| (x - mean).powi(2) * d)
        .sum::<f64>()
        / mass
}

/// WHY: closes the class of a blur radius the shaders clamp below the radius
/// they were given. The blur passes stopped at 32 texels, so `gaussian_blur`
/// drew the same picture for every radius from 32 to its declared 64, and a
/// radius scaled up for an export larger than the canvas blurred less than the
/// preview did.
///
/// A hard edge across each axis is blurred at radii up to the catalog maximum,
/// two and four times it (an export at twice and four times the canvas size),
/// and past the reach of one pass. Each must spread the edge to the variance
/// of one box of that radius, `r (r + 1) / 3`, within 10%. Not caught: a
/// radius past `BLUR_MAX_PAIRS` pairs, which is drawn at that bound.
#[test]
fn a_blur_spreads_an_edge_as_far_as_its_radius_states() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let max = aria_models::effect_spec("gaussian_blur")
        .and_then(|spec| spec.param("radius").map(|p| p.max))
        .expect("the catalog declares the blur radius");
    let radii = [
        4.0,
        16.0,
        max / 2.0,
        max / 2.0 + 1.0,
        max,
        2.0 * max,
        4.0 * max,
        400.0,
    ];
    const LONG: u32 = 2048;
    for axis in 0..2 {
        let (w, h) = if axis == 0 { (LONG, 4) } else { (4, LONG) };
        let config = CompositorConfig::new(w, h).with_background([0, 0, 0, 255]);
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let along = if axis == 0 { x } else { y };
                let v = if along < LONG / 2 { 255 } else { 0 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let plate = RgbaImage::new(w, h, pixels);
        let mut last = 0.0;
        for radius in radii {
            let effects = [PassInstance::new("gaussian_blur", vec![radius])];
            let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
                .with_effects(&effects);
            let image = comp.render(&gpu, &config, &[layer]).expect("blur renders");
            let spread = edge_variance(&image, axis, LONG);
            let want = f64::from(radius * (radius + 1.0)) / 3.0;
            assert!(
                (spread - want).abs() <= want * 0.1,
                "radius {radius} across axis {axis} spread the edge to variance {spread:.1}, \
                 a box of that radius spreads it to {want:.1}"
            );
            assert!(
                spread > last,
                "radius {radius} blurred no further than the one before"
            );
            last = spread;
        }
    }
}

#[test]
fn pixelate_produces_uniform_blocks() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64);
    // Horizontal gradient bitmap.
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for _y in 0..64 {
        for x in 0..64 {
            let v = (x * 4) as u8;
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let bmp = RgbaImage::new(64, 64, pixels);
    let smooth = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let smooth_img = comp.render(&gpu, &config, &[smooth]).expect("smooth");

    let pixelate = PassInstance::new("pixelate", vec![8.0]);
    let effects = [pixelate];
    let blocky_layer =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    let blocky = comp
        .render(&gpu, &config, &[blocky_layer])
        .expect("pixelate");

    // Within an 8px block, colors should match after pixelate.
    let a = blocky.pixel(10, 10);
    let b = blocky.pixel(11, 10);
    assert_eq!(a, b, "pixelate should flatten local variation");
    // And differ from the smooth original at the same coords.
    assert_ne!(smooth_img.pixel(10, 10), a);
}

#[test]
fn canvas_pass_pixelates_the_composited_stack() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64);
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for _y in 0..64 {
        for x in 0..64 {
            let v = (x * 4) as u8;
            pixels.extend_from_slice(&[v, 255u8.saturating_sub(v), 32, 255]);
        }
    }
    let bmp = RgbaImage::new(64, 64, pixels);
    let gradient = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let mut overlay_place = LayerPlacement::full_canvas(&config);
    overlay_place.opacity = 0.5;
    let overlay = CompositeLayer::solid([0, 0, 255, 255], overlay_place);

    let baseline = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::layer(&gradient),
                CompositorLayer::layer(&overlay),
            ],
        )
        .expect("baseline");

    let pixelate = PassInstance::new("pixelate", vec![8.0]);
    let effects = [pixelate];
    let blocky = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::layer(&gradient),
                CompositorLayer::layer(&overlay),
                CompositorLayer::CanvasPass {
                    effects: &effects,
                    grade: None,
                    lut: None,
                    stencil: None,
                },
            ],
        )
        .expect("canvas pass pixelate");

    assert_eq!(
        blocky.pixel(10, 10),
        blocky.pixel(11, 10),
        "canvas pass should flatten local variation"
    );
    assert_ne!(baseline.pixel(10, 10), blocky.pixel(10, 10));
}

#[test]
fn production_render_rejects_effect_parameter_count_mismatch() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64);

    // vignette expects 1 float
    let bad_effect = PassInstance::new("vignette", vec![0.5, 1.0]);
    let effects = [bad_effect];
    let layer = CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config))
        .with_effects(&effects);

    let err = comp
        .render(&gpu, &config, &[layer])
        .expect_err("production render must reject parameter count mismatch");
    match err {
        CompositorError::InvalidEffectParams {
            effect,
            expected,
            actual,
        } => {
            assert_eq!(effect, "vignette");
            assert_eq!(expected, 1);
            assert_eq!(actual, 2);
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn production_render_rejects_effect_parameter_overflow() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64);

    // exceeds 16 uniform floats
    let bad_effect = PassInstance::new("gaussian_blur", vec![1.0; 17]);
    let effects = [bad_effect];
    let layer = CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config))
        .with_effects(&effects);

    let err = comp
        .render(&gpu, &config, &[layer])
        .expect_err("production render must reject uniform overflow");
    match err {
        CompositorError::InvalidEffectParams {
            effect,
            expected,
            actual,
        } => {
            assert_eq!(effect, "gaussian_blur");
            assert_eq!(expected, 1);
            assert_eq!(actual, 17);
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn production_render_rejects_every_catalog_effect_variant_on_short_or_long_params() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64);

    let descriptors = aria_compositor::shipped_effect_descriptors();
    assert!(
        !descriptors.is_empty(),
        "effect descriptors must not be empty"
    );

    for desc in descriptors {
        let expected = desc.float_count();

        // Long parameters rejected through production render
        let bad_long = PassInstance::new(desc.id, vec![0.5; expected + 1]);
        let layer_long =
            CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config))
                .with_effects(std::slice::from_ref(&bad_long));
        let err_long = comp
            .render(&gpu, &config, &[layer_long])
            .expect_err(&format!(
                "production render must reject long params for '{}'",
                desc.id
            ));
        match err_long {
            CompositorError::InvalidEffectParams {
                ref effect,
                expected: exp,
                actual,
            } => {
                assert_eq!(effect, desc.id);
                assert_eq!(exp, expected);
                assert_eq!(actual, expected + 1);
            }
            other => panic!("unexpected error variant for '{}': {other:?}", desc.id),
        }

        // Short parameters (empty) rejected through production render when expected > 0
        if expected > 0 {
            let bad_short = PassInstance::new(desc.id, Vec::new());
            let layer_short =
                CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config))
                    .with_effects(std::slice::from_ref(&bad_short));
            let err_short = comp
                .render(&gpu, &config, &[layer_short])
                .expect_err(&format!(
                    "production render must reject empty params for '{}'",
                    desc.id
                ));
            match err_short {
                CompositorError::InvalidEffectParams {
                    ref effect,
                    expected: exp,
                    actual,
                } => {
                    assert_eq!(effect, desc.id);
                    assert_eq!(exp, expected);
                    assert_eq!(actual, 0);
                }
                other => panic!("unexpected error variant for '{}': {other:?}", desc.id),
            }
        }
    }
}
