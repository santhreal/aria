//! Layers that overwrite their target: which layers qualify, and that what
//! they draw never depends on the target's prior contents.
//!
//! WHY: a video frame or solid fill that covers its whole target at full
//! opacity draws with blending disabled onto a target its pass never loads.
//! A layer admitted to that path that leaves a pixel unwritten, or whose
//! source alpha is below 1, shows the previous frame or undefined memory
//! where the background or a blend belongs. A covering layer kept off the
//! path costs a clear and a read of the whole target on every frame.
//!
//! Does not catch: a cover pipeline built with blending enabled. It reads the
//! target and draws the same pixels.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use aria_core::{
    ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation,
    VideoFrame,
};

use super::covers_target;
use crate::compositor::LayerPipeline;
use crate::compositor::upload::placement_affine_rot;
use crate::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerEffects, LayerMask,
    LayerPlacement, RgbaImage, mask_kind,
};

const W: u32 = 16;
const H: u32 = 10;
const FILL: [u8; 4] = [200, 100, 50, 255];
const BACKGROUND: [u8; 4] = [10, 20, 30, 255];
/// What the canvas holds from the frame before the one under test.
const STALE: [u8; 4] = [255, 0, 255, 255];

fn config() -> CompositorConfig {
    CompositorConfig::new(W, H)
}

fn placed(center: [f32; 2], size: [f32; 2], rotation: f32, opacity: f32) -> LayerPlacement {
    LayerPlacement {
        center,
        size,
        rotation,
        opacity,
    }
}

fn full() -> LayerPlacement {
    LayerPlacement::full_canvas(&config())
}

fn covers(p: LayerPlacement) -> bool {
    let (linear, trans) = placement_affine_rot(&config(), &p, p.rotation);
    covers_target(linear, trans)
}

#[test]
fn placements_cover_the_canvas_only_when_they_contain_every_corner() {
    let (w, h) = (W as f32, H as f32);
    let mid = [w / 2.0, h / 2.0];
    // A square turned 45 degrees contains the canvas once its side reaches
    // (w + h) / sqrt(2), 18.38 canvas pixels.
    let cases = [
        ("full canvas", full(), true),
        ("overscan", placed(mid, [w * 1.5, h * 1.5], 0.0, 1.0), true),
        ("mirrored", placed(mid, [-w, h], 0.0, 1.0), true),
        (
            "one pixel narrow",
            placed(mid, [w - 1.0, h], 0.0, 1.0),
            false,
        ),
        (
            "one pixel short",
            placed(mid, [w, h - 1.0], 0.0, 1.0),
            false,
        ),
        (
            "one pixel right",
            placed([mid[0] + 1.0, mid[1]], [w, h], 0.0, 1.0),
            false,
        ),
        (
            "one pixel up",
            placed([mid[0], mid[1] - 1.0], [w, h], 0.0, 1.0),
            false,
        ),
        ("tilted", placed(mid, [w, h], 0.01, 1.0), false),
        (
            "turned square",
            placed(mid, [18.5, 18.5], FRAC_PI_4, 1.0),
            true,
        ),
        (
            "small turned square",
            placed(mid, [18.2, 18.2], FRAC_PI_4, 1.0),
            false,
        ),
        (
            "quarter turn",
            placed(mid, [h + 2.0, w + 2.0], FRAC_PI_2, 1.0),
            true,
        ),
        (
            "quarter turn unswapped",
            placed(mid, [w + 2.0, h + 2.0], FRAC_PI_2, 1.0),
            false,
        ),
        // A long strip turned 20 degrees misses two canvas corners, and a
        // near-square turned 40 degrees contains all four. Inverting the
        // placement with its off-diagonal terms swapped answers both wrongly.
        (
            "turned strip",
            placed(mid, [36.0, 13.0], 20f32.to_radians(), 1.0),
            false,
        ),
        (
            "turned rectangle",
            placed(mid, [21.0, 20.0], 40f32.to_radians(), 1.0),
            true,
        ),
        ("translucent", placed(mid, [w, h], 0.0, 0.999), false),
        ("over-opaque", placed(mid, [w, h], 0.0, 1.5), false),
        ("collapsed", placed(mid, [0.0, h], 0.0, 1.0), false),
        (
            "undefined center",
            placed([f32::NAN, mid[1]], [w, h], 0.0, 1.0),
            false,
        ),
    ];
    for (what, placement, expected) in cases {
        assert_eq!(covers(placement), expected, "{what}: {placement:?}");
    }
}

