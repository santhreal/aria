//! GPU readback checks for catalog effect passes.
//!
//! Every id in `shipped_effect_descriptors()` has a WGSL pass, so each one is checked
//! here by what it does to a known plate rather than by whether it dispatches.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CpuFrameIn, CpuFrameOut, EffectDefinition,
    EffectParams, GpuContext, LayerMask, LayerPlacement, MaskOp, PassInstance, RgbaImage,
    StencilCoverage, StencilMask, mask_kind, pass_id, register_effect,
};
use aria_models::EffectSpec;

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping effects test: no GPU adapter ({e})");
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

fn horizontal_gradient(width: u32, height: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let t = if width <= 1 {
                0.0
            } else {
                x as f32 / (width - 1) as f32
            };
            let v = (t * 255.0).round() as u8;
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    RgbaImage::new(width, height, pixels)
}

#[test]
fn duotone_maps_black_and_white_to_shadow_and_highlight() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 16).with_background([0, 0, 0, 255]);
    let bmp = horizontal_gradient(64, 16);

    // shadow=[20,16,60,255], highlight=[255,220,160,255], intensity=1
    let params = [
        20.0 / 255.0,
        16.0 / 255.0,
        60.0 / 255.0,
        1.0,
        255.0 / 255.0,
        220.0 / 255.0,
        160.0 / 255.0,
        1.0,
        1.0,
    ];
    let effects = [PassInstance::new("duotone", params.to_vec())];
    let layer =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("duotone");

    assert_px(&img, 0, 8, [20, 16, 60, 255], 3);
    assert_px(&img, 63, 8, [255, 220, 160, 255], 3);
}

#[test]
fn color_overlay_amount_one_tints_gray_red() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 0, 255]);

    // color=red opaque, offset=0, amount=1 → full red overlay
    let params = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let effects = [PassInstance::new("color_overlay", params.to_vec())];
    let layer = CompositeLayer::solid([128, 128, 128, 255], LayerPlacement::full_canvas(&config))
        .with_effects(&effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("overlay");

    assert_px(&img, 8, 8, [255, 0, 0, 255], 3);
}

#[test]
fn color_overlay_amount_zero_leaves_gray_unchanged() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 0, 255]);

    // amount=0 → noop (skipped by effect_is_noop)
    let params = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    let effects = [PassInstance::new("color_overlay", params.to_vec())];
    let layer = CompositeLayer::solid([128, 128, 128, 255], LayerPlacement::full_canvas(&config))
        .with_effects(&effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("overlay zero");

    assert_px(&img, 8, 8, [128, 128, 128, 255], 2);
}

/// Two flat halves split down the middle, so an edge-sensitive pass has an edge
/// to work on and both sides stay away from the clamp.
fn vertical_step(width: u32, height: u32, left: u8, right: u8) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let v = if x < width / 2 { left } else { right };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    RgbaImage::new(width, height, pixels)
}

