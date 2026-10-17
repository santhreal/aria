//! Ruler and guide contracts: rulers number canvas pixels at a readable step
//! for any zoom, a guide pulled from a ruler lands on the canvas pixel under
//! the pointer and is deleted when dropped back on its ruler or off the
//! canvas, and a moved layer snaps to the nearest guide only within reach.
//!
//! Not covered: the drawn rulers and guide lines, which the shell tests and
//! the headed captures show.

use super::*;
use crate::app::canvas_overlay::FrameBox;
use crate::app::premiere_tests::with_clip;

/// A 1920x1080 canvas drawn at half size, 100 px right and 50 px down.
fn half() -> Rect {
    Rect::new(100.0, 50.0, 960.0, 540.0)
}

const HD: [u32; 2] = [1920, 1080];

#[test]
fn the_ruler_step_keeps_numbers_apart_at_every_zoom() {
    for (scale, step) in [
        (0.05, 2500),
        (0.25, 250),
        (0.5, 250),
        (1.0, 100),
        (4.0, 25),
        (20.0, 5),
    ] {
        assert_eq!(ruler_step(scale), step, "scale {scale}");
        if step < 2500 {
            assert!(step as f32 * scale >= LABEL_SPACING_PX, "scale {scale}");
        }
    }
    assert_eq!(
        ruler_step(0.0),
        2500,
        "a collapsed canvas numbers nothing densely"
    );
    assert_eq!(ruler_step(f32::NAN), 2500);
}

#[test]
fn ruler_ticks_number_canvas_pixels_from_the_canvas_origin() {
    assert_eq!(
        ruler_ticks(0.0, 200.0, 50.0, 1.0),
        vec![(50.0, 0), (150.0, 100)]
    );
    // Panned so the canvas starts left of the ruler: negative numbers stay
    // off, the visible ones keep their canvas values.
    assert_eq!(
        ruler_ticks(0.0, 120.0, -130.0, 1.0),
        vec![(-30.0 + 100.0, 200)]
            .into_iter()
            .filter(|(screen, _)| *screen <= 120.0)
            .chain([(70.0, 200)])
            .take(1)
            .collect::<Vec<_>>()
    );
    assert!(ruler_ticks(10.0, 10.0, 0.0, 1.0).is_empty(), "no span");
    assert!(ruler_ticks(0.0, 100.0, 0.0, 0.0).is_empty(), "no scale");
}

#[test]
fn guide_positions_round_trip_between_screen_and_canvas() {
    let vertical = Guide {
        axis: GuideAxis::Vertical,
        at: 960.0,
    };
    assert_eq!(guide_screen(vertical, half(), HD), Some(580.0));
    assert_eq!(
        canvas_at(GuideAxis::Vertical, 580.0, half(), HD),
        Some(960.0)
    );
    let horizontal = Guide {
        axis: GuideAxis::Horizontal,
        at: 540.0,
    };
    assert_eq!(guide_screen(horizontal, half(), HD), Some(320.0));
    // A pointer between two canvas pixels lands on the nearer one.
    assert_eq!(
        canvas_at(GuideAxis::Horizontal, 320.4, half(), HD),
        Some(541.0)
    );
    assert_eq!(canvas_at(GuideAxis::Vertical, 0.0, half(), [0, 0]), None);
}

#[test]
fn a_layer_snaps_its_nearest_edge_within_reach_on_each_axis() {
    let guides = [
        Guide {
            axis: GuideAxis::Vertical,
            at: 960.0,
        },
        Guide {
            axis: GuideAxis::Horizontal,
            at: 200.0,
        },
    ];
    // The vertical guide is at screen x 580, the horizontal at y 150.
    let near = Rect::new(483.0, 156.0, 100.0, 50.0);
    // Right edge 583 is 3 px past 580; top 156 is 6 px below 150.
    assert_eq!(snap_shift(near, half(), HD, &guides), [-3.0, -6.0]);
    // The center wins when it is the closest of the three edges.
    let centered = Rect::new(529.0, 400.0, 100.0, 50.0);
    assert_eq!(snap_shift(centered, half(), HD, &guides), [1.0, 0.0]);
    // Out of reach on both axes.
    let far = Rect::new(400.0, 300.0, 100.0, 50.0);
    assert_eq!(snap_shift(far, half(), HD, &guides), [0.0, 0.0]);
    assert_eq!(snap_shift(near, half(), HD, &[]), [0.0, 0.0]);
}