/// A diamond `|x| + |y| <= 2.2` around `center` in clip space: it contains
/// all four clip corners at the origin and loses one as it moves away.
fn diamond(center: [f32; 2]) -> bool {
    covers_target([2.2, 2.2, -2.2, 2.2], [center[0], center[1], 1.0, 0.0])
}

#[test]
fn a_quad_missing_any_one_clip_corner_does_not_cover() {
    assert!(diamond([0.0, 0.0]));
    for corner in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
        let away = [-0.15 * corner[0], -0.15 * corner[1]];
        assert!(!diamond(away), "the corner {corner:?} lies outside");
    }
}

fn nv12(y: u8, u: u8, v: u8) -> VideoFrame {
    let luma = Plane::new(vec![y; (W * H) as usize], W as usize, H as usize);
    let (cw, ch) = (W as usize / 2, H as usize / 2);
    let chroma = Plane::new([u, v].repeat(cw * ch), cw * 2, ch);
    VideoFrame::new(
        RationalTime::new(0, Rational::FPS_30),
        PixelFormat::Nv12,
        ColorSpace::BT709,
        (W, H),
        Rect::from_size(W, H),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![luma, chroma])),
    )
}

/// A circle mask: it leaves the canvas corners uncovered.
fn circle_mask() -> LayerEffects {
    LayerEffects {
        mask: Some(LayerMask::new(mask_kind::CIRCLE)),
        chroma_key: None,
    }
}

fn gpu() -> GpuContext {
    GpuContext::new_headless_blocking().expect("headless GPU")
}

#[test]
fn covering_opaque_layers_draw_without_blending() {
    let gpu = gpu();
    let comp = Compositor::new(&gpu);
    let frame = nv12(120, 90, 160);
    let mid = [W as f32 / 2.0, H as f32 / 2.0];
    let inset = placed(mid, [W as f32 - 2.0, H as f32 - 2.0], 0.0, 1.0);
    let half = placed(mid, [W as f32, H as f32], 0.0, 0.5);
    let cases = [
        (
            "opaque fill",
            CompositeLayer::solid(FILL, full()),
            LayerPipeline::SolidCover,
        ),
        (
            "translucent fill",
            CompositeLayer::solid([200, 100, 50, 254], full()),
            LayerPipeline::Solid,
        ),
        (
            "half-opacity fill",
            CompositeLayer::solid(FILL, half),
            LayerPipeline::Solid,
        ),
        (
            "inset fill",
            CompositeLayer::solid(FILL, inset),
            LayerPipeline::Solid,
        ),
        (
            "frame",
            CompositeLayer::frame(&frame, full()),
            LayerPipeline::YuvCover,
        ),
        (
            "half-opacity frame",
            CompositeLayer::frame(&frame, half),
            LayerPipeline::Yuv,
        ),
        (
            "inset frame",
            CompositeLayer::frame(&frame, inset),
            LayerPipeline::Yuv,
        ),
        (
            "masked frame",
            CompositeLayer::frame(&frame, full()).with_fx(circle_mask()),
            LayerPipeline::YuvFx,
        ),
    ];
    for (what, layer, expected) in cases {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let built = comp
            .build_layer(&gpu, &mut encoder, &config(), &layer)
            .expect("build layer");
        assert_eq!(built.pipeline, expected, "{what}");
        let covers = matches!(
            expected,
            LayerPipeline::SolidCover | LayerPipeline::YuvCover
        );
        assert_eq!(built.pipeline.overwrites_target(), covers, "{what}");
    }
}

