//! End-to-end smoke test: resolve a generator-only project and composite it on
//! a headless GPU. Skips cleanly when no GPU adapter is available (CI).

use std::path::Path;

use aria_compositor::{ColorGrade, GpuContext};
use aria_models::{
    BlendMode, CanvasAspect, CanvasSettings, ChromaKey, ClipParam, ClipTransform, ColorAdjustments,
    Easing, Filter, Generator, LayerBackground, LayerShadow, LayerStyles, Mask, MaskKind,
    MediaSource, MotionBlur, Param, ParamValue, Project, Rational, RationalTime, Shape, ShapePath,
    ShapePathPoint, TextStyle, TimeRange, TrackKind,
};
use aria_render::Renderer;

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn write_solid_png(path: &Path, width: u32, height: u32, rgba: [u8; 4]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    let flat: Vec<u8> = rgba
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    writer.write_image_data(&flat).expect("png data");
}

fn assert_near(actual: [u8; 4], expected: [u8; 4], tolerance: u8, what: &str) {
    for (a, e) in actual.iter().zip(expected.iter()) {
        assert!(
            a.abs_diff(*e) <= tolerance,
            "{what}: got {actual:?}, expected ~{expected:?} (±{tolerance})"
        );
    }
}

/// CPU mirror of `grade.wgsl`'s `apply_color_grade`, for tolerance comparisons.
fn grade_ref_u8(rgba: [u8; 4], grade: ColorGrade) -> [u8; 4] {
    let c = grade.apply([
        f32::from(rgba[0]) / 255.0,
        f32::from(rgba[1]) / 255.0,
        f32::from(rgba[2]) / 255.0,
    ]);
    let quant = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    [quant(c[0]), quant(c[1]), quant(c[2]), rgba[3]]
}

fn pixel_tol(gpu: &GpuContext) -> i32 {
    if gpu.is_software() { 8 } else { 3 }
}

fn assert_px_close(got: [u8; 4], expect: [u8; 4], tol: i32, label: &str) {
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "{label}: got {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

fn red_solid_project() -> (Project, aria_models::ClipId) {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    (project, clip)
}

#[path = "render_frame_masks_and_adjustment.rs"]
mod masks_and_adjustment;

#[path = "render_frame_generators_and_shapes.rs"]
mod generators_and_shapes;

#[path = "render_frame_effects_and_transitions.rs"]
mod effects_and_transitions;

#[path = "render_frame_composites.rs"]
mod composites;

#[path = "render_frame_nests.rs"]
mod nests;
