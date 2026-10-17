//! Integration and GPU parity tests for [`aria_render::RenderFiles`].
//!
//! Closes the class where a render-file cache either diverges visually from the
//! live compositor output (at full canvas size or scaled to a preview fit box)
//! or leaks into live-override previews and final exports.

use std::path::PathBuf;
use std::sync::Arc;

use aria_core::{Rational, RationalTime, RgbaImage};
use aria_models::{
    CanvasSettings, ClipId, ClipTransform, Generator, Param, Project, Scale2, TimeRange, TrackKind,
};
use aria_render::{
    RenderFiles, Renderer, ResolveOverrides, export_frame_png, resolve, resolve_with,
};

fn renderer() -> Option<Renderer> {
    match Renderer::new_headless() {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            eprintln!("skipping: no headless GPU available: {err}");
            None
        }
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aria-render-files-it-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn build_two_layer_project() -> (Project, ClipId) {
    let mut project = Project::new("RenderFileParity", Rational::FPS_30);
    let mut canvas = CanvasSettings::default();
    canvas.resolution = Some([160, 120]);
    canvas.background = [18, 28, 44];
    project.timeline_mut().set_canvas(canvas);

    let v1 = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            v1,
            Generator::SolidColor {
                rgba: [40, 90, 160, 255],
            },
            TimeRange::at_rate(0, 10, Rational::FPS_30),
        )
        .expect("backdrop");

    let v2 = project.add_track(TrackKind::Sticker, "S2");
    let fg_id = project
        .add_generated(
            v2,
            Generator::SolidColor {
                rgba: [230, 110, 45, 255],
            },
            TimeRange::at_rate(0, 10, Rational::FPS_30),
        )
        .expect("foreground layer");
    let fg = project.clip_mut(fg_id).expect("fg clip");
    fg.transform.scale = Param::Constant(Scale2::uniform(0.5));
    fg.transform.opacity = Param::Constant(0.6);
    (project, fg_id)
}

fn assert_images_within_one_level(actual: &RgbaImage, expected: &RgbaImage, context: &str) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height),
        "{context}: dimensions differ"
    );
    for (idx, (&a, &e)) in actual.pixels.iter().zip(&expected.pixels).enumerate() {
        let diff = a.abs_diff(e);
        assert!(
            diff <= 1,
            "{context}: channel byte {idx} differs by {diff} (actual={a}, expected={e})"
        );
    }
}

#[test]
fn render_file_matches_live_render_at_full_and_fitted_sizes() {
    let Some(mut live) = renderer() else {
        return;
    };
    let (project, _) = build_two_layer_project();
    let t = RationalTime::new(2, Rational::FPS_30);

    let expected_full = live.render_frame(&project, t).expect("live full render");
    let expected_fit = live
        .render_frame_fit(&project, t, 80, 60)
        .expect("live fitted render");
    assert_eq!((expected_full.width, expected_full.height), (160, 120));
    assert_eq!((expected_fit.width, expected_fit.height), (80, 60));

    let temp = scratch_dir("parity");
    let files = Arc::new(RenderFiles::new(&temp, "plugin.a@1.0.0"));
    let unscaled_scene = resolve(&project, t).expect("resolves unscaled scene");
    let key = files.key(&project, &unscaled_scene).expect("frame key");
    files
        .store(&key, &expected_full)
        .expect("stores full-canvas render file");

    live.set_render_files(Some(Arc::clone(&files)));

    let cached_full = live.render_frame(&project, t).expect("cached full render");
    assert_images_within_one_level(&cached_full, &expected_full, "full size from render file");
    assert_eq!(live.last_frame_stats().raster_ms, 0.0);

    let cached_fit = live
        .render_frame_fit(&project, t, 80, 60)
        .expect("cached fitted render");
    assert_images_within_one_level(&cached_fit, &expected_fit, "fitted size from render file");
    assert_eq!(live.last_frame_stats().raster_ms, 0.0);
}

#[test]
fn render_files_are_used_for_settled_preview_and_bypassed_for_overrides_and_export() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let (project, fg_id) = build_two_layer_project();
    let t = RationalTime::new(0, Rational::FPS_30);

    let live_full = renderer.render_frame(&project, t).expect("live frame");
    let sentinel = RgbaImage::new(160, 120, [250u8, 12, 240, 255].repeat(160 * 120));
    assert_ne!(live_full.pixels[..4], sentinel.pixels[..4]);

    let temp = scratch_dir("bypass");
    let files = Arc::new(RenderFiles::new(temp.join("cache"), ""));
    let scene = resolve(&project, t).expect("scene");
    let key = files.key(&project, &scene).expect("key");
    files.store(&key, &sentinel).expect("stores sentinel");

    renderer.set_render_files(Some(Arc::clone(&files)));

    // Preview with no overrides serves the stored sentinel image.
    let preview_hit = renderer.render_frame(&project, t).expect("preview hit");
    assert_eq!(&preview_hit.pixels[..4], &[250, 12, 240, 255]);

    // Active ResolveOverrides bypass the stored render file and realize live.
    let moved = ClipTransform {
        opacity: 0.25,
        ..ClipTransform::IDENTITY
    };
    let overrides = ResolveOverrides {
        transform: Some((fg_id, moved)),
        ..ResolveOverrides::default()
    };
    let override_scene = resolve_with(&project, t, overrides).expect("override scene");
    let override_key = files.key(&project, &override_scene).expect("override key");
    files
        .store(&override_key, &sentinel)
        .expect("stores sentinel for override key too");
    let override_frame = renderer
        .render_frame_with(&project, t, overrides)
        .expect("override frame");
    assert_ne!(&override_frame.pixels[..4], &[250, 12, 240, 255]);

    // Export (export_frame_png) never reads render files.
    let png_path = temp.join("exported.png");
    export_frame_png(&mut renderer, &project, t, &png_path).expect("exports png");
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(&png_path).expect("opens exported png"),
    ));
    let mut reader = decoder.read_info().expect("png header");
    let mut exported_pixels = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut exported_pixels).expect("png pixels");
    assert_eq!(&exported_pixels[..4], &live_full.pixels[..4]);
}