/// Render `layers` over `background` on `comp`, after a frame that filled
/// the canvas with [`STALE`].
fn render_after_stale(
    gpu: &GpuContext,
    comp: &mut Compositor,
    background: [u8; 4],
    layers: &[CompositeLayer<'_>],
) -> RgbaImage {
    let stale = [CompositeLayer::solid(STALE, full())];
    comp.render(gpu, &config(), &stale).expect("stale frame");
    comp.render(gpu, &config().with_background(background), layers)
        .expect("frame under test")
}

#[test]
fn covering_layers_draw_the_same_pixels_over_anything() {
    let gpu = gpu();
    let frame = nv12(120, 90, 160);
    let under = CompositeLayer::solid([0, 255, 0, 255], full());
    let scenes: [(&str, Vec<CompositeLayer<'_>>); 3] = [
        ("fill", vec![CompositeLayer::solid(FILL, full())]),
        ("frame", vec![CompositeLayer::frame(&frame, full())]),
        (
            "frame over a fill",
            vec![under, CompositeLayer::frame(&frame, full())],
        ),
    ];
    let mut fresh = Compositor::new(&gpu);
    let mut reused = Compositor::new(&gpu);
    for (what, layers) in &scenes {
        let clean = fresh
            .render(&gpu, &config().with_background([0, 0, 0, 0]), layers)
            .expect("clean render");
        let over = render_after_stale(&gpu, &mut reused, BACKGROUND, layers);
        assert_eq!(over.pixels, clean.pixels, "{what}");
    }
    let fill = render_after_stale(&gpu, &mut reused, BACKGROUND, &scenes[0].1);
    assert!(
        fill.pixels.as_chunks::<4>().0.iter().all(|px| *px == FILL),
        "an opaque fill replaces every pixel"
    );
}

/// Straight-alpha src-over of `fill` at `alpha` over the opaque `background`.
fn over(fill: [u8; 4], alpha: f32, background: [u8; 4]) -> [u8; 4] {
    let mix = |s: u8, d: u8| (f32::from(s) * alpha + f32::from(d) * (1.0 - alpha)).round() as u8;
    [
        mix(fill[0], background[0]),
        mix(fill[1], background[1]),
        mix(fill[2], background[2]),
        255,
    ]
}

#[track_caller]
fn assert_near(got: [u8; 4], expected: [u8; 4], what: &str) {
    let off = got
        .iter()
        .zip(expected)
        .map(|(&g, e)| (i16::from(g) - i16::from(e)).abs())
        .max();
    assert!(off <= Some(1), "{what}: {got:?}, expected {expected:?}");
}

#[test]
fn layers_that_miss_a_pixel_or_let_light_through_keep_what_they_cover() {
    let gpu = gpu();
    let mut comp = Compositor::new(&gpu);
    let frame = nv12(120, 90, 160);
    let mid = [W as f32 / 2.0, H as f32 / 2.0];
    let inset = placed(mid, [W as f32 - 2.0, H as f32 - 2.0], 0.0, 1.0);
    let tilted = placed(mid, [W as f32, H as f32], 0.3, 1.0);
    let uncovered = [
        ("inset fill", CompositeLayer::solid(FILL, inset)),
        ("tilted fill", CompositeLayer::solid(FILL, tilted)),
        ("inset frame", CompositeLayer::frame(&frame, inset)),
        ("tilted frame", CompositeLayer::frame(&frame, tilted)),
        (
            "masked frame",
            CompositeLayer::frame(&frame, full()).with_fx(circle_mask()),
        ),
    ];
    for (what, layer) in uncovered {
        let image = render_after_stale(&gpu, &mut comp, BACKGROUND, &[layer]);
        assert_eq!(image.pixel(0, 0), BACKGROUND, "{what} leaves the corner");
    }

    let half = placed(mid, [W as f32, H as f32], 0.0, 0.5);
    let translucent = [
        (
            "translucent fill",
            CompositeLayer::solid([200, 100, 50, 128], full()),
            128.0 / 255.0,
        ),
        ("half-opacity fill", CompositeLayer::solid(FILL, half), 0.5),
    ];
    for (what, layer, alpha) in translucent {
        let image = render_after_stale(&gpu, &mut comp, BACKGROUND, &[layer]);
        assert_near(
            image.pixel(W / 2, H / 2),
            over(FILL, alpha, BACKGROUND),
            what,
        );
    }
}
