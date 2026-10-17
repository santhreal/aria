//! `render_frame` tests covering masks and adjustment. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `render_frame` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn renders_still_with_circle_mask_and_chroma_key() {
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("green.png");
    write_solid_png(&png_path, 320, 180, [0, 255, 0, 255]);

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(MediaSource::image(&png_path, 320, 180));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, window, rt(0)).unwrap();
    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    project
        .set_clip_chroma_key(
            clip,
            Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.5.into(),
                shadow: 0.0.into(),
            }),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("render masked still");

    // Center is inside the circle but green is keyed out → black background.
    assert_near(frame.pixel(320, 180), [0, 0, 0, 255], 8, "center");
    // Corner is outside the circle → background.
    assert_near(frame.pixel(10, 10), [0, 0, 0, 255], 8, "corner");
}

#[test]
fn renders_still_with_half_size_circle_mask() {
    // Square canvas + square still fills the frame; fit_within keeps the
    // resolve→realize→composite path cheap for CI.
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("red.png");
    write_solid_png(&png_path, 200, 200, [255, 0, 0, 255]);

    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    let media = project.add_media(MediaSource::image(&png_path, 200, 200));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, window, rt(0)).unwrap();
    let mut mask = Mask::new(MaskKind::Circle);
    mask.size = Param::Constant([0.5, 0.5]);
    project.add_clip_mask(clip, mask).unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 200, 200)
        .expect("render half-size mask");

    // Center stays red; ~0.7×half-width falls outside size 0.5 → background.
    let cx = frame.width / 2;
    let cy = frame.height / 2;
    let probe_x = cx + ((0.7 * (frame.width as f32 * 0.5)).round() as u32);
    assert_near(frame.pixel(cx, cy), [255, 0, 0, 255], 8, "center");
    assert_near(frame.pixel(probe_x, cy), [0, 0, 0, 255], 8, "mid-radius");
}

#[test]
fn renders_still_with_pen_path_mask() {
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("red.png");
    write_solid_png(&png_path, 200, 200, [255, 0, 0, 255]);

    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        resolution: Some([200, 200]),
        background: [0, 0, 0],
    });
    let media = project.add_media(MediaSource::image(&png_path, 200, 200));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, window, rt(0)).unwrap();
    project
        .add_clip_mask(
            clip,
            Mask::from_path(
                vec![
                    aria_models::ShapePathPoint::corner([0.25, 0.25]),
                    aria_models::ShapePathPoint::corner([0.75, 0.25]),
                    aria_models::ShapePathPoint::corner([0.75, 0.75]),
                    aria_models::ShapePathPoint::corner([0.25, 0.75]),
                ],
                true,
            ),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 200, 200)
        .expect("render path mask");

    assert_near(
        frame.pixel(100, 100),
        [255, 0, 0, 255],
        8,
        "center inside path",
    );
    assert_near(
        frame.pixel(10, 10),
        [0, 0, 0, 255],
        8,
        "corner outside path",
    );
}

/// A red square still filling a `canvas`-pixel square canvas, with `masks`
/// stacked on its clip, drawn at 200×200.
fn render_masked_square(canvas: u32, masks: Vec<Mask>) -> Option<aria_compositor::RgbaImage> {
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("red.png");
    write_solid_png(&png_path, 200, 200, [255, 0, 0, 255]);

    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        resolution: Some([canvas, canvas]),
        background: [0, 0, 0],
    });
    let media = project.add_media(MediaSource::image(&png_path, 200, 200));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, window, rt(0)).unwrap();
    for mask in masks {
        project.add_clip_mask(clip, mask).unwrap();
    }
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return None;
    };
    Some(
        renderer
            .render_frame_fit(&project, rt(0), 200, 200)
            .expect("render a masked square"),
    )
}

/// A stack of masks cuts by each mask's mode and opacity: a half-opacity
/// circle subtracted from a rectangle covering the clip dims the middle to
/// half and keeps the corners.
///
/// Not caught: the ops the stack's pixels do not tell apart here, which the
/// compositor's `mask_stacks` suite checks one by one.
#[test]
fn renders_a_mask_stack_by_mode_and_opacity() {
    let mut hole = Mask::new(MaskKind::Circle);
    hole.size = Param::Constant([0.5, 0.5]);
    hole.mode = aria_models::MaskMode::Subtract;
    hole.opacity = Param::Constant(0.5);
    let Some(frame) = render_masked_square(200, vec![Mask::new(MaskKind::Rectangle), hole]) else {
        return;
    };
    assert_near(frame.pixel(100, 100), [128, 0, 0, 255], 8, "dimmed middle");
    assert_near(frame.pixel(10, 10), [255, 0, 0, 255], 8, "kept corner");

    // A mask whose mode is None draws nothing: the rectangle alone remains.
    let mut off = Mask::new(MaskKind::Circle);
    off.size = Param::Constant([0.5, 0.5]);
    off.mode = aria_models::MaskMode::None;
    let Some(frame) = render_masked_square(200, vec![Mask::new(MaskKind::Rectangle), off]) else {
        return;
    };
    assert_near(
        frame.pixel(100, 100),
        [255, 0, 0, 255],
        8,
        "a None mask cut",
    );
}

/// Expansion moves a mask's edge by reference pixels scaled to the canvas,
/// and a preview drawn smaller than the canvas scales it with the picture.
///
/// A 400 px canvas draws 270 reference px as 100 canvas px; at the 200 px
/// preview that is 50 px, so a half-size rectangle (50..150) grows to
/// 25..175 rather than filling the frame.
#[test]
fn a_mask_expansion_scales_with_the_canvas_and_the_preview() {
    let mut grown = Mask::new(MaskKind::Rectangle);
    grown.size = Param::Constant([0.5, 0.5]);
    grown.expansion = Param::Constant(135.0);
    let Some(frame) = render_masked_square(400, vec![grown]) else {
        return;
    };
    assert_near(
        frame.pixel(40, 100),
        [255, 0, 0, 255],
        8,
        "past the plain edge",
    );
    assert_near(
        frame.pixel(10, 100),
        [0, 0, 0, 255],
        8,
        "past the grown edge",
    );
}

#[test]
fn renders_adjustment_lane_over_still_media() {
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("red.png");
    write_solid_png(&png_path, 320, 180, [255, 0, 0, 255]);

    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(MediaSource::image(&png_path, 320, 180));
    let window = project.media(media).unwrap().full_range();
    let video = project.add_track(TrackKind::Video, "V1");
    project.add_clip(video, media, window, rt(0)).unwrap();
    let adjustment = project.add_track(TrackKind::Adjustment, "A1");
    let bar = project
        .add_generated(
            adjustment,
            Generator::Adjustment,
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    project
        .set_clip_adjustments(
            bar,
            ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let frame = renderer
        .render_frame_fit(&project, rt(0), 320, 180)
        .expect("render adjustment lane");

    assert_px_close(
        frame.pixel(160, 90),
        grade_ref_u8([255, 0, 0, 255], grade),
        8,
        "adjusted center",
    );
}
