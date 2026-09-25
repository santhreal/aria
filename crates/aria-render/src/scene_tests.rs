use super::*;
use aria_core::Rational;
use aria_models::MediaId;
use aria_shapes::{SdfParams, Stroke};

fn media_layer(center: [f32; 2], size: [f32; 2]) -> SceneLayer {
    SceneLayer {
        clip: None,
        source: LayerSource::Media {
            media: MediaId::from_raw(1),
            source_time: RationalTime::new(0, Rational::FPS_30),
        },
        center,
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed(size),
        rotation: 0.5,
        opacity: 0.8,
        uv: [0.1, 0.2, 0.9, 0.8],
        effects: Vec::new(),
        mask: None,
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

fn shape_layer() -> SceneLayer {
    SceneLayer {
        clip: None,
        source: LayerSource::Shape {
            params: SdfParams::Ellipse,
            fill: [255, 0, 0, 255],
            stroke: Some(Stroke {
                rgba: [0, 0, 0, 255],
                width: 8.0,
            }),
            pad: 6.0,
        },
        center: [100.0, 100.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([212.0, 112.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        mask: None,
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

fn text_layer() -> SceneLayer {
    SceneLayer {
        clip: None,
        source: LayerSource::Text {
            content: "hi".into(),
            style: TextStyle::new(48.0),
            animation: None,
        },
        center: [50.0, 25.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::BitmapScaled([2.0, 2.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        mask: None,
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

#[test]
fn text_alignment_positions_tight_bounds_against_canvas_edges() {
    let layer = text_layer();
    let size = [200.0, 100.0];
    let canvas = [1920.0, 1080.0];

    assert_eq!(
        layer.text_quad_center(
            &TextStyle::new(48.0)
                .with_align(TextAlign::Left)
                .with_vertical_align(TextVerticalAlign::Top),
            size,
            canvas,
        ),
        [-810.0, -465.0]
    );
    assert_eq!(
        layer.text_quad_center(
            &TextStyle::new(48.0)
                .with_align(TextAlign::Right)
                .with_vertical_align(TextVerticalAlign::Bottom),
            size,
            canvas,
        ),
        [910.0, 515.0]
    );
}

#[test]
fn scale_halves_canvas_and_layer_geometry() {
    let mut scene = Scene::empty(1920, 1080, [0, 0, 0, 255]);
    scene
        .layers
        .push(media_layer([960.0, 540.0], [1920.0, 1080.0]));
    scene.layers.push(text_layer());
    scene.layers.push(shape_layer());

    scene.scale(0.5);

    assert_eq!((scene.width, scene.height), (960, 540));
    let SizeSpec::Fixed(size) = scene.layers[0].size else {
        panic!("media layer keeps a fixed size");
    };
    assert_eq!(scene.layers[0].center, [480.0, 270.0]);
    assert_eq!(size, [960.0, 540.0]);
    // Rotation, opacity, and uv (content-relative) are untouched.
    assert_eq!(scene.layers[0].rotation, 0.5);
    assert_eq!(scene.layers[0].opacity, 0.8);
    assert_eq!(scene.layers[0].uv, [0.1, 0.2, 0.9, 0.8]);

    // Text scales through its bitmap multiplier.
    assert_eq!(scene.layers[1].size, SizeSpec::BitmapScaled([1.0, 1.0]));

    // SDF stroke width and pad are canvas-pixel quantities.
    let LayerSource::Shape { stroke, pad, .. } = &scene.layers[2].source else {
        panic!("shape layer");
    };
    assert_eq!(stroke.unwrap().width, 4.0);
    assert_eq!(*pad, 3.0);
}

#[test]
fn scale_ignores_degenerate_factors() {
    let mut scene = Scene::empty(100, 50, [0, 0, 0, 255]);
    scene.scale(0.0);
    scene.scale(-1.0);
    scene.scale(f32::NAN);
    assert_eq!((scene.width, scene.height), (100, 50));
}

#[test]
fn fit_within_never_upscales() {
    let mut scene = Scene::empty(640, 360, [0, 0, 0, 255]);
    scene.fit_within(4000, 4000);
    assert_eq!((scene.width, scene.height), (640, 360));
}

#[test]
fn fit_within_shrinks_to_the_tighter_axis() {
    let mut scene = Scene::empty(1920, 1080, [0, 0, 0, 255]);
    scene.fit_within(400, 400);
    assert_eq!((scene.width, scene.height), (400, 225));
}

#[test]
fn fit_within_survives_a_zero_canvas() {
    let mut scene = Scene::empty(0, 0, [0, 0, 0, 255]);
    scene.fit_within(100, 100);
    assert_eq!((scene.width, scene.height), (0, 0));
}

#[test]
fn fit_into_letterboxes_an_aspect_mismatch() {
    // 16:9 content into a square: scaled to 400×225, centered vertically.
    let mut scene = Scene::empty(1920, 1080, [1, 2, 3, 255]);
    scene
        .layers
        .push(media_layer([960.0, 540.0], [1920.0, 1080.0]));

    scene.fit_into(400, 400);

    assert_eq!((scene.width, scene.height), (400, 400));
    let SizeSpec::Fixed(size) = scene.layers[0].size else {
        panic!("media layer keeps a fixed size");
    };
    // Content box is 400×225; its center sits at the canvas center.
    assert!((size[0] - 400.0).abs() < 1e-3);
    assert!((size[1] - 225.0).abs() < 1e-3);
    assert!((scene.layers[0].center[0] - 200.0).abs() < 1e-3);
    assert!((scene.layers[0].center[1] - 200.0).abs() < 1e-3);
}

#[test]
fn fit_into_upscales_for_export_overrides() {
    let mut scene = Scene::empty(960, 540, [0, 0, 0, 255]);
    scene
        .layers
        .push(media_layer([480.0, 270.0], [960.0, 540.0]));
    scene.fit_into(1920, 1080);
    assert_eq!((scene.width, scene.height), (1920, 1080));
    assert_eq!(scene.layers[0].center, [960.0, 540.0]);
}
