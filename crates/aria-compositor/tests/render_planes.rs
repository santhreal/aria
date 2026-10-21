//! Decoded CPU planes: staging at the copy pitch, row strides, one upload per
//! frame per render, staging reuse across submissions, chroma rounding, and
//! rejection of malformed planes.

use super::*;

/// Luma of [`nv12_strided`] at `(x, y)` for `seed`.
fn strided_luma(x: u32, y: u32, seed: u32) -> u8 {
    (16 + (x * 7 + y * 13 + seed) % 224) as u8
}

/// Write [`strided_luma`] for `seed` into the first `w` bytes of every
/// `stride`-byte row of `luma`.
fn write_strided_luma(luma: &mut [u8], w: u32, stride: usize, seed: u32) {
    for (y, row) in (0..).zip(luma.chunks_mut(stride)) {
        for x in 0..w {
            row[x as usize] = strided_luma(x, y, seed);
        }
    }
}

/// Rewrite the luma of an [`nv12_strided`] frame in place for `seed`, as a
/// decoder refilling one buffer does.
fn reseed_strided(f: &mut VideoFrame, w: u32, stride: usize, seed: u32) {
    let FrameData::Cpu(image) = &mut f.data else {
        panic!("nv12_strided builds CPU frames");
    };
    write_strided_luma(&mut image.planes[0].data, w, stride, seed);
}

/// Every pixel of `img` (`w×h`) is opaque gray at `luma(x, y)`, within 1.
#[track_caller]
fn assert_luma(img: &RgbaImage, w: u32, h: u32, what: &str, luma: impl Fn(u32, u32) -> u8) {
    for y in 0..h {
        for x in 0..w {
            let l = luma(x, y);
            let got = img.pixel(x, y);
            let off = (0..3).any(|c| (i32::from(got[c]) - i32::from(l)).abs() > 1);
            assert!(
                !off && got[3] == 255,
                "{what}: pixel ({x},{y}) = {got:?}, want luma {l}"
            );
        }
    }
}

/// A full-range NV12 frame of `w×h` whose luma and chroma rows sit `stride`
/// bytes apart, with a luma gradient set by `seed`, neutral chroma, and `0xEE`
/// in every padding byte.
fn nv12_strided(w: u32, h: u32, stride: usize, seed: u32) -> VideoFrame {
    let mut luma = vec![0xEE; stride * h as usize];
    write_strided_luma(&mut luma, w, stride, seed);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut chroma = vec![0xEE; stride * ch as usize];
    for row in chroma.chunks_mut(stride) {
        row[..(cw * 2) as usize].fill(128);
    }
    let full = ColorSpace {
        range: ColorRange::Full,
        ..ColorSpace::BT709
    };
    frame(
        PixelFormat::Nv12,
        full,
        w,
        h,
        vec![
            Plane::new(luma, stride, h as usize),
            Plane::new(chroma, stride, ch as usize),
        ],
    )
}

/// WHY: CPU planes are staged at the 256-byte copy pitch, in one block when the
/// frame's stride equals that pitch and row by row otherwise. The geometries
/// cover a row shorter than, equal to, and longer than the pitch, an odd width,
/// and a stride below, equal to, and above the pitch, so both staging paths
/// and both plane texel sizes run; every pixel must show its own row and
/// column. The second render rewrites the same frame in place, so it also
/// fails when a render reuses planes a previous render uploaded for that
/// frame. Does not cover I420, whose planes stage through the same function.
#[test]
fn strided_planes_land_at_their_rows_and_columns() {
    let gpu = gpu_or_skip!();
    let h = 6;
    for (w, stride) in [
        (64, 64),
        (63, 64),
        (64, 96),
        (200, 256),
        (256, 256),
        (256, 320),
        (300, 512),
    ] {
        let mut comp = Compositor::new(&gpu);
        let mut f = nv12_strided(w, h, stride, 0);
        for seed in [0, 101] {
            reseed_strided(&mut f, w, stride, seed);
            let img = render_frame(&gpu, &mut comp, w, h, &f);
            let what = format!("{w}x{h} at stride {stride}, seed {seed}");
            assert_luma(&img, w, h, &what, |x, y| strided_luma(x, y, seed));
        }
    }
}

