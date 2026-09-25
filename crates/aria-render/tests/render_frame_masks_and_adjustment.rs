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
        .set_clip_mask(clip, Some(Mask::new(MaskKind::Circle)))
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
    project.set_clip_mask(clip, Some(mask)).unwrap();

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