/// A white block centered in an opaque black frame.
fn center_block(width: u32, height: u32, block: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let (x0, x1) = (width / 2 - block / 2, width / 2 + block / 2);
    let (y0, y1) = (height / 2 - block / 2, height / 2 + block / 2);
    for y in 0..height {
        for x in 0..width {
            let lit = x >= x0 && x < x1 && y >= y0 && y < y1;
            let v = if lit { 255 } else { 0 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    RgbaImage::new(width, height, pixels)
}

fn render_effect(
    gpu: &GpuContext,
    comp: &mut Compositor,
    config: &CompositorConfig,
    bmp: &RgbaImage,
    id: &'static str,
    params: &[f32],
) -> RgbaImage {
    let effects = [PassInstance::new(id, params.to_vec())];
    let layer =
        CompositeLayer::rgba(bmp, LayerPlacement::full_canvas(config)).with_effects(&effects);
    comp.render(gpu, config, &[layer]).expect(id)
}

#[test]
fn sharpen_overshoots_on_both_sides_of_an_edge() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 16).with_background([0, 0, 0, 255]);
    let bmp = vertical_step(64, 16, 100, 160);

    let img = render_effect(&gpu, &mut comp, &config, &bmp, "sharpen", &[1.0]);

    // Unsharp masking darkens the dark side of an edge and brightens the light
    // side; away from the edge a flat region is unchanged.
    let dark_side = img.pixel(31, 8)[0];
    let light_side = img.pixel(32, 8)[0];
    assert!(
        dark_side < 100,
        "dark side of the edge did not undershoot: {dark_side}"
    );
    assert!(
        light_side > 160,
        "light side of the edge did not overshoot: {light_side}"
    );
    assert_px(&img, 8, 8, [100, 100, 100, 255], 2);
    assert_px(&img, 55, 8, [160, 160, 160, 255], 2);
}

#[test]
fn chromatic_aberration_splits_channels_in_opposite_directions() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 16).with_background([0, 0, 0, 255]);
    let bmp = horizontal_gradient(64, 16);

    let img = render_effect(
        &gpu,
        &mut comp,
        &config,
        &bmp,
        "chromatic_aberration",
        &[1.0],
    );

    // Red samples outward from the center, blue inward. On a dark-to-bright ramp
    // that means red leads blue right of center and trails it on the left.
    let right = img.pixel(56, 8);
    let left = img.pixel(8, 8);
    assert!(
        right[0] > right[2],
        "right of center: red {} should lead blue {}",
        right[0],
        right[2]
    );
    assert!(
        left[0] < left[2],
        "left of center: red {} should trail blue {}",
        left[0],
        left[2]
    );
    // The center column has nowhere to split.
    let center = img.pixel(32, 8);
    assert_eq!(center[0], center[2], "center column split anyway");
}

#[test]
fn grain_is_seeded_noise_around_the_source_value() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(32, 32).with_background([0, 0, 0, 255]);
    let bmp = vertical_step(32, 32, 128, 128);

    let seeded = render_effect(&gpu, &mut comp, &config, &bmp, "grain", &[1.0, 7.0]);
    let again = render_effect(&gpu, &mut comp, &config, &bmp, "grain", &[1.0, 7.0]);
    let other = render_effect(&gpu, &mut comp, &config, &bmp, "grain", &[1.0, 8.0]);

    assert_eq!(
        seeded.pixels, again.pixels,
        "one seed must render one grain pattern"
    );
    assert_ne!(
        seeded.pixels, other.pixels,
        "a different seed rendered the same grain"
    );

    // A flat plate stops being flat, and the mean stays near the source.
    let lit: Vec<u8> = seeded
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0])
        .collect();
    let min = *lit.iter().min().expect("pixels");
    let max = *lit.iter().max().expect("pixels");
    assert!(max > min + 8, "grain left the plate flat: {min}…{max}");
    let mean = lit.iter().map(|&v| f64::from(v)).sum::<f64>() / lit.len() as f64;
    assert!(
        (mean - 128.0).abs() < 8.0,
        "grain shifted the exposure: mean {mean}"
    );
}

#[test]
fn glow_spills_light_outside_a_highlight() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let bmp = center_block(64, 64, 16);

    let img = render_effect(&gpu, &mut comp, &config, &bmp, "glow", &[0.5, 2.0]);

    // Four texels outside the block was pure black in the source.
    let near = img.pixel(20, 32)[0];
    assert!(near > 8, "no spill outside the highlight: {near}");
    // Far away stays black: the spill has a radius, it is not a lift.
    assert_px(&img, 1, 1, [0, 0, 0, 255], 2);
}

#[test]
fn zoom_blur_streaks_a_highlight_toward_the_center() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let bmp = center_block(64, 64, 16);

    let img = render_effect(&gpu, &mut comp, &config, &bmp, "zoom_blur", &[1.0]);

    // Just outside the block, on the radius through it, samples march inward and
    // hit the block, so the streak reaches past the block's edge. The march is
    // 30% of the distance to the center, so the reach at two texels out is a
    // little under three texels.
    let outside = img.pixel(32, 22)[0];
    assert!(outside > 8, "no streak outside the block: {outside}");
    // The center is its own march origin and stays lit.
    assert!(img.pixel(32, 32)[0] > 200, "the center lost its highlight");
}

