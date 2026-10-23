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

/// WHY: a still or a cached title is drawn on every frame it is on screen, and
/// hashing the pixels of a 12-megapixel still takes tens of milliseconds. A
/// shared bitmap must be hashed on its first draw only and a borrowed one on
/// every draw, and a shared bitmap changed through its `Arc` must draw its new
/// pixels.
#[test]
fn shared_bitmaps_hash_once_and_draw_their_current_pixels() {
    let gpu = gpu_or_skip!();
    let config = CompositorConfig::new(64, 32).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let mut comp = Compositor::new(&gpu);
    let mut still = std::sync::Arc::new(rgba_uniform(64, 32, [24, 80, 160, 255]));
    let raster = rgba_uniform(64, 32, [24, 80, 160, 255]);

    let mut hashed = Vec::new();
    for _ in 0..3 {
        let layers = [
            CompositeLayer::rgba(&still, placement),
            CompositeLayer::cutout(&still, placement),
        ];
        comp.render(&gpu, &config, &layers).expect("shared bitmap");
        hashed.push(comp.bitmaps_hashed());
    }
    assert_eq!(hashed, [2, 0, 0], "shared bitmap hashes per frame");

    for _ in 0..2 {
        let layers = [CompositeLayer::rgba(&raster, placement)];
        comp.render(&gpu, &config, &layers)
            .expect("borrowed bitmap");
        assert_eq!(
            comp.bitmaps_hashed(),
            1,
            "a borrowed bitmap skipped its hash"
        );
        assert_eq!(
            comp.rgba_cache_stats().textures_created,
            0,
            "a borrowed bitmap of the shared pixels was uploaded again"
        );
    }

    std::sync::Arc::make_mut(&mut still)
        .pixels
        .copy_from_slice(&rgba_uniform(64, 32, [192, 48, 16, 255]).pixels);
    let changed = comp
        .render(&gpu, &config, &[CompositeLayer::rgba(&still, placement)])
        .expect("changed shared bitmap");
    assert_eq!(comp.bitmaps_hashed(), 1);
    assert_eq!(
        changed.pixel(32, 16),
        [192, 48, 16, 255],
        "a shared bitmap changed through its Arc drew its old pixels"
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
        .map(|s| [PassInstance::new("vignette", vec![*s])])
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
            stencil: None,
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

/// WHY: Every `wgpu::CommandEncoder::begin_render_pass` and its
/// `encode_render_pass` step in `CommandEncoder::finish` allocates ~11 heap
/// blocks in `wgpu-core`/lavapipe. Consecutive direct layers across all
/// pipeline families (`SolidCover`, `Solid`, `YuvCover`, `Yuv`, `YuvFx`,
/// `Rgba`, `RgbaFx`, `Cutout`, `CutoutFx`, `Shapes`) and target blits
/// (`blit_premultiplied_to_target`, `blit_replace_to_target`, style shadow/glow
/// offset blits, transition/matte composites, nest/reference initial clears)
/// must share one `wgpu::RenderPass` per target, flushing only when a pass
/// snapshots the target texture or reuses an `OffscreenPool` slot bound by a
/// queued draw.
///
/// Does not catch: allocations inside individual `wgpu` draw calls within a
/// render pass, which `steady_frame_render_allocations_stay_at_wgpu_floor`
/// bounds across a full frame.
#[test]
#[allow(clippy::expect_used)]
fn target_pass_batching_merges_consecutive_draws_and_flushes_on_hazards() {
    use aria_compositor::LayerBackground;

    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([12, 24, 36, 255]);
    let full = LayerPlacement::full_canvas(&config);
    let inset = |cx: f32, cy: f32, w: f32, h: f32| LayerPlacement {
        center: [cx, cy],
        size: [w, h],
        rotation: 0.0,
        opacity: 1.0,
    };

    // 1. Stills stack (`stills-4k` shape): 3 RGBA layers side by side fuse
    //    with the canvas background clear into 1 render pass.
    let still_a = rgba_uniform(64, 64, [200, 40, 40, 255]);
    let still_b = rgba_uniform(64, 64, [40, 200, 40, 255]);
    let still_c = rgba_uniform(64, 64, [40, 40, 200, 255]);
    let stills = [
        CompositeLayer::rgba(&still_a, inset(12.0, 32.0, 20.0, 64.0)),
        CompositeLayer::rgba(&still_b, inset(32.0, 32.0, 20.0, 64.0)),
        CompositeLayer::rgba(&still_c, inset(52.0, 32.0, 20.0, 64.0)),
    ];
    let stills_img = comp.render(&gpu, &config, &stills).expect("stills stack");
    assert_eq!(
        comp.render_pass_count(),
        1,
        "three side-by-side RGBA layers must share one render pass"
    );
    assert_px(&stills_img, 12, 32, [200, 40, 40, 255], 1);
    assert_px(&stills_img, 32, 32, [40, 200, 40, 255], 1);
    assert_px(&stills_img, 52, 32, [40, 40, 200, 255], 1);

    // 2. All direct pipeline families across >8 layers (spilling from inline
    //    storage into overflow): `YuvCover`, `Yuv`, `YuvFx`, `Rgba`, `RgbaFx`,
    //    `Cutout`, `CutoutFx`, `Solid`, `Shapes` all share 1 render pass.
    let yuv_base = nv12_uniform(64, 64, 80, 128, 128, ColorSpace::BT709);
    let yuv_pip = i420_uniform(64, 64, 180, 100, 150, ColorSpace::BT709);
    let circle_fx = LayerEffects {
        mask: Some(LayerMask::new(mask_kind::CIRCLE)),
        chroma_key: None,
    };
    let shape_layer = SdfLayer {
        shape: (SdfParams::RoundedRect { radius: 2.0 }).with_half([6.0, 6.0]),
        fill: [255, 220, 40, 255],
        stroke: None,
    };
    let direct_ten = [
        CompositeLayer::frame(&yuv_base, full),
        CompositeLayer::frame(&yuv_pip, inset(16.0, 16.0, 20.0, 20.0)),
        CompositeLayer::frame(&yuv_pip, inset(48.0, 16.0, 20.0, 20.0)).with_fx(circle_fx),
        CompositeLayer::rgba(&still_a, inset(16.0, 48.0, 16.0, 16.0)),
        CompositeLayer::rgba(&still_b, inset(32.0, 48.0, 16.0, 16.0)).with_fx(circle_fx),
        CompositeLayer::cutout(&still_c, inset(48.0, 32.0, 12.0, 12.0)),
        CompositeLayer::cutout(&still_a, inset(16.0, 32.0, 12.0, 12.0)).with_fx(circle_fx),
        CompositeLayer::solid([120, 60, 200, 200], inset(32.0, 16.0, 14.0, 14.0)),
        CompositeLayer::solid([60, 200, 120, 200], inset(32.0, 32.0, 14.0, 14.0)),
        CompositeLayer::sdf(shape_layer, inset(54.0, 54.0, 16.0, 16.0)),
    ];
    let direct_img = comp
        .render(&gpu, &config, &direct_ten)
        .expect("ten direct layers across all pipeline families");
    assert_eq!(
        comp.render_pass_count(),
        1,
        "ten consecutive direct layers across all pipeline families must share one render pass"
    );
    assert_px(&direct_img, 54, 54, [255, 220, 40, 255], 2);

    // 3. Overwrite reset: an opaque covering layer (`SolidCover` / `YuvCover`)
    //    pushed after earlier draws, including an offscreen-effect blit that
    //    set `offscreen_mask`, drops the earlier batch and resets
    //    `offscreen_mask`, so a subsequent offscreen-effect layer does not
    //    flush a dead canvas pass before reusing the pool slots.
    let vig_fx = [PassInstance::new("vignette", vec![0.4])];
    let covered = [
        CompositeLayer::rgba(&still_a, inset(16.0, 16.0, 24.0, 24.0)).with_effects(&vig_fx),
        CompositeLayer::solid([30, 90, 150, 255], full),
        CompositeLayer::rgba(&still_b, inset(48.0, 48.0, 16.0, 16.0)).with_effects(&vig_fx),
    ];
    let covered_img = comp.render(&gpu, &config, &covered).expect("covered stack");
    // 2 offscreen passes for covered[0] + 2 offscreen passes for covered[2] +
    // 1 shared canvas pass for covered[1] (`SolidCover`) and covered[2]'s blit
    // = 5 passes (vs 6 if `SolidCover` did not reset `offscreen_mask`).
    assert_eq!(
        comp.render_pass_count(),
        5,
        "covering layer must discard prior queued blit and reset offscreen_mask"
    );
    let clean_cover = comp
        .render(&gpu, &config, &covered[1..])
        .expect("clean cover");
    assert_eq!(covered_img.pixels, clean_cover.pixels);

    // 4. Direct layers + styled offscreen-effect layer + trailing direct layer:
    //    L1 (`YuvCover`) + L2 (`Yuv` PIP) stay queued while L3's offscreen
    //    effect chain (`gaussian_blur` + `vignette`) and shadow blur passes
    //    execute, then L3's background plate, shadow blit, content blit, and
    //    L4 (`Rgba` title) all fuse into 1 canvas render pass.
    let l3_fx = [
        PassInstance::new("gaussian_blur", vec![2.0]),
        PassInstance::new("vignette", vec![0.4]),
    ];
    let l1 = CompositeLayer::frame(&yuv_base, full);
    let l2 = CompositeLayer::frame(&yuv_pip, inset(16.0, 16.0, 24.0, 24.0));
    let l3 = CompositeLayer::rgba(&still_a, inset(32.0, 48.0, 40.0, 16.0))
        .with_effects(&l3_fx)
        .with_styles(LayerStyles {
            background: Some(LayerBackground {
                rgba: [10, 10, 10, 200],
                padding: 2.0,
                radius: 0.0,
            }),
            shadow: Some(LayerShadow {
                rgba: [0, 0, 0, 200],
                offset: [2.0, 2.0],
                blur: 2.0,
            }),
            glow: None,
            outline: None,
        });
    let l4 = CompositeLayer::rgba(&still_c, inset(48.0, 16.0, 16.0, 16.0));
    let mixed = [l1, l2, l3, l4];
    let mixed_img = comp.render(&gpu, &config, &mixed).expect("mixed stack");
    // 7 offscreen passes (1 draw_samples + 2 blur + 1 vignette + 3 shadow) +
    // 1 shared canvas pass for L1, L2, L3 plate/shadow/content, and L4 = 8
    // passes (vs 13 without TargetPass batching).
    assert_eq!(
        comp.render_pass_count(),
        8,
        "L1, L2, L3 styles/content, and L4 must share one canvas render pass"
    );
    assert_px(&mixed_img, 48, 16, [40, 40, 200, 255], 1);

    // Adding `glow` and `outline` to L3 reuses `slot_a`/`slot_b` after prior
    // style blits queued draws reading those slots, so `flush_if_reading_slots`
    // flushes before `glow` (3 offscreen passes + 1 canvas pass) and before
    // `outline` (4 offscreen passes + 1 canvas pass fusing outline blit + L4).
    let l3_outlined = l3.with_styles(LayerStyles {
        glow: Some(LayerGlow {
            rgba: [255, 255, 0, 180],
            radius: 2.0,
            intensity: 1.0,
        }),
        outline: Some(LayerOutline {
            rgba: [0, 255, 128, 255],
            width: 2.0,
        }),
        ..l3.styles
    });
    let outlined_scene = [l1, l2, l3_outlined, l4];
    let outlined_img = comp
        .render(&gpu, &config, &outlined_scene)
        .expect("outlined mixed stack");
    assert_eq!(
        comp.render_pass_count(),
        16,
        "glow and outline slot hazards must flush and fuse outline blit with L4"
    );
    assert_px(&outlined_img, 48, 16, [40, 40, 200, 255], 1);

    // 5. Nest + Transition + Matte + non-Normal BlendMode + CanvasPass fusion.
    let nest_a = CompositeLayer::rgba(&still_a, inset(16.0, 32.0, 24.0, 48.0));
    let nest_b = CompositeLayer::rgba(&still_b, inset(48.0, 32.0, 24.0, 48.0));
    let nest_items = [
        CompositorLayer::layer(&nest_a),
        CompositorLayer::layer(&nest_b),
    ];
    let use_nest = CompositeLayer::nest(7, full);
    let over_nest = CompositeLayer::rgba(&still_c, inset(32.0, 32.0, 16.0, 16.0));
    let nest_scene = [
        CompositorLayer::Nest {
            nest: 7,
            items: &nest_items,
        },
        CompositorLayer::layer(&use_nest),
        CompositorLayer::layer(&over_nest),
    ];
    comp.render_compositor_layers(&gpu, &config, &nest_scene)
        .expect("nest scene");
    assert_eq!(
        comp.render_pass_count(),
        2,
        "nest clear+layers must share 1 nest pass and outer layers 1 canvas pass"
    );

    // Leading direct layer + Transition (or Matte) + trailing direct layer:
    // 2 side passes (each fusing its transparent clear) + 1 transition/matte
    // pass + 1 shared canvas pass = 4 passes.
    let lead = CompositeLayer::solid([30, 30, 30, 255], full);
    let trail = CompositeLayer::rgba(&still_c, inset(48.0, 48.0, 16.0, 16.0));
    let trans_scene = [
        CompositorLayer::layer(&lead),
        CompositorLayer::Transition {
            outgoing: &nest_a,
            incoming: &nest_b,
            transition_id: "crossfade",
            progress: 0.5,
        },
        CompositorLayer::layer(&trail),
    ];
    comp.render_compositor_layers(&gpu, &config, &trans_scene)
        .expect("transition scene");
    assert_eq!(
        comp.render_pass_count(),
        4,
        "transition surrounded by direct layers must share 1 canvas pass (4 total)"
    );

    let matte_scene = [
        CompositorLayer::layer(&lead),
        CompositorLayer::Matte {
            content: &nest_a,
            stencil: &nest_b,
            read: MatteRead::Alpha,
            invert: false,
            softness: 0.1,
        },
        CompositorLayer::layer(&trail),
    ];
    comp.render_compositor_layers(&gpu, &config, &matte_scene)
        .expect("matte scene");
    assert_eq!(
        comp.render_pass_count(),
        4,
        "matte surrounded by direct layers must share 1 canvas pass (4 total)"
    );

    // Non-Normal blend mode flushes before snapshotting the canvas, then fuses
    // its Replace composite quad with trailing direct layers (1 pre-blend
    // canvas pass + 1 offscreen layer pass + 1 post-blend canvas pass = 3).
    let screen_layer = CompositeLayer::rgba(&still_a, inset(32.0, 32.0, 32.0, 32.0))
        .with_blend_mode(BlendMode::Screen);
    let blend_scene = [
        CompositorLayer::layer(&lead),
        CompositorLayer::layer(&screen_layer),
        CompositorLayer::layer(&trail),
    ];
    comp.render_compositor_layers(&gpu, &config, &blend_scene)
        .expect("blend scene");
    assert_eq!(
        comp.render_pass_count(),
        3,
        "blend composite Replace quad must fuse with trailing direct layers"
    );

    // 6. Motion-blurred layer (3 samples) fuses the accumulation clear into
    //    sample 0 and fuses its final blit with leading and trailing direct
    //    layers (6 sample passes + 1 canvas pass = 7).
    let samples = [
        inset(28.0, 32.0, 36.0, 32.0),
        inset(32.0, 32.0, 32.0, 32.0),
        inset(36.0, 32.0, 28.0, 32.0),
    ];
    let mb_layer =
        CompositeLayer::rgba(&still_a, inset(32.0, 32.0, 32.0, 32.0)).with_blur_passes(&samples);
    let mb_scene = [
        CompositorLayer::layer(&lead),
        CompositorLayer::layer(&mb_layer),
        CompositorLayer::layer(&trail),
    ];
    comp.render_compositor_layers(&gpu, &config, &mb_scene)
        .expect("motion blur scene");
    assert_eq!(
        comp.render_pass_count(),
        7,
        "lead direct layer + 3-sample motion-blurred layer + trailing direct layer must use 7 passes"
    );

    // 7. Unmasked CanvasPass flushes prior direct layers, snapshots the canvas,
    //    runs its effect, and fuses its Replace blit with a trailing direct layer
    //    in one overwrite pass (1 pre-pass canvas + 1 snapshot + 2 blur passes +
    //    1 post-pass canvas = 5, vs 6 without TargetPass batching).
    let cp_scene = [
        CompositorLayer::layer(&lead),
        CompositorLayer::CanvasPass {
            effects: &l3_fx[..1],
            grade: None,
            lut: None,
            stencil: None,
        },
        CompositorLayer::layer(&trail),
    ];
    comp.render_compositor_layers(&gpu, &config, &cp_scene)
        .expect("canvas pass scene");
    assert_eq!(
        comp.render_pass_count(),
        5,
        "direct layer + unmasked 2-pass CanvasPass + trailing direct layer must use 5 passes"
    );

    // 8. Two consecutive offscreen-effect layers: the second layer's effect
    //    chain rewrites the pool slots, so the first layer's queued blit is
    //    flushed before it (2 offscreen passes + 1 canvas pass per layer = 6,
    //    vs 5 if both blits waited for one canvas pass), and each layer draws
    //    its own pixels, as it does rendered alone.
    let left_fx =
        CompositeLayer::rgba(&still_a, inset(16.0, 32.0, 24.0, 48.0)).with_effects(&vig_fx);
    let right_fx =
        CompositeLayer::rgba(&still_b, inset(48.0, 32.0, 24.0, 48.0)).with_effects(&vig_fx);
    let left_alone = comp
        .render(&gpu, &config, &[left_fx])
        .expect("left effect layer");
    let right_alone = comp
        .render(&gpu, &config, &[right_fx])
        .expect("right effect layer");
    let both = comp
        .render(&gpu, &config, &[left_fx, right_fx])
        .expect("two effect layers");
    assert_eq!(
        comp.render_pass_count(),
        6,
        "a queued effect blit must flush before the next effect chain reuses the pool"
    );
    for y in 0..64 {
        for x in 0..64 {
            let alone = if x < 32 { &left_alone } else { &right_alone };
            assert_eq!(
                both.pixel(x, y),
                alone.pixel(x, y),
                "pixel ({x},{y}) of two effect layers differs from its layer drawn alone"
            );
        }
    }
}