/// WHY: a frame drawn more than once in one render (its motion-blur samples, a
/// second layer showing it) is one set of pixels, so its planes upload once
/// and every draw samples the same textures, while a different frame in the
/// same render keeps its own. Does not cover GPU-imported frames, which upload
/// nothing.
#[test]
fn a_frame_drawn_several_times_uploads_its_planes_once() {
    let gpu = gpu_or_skip!();
    let (w, h) = (64, 32);
    let f = nv12_strided(w, h, 64, 7);
    let g = nv12_strided(w / 2, h, 64, 50);
    let config = CompositorConfig::new(w, h);
    let placement = LayerPlacement::full_canvas(&config);
    let right_half = LayerPlacement {
        center: [w as f32 * 0.75, h as f32 * 0.5],
        size: [w as f32 * 0.5, h as f32],
        rotation: 0.0,
        opacity: 1.0,
    };
    let samples = [placement; 4];
    let layers = [
        CompositeLayer::frame(&f, placement).with_blur_passes(&samples),
        CompositeLayer::frame(&f, placement),
        CompositeLayer::frame(&g, right_half),
    ];
    let mut comp = Compositor::new(&gpu);
    let img = comp
        .render(&gpu, &config, &layers)
        .expect("one frame drawn five times beside another");
    let stats = comp.plane_pool_stats();
    assert_eq!(
        (stats.textures_created, stats.retained_textures),
        (4, 4),
        "two NV12 frames upload two plane textures each, however often each draws: {stats:?}"
    );
    assert_luma(&img, w, h, "shared planes", |x, y| {
        if x < w / 2 {
            strided_luma(x, y, 7)
        } else {
            strided_luma(x - w / 2, y, 50)
        }
    });
}

/// WHY: plane staging is retained, and every submission recalls it to the
/// pool, so steady frames of one size allocate no GPU memory after warm-up.
/// A submission path that skipped the recall would allocate a new staging
/// chunk on every frame, so each path runs: a readback, a frame target drawn
/// directly, and a frame target copied from the canvas. Checked where the
/// device reports its allocator; does not bound staging held while frames are
/// still in flight.
#[test]
fn steady_frames_reuse_their_plane_staging() {
    let shared = gpu_or_skip!();
    // The process-wide device also allocates for the other tests, so count on
    // a device of this test's own. A GL device is handed out unshared already.
    let gpu = if shared.adapter.get_info().backend == wgpu::Backend::Gl {
        shared
    } else {
        let (device, queue) =
            pollster::block_on(shared.adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("plane-staging-reuse"),
                required_limits: shared.adapter.limits(),
                ..Default::default()
            }))
            .expect("a device of the test's own");
        GpuContext::from_parts(shared.instance, shared.adapter, device, queue)
    };
    if gpu.device.generate_allocator_report().is_none() {
        eprintln!(
            "staging reuse not checked: {:?} reports no allocator",
            gpu.adapter.get_info().backend
        );
        return;
    }
    let (w, h, stride) = (64, 32, 64);
    let config = CompositorConfig::new(w, h);
    let direct = aria_compositor::create_frame_target(&gpu.device, w, h);
    let copied = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("plane-staging-reuse.copied"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    for (path, target) in [
        ("readback", None),
        ("direct frame target", Some(&direct)),
        ("copied frame target", Some(&copied)),
    ] {
        let mut f = nv12_strided(w, h, stride, 0);
        let mut comp = Compositor::new(&gpu);
        let mut allocated = Vec::new();
        for seed in 0..12 {
            reseed_strided(&mut f, w, stride, seed);
            if let Some(texture) = target {
                let layer = CompositeLayer::frame(&f, LayerPlacement::full_canvas(&config));
                comp.submit_compositor_layers_to_texture(
                    &gpu,
                    &config,
                    &[CompositorLayer::layer(&layer)],
                    texture,
                )
                .expect("frame target submission");
            } else {
                let img = render_frame(&gpu, &mut comp, w, h, &f);
                assert_luma(&img, w, h, "steady frame", |x, y| strided_luma(x, y, seed));
            }
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let report = gpu
                .device
                .generate_allocator_report()
                .expect("allocator report");
            allocated.push(report.total_allocated_bytes);
        }
        assert!(
            allocated[2..].iter().all(|&bytes| bytes == allocated[2]),
            "{path}: GPU memory grew across steady frames of one size: {allocated:?}"
        );
    }
}

