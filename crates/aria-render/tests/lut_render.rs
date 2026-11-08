//! End-to-end `.cube` LUT rendering: a per-clip LUT on a solid, intensity
//! blending, a filter-lane LUT grading the whole canvas, and a missing or
//! unparseable file — all composited on a headless GPU. GPU tests skip
//! cleanly when no adapter is available (CI).

use std::path::Path;

use aria_core::{Rational, RationalTime};
use aria_models::{CanvasAspect, CanvasSettings, Generator, Lut, Project, TimeRange, TrackKind};
use aria_render::{ExportSettings, RenderError, Renderer, check_export_media};

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn pixel(image: &aria_compositor::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * image.width + x) * 4) as usize;
    image.pixels[idx..idx + 4].try_into().unwrap()
}

/// A 2-point LUT that swaps the red and green channels — visually unmistakable
/// and exactly representable at any grid size.
fn swap_rg_cube() -> String {
    aria_compositor::CubeLut::from_fn(2, |[r, g, b]| [g, r, b]).to_cube_string("swap-rg")
}

/// One fixed path per concurrent caller, rewritten before it is read.
fn temp_cube(source: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let slot = std::env::var("NEXTEST_TEST_GLOBAL_SLOT").unwrap_or_default();
    let path = std::env::temp_dir().join(format!("aria_lut_test_{slot}_{seq}.cube"));
    std::fs::write(&path, source).unwrap();
    path
}

