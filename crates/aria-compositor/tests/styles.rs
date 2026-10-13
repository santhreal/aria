//! GPU readback checks for per-layer shadow, glow, outline, and background styles.

use aria_compositor::{
    BlendMode, CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerBackground,
    LayerGlow, LayerOutline, LayerPlacement, LayerShadow, LayerStyles, PassInstance, RgbaImage,
};

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping styles test: no GPU adapter ({e})");
            None
        }
    }
}

macro_rules! gpu_or_skip {
    () => {
        match try_gpu() {
            Some(g) => g,
            None => return,
        }
    };
}

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

#[track_caller]
fn assert_not_px(img: &RgbaImage, x: u32, y: u32, forbidden: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    let all_close = (0..4).all(|ch| (i32::from(got[ch]) - i32::from(forbidden[ch])).abs() <= tol);
    assert!(
        !all_close,
        "pixel({x},{y}) = {got:?} unexpectedly matches ~{forbidden:?} (tol {tol})"
    );
}

fn centered_red_layer(config: &CompositorConfig) -> CompositeLayer<'static> {
    let placement = LayerPlacement {
        center: [config.width as f32 / 2.0, config.height as f32 / 2.0],
        size: [100.0, 100.0],
        rotation: 0.0,
        opacity: 1.0,
    };
    CompositeLayer::solid([255, 0, 0, 255], placement)
}

