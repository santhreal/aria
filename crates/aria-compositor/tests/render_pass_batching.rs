//! What a frame's passes cost, and that sharing a pass's bind group across
//! layers does not share its pixels.
//!
//! A pass is (layout, bound resources). Passes agreeing on both are the same
//! binding, so one bind group serves them all — the binding is shared, the draw
//! is not. These check both halves: the accounting (`Compositor::pass_stats`)
//! and the picture.
//!
//! The counts are asserted as invariants rather than as numbers: what a frame
//! costs per pass family is an implementation detail, but a repeated frame
//! building nothing, and a build count that does not grow with the layers, are
//! the contract the cache exists for. A number here would be re-blessed every
//! time a pass family joined, which is how a count stops meaning anything.

use super::*;
use aria_compositor::PassStats;

/// A scene of `count` full-canvas plates, each with the same effect and params.
/// Every plate reads pool slot 0, so every pass shares one descriptor.
fn same_effect_plates<'a>(
    config: &CompositorConfig,
    effects: &'a [PassInstance],
    count: usize,
) -> Vec<CompositeLayer<'a>> {
    (0..count)
        .map(|i| {
            let shade = 40 + (i as u8) * 10;
            CompositeLayer::solid(
                [shade, shade, shade, 255],
                LayerPlacement::full_canvas(config),
            )
            .with_effects(effects)
        })
        .collect()
}

/// One frame of `layers` on a fresh compositor, and what its passes cost.
fn cold_frame(
    gpu: &GpuContext,
    config: &CompositorConfig,
    layers: &[CompositeLayer<'_>],
) -> PassStats {
    let mut comp = Compositor::new(gpu);
    comp.render(gpu, config, layers).expect("frame");
    comp.pass_stats()
}

#[test]
fn pass_batching_shares_one_bind_group_across_identical_passes() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let effects = [PassInstance::new("vignette", vec![0.8])];
    let layers = same_effect_plates(&config, &effects, 8);

    comp.render(&gpu, &config, &layers).expect("first frame");
    let first = comp.pass_stats();
    assert!(
        first.passes >= 8,
        "one pass per plate at least, got {first:?}"
    );
    assert!(
        first.reused >= 7,
        "eight identical plates must reuse, got {first:?}"
    );
    assert!(
        first.created < first.passes,
        "sharing must build fewer descriptors than it draws passes, got {first:?}"
    );

    comp.render(&gpu, &config, &layers).expect("second frame");
    let second = comp.pass_stats();
    assert_eq!(second.passes, first.passes);
    assert_eq!(
        second.created, 0,
        "a repeated frame builds nothing, got {second:?}"
    );
    assert_eq!(
        second.reused, second.passes,
        "every pass of a repeated frame is a reuse, got {second:?}"
    );
    assert_eq!(second.entries, first.created);
}

#[test]
fn generated_layer_bindings_are_reused_after_warmup() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let layers = [CompositeLayer::solid(
        [64, 128, 192, 255],
        LayerPlacement::full_canvas(&config),
    )];
    let mut comp = Compositor::new(&gpu);

    comp.render(&gpu, &config, &layers).expect("warm layer");
    let warm = comp.layer_pass_stats();
    assert_eq!(warm.passes, 1);
    assert_eq!(warm.created, 1);

    comp.render(&gpu, &config, &layers).expect("steady layer");
    let steady = comp.layer_pass_stats();
    assert_eq!(steady.passes, 1);
    assert_eq!(
        steady.created, 0,
        "a stable generated layer rebuilt its bind group: {steady:?}"
    );
    assert_eq!(steady.uniform_buffers_created, 0);
}