/// A 16:9 project whose main track is a full-canvas red solid.
fn red_solid_project() -> (Project, aria_models::ClipId) {
    let mut project = Project::new("lut", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
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

#[test]
fn clip_lut_remaps_layer_colors() {
    let file = temp_cube(&swap_rg_cube());
    let (mut project, clip) = red_solid_project();
    project
        .set_clip_lut(
            clip,
            Some(Lut {
                path: file.to_string_lossy().into_owned(),
                intensity: 1.0.into(),
            }),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 320, 320)
        .expect("render LUT frame");
    let center = pixel(&frame, 160, 90);
    assert!(
        center[0] < 30 && center[1] > 225,
        "red should become green through the swap LUT, got {center:?}"
    );

    // Half intensity blends the swapped result over the original 50/50.
    project
        .set_clip_lut(
            clip,
            Some(Lut {
                path: file.to_string_lossy().into_owned(),
                intensity: 0.5.into(),
            }),
        )
        .unwrap();
    let blended = renderer
        .render_frame_fit(&project, rt(0), 320, 320)
        .expect("render blended LUT frame");
    let mid = pixel(&blended, 160, 90);
    assert!(
        (100..=155).contains(&mid[0]) && (100..=155).contains(&mid[1]),
        "half intensity should sit between red and green, got {mid:?}"
    );

    let _ = std::fs::remove_file(&file);
}

#[test]
fn filter_lane_lut_grades_the_canvas() {
    let file = temp_cube(&swap_rg_cube());
    let (mut project, _clip) = red_solid_project();
    // A filter lane bar above the solid: its LUT grades everything beneath.
    let lane = project.add_track(TrackKind::Filter, "F1");
    let bar = project
        .add_generated(lane, Generator::Filter, TimeRange::at_rate(0, 100, FPS_24))
        .unwrap();
    project
        .set_clip_lut(
            bar,
            Some(Lut {
                path: file.to_string_lossy().into_owned(),
                intensity: 1.0.into(),
            }),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 320, 320)
        .expect("render canvas LUT frame");
    let center = pixel(&frame, 160, 90);
    assert!(
        center[0] < 30 && center[1] > 225,
        "the filter-lane LUT should swap the canvas red to green, got {center:?}"
    );

    let _ = std::fs::remove_file(&file);
}

/// A unique `.cube` path in the temp directory with no file at it.
fn missing_cube() -> std::path::PathBuf {
    let path = temp_cube("");
    std::fs::remove_file(&path).unwrap();
    path
}

fn expect_offline(result: Result<aria_compositor::RgbaImage, RenderError>, file: &Path) {
    match result {
        Err(RenderError::FileOffline { path }) => assert_eq!(path, file),
        Err(other) => panic!("expected the missing LUT file, got {other}"),
        Ok(_) => panic!("a missing LUT must fail a frame rendered without the slate"),
    }
}

/// WHY: a clip whose `.cube` LUT was missing rendered and exported ungraded
/// with no error, because the renderer cached the failed load as "grade
/// nothing". A missing LUT fails a frame rendered without the offline slate
/// and the export media check; a preview, which renders with the slate, grades
/// nothing. The next frame looks for the file again, so moving it back
/// restores the grade. Cases: a clip's own LUT, and a filter-lane LUT that
/// grades the canvas.
#[test]
fn a_missing_lut_fails_the_export_grades_nothing_in_preview_and_recovers() {
    let file = missing_cube();
    let lut = || Lut {
        path: file.to_string_lossy().into_owned(),
        intensity: 1.0.into(),
    };
    let (mut clip_lut, clip) = red_solid_project();
    clip_lut.set_clip_lut(clip, Some(lut())).unwrap();
    let (mut lane_lut, _clip) = red_solid_project();
    let lane = lane_lut.add_track(TrackKind::Filter, "F1");
    let bar = lane_lut
        .add_generated(lane, Generator::Filter, TimeRange::at_rate(0, 100, FPS_24))
        .unwrap();
    lane_lut.set_clip_lut(bar, Some(lut())).unwrap();

    for project in [&clip_lut, &lane_lut] {
        let settings = ExportSettings::for_project(project);
        match check_export_media(project, settings, true) {
            Err(RenderError::FileOffline { path }) => assert_eq!(path, file),
            other => panic!("the export check should name the LUT file, got {other:?}"),
        }
    }

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    for project in [&clip_lut, &lane_lut] {
        renderer.set_offline_slate(false);
        expect_offline(renderer.render_frame_fit(project, rt(0), 320, 320), &file);
        renderer.set_offline_slate(true);
        let frame = renderer
            .render_frame_fit(project, rt(0), 320, 320)
            .expect("a preview renders without the missing LUT");
        assert_eq!(pixel(&frame, 160, 90), [255, 0, 0, 255], "ungraded");
    }

    std::fs::write(&file, swap_rg_cube()).unwrap();
    for project in [&clip_lut, &lane_lut] {
        let frame = renderer
            .render_frame_fit(project, rt(0), 320, 320)
            .expect("the restored LUT grades");
        let center = pixel(&frame, 160, 90);
        assert!(
            center[0] < 30 && center[1] > 225,
            "the restored LUT swaps red to green, got {center:?}"
        );
    }
    let _ = std::fs::remove_file(&file);
}

/// WHY: a `.cube` file that did not parse graded nothing, silently, in the
/// preview and the export. It fails the frame with the file and the parser's
/// reason, with the offline slate on, on every frame rather than only the one
/// that first loaded it.
#[test]
fn a_lut_that_does_not_parse_fails_every_frame() {
    let file = temp_cube("LUT_3D_SIZE 2\n0 0 0\n");
    let (mut project, clip) = red_solid_project();
    project
        .set_clip_lut(clip, Some(Lut::new(file.to_string_lossy())))
        .unwrap();
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        let _ = std::fs::remove_file(&file);
        return;
    };
    renderer.set_offline_slate(true);
    for frame in [0, 1] {
        match renderer.render_frame_fit(&project, rt(frame), 320, 320) {
            Err(error @ RenderError::FileUnusable { .. }) => assert_eq!(
                error.to_string(),
                format!(
                    "{} cannot be used (expected 8 data rows, found 1); replace the file or \
                     remove the clip or LUT that uses it",
                    file.display()
                )
            ),
            Err(other) => panic!("expected the unusable LUT file, got {other}"),
            Ok(_) => panic!("frame {frame}: a LUT that does not parse must fail"),
        }
    }
    let _ = std::fs::remove_file(&file);
}