/// WHY: CPU planes from a decoder are validated before upload. A luma plane
/// narrower than its texture, with too few rows, or with a buffer shorter than
/// stride by rows, an interleaved chroma row narrower than its texel pairs (odd
/// width), odd-width planar chroma narrower than its rounded-up width, and a
/// wrong plane count are each rejected as a malformed frame, and the same
/// compositor then renders the next good frame, including after a frame whose
/// luma staged before its chroma failed. Does not cover GPU-imported frames.
#[test]
fn malformed_cpu_planes_are_rejected_and_the_next_frame_renders() {
    let gpu = gpu_or_skip!();
    let (w, h) = (64, 6);
    let config = CompositorConfig::new(w, h);
    let placement = LayerPlacement::full_canvas(&config);
    let luma = |stride: usize, rows: usize, len: usize| Plane::new(vec![16; len], stride, rows);
    let chroma = |stride: usize| Plane::new(vec![128; stride * 3], stride, 3);
    let space = ColorSpace::BT709;
    let cases = [
        (
            "luma stride below its width",
            frame(
                PixelFormat::Nv12,
                space,
                w,
                h,
                vec![luma(32, 6, 32 * 6), chroma(64)],
            ),
        ),
        (
            "luma with too few rows",
            frame(
                PixelFormat::Nv12,
                space,
                w,
                h,
                vec![luma(64, 5, 64 * 5), chroma(64)],
            ),
        ),
        (
            "luma buffer shorter than stride by rows",
            frame(
                PixelFormat::Nv12,
                space,
                w,
                h,
                vec![luma(64, 6, 64 * 6 - 1), chroma(64)],
            ),
        ),
        (
            "odd-width chroma row narrower than its texel pairs",
            frame(
                PixelFormat::Nv12,
                space,
                63,
                h,
                vec![luma(64, 6, 64 * 6), chroma(63)],
            ),
        ),
        (
            "odd-width planar chroma narrower than its rounded-up width",
            frame(
                PixelFormat::Yuv420p,
                space,
                63,
                h,
                vec![luma(64, 6, 64 * 6), chroma(31), chroma(31)],
            ),
        ),
        (
            "one plane for NV12",
            frame(PixelFormat::Nv12, space, w, h, vec![luma(64, 6, 64 * 6)]),
        ),
        (
            "two planes for I420",
            frame(
                PixelFormat::Yuv420p,
                space,
                w,
                h,
                vec![luma(64, 6, 64 * 6), chroma(32)],
            ),
        ),
    ];
    let mut comp = Compositor::new(&gpu);
    let mut good = nv12_strided(w, h, 64, 0);
    for (seed, (what, bad)) in (1..).zip(&cases) {
        let result = comp.render(&gpu, &config, &[CompositeLayer::frame(bad, placement)]);
        assert!(
            matches!(result, Err(CompositorError::MalformedFrame(_))),
            "{what}: want a malformed-frame error, got {result:?}"
        );
        reseed_strided(&mut good, w, 64, seed);
        let img = render_frame(&gpu, &mut comp, w, h, &good);
        assert_luma(&img, w, h, &format!("good frame after {what}"), |x, y| {
            strided_luma(x, y, seed)
        });
    }
}

/// WHY: a 4:2:0 frame of odd width carries chroma rounded up, as the decoders
/// emit it, so the last luma column has a chroma sample of its own. A chroma
/// texture sized by the rounded-down width drops that sample and stretches the
/// rest, so the right edge takes its neighbour's color. Checked for both 8-bit
/// layouts; does not cover odd heights, which share the same rounding.
#[test]
fn odd_width_chroma_reaches_the_last_column() {
    let gpu = gpu_or_skip!();
    let full = ColorSpace {
        range: ColorRange::Full,
        ..ColorSpace::BT709
    };
    let (w, h) = (3, 2);
    let luma = || Plane::new(vec![128; 3 * 2], 3, 2);
    let (edge_u, edge_v) = (200, 60);
    let frames = [
        (
            "NV12",
            frame(
                PixelFormat::Nv12,
                full,
                w,
                h,
                vec![luma(), Plane::new(vec![128, 128, edge_u, edge_v], 4, 1)],
            ),
        ),
        (
            "I420",
            frame(
                PixelFormat::Yuv420p,
                full,
                w,
                h,
                vec![
                    luma(),
                    Plane::new(vec![128, edge_u], 2, 1),
                    Plane::new(vec![128, edge_v], 2, 1),
                ],
            ),
        ),
    ];
    let mut comp = Compositor::new(&gpu);
    for (what, f) in &frames {
        let img = render_frame(&gpu, &mut comp, w, h, f);
        let [r, g, b] = yuv_ref(128, 128, 128, full);
        assert_px(&img, 0, 0, [r, g, b, 255], 2);
        let [r, g, b] = yuv_ref(128, edge_u, edge_v, full);
        let edge = img.pixel(2, 1);
        let off = (0..3).any(|c| (i32::from(edge[c]) - i32::from([r, g, b][c])).abs() > 2);
        assert!(
            !off,
            "{what}: right-edge pixel {edge:?}, want {:?} from its own chroma sample",
            [r, g, b]
        );
    }
}