/// WHY: decoded CPU frames arrive with new pixel bytes on every tick, but their
/// dimension/format storage is stable. Cover every shipped 8-bit YUV layout so
/// adding another layout requires an explicit pooling decision.
#[test]
fn decoded_plane_textures_are_rewritten_without_reallocation() {
    let gpu = gpu_or_skip!();
    for format in aria_core::every_pixel_format() {
        let make_frame = |luma| match format {
            PixelFormat::Nv12 => Some(nv12_uniform(64, 32, luma, 128, 128, ColorSpace::default())),
            PixelFormat::Yuv420p => {
                Some(i420_uniform(64, 32, luma, 128, 128, ColorSpace::default()))
            }
            PixelFormat::P010 | PixelFormat::Rgba8 | PixelFormat::Bgra8 => None,
        };
        let Some(warm_frame) = make_frame(64) else {
            continue;
        };
        let steady_frame = make_frame(192).expect("supported format stays supported");
        let mut comp = Compositor::new(&gpu);

        let warm_image = render_frame(&gpu, &mut comp, 64, 32, &warm_frame);
        let warm = comp.plane_pool_stats();
        assert_eq!(warm.textures_created as usize, format.plane_count());
        assert_eq!(warm.retained_textures, format.plane_count());

        let steady_image = render_frame(&gpu, &mut comp, 64, 32, &steady_frame);
        let steady = comp.plane_pool_stats();
        assert_eq!(
            steady.textures_created, 0,
            "{format:?} allocated plane textures after warm-up: {steady:?}"
        );
        assert_eq!(steady.retained_textures, warm.retained_textures);
        assert_ne!(
            steady_image.pixel(0, 0),
            warm_image.pixel(0, 0),
            "retained {format:?} textures were not rewritten"
        );
    }
}

/// WHY: titles, shapes, stickers, and stills reuse immutable rasters over many
/// frames. The authored-pixel digest must retain both the upload and its binding,
/// while different pixels at the same dimensions must not alias.
#[test]
fn static_rgba_textures_upload_once_per_image_digest() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 32).with_background([0, 0, 0, 255]);
    let first_image = rgba_uniform(64, 32, [24, 80, 160, 255]);
    let changed_image = rgba_uniform(64, 32, [192, 48, 16, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let mut comp = Compositor::new(&gpu);

    let first_layer = [CompositeLayer::rgba(&first_image, placement)];
    let first = comp
        .render(&gpu, &config, &first_layer)
        .expect("first static bitmap");
    assert_eq!(comp.rgba_cache_stats().textures_created, 1);

    let repeated = comp
        .render(&gpu, &config, &first_layer)
        .expect("repeated static bitmap");
    let steady = comp.rgba_cache_stats();
    assert_eq!(
        steady.textures_created, 0,
        "an unchanged bitmap was uploaded again: {steady:?}"
    );
    assert_eq!(steady.retained_textures, 1);
    assert_eq!(
        comp.layer_pass_stats().created,
        0,
        "the retained bitmap view did not retain its bind group"
    );
    assert_eq!(repeated, first);

    let changed_layer = [CompositeLayer::rgba(&changed_image, placement)];
    let changed = comp
        .render(&gpu, &config, &changed_layer)
        .expect("changed static bitmap");
    assert_eq!(comp.rgba_cache_stats().textures_created, 1);
    assert_eq!(comp.rgba_cache_stats().retained_textures, 2);
    assert_ne!(
        changed.pixel(0, 0),
        first.pixel(0, 0),
        "same-size images with different digests aliased"
    );
}

/// WHY: export must submit the next GPU frame before copying and encoding the
/// prior one. Two retained slots preserve order and pixels; a third unresolved
/// frame fails with bounded backpressure instead of allocating without limit.
#[test]
fn two_readbacks_pipeline_in_order_and_bound_in_flight_work() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 32).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let red = [CompositeLayer::solid([255, 0, 0, 255], placement)];
    let blue = [CompositeLayer::solid([0, 0, 255, 255], placement)];
    let green = [CompositeLayer::solid([0, 255, 0, 255], placement)];
    let mut comp = Compositor::new(&gpu);

    let first = comp.submit(&gpu, &config, &red).expect("first submit");
    let second = comp.submit(&gpu, &config, &blue).expect("second submit");
    assert!(matches!(
        comp.submit(&gpu, &config, &green),
        Err(CompositorError::ReadbackBusy)
    ));

    let first = first.into_image().expect("first readback");
    assert_px(&first, 0, 0, [255, 0, 0, 255], 0);
    let third = comp
        .submit(&gpu, &config, &green)
        .expect("released first slot");
    let second = second.into_image().expect("second readback");
    let third = third.into_image().expect("third readback");
    assert_px(&second, 0, 0, [0, 0, 255, 255], 0);
    assert_px(&third, 0, 0, [0, 255, 0, 255], 0);
}

