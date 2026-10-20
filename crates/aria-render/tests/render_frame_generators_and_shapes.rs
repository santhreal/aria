//! `render_frame` tests covering generators and shapes. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `render_frame` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn renders_a_solid_generator_to_a_red_canvas() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    let image = renderer.render_frame(&project, rt(0)).expect("render");
    assert_eq!((image.width, image.height), (1920, 1080));
    assert_eq!(image.pixels.len(), 1920 * 1080 * 4);

    // The solid fills the whole canvas, so every corner should read red.
    let top_left = &image.pixels[0..4];
    assert!(
        top_left[0] > 240 && top_left[1] < 16 && top_left[2] < 16,
        "expected a red canvas, got {top_left:?}"
    );
}

/// A shipped generator placed the way the Generators tab places one, a
/// transparent solid with the generator first in its chain, draws its picture
/// across the frame through the project path.
#[test]
fn a_shipped_generator_draws_over_the_transparent_solid_that_carries_it() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor { rgba: [0; 4] },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project
        .add_effect(clip, "linear_gradient")
        .expect("a shipped generator is in the catalog");
    assert!(
        project.clip(clip).expect("the clip").effects[0].generator,
        "the instance records that it was placed as a generator"
    );

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");
    // The default ramp runs from black at the left edge to white at the right.
    assert_near(image.pixel(0, 540), [0, 0, 0, 255], 2, "the left edge");
    assert_near(image.pixel(960, 540), [128, 128, 128, 255], 2, "the middle");
    assert_near(
        image.pixel(1919, 540),
        [255, 255, 255, 255],
        2,
        "the right edge",
    );
}

#[test]
fn gesture_sprite_contains_unwrapped_text_pixels() {
    let mut project = Project::new("text-gesture", FPS_24);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "hello worldhello worldhello worldhello world".into(),
                style: TextStyle {
                    font: "Micro 5".into(),
                    size: 56.0.into(),
                    letter_spacing: 30.0.into(),
                    wrap: false,
                    ..TextStyle::default()
                },
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    renderer.load_font(include_bytes!("../../aria-text/assets/Micro5-Regular.ttf").to_vec());
    let frames = renderer
        .render_gesture_frames(&project, rt(0), clip, 960, 540)
        .expect("render gesture frames")
        .expect("text clip should support sprite partitioning");

    let visible = frames
        .sprite
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 0)
        .count();
    assert!(
        visible > 100,
        "gesture sprite was transparent ({visible} pixels)"
    );
}

#[test]
fn renders_an_ellipse_through_the_sdf_pipeline() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::shape(Shape::Ellipse, [0, 255, 0, 255]),
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");

    // Drop size 200×200 centered on the 1920×1080 canvas: the center is
    // inside the ellipse, a point 105px right of center is outside it, and
    // the canvas corner is untouched background (black).
    let center = image.pixel(960, 540);
    assert!(
        center[1] > 240 && center[0] < 16,
        "ellipse center should be green, got {center:?}"
    );
    let outside = image.pixel(960 + 105, 540);
    assert!(
        outside[1] < 16,
        "outside the ellipse should be background, got {outside:?}"
    );
    // On the horizontal axis 90px out (inside the 100px semi-axis).
    let on_axis = image.pixel(960 + 90, 540);
    assert!(on_axis[1] > 240, "90px along +x is inside, got {on_axis:?}");
}

#[test]
fn renders_a_pen_path_through_the_bitmap_pipeline() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    // A 160×160 diamond around the origin.
    let path = ShapePath {
        points: vec![
            ShapePathPoint::corner([0.0, -80.0]),
            ShapePathPoint::corner([80.0, 0.0]),
            ShapePathPoint::corner([0.0, 80.0]),
            ShapePathPoint::corner([-80.0, 0.0]),
        ],
        closed: true,
    };
    project
        .add_generated(
            track,
            Generator::shape(Shape::Path(path), [0, 128, 255, 255]),
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");

    let center = image.pixel(960, 540);
    assert!(
        center[2] > 240 && center[1] > 100,
        "diamond center should be the fill color, got {center:?}"
    );
    // The diamond's corner region (70, 70) from center is outside the fill.
    let outside = image.pixel(960 + 70, 540 + 70);
    assert!(
        outside[2] < 16,
        "outside the diamond should be background, got {outside:?}"
    );
}
