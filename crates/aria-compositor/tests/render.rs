//! End-to-end compositor checks: build a scene, render it on a real (headless)
//! GPU, and assert the read-back pixels.
//!
//! Color tests compare against a CPU reimplementation of the shader math within
//! a small tolerance (f32 + 8-bit unorm rounding differs slightly across
//! drivers, including software adapters), rather than pinning exact goldens.
//! If no GPU adapter is available the test skips rather than fails.

use aria_compositor::{
    BlendMode, ColorGrade, CompositeLayer, Compositor, CompositorConfig, CompositorError,
    CompositorLayer, GpuContext, LayerChromaKey, LayerEffects, LayerGlow, LayerMask, LayerOutline,
    LayerPlacement, LayerShadow, LayerStyles, MatteRead, PassInstance, RgbaImage, mask_kind,
};
use aria_core::{
    ColorRange, ColorSpace, CpuImage, FrameData, MatrixCoefficients, PixelFormat, Plane, Rational,
    RationalTime, Rect, Rotation, VideoFrame,
};

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping compositor test: no GPU adapter ({e})");
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

fn frame(format: PixelFormat, color: ColorSpace, w: u32, h: u32, planes: Vec<Plane>) -> VideoFrame {
    VideoFrame::new(
        RationalTime::new(0, Rational::FPS_30),
        format,
        color,
        (w, h),
        Rect::from_size(w, h),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(planes)),
    )
}

/// A `w×h` NV12 frame with every pixel set to the same (Y, U, V).
fn nv12_uniform(w: u32, h: u32, y: u8, u: u8, v: u8, color: ColorSpace) -> VideoFrame {
    let yp = Plane::new(vec![y; (w * h) as usize], w as usize, h as usize);
    let (cw, ch) = (w / 2, h / 2);
    let mut uv = Vec::with_capacity((cw * ch * 2) as usize);
    for _ in 0..(cw * ch) {
        uv.push(u);
        uv.push(v);
    }
    let uvp = Plane::new(uv, (cw * 2) as usize, ch as usize);
    frame(PixelFormat::Nv12, color, w, h, vec![yp, uvp])
}

/// A `w×h` I420 frame with every pixel set to the same (Y, U, V).
fn i420_uniform(w: u32, h: u32, y: u8, u: u8, v: u8, color: ColorSpace) -> VideoFrame {
    let yp = Plane::new(vec![y; (w * h) as usize], w as usize, h as usize);
    let (cw, ch) = (w / 2, h / 2);
    let up = Plane::new(vec![u; (cw * ch) as usize], cw as usize, ch as usize);
    let vp = Plane::new(vec![v; (cw * ch) as usize], cw as usize, ch as usize);
    frame(PixelFormat::Yuv420p, color, w, h, vec![yp, up, vp])
}

