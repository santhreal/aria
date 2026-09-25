//! `render_frame` tests covering composites. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `render_frame` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn red_solid_with_white_background_plate_shows_padding() {
    // Default solids fill the canvas; scale down so the padded plate has a
    // probeable band outside the content on a black canvas.
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
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: 0.5.into(),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                background: Some(LayerBackground {
                    rgba: Param::Constant([255, 255, 255, 255]),
                    padding: Param::Constant(20.0),
                    radius: Param::Constant(0.0),
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let image = renderer.render_frame(&project, rt(0)).expect("render");

    // Content is 960×540 centered; canvas padding = 20 × 0.5 = 10px.
    // x=1445 sits ~5px into the plate past the content's right edge.
    let plate = image.pixel(1445, 540);
    assert_near(plate, [255, 255, 255, 255], 3, "background plate padding");
    let center = image.pixel(960, 540);
    assert_near(center, [255, 0, 0, 255], 3, "solid content center");
    let corner = image.pixel(10, 10);
    assert_near(corner, [0, 0, 0, 255], 3, "canvas outside plate");
}

#[test]
fn text_clip_with_layer_shadow_composites() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "styles".into(),
                style: TextStyle {
                    font: "Micro 5".into(),
                    size: 72.0.into(),
                    ..TextStyle::default()
                },
            },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 180]),
                    offset: Param::Constant([8.0, 8.0]),
                    blur: Param::Constant(4.0),
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    renderer.load_font(include_bytes!("../../aria-text/assets/Micro5-Regular.ttf").to_vec());
    let image = renderer
        .render_frame(&project, rt(0))
        .expect("text + layer shadow should composite");
    assert_eq!((image.width, image.height), (1920, 1080));
}
