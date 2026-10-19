//! The selection box and the canvas pick of a layer sized by its raster.
//!
//! A title or pen path has no size until the renderer rasterizes it, and the
//! handles and the pick both stopped there: no box was drawn around a title, and
//! a click on one selected whatever lay beneath it. Both now read the quad the
//! renderer measures, which `aria-render`'s conformance sweep holds against the
//! pixels a composite inks. What this does not catch: a quad a few pixels off
//! the ink, which is the sweep's job.

use aria_models::{ClipId, Generator, Shape, ShapePath, ShapePathPoint};

use crate::app::AppState;
use crate::app::canvas_overlay::Rect;
use crate::app::premiere_tests::with_clip;
use crate::app::text_tests::with_title;

/// A 1920×1080 canvas laid over the window one to one, so a canvas pixel is a
/// click. Explicit, so the size does not follow whatever the pool derives.
fn aim(state: &mut AppState) {
    state
        .set_canvas_resolution(Some([1920, 1080]))
        .expect("set the canvas size");
    state.overlay_frame.canvas = Some(Rect::new(0.0, 0.0, 1920.0, 1080.0));
}

/// The clip on the timeline that is not `clip`.
fn other(state: &AppState, clip: ClipId) -> ClipId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|ui| ClipId::from_raw(ui.id))
        .find(|id| *id != clip)
        .expect("a second clip")
}

#[test]
fn a_title_has_a_selection_box_and_a_click_on_it_selects_it() {
    let mut state = with_title();
    let title = state.selected_clip().expect("the title is selected");
    let under = other(&state, title);
    aim(&mut state);
    let (center, size) = state.selected_layer_quad().expect("a title has a quad");
    assert!(
        size[0] > 0.0 && size[1] > 0.0 && size[0] < 1920.0 && size[1] < 1080.0,
        "a title's box is its raster, not the canvas: {size:?}"
    );
    state.select_clip(under);
    assert!(
        state.select_layer_at(center, false),
        "a click on the title missed it"
    );
    assert_eq!(state.selected_clip(), Some(title));
    let beside = [center[0] + size[0] * 0.5 + 4.0, center[1]];
    assert!(
        state.select_layer_at(beside, false),
        "a click beside the title did not reach the picture beneath"
    );
    assert_eq!(state.selected_clip(), Some(under));
}

/// The empty half of a right triangle's box is a click on what lies beneath.
#[test]
fn a_pen_path_has_a_selection_box_and_is_picked_by_its_ink() {
    let (mut state, under) = with_clip();
    let corner = ShapePathPoint::corner;
    let triangle = ShapePath {
        points: vec![
            corner([-100.0, -100.0]),
            corner([100.0, -100.0]),
            corner([-100.0, 100.0]),
        ],
        closed: true,
    };
    state
        .add_generated(Generator::shape(
            Shape::Path(triangle),
            [255, 255, 255, 255],
        ))
        .expect("place a pen path");
    let path = other(&state, under);
    state.select_clip(path);
    aim(&mut state);
    let (center, size) = state.selected_layer_quad().expect("a pen path has a quad");
    assert!(size[0] > 0.0 && size[1] > 0.0, "{size:?}");
    state.select_clip(under);
    let inked = [center[0] - size[0] * 0.3, center[1] - size[1] * 0.3];
    assert!(
        state.select_layer_at(inked, false),
        "a click on the path's ink missed it"
    );
    assert_eq!(state.selected_clip(), Some(path));
    let empty = [center[0] + size[0] * 0.3, center[1] + size[1] * 0.3];
    assert!(
        state.select_layer_at(empty, false),
        "a click in the empty half of the path's box did not reach the picture beneath"
    );
    assert_eq!(state.selected_clip(), Some(under));
}

/// A registered effect that draws past its layer is picked where it draws. A
/// click inside the margin its spec declares selects the layer; a click past
/// that margin, or beside the side it does not grow, reaches the picture
/// beneath.
///
/// Catches: a pick that ignores effect bounds, and a margin applied to the
/// wrong side. Does not catch a margin the compositor fails to draw.
#[test]
fn a_click_inside_an_effect_margin_selects_the_layer_drawing_it() {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        use aria_compositor::{EffectDefinition, register_effect};
        use aria_models::{EffectBounds, EffectSpec};

        let _registration = register_effect(EffectDefinition {
            spec: EffectSpec {
                id: "test.ui.pick_margin".into(),
                label: "Pick Margin".into(),
                bounds: EffectBounds::new(0.0, 0.0, 200.0, 0.0),
                ..Default::default()
            },
            passes: vec![
                "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return textureSample(input_tex, aria_sampler, in.uv);\n}\n"
                    .into(),
            ],
        })
        .expect("register the margin effect");
        let mut state = with_title();
        let title = state.selected_clip().expect("the title is selected");
        let under = other(&state, title);
        aim(&mut state);
        state
            .add_effect_to_selected("test.ui.pick_margin")
            .expect("add the margin effect");
        let (center, size) = state.selected_layer_quad().expect("a title has a quad");
        let right = center[0] + size[0] * 0.5;
        let left = center[0] - size[0] * 0.5;
        for (x, picked, what) in [
            (right + 100.0, title, "inside the right margin"),
            (right + 300.0, under, "past the right margin"),
            (
                left - 100.0,
                under,
                "beside the left side, which has no margin",
            ),
        ] {
            state.select_clip(if picked == title { under } else { title });
            state.select_layer_at([x, center[1]], false);
            assert_eq!(state.selected_clip(), Some(picked), "a click {what}");
        }
    });
}