/// What the cache is for: the descriptors a frame builds are a property of the
/// distinct bindings in it, not of how many layers carry them.
#[test]
fn pass_batching_builds_no_more_for_more_layers() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let effects = [PassInstance::new("vignette", vec![0.8])];

    let few = cold_frame(&gpu, &config, &same_effect_plates(&config, &effects, 2));
    let many = cold_frame(&gpu, &config, &same_effect_plates(&config, &effects, 12));

    assert!(many.passes > few.passes, "twelve plates draw more passes");
    assert_eq!(
        many.created, few.created,
        "the builds must not grow with the layers: {few:?} then {many:?}"
    );
    assert_eq!(many.reused, many.passes - many.created);
}

#[test]
fn pass_batching_keys_on_the_constants() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    // Same pipeline, same input slot, four different strengths: four passes
    // that are not the same pass, against four that are.
    let strengths = [0.2f32, 0.4, 0.6, 0.8];
    let distinct: Vec<_> = strengths
        .iter()
        .map(|s| {
            [PassInstance::new("vignette", vec![*s])]
        })
        .collect();
    let layers: Vec<_> = distinct
        .iter()
        .map(|fx| {
            CompositeLayer::solid([200, 200, 200, 255], LayerPlacement::full_canvas(&config))
                .with_effects(fx.as_slice())
        })
        .collect();
    let varied = cold_frame(&gpu, &config, &layers);

    let same = [PassInstance::new("vignette", vec![0.2])];
    let flat = cold_frame(&gpu, &config, &same_effect_plates(&config, &same, 4));

    assert_eq!(
        varied.passes, flat.passes,
        "the same scene shape either way"
    );
    assert_eq!(
        varied.created,
        flat.created + 3,
        "four strengths are four descriptors where one strength is one: {flat:?} then {varied:?}"
    );
}

#[test]
fn animated_uniforms_rewrite_persistent_storage_without_reallocation() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let mut comp = Compositor::new(&gpu);

    let first_effect = [PassInstance::new("vignette", vec![0.2])];
    let first = [
        CompositeLayer::solid([200, 200, 200, 255], LayerPlacement::full_canvas(&config))
            .with_effects(&first_effect),
    ];
    comp.render(&gpu, &config, &first).expect("warm frame");
    let warm = comp.pass_stats();
    assert!(
        warm.uniform_buffers_created > 0,
        "the first uniform pass allocated no persistent slot: {warm:?}"
    );

    let changed_effect = [PassInstance::new("vignette", vec![0.8])];
    let changed =
        [
            CompositeLayer::solid([200, 200, 200, 255], LayerPlacement::full_canvas(&config))
                .with_effects(&changed_effect),
        ];
    comp.render(&gpu, &config, &changed)
        .expect("animated frame");
    let animated = comp.pass_stats();

    assert_eq!(
        animated.uniform_buffers_created, 0,
        "animated constants allocated a new GPU buffer: {animated:?}"
    );
    assert_eq!(
        animated.created, 0,
        "the persistent uniform binding rebuilt its bind group: {animated:?}"
    );
    assert!(
        animated.uniform_writes > 0,
        "the changed constants never reached the persistent buffer: {animated:?}"
    );
}