/// CPU mirror of `yuv.wgsl`'s `yuv_to_rgb`, for tolerance comparisons.
fn yuv_ref(y: u8, cb: u8, cr: u8, color: ColorSpace) -> [u8; 3] {
    let (kr, kb): (f32, f32) = match color.resolved().matrix {
        MatrixCoefficients::Bt601 => (0.299, 0.114),
        MatrixCoefficients::Bt2020Ncl => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let (ys, cbs, crs) = (
        f32::from(y) / 255.0,
        f32::from(cb) / 255.0,
        f32::from(cr) / 255.0,
    );
    let (yv, cbv, crv) = if color.range.is_full() {
        (ys, cbs - 128.0 / 255.0, crs - 128.0 / 255.0)
    } else {
        (
            (ys - 16.0 / 255.0) * (255.0 / 219.0),
            (cbs - 128.0 / 255.0) * (255.0 / 224.0),
            (crs - 128.0 / 255.0) * (255.0 / 224.0),
        )
    };
    let kg = 1.0 - kr - kb;
    let r = yv + 2.0 * (1.0 - kr) * crv;
    let b = yv + 2.0 * (1.0 - kb) * cbv;
    let g = yv - (2.0 * (1.0 - kr) * kr / kg) * crv - (2.0 * (1.0 - kb) * kb / kg) * cbv;
    [quant(r), quant(g), quant(b)]
}

fn quant(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CPU mirror of `grade.wgsl`'s `apply_grade` — the canonical version lives
/// on [`ColorGrade::apply`] so the LUT baker shares it.
fn grade_ref(rgb: [f32; 3], grade: ColorGrade) -> [f32; 3] {
    grade.apply(rgb)
}

fn grade_ref_u8(rgba: [u8; 4], grade: ColorGrade) -> [u8; 4] {
    let rgb = grade_ref(
        [
            f32::from(rgba[0]) / 255.0,
            f32::from(rgba[1]) / 255.0,
            f32::from(rgba[2]) / 255.0,
        ],
        grade,
    );
    [quant(rgb[0]), quant(rgb[1]), quant(rgb[2]), rgba[3]]
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

/// Render a single full-canvas frame layer and read it back.
fn render_frame(
    gpu: &GpuContext,
    comp: &mut Compositor,
    w: u32,
    h: u32,
    f: &VideoFrame,
) -> RgbaImage {
    let config = CompositorConfig::new(w, h);
    let layer = CompositeLayer::frame(f, LayerPlacement::full_canvas(&config));
    comp.render(gpu, &config, &[layer]).expect("render")
}

/// A `w×h` RGBA bitmap with every pixel set to `rgba` (straight alpha).
fn rgba_uniform(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        pixels.extend_from_slice(&rgba);
    }
    RgbaImage::new(w, h, pixels)
}

// --- SDF shapes ---------------------------------------------------------
//
// shape.wgsl and aria-shapes/src/sdf.rs implement the same math by
// contract. These goldens render each shape through both — the GPU SDF
// pipeline, and the CPU reference raster composited as an RGBA layer — and
// assert the canvases agree pixel-for-pixel within rounding tolerance.

use aria_compositor::{SdfLayer, SdfParams, Stroke};
use aria_shapes::ShapeStyle;

/// Composite one layer on a small canvas sized to the CPU raster.
fn render_shape_pair(
    gpu: &GpuContext,
    comp: &mut Compositor,
    params: SdfParams,
    half: [f32; 2],
    style: ShapeStyle,
    opacity: f32,
) -> (RgbaImage, RgbaImage) {
    let shape = params.with_half(half);
    let cpu_raster = aria_shapes::raster(&shape, &style);
    let (w, h) = (cpu_raster.width, cpu_raster.height);
    let config = CompositorConfig::new(w, h).with_background([40, 60, 80, 255]);
    let placement = LayerPlacement {
        center: [w as f32 / 2.0, h as f32 / 2.0],
        size: [w as f32, h as f32],
        rotation: 0.0,
        opacity,
    };

    let sdf_layer = CompositeLayer::sdf(
        SdfLayer {
            shape,
            fill: style.fill.unwrap_or([0, 0, 0, 0]),
            stroke: style.stroke,
        },
        placement,
    );
    let gpu_img = comp.render(gpu, &config, &[sdf_layer]).expect("gpu sdf");

    let cpu_layer = CompositeLayer::rgba(&cpu_raster, placement);
    let cpu_img = comp.render(gpu, &config, &[cpu_layer]).expect("cpu ref");
    (gpu_img, cpu_img)
}

#[track_caller]
fn assert_images_agree(gpu_img: &RgbaImage, cpu_img: &RgbaImage, what: &str, tol: i32) {
    assert_eq!(
        (gpu_img.width, gpu_img.height),
        (cpu_img.width, cpu_img.height)
    );
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);
    for y in 0..gpu_img.height {
        for x in 0..gpu_img.width {
            let a = gpu_img.pixel(x, y);
            let b = cpu_img.pixel(x, y);
            for c in 0..4 {
                let d = (i32::from(a[c]) - i32::from(b[c])).abs();
                if d > worst {
                    worst = d;
                    worst_at = (x, y);
                }
            }
        }
    }
    assert!(
        worst <= tol,
        "{what}: GPU and CPU disagree by {worst} at {worst_at:?} \
         (gpu {:?} vs cpu {:?}, tol {tol})",
        gpu_img.pixel(worst_at.0, worst_at.1),
        cpu_img.pixel(worst_at.0, worst_at.1),
    );
}

fn fill_only(rgba: [u8; 4]) -> ShapeStyle {
    ShapeStyle {
        fill: Some(rgba),
        stroke: None,
    }
}

/// Left half opaque red, right half fully transparent tinted red.
fn rgba_opaque_and_transparent(w: u32, h: u32) -> RgbaImage {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if x < w / 2 {
                pixels[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            } else {
                pixels[i..i + 4].copy_from_slice(&[255, 0, 0, 0]);
            }
        }
    }
    RgbaImage::new(w, h, pixels)
}

// Test bodies live in concern-named siblings so no file passes the 500-line
// ceiling; every helper above stays here, shared through `use super::*`.
#[path = "render_canvas.rs"]
mod canvas;
#[path = "render_effects.rs"]
mod effects;
#[path = "render_grade.rs"]
mod grade;
#[path = "render_pass_batching.rs"]
mod pass_batching;
#[path = "render_rgba.rs"]
mod rgba;
#[path = "render_sdf.rs"]
mod sdf;
#[path = "render_yuv.rs"]
mod yuv;