#[test]
fn mirror_copies_one_half_onto_the_other() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 16).with_background([0, 0, 0, 255]);
    let bmp = horizontal_gradient(64, 16);

    // Mode 0: the dark left half is mirrored onto the right.
    let left_onto_right = render_effect(&gpu, &mut comp, &config, &bmp, "mirror", &[0.0]);
    assert_px(&left_onto_right, 0, 8, [0, 0, 0, 255], 2);
    assert_px(&left_onto_right, 63, 8, [0, 0, 0, 255], 2);

    // Mode 1: the bright right half is mirrored onto the left.
    let right_onto_left = render_effect(&gpu, &mut comp, &config, &bmp, "mirror", &[1.0]);
    assert_px(&right_onto_left, 0, 8, [255, 255, 255, 255], 2);
    assert_px(&right_onto_left, 63, 8, [255, 255, 255, 255], 2);

    // Modes 2 and 3 work on the other axis, so a horizontal ramp is untouched.
    let top_onto_bottom = render_effect(&gpu, &mut comp, &config, &bmp, "mirror", &[2.0]);
    assert_px(&top_onto_bottom, 0, 8, [0, 0, 0, 255], 2);
    assert_px(&top_onto_bottom, 63, 8, [255, 255, 255, 255], 2);
}

#[test]
fn glitch_tears_bands_and_repeats_for_one_seed() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let bmp = vertical_step(64, 64, 0, 255);

    let torn = render_effect(&gpu, &mut comp, &config, &bmp, "glitch", &[1.0, 3.0]);
    let again = render_effect(&gpu, &mut comp, &config, &bmp, "glitch", &[1.0, 3.0]);
    let other = render_effect(&gpu, &mut comp, &config, &bmp, "glitch", &[1.0, 4.0]);
    let clean = render_effect(&gpu, &mut comp, &config, &bmp, "glitch", &[0.0, 3.0]);

    assert_eq!(
        torn.pixels, again.pixels,
        "one seed must render one tear pattern"
    );
    assert_ne!(torn.pixels, other.pixels, "a different seed tore the same");
    assert_ne!(torn.pixels, clean.pixels, "nothing moved at full amount");

    // Amount zero is skipped as a no-op, so the plate comes through as it went in.
    assert_px(&clean, 8, 8, [0, 0, 0, 255], 2);
    assert_px(&clean, 56, 8, [255, 255, 255, 255], 2);

    // The edge moved on at least one row: some row's midpoint is no longer the
    // value the untouched plate has there.
    let moved = (0..64).any(|y| torn.pixel(32, y)[0] != 255);
    assert!(moved, "the edge did not move on any row");
}

/// Closes the class "an effect's compositing opacity is ignored by GPU or CPU
/// passes, or adds slot overhead at 100%".
#[test]
fn effect_opacity_mixes_dry_and_wet_across_gpu_and_cpu_passes() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 0, 255]);
    let dry = [0u8, 100, 200, 255];
    let wet = [255u8, 0, 0, 255];
    let half = [128u8, 50, 100, 255];

    const CPU_RED: &str = "test.compositing.cpu_red";
    let _cpu_reg = register_effect(EffectDefinition::cpu(
        EffectSpec {
            id: CPU_RED.into(),
            label: "CPU Red".into(),
            ..Default::default()
        },
        |input: CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>, _: &EffectParams<'_>, _: f32| {
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let [_, _, _, a] = input.pixel_f32(x, y);
                    output.set_pixel_f32(x, y, [a, 0.0, 0.0, a]);
                }
            }
            Ok(())
        },
    ))
    .expect("register CPU red effect");

    let gpu_base = PassInstance::new("color_overlay", vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let cpu_base = PassInstance::new(pass_id(CPU_RED).expect("cpu pass id"), Vec::new());

    for (label, base) in [("gpu", gpu_base), ("cpu", cpu_base)] {
        assert_eq!(
            base.slot_demand(),
            2,
            "{label}: opacity 1.0 unmasked must demand 2 slots"
        );
        let zero = base.clone().with_opacity(0.0);
        assert!(zero.is_noop(), "{label}: opacity 0.0 must be a no-op");

        let half_fx = [base.clone().with_opacity(0.5)];
        assert_eq!(half_fx[0].slot_demand(), 3);
        let layer =
            CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&half_fx);
        let img = comp.render(&gpu, &config, &[layer]).expect("render half");
        assert_px(&img, 8, 8, half, 3);

        let full_fx = [base.with_opacity(1.0)];
        let layer =
            CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&full_fx);
        let img = comp.render(&gpu, &config, &[layer]).expect("render full");
        assert_px(&img, 8, 8, wet, 3);
    }
}