#[test]
fn pass_batching_draws_each_layer_from_its_own_pixels() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // Two plates, same effect and params, different colors, side by side. A
    // shared bind group must not make the second read the first's texels.
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let effects = [PassInstance::new("sharpen", vec![0.5])];
    let half = [32.0, 64.0];
    let left = CompositeLayer::solid(
        [200, 0, 0, 255],
        LayerPlacement {
            center: [16.0, 32.0],
            size: half,
            ..LayerPlacement::full_canvas(&config)
        },
    )
    .with_effects(&effects);
    let right = CompositeLayer::solid(
        [0, 0, 200, 255],
        LayerPlacement {
            center: [48.0, 32.0],
            size: half,
            ..LayerPlacement::full_canvas(&config)
        },
    )
    .with_effects(&effects);

    let img = comp
        .render(&gpu, &config, &[left, right])
        .expect("two plates");
    let stats = comp.pass_stats();
    assert!(
        stats.reused > 0,
        "the two effect passes are one descriptor, got {stats:?}"
    );

    let l = img.pixel(16, 32);
    let r = img.pixel(48, 32);
    assert!(
        l[0] > 150 && l[2] < 60,
        "left plate must stay red, got {l:?}"
    );
    assert!(
        r[2] > 150 && r[0] < 60,
        "right plate must stay blue, got {r:?}"
    );
}

/// Every family of pass the cache covers, driven one scene at a time: a family
/// whose descriptors are keyed on something that changes every frame rebuilds
/// them, which the second frame of each scene here refuses.
///
/// What this cannot see is a family that never asks the cache at all, since a
/// pass outside the cache is outside the counts. That case is closed in the
/// crate's own `effect_render::pass_census`, which allows `create_bind_group`
/// in one file.
#[test]
fn pass_batching_covers_every_family_of_pass() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 0, 255]);
    let plate = || CompositeLayer::solid([180, 120, 60, 255], LayerPlacement::full_canvas(&config));
    let effects = [PassInstance::new("vignette", vec![0.5])];

    // Effect chain, blit and canvas grade.
    let graded = plate();
    let grade = ColorGrade {
        saturation: -0.5,
        ..ColorGrade::IDENTITY
    };
    let canvas_pass = [
        CompositorLayer::layer(&graded),
        CompositorLayer::CanvasPass {
            effects: &effects,
            grade: Some(grade),
            lut: None,
        },
    ];

    // Silhouette, style blur, offset blit, additive blit and harden.
    let outlined = plate().with_styles(LayerStyles {
        shadow: Some(LayerShadow {
            rgba: [0, 0, 0, 200],
            offset: [3.0, 3.0],
            blur: 4.0,
        }),
        glow: Some(LayerGlow {
            rgba: [0, 255, 255, 200],
            radius: 4.0,
            intensity: 1.2,
        }),
        outline: Some(LayerOutline {
            rgba: [255, 255, 0, 255],
            width: 3.0,
        }),
        background: None,
    });
    let styled = [CompositorLayer::layer(&outlined)];

    // Blend composite: a non-`Normal` mode snapshots the canvas and blends.
    let under = plate();
    let over = plate().with_blend_mode(BlendMode::Screen);
    let blended = [
        CompositorLayer::layer(&under),
        CompositorLayer::layer(&over),
    ];

    // Transition, then the same two plates as a matte.
    let out_plate = plate();
    let in_plate = CompositeLayer::solid([20, 40, 200, 255], LayerPlacement::full_canvas(&config));
    let transition = [CompositorLayer::Transition {
        outgoing: &out_plate,
        incoming: &in_plate,
        transition_id: "crossfade",
        progress: 0.5,
    }];
    let matted = [CompositorLayer::Matte {
        content: &out_plate,
        stencil: &in_plate,
        read: MatteRead::Luma,
        invert: false,
        softness: 0.2,
    }];

    for (family, scene) in [
        ("effects, grade and blit", canvas_pass.as_slice()),
        ("shadow, glow and outline", styled.as_slice()),
        ("blend composite", blended.as_slice()),
        ("transition", transition.as_slice()),
        ("matte", matted.as_slice()),
    ] {
        let mut comp = Compositor::new(&gpu);
        comp.render_compositor_layers(&gpu, &config, scene)
            .unwrap_or_else(|err| panic!("{family} first frame: {err}"));
        let first = comp.pass_stats();
        assert!(first.passes > 0, "{family} drew no cached pass");
        comp.render_compositor_layers(&gpu, &config, scene)
            .unwrap_or_else(|err| panic!("{family} second frame: {err}"));
        let second = comp.pass_stats();
        assert_eq!(
            second.created, 0,
            "{family} rebuilds a descriptor every frame, got {second:?} after {first:?}"
        );
        assert_eq!(
            second.passes, first.passes,
            "{family} drew a different frame"
        );
    }
}