#[test]
fn shadow_offset_no_blur_on_white_canvas() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([255, 255, 255, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        shadow: Some(LayerShadow {
            rgba: [0, 0, 0, 255],
            offset: [50.0, 0.0],
            blur: 0.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let cx = 200u32;
    let cy = 200u32;
    // Content center stays red.
    assert_px(&img, cx, cy, [255, 0, 0, 255], 3);
    // Inside the offset silhouette (center + 75, center), outside the red quad.
    assert_px(&img, cx + 75, cy, [0, 0, 0, 255], 3);
    // Far corner still white.
    assert_px(&img, 10, 10, [255, 255, 255, 255], 3);
}

#[test]
fn shadow_blur_softens_silhouette_edge() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([255, 255, 255, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        shadow: Some(LayerShadow {
            rgba: [0, 0, 0, 255],
            offset: [50.0, 0.0],
            blur: 12.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    // Sharp silhouette right edge is at x = 200+50+50 = 300. Probe just outside.
    let probe_x = 302u32;
    let probe_y = 200u32;
    let got = img.pixel(probe_x, probe_y);
    assert_not_px(&img, probe_x, probe_y, [255, 255, 255, 255], 3);
    assert_not_px(&img, probe_x, probe_y, [0, 0, 0, 255], 3);
    // Partial darkening: channels between white and black.
    assert!(
        got[0] > 3 && got[0] < 252,
        "expected partial shadow darkening, got {got:?}"
    );
}

#[test]
fn glow_extends_outside_content_on_black() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        glow: Some(LayerGlow {
            rgba: [255, 255, 255, 255],
            radius: 10.0,
            intensity: 2.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let cx = 200u32;
    let cy = 200u32;
    // Content remains red-ish (glow under content).
    let center = img.pixel(cx, cy);
    assert!(
        center[0] > 200 && center[1] < 80 && center[2] < 80,
        "content should stay red-ish, got {center:?}"
    );
    // 4px outside the content edge (right edge at x=250).
    let glow_px = img.pixel(254, cy);
    assert!(
        glow_px[0] > 3 || glow_px[1] > 3 || glow_px[2] > 3,
        "expected non-black glow outside content, got {glow_px:?}"
    );
}

#[test]
fn empty_styles_matches_styles_free_render() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let layer = centered_red_layer(&config);
    let control = comp.render(&gpu, &config, &[layer]).expect("control");

    let layer_empty = centered_red_layer(&config).with_styles(LayerStyles::default());
    let with_empty = comp
        .render(&gpu, &config, &[layer_empty])
        .expect("empty styles");

    assert_px(&with_empty, 200, 200, control.pixel(200, 200), 3);
    assert_px(&with_empty, 10, 10, control.pixel(10, 10), 3);
    assert_px(&with_empty, 250, 200, control.pixel(250, 200), 3);
}

#[test]
fn outline_white_ring_outside_red_on_black() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        outline: Some(LayerOutline {
            rgba: [255, 255, 255, 255],
            width: 6.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let cx = 200u32;
    let cy = 200u32;
    // Content center stays red.
    assert_px(&img, cx, cy, [255, 0, 0, 255], 3);
    // ~2–3px outside the right content edge (edge near x=250). With
    // smoothstep(0.02, 0.35) harden, full white covers the inner ring.
    let ring = img.pixel(252, cy);
    assert!(
        ring[0] > 200 && ring[1] > 200 && ring[2] > 200,
        "expected white-ish outline ring, got {ring:?}"
    );
    // Well outside the dilation.
    assert_px(&img, cx + 80, cy, [0, 0, 0, 255], 3);
}

#[test]
fn background_plate_padded_white_behind_red() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        background: Some(LayerBackground {
            rgba: [255, 255, 255, 255],
            padding: 20.0,
            radius: 0.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    let cx = 200u32;
    let cy = 200u32;
    assert_px(&img, cx, cy, [255, 0, 0, 255], 3);
    // Content right edge at 250; +10px is inside the padding plate.
    assert_px(&img, 260, cy, [255, 255, 255, 255], 3);
    // Far corner stays black.
    assert_px(&img, 10, 10, [0, 0, 0, 255], 3);
}

#[test]
fn background_plate_rotates_with_layer() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let mut layer = centered_red_layer(&config);
    layer.placement.rotation = std::f32::consts::FRAC_PI_4;
    let layer = layer.with_styles(LayerStyles {
        background: Some(LayerBackground {
            rgba: [255, 255, 255, 255],
            padding: 20.0,
            radius: 0.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    // Unrotated plate corner (130,130) lies outside the 45°-rotated plate.
    assert_px(&img, 130, 130, [0, 0, 0, 255], 3);
    // Content center still red.
    assert_px(&img, 200, 200, [255, 0, 0, 255], 3);
}

#[test]
fn shadow_outline_background_combined() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(400, 400).with_background([0, 0, 0, 255]);
    let layer = centered_red_layer(&config).with_styles(LayerStyles {
        shadow: Some(LayerShadow {
            rgba: [0, 0, 0, 180],
            offset: [8.0, 8.0],
            blur: 4.0,
        }),
        outline: Some(LayerOutline {
            rgba: [255, 255, 255, 255],
            width: 4.0,
        }),
        background: Some(LayerBackground {
            rgba: [40, 40, 40, 255],
            padding: 12.0,
            radius: 4.0,
        }),
        ..Default::default()
    });
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    assert_px(&img, 200, 200, [255, 0, 0, 255], 3);
}

#[test]
fn nested_effect_style_and_blend_chain_runs_without_collision() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(200, 200).with_background([20, 20, 20, 255]);

    // Nested chain: vignette + gaussian_blur + sharpen + shadow + glow + outline + Multiply blend
    let effects = [
        PassInstance::new("vignette", vec![0.5]),
        PassInstance::new("gaussian_blur", vec![4.0]),
        PassInstance::new("sharpen", vec![0.5]),
    ];

    let layer = centered_red_layer(&config)
        .with_effects(&effects)
        .with_styles(LayerStyles {
            shadow: Some(LayerShadow {
                rgba: [0, 0, 0, 200],
                offset: [10.0, 10.0],
                blur: 8.0,
            }),
            glow: Some(LayerGlow {
                rgba: [255, 255, 0, 255],
                radius: 6.0,
                intensity: 1.5,
            }),
            outline: Some(LayerOutline {
                rgba: [255, 255, 255, 255],
                width: 4.0,
            }),
            background: Some(LayerBackground {
                rgba: [0, 0, 255, 255],
                padding: 10.0,
                radius: 2.0,
            }),
        })
        .with_blend_mode(BlendMode::Multiply);

    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("nested chain render");
    assert_eq!(img.width, 200);
    assert_eq!(img.height, 200);
    // Verify valid rendered alpha
    assert_eq!(img.pixel(100, 100)[3], 255);
}

#[test]
fn style_blur_reduces_uniform_allocations_via_slot_reuse() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(200, 200).with_background([0, 0, 0, 255]);

    // Iterated style blur (radius 32 = 1 iter = 2 passes)
    let layer = || {
        centered_red_layer(&config).with_styles(LayerStyles {
            shadow: Some(LayerShadow {
                rgba: [0, 0, 0, 255],
                offset: [0.0, 0.0],
                blur: 32.0,
            }),
            ..Default::default()
        })
    };

    comp.render(&gpu, &config, &[layer()]).expect("warm frame");
    let warm_stats = comp.pass_stats();
    // All blur passes share identical uniforms, so uniform slots created must be minimal
    assert!(
        warm_stats.uniform_buffers_created < warm_stats.passes,
        "uniform reuse must allocate fewer buffers than passes: {warm_stats:?}"
    );

    comp.render(&gpu, &config, &[layer()])
        .expect("steady frame");
    let steady_stats = comp.pass_stats();
    assert_eq!(
        steady_stats.uniform_buffers_created, 0,
        "steady frame must allocate zero new uniform buffers: {steady_stats:?}"
    );
}

/// WHY: the catalog blur shaders used to spend one sample per texel, capping a
/// pass at 16 texels, so `run_style_blur` needed `ceil(radius/16)` clamped to 3
/// iterations and a 48 px shadow cost six render passes. Bilinear midpoint
/// pairing reads two texels per sample, so one pass now covers 32 texels with
/// the same 33 samples and the same clamped-edge box support, and the cap radius
/// costs four passes. This pins the pass count so restoring the per-texel loop,
/// or reverting the `/32` iteration basis, goes red instead of quietly doubling
/// the offscreen traffic of every shadow and glow.
///
/// It does not check the sampled weights: an implementation that paired the
/// wrong midpoints would keep this count and blur unevenly. `RgbaImage`
/// readback in the surrounding tests covers the visual result.
#[test]
fn the_style_blur_cap_radius_costs_one_extra_pass_pair_over_a_single_pass_radius() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(200, 200).with_background([0, 0, 0, 255]);

    let shadowed = |blur: f32| {
        centered_red_layer(&config).with_styles(LayerStyles {
            shadow: Some(LayerShadow {
                rgba: [0, 0, 0, 255],
                offset: [0.0, 0.0],
                blur,
            }),
            ..Default::default()
        })
    };

    // 32 px is exactly one pass of support; 48 px is the clamp ceiling and needs
    // a second H+V pair. Every other pass in the frame is identical.
    comp.render(&gpu, &config, &[shadowed(32.0)])
        .expect("single-iteration blur renders");
    let one_pair = comp.pass_stats().passes;
    comp.render(&gpu, &config, &[shadowed(48.0)])
        .expect("two-iteration blur renders");
    let two_pairs = comp.pass_stats().passes;

    assert_eq!(
        two_pairs - one_pair,
        2,
        "the cap radius must add exactly one H+V pair over a single-pass radius, \
         not two: {one_pair} then {two_pairs} passes"
    );

    // A radius under the single-pass width must not iterate at all.
    comp.render(&gpu, &config, &[shadowed(4.0)])
        .expect("small blur renders");
    assert_eq!(
        comp.pass_stats().passes,
        one_pair,
        "a 4 px blur must cost the same one pair as a 32 px blur"
    );
}
