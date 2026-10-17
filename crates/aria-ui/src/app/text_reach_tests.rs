//! What the canvas pick and the shaping report must answer.
//!
//! Both are geometry and text, not GPU: they run headless, and the pick's
//! coordinate math is exactly where an off-by-a-letterbox never shows up in a
//! screenshot.

use aria_render::{LayerSource, SceneLayer, SizeSpec};
use aria_shapes::SdfParams;

use super::shapes::{canvas_pixel, layer_covers};
use crate::app::canvas_overlay::Rect;

/// The pick for a fixed-size layer on the quad the renderer measures for it:
/// the anchor folded into the centre, the size as resolved.
fn covers(layer: &SceneLayer, point: [f32; 2]) -> bool {
    let SizeSpec::Fixed(size) = layer.size else {
        panic!("the samples here are fixed-size layers");
    };
    layer_covers(layer, (layer.quad_center(size), size), point)
}

/// A layer with the fields a pick reads and defaults everywhere else.
fn layer(source: LayerSource, center: [f32; 2], size: [f32; 2]) -> SceneLayer {
    SceneLayer {
        clip: None,
        source,
        center,
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed(size),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

/// A click on the letterbox is a click on nothing.
///
/// Clamping instead would map every miss beside a 4:3 sequence onto the frame
/// edge and select whatever touches it, which reads as "clicking empty space
/// selects a random clip".
#[test]
fn the_letterbox_is_not_the_canvas() {
    let canvas = Rect::new(100.0, 50.0, 400.0, 200.0);
    assert_eq!(
        canvas_pixel(canvas, [100.0, 50.0], [800, 400]),
        Some([0.0, 0.0]),
        "the top-left corner of the picture is the canvas origin"
    );
    assert_eq!(
        canvas_pixel(canvas, [500.0, 250.0], [800, 400]),
        Some([800.0, 400.0]),
        "the bottom-right corner is the far canvas pixel"
    );
    assert_eq!(
        canvas_pixel(canvas, [300.0, 150.0], [800, 400]),
        Some([400.0, 200.0]),
        "the middle of the picture is the middle of the canvas"
    );
    for outside in [[99.0, 150.0], [501.0, 150.0], [300.0, 49.0], [300.0, 251.0]] {
        assert_eq!(
            canvas_pixel(canvas, outside, [800, 400]),
            None,
            "{outside:?} is outside the picture and was mapped anyway"
        );
    }
    assert_eq!(
        canvas_pixel(Rect::new(0.0, 0.0, 0.0, 0.0), [0.0, 0.0], [800, 400]),
        None,
        "a picture with no area answered a pick"
    );
}

/// A shape is picked by its ink, not by its quad.
///
/// The quad is padded for stroke overhang and the anti-aliasing ramp, and for a
/// star most of it is empty. Testing the quad would make the gaps between a
/// star's spikes select it, which is the whole reason `hit_test` exists.
#[test]
fn a_star_is_picked_by_its_ink_and_not_its_corners() {
    let pad = 4.0;
    let size = [200.0 + 2.0 * pad, 200.0 + 2.0 * pad];
    let star = layer(
        LayerSource::Shape {
            params: SdfParams::Star {
                points: 5,
                inner: 0.4,
                round: 0.0,
            },
            fill: [255, 255, 255, 255],
            stroke: None,
            pad,
        },
        [500.0, 500.0],
        size,
    );
    assert!(
        covers(&star, [500.0, 500.0]),
        "the centre of a star is not inside it"
    );
    // A quad corner is outside every star: the outer vertices sit on the
    // inscribed circle, so the corners of the box are always empty.
    let corner = [500.0 + size[0] * 0.5 - 1.0, 500.0 + size[1] * 0.5 - 1.0];
    assert!(
        !covers(&star, corner),
        "the empty corner of a star's quad selected it"
    );
    assert!(
        !covers(&star, [500.0 + size[0], 500.0]),
        "a point outside the quad selected the shape"
    );
}

/// Every other topology fills its quad, and a canvas pass is not pickable.
///
/// The `paints` column is read rather than a topology named here, so a topology
/// added to `aria-render` gets the pick's answer from its own row.
#[test]
fn a_canvas_pass_is_never_picked_and_a_solid_fills_its_quad() {
    let solid = layer(
        LayerSource::Solid([0, 0, 0, 255]),
        [100.0, 100.0],
        [50.0, 50.0],
    );
    assert!(covers(&solid, [100.0, 100.0]));
    assert!(covers(&solid, [124.0, 124.0]), "the quad edge is ink");
    assert!(!covers(&solid, [126.0, 100.0]), "outside the quad");

    let pass = layer(LayerSource::CanvasPass, [100.0, 100.0], [50.0, 50.0]);
    assert!(
        !pass.source.spec().paints,
        "a canvas pass claims to place pixels"
    );
    assert!(!covers(&pass, [100.0, 100.0]), "a canvas pass was picked");
}

/// Rotation moves the ink, so the pick has to unrotate before it compares.
#[test]
fn a_rotated_quad_is_picked_where_it_is_drawn() {
    let mut bar = layer(
        LayerSource::Solid([255, 0, 0, 255]),
        [100.0, 100.0],
        [200.0, 20.0],
    );
    // A point 60 px right of centre is inside the unrotated bar.
    assert!(covers(&bar, [160.0, 100.0]));
    assert!(!covers(&bar, [100.0, 160.0]));
    // Quarter turn: the same offsets swap.
    bar.rotation = std::f32::consts::FRAC_PI_2;
    assert!(covers(&bar, [100.0, 160.0]), "the bar did not rotate");
    assert!(
        !covers(&bar, [160.0, 100.0]),
        "the bar was picked where it no longer draws"
    );
}

/// The font preflight, driven the way the palette drives it.
///
/// The class this closes is a picking row that opens a dialog and drops the
/// answer: the report is the only evidence the bytes were read, so both of its
/// branches are driven — a real face, and a file that is not a font at all.
/// Asserted as "at least one face" rather than exactly one, because a
/// collection is the case the count exists for.
#[test]
fn the_font_preflight_reports_on_the_file_it_was_handed() {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    state.supply_pick(crate::app::picker::Answer::file(asset("fonts/Geist.ttf")));

    font_row(&mut state);

    assert_eq!(state.picks_asked(), 1, "the row did not ask for a file");
    let line = state
        .status_message
        .take()
        .expect("the row reported nothing");
    assert!(
        line.starts_with("Geist.ttf"),
        "the report must name the file that was picked: {line}"
    );
    assert!(
        line.contains("face(s) parsed"),
        "a shipped font parsed as nothing: {line}"
    );
    assert!(state.last_error.is_none(), "{:?}", state.last_error);
}

#[test]
fn a_file_that_is_not_a_font_is_reported_as_one_that_is_not() {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    state.supply_pick(crate::app::picker::Answer::file(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/texture.png"),
    ));

    font_row(&mut state);

    let line = state
        .status_message
        .take()
        .expect("the row reported nothing");
    assert!(
        line.contains("not a parseable font"),
        "a PNG was accepted as a font: {line}"
    );
}

/// A path inside this crate's assets, so the fixture ships with the test.
fn asset(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name)
}

/// The palette row itself, not the method behind it: the registry entry is the
/// part a new row can forget to wire.
fn font_row(state: &mut crate::app::AppState) {
    let row = crate::commands::BUILTIN_COMMANDS
        .iter()
        .find(|row| row.id == "pal-font-check")
        .expect("the font row must still be registered");
    (row.run)(state);
}