/// A state whose Program view drew the canvas at [`half`] inside a pane at
/// (100, 50).
fn drawn() -> crate::app::AppState {
    let (mut state, _) = with_clip();
    let size = state.canvas_size();
    state.overlay_frame = FrameBox {
        container: Some(Rect::new(100.0, 50.0, 960.0, 540.0)),
        canvas: Some(Rect::new(
            100.0,
            50.0,
            960.0,
            960.0 * size[1] as f32 / size[0] as f32,
        )),
        layer: None,
    };
    state
}

#[test]
fn a_pulled_guide_follows_the_pointer_and_stays_on_release() {
    let mut state = drawn();
    let size = state.canvas_size();
    let per_px = 960.0 / size[0] as f32;
    state.pull_guide(GuideAxis::Vertical, [104.0, 300.0]);
    assert_eq!(state.viewer_guides.dragging, Some(0));
    state.drag_guide([100.0 + 400.0 * per_px, 300.0]);
    state.release_guide([100.0 + 400.0 * per_px, 300.0]);
    assert_eq!(state.viewer_guides.dragging, None);
    assert_eq!(
        state.viewer_guides.guides,
        vec![Guide {
            axis: GuideAxis::Vertical,
            at: 400.0
        }]
    );
}

#[test]
fn a_guide_dropped_on_its_ruler_or_off_the_canvas_is_deleted() {
    let mut state = drawn();
    let size = state.canvas_size();
    let per_px = 960.0 / size[0] as f32;
    // Back onto the left ruler.
    state.pull_guide(GuideAxis::Vertical, [200.0, 300.0]);
    state.drag_guide([105.0, 300.0]);
    state.release_guide([105.0, 300.0]);
    assert!(state.viewer_guides.guides.is_empty());
    // Past the right edge of the canvas.
    state.pull_guide(GuideAxis::Vertical, [200.0, 300.0]);
    let past = 100.0 + (size[0] as f32 + 40.0) * per_px;
    state.drag_guide([past, 300.0]);
    state.release_guide([past, 300.0]);
    assert!(state.viewer_guides.guides.is_empty());
    // A horizontal guide dropped on the top ruler, after a second one stayed.
    state.pull_guide(GuideAxis::Horizontal, [300.0, 120.0]);
    state.release_guide([300.0, 120.0]);
    state.pull_guide(GuideAxis::Horizontal, [300.0, 200.0]);
    state.release_guide([300.0, 55.0]);
    assert_eq!(state.viewer_guides.guides.len(), 1);
    assert_eq!(state.viewer_guides.guides[0].axis, GuideAxis::Horizontal);
}

#[test]
fn snapping_is_off_until_turned_on_and_clear_removes_every_guide() {
    let mut state = drawn();
    state.pull_guide(GuideAxis::Vertical, [300.0, 300.0]);
    state.release_guide([300.0, 300.0]);
    let layer = Rect::new(297.0, 100.0, 50.0, 50.0);
    assert_eq!(state.guide_snap_shift(layer), [0.0, 0.0], "snap is off");
    state.toggle_guide_snap();
    assert_eq!(state.guide_snap_shift(layer), [3.0, 0.0]);
    state.clear_guides();
    assert!(state.viewer_guides.guides.is_empty());
    assert_eq!(state.guide_snap_shift(layer), [0.0, 0.0]);
}

#[test]
fn hidden_guides_are_kept_and_neither_snap_nor_survive_a_new_pull_hidden() {
    let mut state = drawn();
    state.pull_guide(GuideAxis::Vertical, [300.0, 300.0]);
    state.release_guide([300.0, 300.0]);
    state.toggle_guide_snap();
    let layer = Rect::new(297.0, 100.0, 50.0, 50.0);
    assert_eq!(state.guide_snap_shift(layer), [3.0, 0.0]);

    state.toggle_guides_shown();
    assert!(state.viewer_guides.hidden);
    assert_eq!(
        state.viewer_guides.guides.len(),
        1,
        "hiding keeps the guide"
    );
    assert_eq!(
        state.guide_snap_shift(layer),
        [0.0, 0.0],
        "a hidden guide does not snap"
    );

    state.toggle_guides_shown();
    assert_eq!(
        state.guide_snap_shift(layer),
        [3.0, 0.0],
        "shown, it snaps again"
    );

    // Pulling a guide while they are hidden shows them all, so the new one is
    // not dropped into an invisible set.
    state.toggle_guides_shown();
    state.pull_guide(GuideAxis::Horizontal, [300.0, 200.0]);
    state.release_guide([300.0, 200.0]);
    assert!(!state.viewer_guides.hidden);
    assert_eq!(state.viewer_guides.guides.len(), 2);
}