#[test]
fn pass_batching_avoids_a_bind_group_build_per_pass_over_many_frames() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(512, 288).with_background([0, 0, 0, 255]);
    let effects = [
        PassInstance::new("vignette", vec![0.6]),
        PassInstance::new("sharpen", vec![0.5]),
    ];
    let layers = same_effect_plates(&config, &effects, 12);

    // Warm the pipelines and the pool before either measurement.
    comp.render(&gpu, &config, &layers).expect("warm");
    let per_frame = comp.pass_stats().passes;
    assert!(per_frame > 0, "the scene must draw cached passes");

    // Interleave the two modes one frame at a time, and report both clocks.
    // A sequential block per mode measures whatever else the machine was
    // doing during that block, and alternating which mode leads each round
    // spreads that across both samples.
    const ROUNDS: usize = 15;
    let mut per_pass_samples = Vec::with_capacity(ROUNDS);
    let mut shared_samples = Vec::with_capacity(ROUNDS);
    let mut avoided = 0u32;
    for round in 0..ROUNDS {
        let modes = if round % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for reuse in modes {
            let (frame, stats) = timed_frame(&mut comp, &gpu, &config, &layers, reuse);
            if reuse {
                assert_eq!(
                    stats.created, 0,
                    "reuse on must build nothing for a repeated frame, got {stats:?}"
                );
                assert_eq!(
                    stats.evictions, 0,
                    "this scene's descriptors are far under the cap, got {stats:?}"
                );
                avoided += stats.passes;
                shared_samples.push(frame);
            } else {
                assert_eq!(
                    stats.created, stats.passes,
                    "reuse off must build one bind group per pass, got {stats:?}"
                );
                per_pass_samples.push(frame);
            }
            assert_eq!(
                stats.passes, per_frame,
                "every frame of one scene draws the same passes, got {stats:?}"
            );
        }
    }

    let per_pass = median(&mut per_pass_samples);
    let shared = median(&mut shared_samples);
    eprintln!(
        "pass batching: {per_frame} passes/frame, {:.2} ms/frame per-pass vs {:.2} ms/frame shared \
         (median of {ROUNDS} interleaved rounds)",
        per_pass.as_secs_f64() * 1000.0,
        shared.as_secs_f64() * 1000.0
    );
    // The clock is reported, never asserted. A frame this size is readback
    // bound, so the builds sharing removes are a sliver of it, and any
    // other process on this GPU moves the reading by more than the whole
    // effect: a workspace run alongside the render tests has read 4.92 ms
    // shared against 1.75 ms per-pass on the same binary that reads 0.56
    // against 0.69 when it has the device to itself. What sharing actually
    // removes is counted, and a count cannot be moved by a neighbour.
    assert_eq!(
        avoided,
        per_frame * ROUNDS as u32,
        "every shared frame must reuse all {per_frame} descriptors"
    );
}

/// One timed frame in `reuse` mode. Switching modes clears the table, so each
/// sample pays for a warm frame first and times the next one -- otherwise the
/// shared sample would measure the build it exists to avoid.
fn timed_frame(
    comp: &mut Compositor,
    gpu: &GpuContext,
    config: &CompositorConfig,
    layers: &[CompositeLayer<'_>],
    reuse: bool,
) -> (std::time::Duration, PassStats) {
    comp.set_pass_reuse(reuse);
    comp.render(gpu, config, layers).expect("mode warm frame");
    let start = std::time::Instant::now();
    comp.render(gpu, config, layers).expect("timed frame");
    (start.elapsed(), comp.pass_stats())
}

/// Middle sample, so one stalled frame cannot move the reading.
fn median(samples: &mut [std::time::Duration]) -> std::time::Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}