/// Closes the class "a masked effect cuts the underlying layer outside its
/// mask, ignores `invert_masks`, or fails to combine multiple masks across
/// `MaskMode`s".
#[test]
fn effect_mask_scope_confines_pass_and_inversion_flips_it() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 16).with_background([0, 0, 0, 255]);
    let dry = [0u8, 100, 200, 255];
    let wet = [255u8, 0, 0, 255];
    let half = [128u8, 50, 100, 255];

    // Left half of the canvas: center = [-0.25, 0.0] (-0.25 * width), half-extents = [0.5, 1.0].
    let left_mask = StencilMask::keep(StencilCoverage::Shape(LayerMask {
        center: [-0.25, 0.0],
        size: [0.5, 1.0],
        ..LayerMask::new(mask_kind::RECTANGLE)
    }));
    let base = PassInstance::new("color_overlay", vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);

    // Uninverted: left is wet, right is dry (and still opaque 255, not cut!).
    let masked = [base.clone().with_masks(vec![left_mask], Vec::new(), false)];
    let layer =
        CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&masked);
    let img = comp.render(&gpu, &config, &[layer]).expect("render masked");
    assert_px(&img, 8, 8, wet, 3);
    assert_px(&img, 56, 8, dry, 3);

    // Inverted: left is dry, right is wet.
    let inverted = [base.clone().with_masks(vec![left_mask], Vec::new(), true)];
    let layer =
        CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&inverted);
    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("render inverted");
    assert_px(&img, 8, 8, dry, 3);
    assert_px(&img, 56, 8, wet, 3);

    // Masked + 50% opacity: left is 50/50 mix, right is dry.
    let masked_half =
        [base
            .clone()
            .with_opacity(0.5)
            .with_masks(vec![left_mask], Vec::new(), false)];
    let layer =
        CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&masked_half);
    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("render masked half");
    assert_px(&img, 8, 8, half, 3);
    assert_px(&img, 56, 8, dry, 3);

    // Every combinable MaskMode on a second mask over the full canvas executes
    // cleanly and obeys its mode semantics over the left/right halves.
    for op in MaskOp::ALL {
        let mut second =
            StencilMask::keep(StencilCoverage::Shape(LayerMask::new(mask_kind::RECTANGLE)));
        second.op = op;
        let fx = [base
            .clone()
            .with_masks(vec![left_mask, second], Vec::new(), false)];
        let layer =
            CompositeLayer::solid(dry, LayerPlacement::full_canvas(&config)).with_effects(&fx);
        let img = comp.render(&gpu, &config, &[layer]).expect("mode render");
        match op {
            MaskOp::Add | MaskOp::Lighten => {
                assert_px(&img, 8, 8, wet, 3);
                assert_px(&img, 56, 8, wet, 3);
            }
            MaskOp::Subtract => {
                assert_px(&img, 8, 8, dry, 3);
                assert_px(&img, 56, 8, dry, 3);
            }
            MaskOp::Intersect | MaskOp::Darken => {
                assert_px(&img, 8, 8, wet, 3);
                assert_px(&img, 56, 8, dry, 3);
            }
            MaskOp::Difference => {
                assert_px(&img, 8, 8, dry, 3);
                assert_px(&img, 56, 8, wet, 3);
            }
        }
    }
}
