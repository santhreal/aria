//! Pen path masks in the Viewer.
//!
//! # Why
//!
//! A path drawn on screen has to land where the renderer cuts the picture, and
//! every gesture on it has to be one undo step that keeps the rest of the mask.
//! The mapping is pinned against the renderer's own stencil, the curve
//! arithmetic against the curve it splits, and each gesture against the
//! commands it sends and the undo that takes it back.
//!
//! Not covered: the drawn elements and the pointer routing, which need a
//! window. `drawn_census_overlay.rs` drags every mask path gesture through the
//! registry the panel presses.

use aria_models::{ClipParam, MaskKind, MaskParam, Param, ShapePathPoint};

use super::*;
use crate::app::canvas_overlay::Rect;
use crate::app::sent_census::Watch;
use crate::app::transform_handle_tests::{framed, selected};
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::types::EditorTool;

/// The path [`masked_path`] puts on the clip: a corner, a smooth vertex, and
/// two corners, so there are vertices to drag, tangents to bend, and a straight
/// segment from the third vertex to the fourth to insert a vertex on.
pub(crate) const MASK_PATH: [ShapePathPoint; 4] = [
    ShapePathPoint {
        anchor: [0.2, 0.2],
        handle_in: [0.2, 0.2],
        handle_out: [0.2, 0.2],
    },
    ShapePathPoint {
        anchor: [0.8, 0.2],
        handle_in: [0.7, 0.1],
        handle_out: [0.9, 0.3],
    },
    ShapePathPoint {
        anchor: [0.8, 0.8],
        handle_in: [0.8, 0.8],
        handle_out: [0.8, 0.8],
    },
    ShapePathPoint {
        anchor: [0.2, 0.8],
        handle_in: [0.2, 0.8],
        handle_out: [0.2, 0.8],
    },
];

/// The selected clip with [`MASK_PATH`], closed, added as its active mask.
pub(crate) fn masked_path(state: &mut AppState) {
    state
        .add_selected_mask_path(MASK_PATH.to_vec())
        .expect("a path mask on the selected clip");
}

/// Where a mask path gesture is pressed on [`MASK_PATH`], in window pixels:
/// on the vertex or the tangent handle it names, and for the Pen halfway along
/// the straight segment from the third vertex to the fourth.
pub(crate) fn mask_path_press(state: &AppState, gesture: Gesture) -> [f32; 2] {
    let drawn = state
        .drawn_mask_path()
        .expect("a framed clip with a path mask draws it");
    match gesture {
        Gesture::MaskVertex(index)
        | Gesture::MaskVertexDelete(index)
        | Gesture::MaskVertexConvert(index) => drawn.vertices[index].at,
        Gesture::MaskTangent(index, side) => drawn.vertices[index]
            .handle(side)
            .expect("a smooth vertex draws both tangent handles"),
        Gesture::MaskPathPen => lerp(drawn.vertices[2].at, drawn.vertices[3].at, 0.5),
        other => panic!("{} is not pressed on a mask path", other.what()),
    }
}

fn near(a: [f32; 2], b: [f32; 2], within: f32) -> bool {
    length(sub(a, b)) <= within
}

fn step(state: &mut AppState, step: Step) {
    overlay_gesture::apply(state, step)
        .unwrap_or_else(|err| panic!("a press on the mask path was refused: {err}"));
}

/// The selected clip, framed, masked by [`MASK_PATH`].
fn with_path() -> AppState {
    let mut state = selected();
    framed(&mut state);
    masked_path(&mut state);
    state
}

/// The selected clip, framed, with no mask, and the Pen in hand.
fn with_pen() -> AppState {
    let mut state = selected();
    framed(&mut state);
    state.tool = EditorTool::Pen;
    state
}

/// The active mask's committed path at the playhead, or nothing.
fn committed(state: &AppState) -> Vec<ShapePathPoint> {
    let tick = state.selected_param_tick().unwrap_or_default();
    state
        .active_model_mask()
        .map(|mask| mask.path_at(tick as f64).into_owned())
        .unwrap_or_default()
}

/// Press, move by `by` window pixels, and release.
fn drag(state: &mut AppState, gesture: Gesture, from: [f32; 2], by: [f32; 2]) {
    step(state, Step::Grab(gesture, from));
    step(state, Step::Carry(add(from, by)));
    step(state, Step::Release);
}

/// A Pen click at layer fraction `at`, which is a path-space point while the
/// clip has no mask to shift or turn it.
fn pen_click(state: &mut AppState, at: [f32; 2]) {
    let layer = state
        .overlay_frame
        .layer
        .expect("`framed` placed the layer");
    let at = layer.at(at);
    assert!(
        state.pen_takes_press(at, false),
        "the Pen passed on a press on the picture"
    );
    step(state, Step::Grab(Gesture::MaskPathPen, at));
    step(state, Step::Release);
}

#[test]
fn window_and_path_space_round_trip_through_a_turn_and_a_shift() {
    let space = PathSpace::new(Rect::new(100.0, 50.0, 200.0, 100.0), [0.15, -0.1], 35.0);
    for x in [-0.5_f32, 0.0, 0.3, 0.5, 1.0, 1.7] {
        for y in [-0.25_f32, 0.0, 0.5, 0.9, 1.4] {
            let back = space.to_path(space.to_window([x, y]));
            assert!(near(back, [x, y], 1e-4), "({x}, {y}) came back as {back:?}");
        }
    }
}

#[test]
fn a_vertex_is_drawn_where_the_renderer_cuts_the_picture() {
    // The stencil comes from the renderer's own call, over a layer at the
    // window origin, so a window pixel is a stencil pixel. The square sits away
    // from the pivot: a turn the wrong way, a pivot at the layer centre rather
    // than the mask centre, or a dropped shift each put its centre on clear
    // stencil.
    let (w, h) = (200_u32, 100_u32);
    let layer = Rect::new(0.0, 0.0, w as f32, h as f32);
    let center = [0.2, -0.15];
    let rotation = 90.0_f32;
    let middle = [0.4_f32, 0.7];
    let half = 0.04;
    let square: Vec<aria_shapes::PathPoint> = [
        [middle[0] - half, middle[1] - half],
        [middle[0] + half, middle[1] - half],
        [middle[0] + half, middle[1] + half],
        [middle[0] - half, middle[1] + half],
    ]
    .map(|anchor| aria_shapes::PathPoint {
        anchor,
        handle_in: anchor,
        handle_out: anchor,
    })
    .to_vec();
    let stencil = aria_shapes::PathRaster::new().rasterize_mask(
        w,
        h,
        &square,
        center,
        rotation.to_radians(),
        0.0,
    );
    let alpha = |[x, y]: [f32; 2]| stencil.pixel(x.round() as u32, y.round() as u32)[3];
    let drawn = PathSpace::new(layer, center, rotation).to_window(middle);
    assert_eq!(
        alpha(drawn),
        255,
        "the square's center is drawn at {drawn:?}, off its stencil"
    );
    let unturned = PathSpace::new(layer, center, 0.0).to_window(middle);
    assert_eq!(
        alpha(unturned),
        0,
        "the stencil did not turn, so this proves nothing"
    );
}

#[test]
fn splitting_a_curve_keeps_its_shape() {
    let a = ShapePathPoint {
        anchor: [0.1, 0.8],
        handle_in: [0.1, 0.8],
        handle_out: [0.2, 0.1],
    };
    let b = ShapePathPoint {
        anchor: [0.9, 0.7],
        handle_in: [0.7, 0.0],
        handle_out: [0.9, 0.7],
    };
    let at = 0.35;
    let mut points = vec![a, b];
    let index = split_segment(&mut points, 0, at);
    assert_eq!((index, points.len()), (1, 3));
    assert_eq!((points[0].anchor, points[2].anchor), (a.anchor, b.anchor));
    for step in 0..=20 {
        let t = step as f32 / 20.0;
        let want = segment_at(&a, &b, t);
        let got = if t <= at {
            segment_at(&points[0], &points[1], t / at)
        } else {
            segment_at(&points[1], &points[2], (t - at) / (1.0 - at))
        };
        assert!(
            near(got, want, 1e-4),
            "at {t} the split curve is at {got:?}, not {want:?}"
        );
    }
}

#[test]
fn splitting_a_straight_closing_segment_adds_a_corner_at_the_end() {
    let mut points: Vec<ShapePathPoint> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]
        .map(ShapePathPoint::corner)
        .to_vec();
    let before = points.clone();
    assert_eq!(split_segment(&mut points, 2, 0.5), 3);
    assert_eq!(points[3], ShapePathPoint::corner([0.5, 0.5]));
    assert_eq!(points[..3], before[..], "a straight split moved a neighbor");
}

#[test]
fn a_vertex_turns_between_corner_and_smooth() {
    let mut points: Vec<ShapePathPoint> = [[0.2, 0.2], [0.5, 0.5], [0.8, 0.2]]
        .map(ShapePathPoint::corner)
        .to_vec();
    toggle_smooth(&mut points, 1);
    // Along the chord between the neighbours, a third of the way to each.
    let reach = length([0.3, 0.3]) / 3.0;
    assert!(
        near(points[1].handle_in, [0.5 - reach, 0.5], 1e-5),
        "{:?}",
        points[1]
    );
    assert!(
        near(points[1].handle_out, [0.5 + reach, 0.5], 1e-5),
        "{:?}",
        points[1]
    );
    toggle_smooth(&mut points, 1);
    assert_eq!(points[1], ShapePathPoint::corner([0.5, 0.5]));
}

#[test]
fn a_smooth_vertex_keeps_its_tangents_opposite_and_a_cusp_does_not() {
    let mut smooth = ShapePathPoint {
        anchor: [0.5, 0.5],
        handle_in: [0.4, 0.5],
        handle_out: [0.7, 0.5],
    };
    set_handle(&mut smooth, TangentSide::Out, [0.5, 0.8]);
    assert_eq!(smooth.handle_out, [0.5, 0.8]);
    assert!(
        near(smooth.handle_in, [0.5, 0.4], 1e-5),
        "the in handle did not turn opposite at its own length: {smooth:?}"
    );
    let mut cusp = ShapePathPoint {
        anchor: [0.5, 0.5],
        handle_in: [0.4, 0.4],
        handle_out: [0.7, 0.5],
    };
    set_handle(&mut cusp, TangentSide::Out, [0.5, 0.8]);
    assert_eq!(cusp.handle_in, [0.4, 0.4], "a cusp's other handle moved");
}

#[test]
fn the_pen_draws_a_path_that_one_undo_takes_away() {
    let mut state = with_pen();
    let watch = Watch::on(&state);
    let corners = [[0.3, 0.3], [0.7, 0.3], [0.5, 0.8]];
    for at in corners {
        pen_click(&mut state, at);
    }
    assert!(
        watch.take().is_empty(),
        "a path still being drawn wrote an edit"
    );
    let drawn = state.drawn_mask_path().expect("the draft is drawn");
    assert_eq!((drawn.vertices.len(), drawn.closed), (3, false));
    assert!(
        drawn.vertices.iter().all(|vertex| !vertex.smooth),
        "a click made a smooth vertex"
    );

    // A click on the first vertex closes the path.
    let gesture = state.mask_vertex_gesture(0, false, false);
    step(&mut state, Step::Grab(gesture, drawn.vertices[0].at));
    step(&mut state, Step::Release);
    assert_eq!(watch.take(), ["Add mask"]);
    assert!(!state.mask_path_drawing());
    let mask = state
        .active_model_mask()
        .cloned()
        .expect("closing the path masks the clip");
    assert_eq!((mask.kind, mask.path_closed), (MaskKind::Path, true));
    let anchors: Vec<[f32; 2]> = mask
        .path_at(0.0)
        .iter()
        .map(|vertex| vertex.anchor)
        .collect();
    assert!(
        anchors
            .iter()
            .zip(corners)
            .all(|(got, want)| near(*got, want, 1e-4)),
        "the path landed at {anchors:?}, not where it was clicked"
    );
    assert!(state.undo());
    assert!(
        state.selected_model().expect("clip").masks.is_empty(),
        "one undo left part of the path behind"
    );
}

#[test]
fn a_drag_while_drawing_pulls_a_smooth_vertex() {
    let mut state = with_pen();
    let layer = state.overlay_frame.layer.expect("framed");
    let from = layer.at([0.4, 0.4]);
    drag(&mut state, Gesture::MaskPathPen, from, [40.0, 0.0]);
    let draft = &state
        .tool_session
        .mask_path
        .as_ref()
        .expect("a draft")
        .points;
    let vertex = draft[0];
    let pulled = layer.fraction_of(add(from, [40.0, 0.0]));
    assert!(
        near(vertex.anchor, [0.4, 0.4], 1e-5),
        "the drag moved the vertex"
    );
    assert!(near(vertex.handle_out, pulled, 1e-5), "{vertex:?}");
    assert!(
        near(vertex.handle_in, [0.8 - pulled[0], 0.8 - pulled[1]], 1e-5),
        "the in tangent is not the out tangent mirrored: {vertex:?}"
    );
}

#[test]
fn enter_closes_only_a_path_with_area_and_escape_drops_it() {
    let mut state = with_pen();
    let watch = Watch::on(&state);
    pen_click(&mut state, [0.3, 0.3]);
    pen_click(&mut state, [0.7, 0.3]);
    state.finish_mask_path().expect("finish");
    assert!(
        watch.take().is_empty(),
        "two vertices were committed as a mask"
    );
    assert!(state.mask_path_drawing(), "two vertices dropped the draft");
    pen_click(&mut state, [0.5, 0.8]);
    state.finish_mask_path().expect("finish");
    assert_eq!(watch.take(), ["Add mask"]);
    assert_eq!(committed(&state).len(), 3);

    let mut state = with_pen();
    let watch = Watch::on(&state);
    for at in [[0.3, 0.3], [0.7, 0.3], [0.5, 0.8]] {
        pen_click(&mut state, at);
    }
    state.cancel_mask_path();
    assert!(watch.take().is_empty(), "Escape committed the draft");
    assert!(!state.mask_path_drawing());
    assert!(
        state.drawn_mask_path().is_none(),
        "the dropped draft is still drawn"
    );
}

#[test]
fn every_reshape_is_one_undo_step() {
    for (gesture, pen) in [
        (Gesture::MaskVertex(0), false),
        (Gesture::MaskTangent(1, TangentSide::In), false),
        (Gesture::MaskTangent(1, TangentSide::Out), false),
        (Gesture::MaskVertexConvert(0), true),
        (Gesture::MaskVertexDelete(0), true),
        (Gesture::MaskPathPen, true),
    ] {
        let mut state = with_path();
        if pen {
            state.tool = EditorTool::Pen;
        }
        let watch = Watch::on(&state);
        let from = mask_path_press(&state, gesture);
        drag(&mut state, gesture, from, [24.0, 12.0]);
        assert_eq!(watch.take(), ["Set mask"], "{}", gesture.what());
        assert_ne!(
            committed(&state),
            MASK_PATH,
            "{} changed nothing",
            gesture.what()
        );
        assert!(state.undo(), "{} left nothing to undo", gesture.what());
        assert_eq!(
            committed(&state),
            MASK_PATH,
            "one undo of {} missed",
            gesture.what()
        );
    }
}

#[test]
fn a_press_that_never_moved_writes_nothing() {
    for gesture in [
        Gesture::MaskVertex(2),
        Gesture::MaskTangent(1, TangentSide::Out),
    ] {
        let mut state = with_path();
        let watch = Watch::on(&state);
        let from = mask_path_press(&state, gesture);
        step(&mut state, Step::Grab(gesture, from));
        step(&mut state, Step::Release);
        assert!(
            watch.take().is_empty(),
            "a click on {} wrote an edit",
            gesture.what()
        );
        assert!(!state.overlay_dragging(), "the click left a hold behind");
    }
}

#[test]
fn an_alt_click_turns_a_vertex_and_a_vertex_tool_picks_its_gesture() {
    let mut state = with_path();
    state.tool = EditorTool::Pen;
    assert_eq!(
        state.mask_vertex_gesture(0, false, true),
        Gesture::MaskVertexConvert(0)
    );
    assert_eq!(
        state.mask_vertex_gesture(0, true, false),
        Gesture::MaskVertexDelete(0)
    );
    assert_eq!(
        state.mask_vertex_gesture(0, false, false),
        Gesture::MaskVertex(0)
    );
    state.set_pen_variant(PenVariant::ConvertVertex);
    assert_eq!(
        state.mask_vertex_gesture(0, false, false),
        Gesture::MaskVertexConvert(0)
    );
    state.set_pen_variant(PenVariant::DeleteVertex);
    assert_eq!(
        state.mask_vertex_gesture(0, false, false),
        Gesture::MaskVertexDelete(0)
    );
    state.set_pen_variant(PenVariant::Standard);
    state.tool = EditorTool::default();
    assert_eq!(
        state.mask_vertex_gesture(0, true, true),
        Gesture::MaskVertex(0)
    );

    state.tool = EditorTool::Pen;
    for index in [0, 1] {
        let gesture = Gesture::MaskVertexConvert(index);
        let at = mask_path_press(&state, gesture);
        step(&mut state, Step::Grab(gesture, at));
        step(&mut state, Step::Release);
    }
    let path = committed(&state);
    assert!(
        is_smooth(&path[0]),
        "the corner stayed a corner: {:?}",
        path[0]
    );
    assert_eq!(path[1], ShapePathPoint::corner(MASK_PATH[1].anchor));
}

#[test]
fn a_path_keeps_three_vertices() {
    let mut state = with_path();
    state.tool = EditorTool::Pen;
    let watch = Watch::on(&state);
    state.delete_mask_vertex(0).expect("delete");
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(committed(&state).len(), 3);
    state.delete_mask_vertex(0).expect("delete");
    assert!(watch.take().is_empty(), "a path went below three vertices");
    assert_eq!(committed(&state).len(), 3);
}

#[test]
fn a_reshape_keeps_the_rest_of_the_mask() {
    // The path command carries the whole mask, so a reshape built the mask from
    // the projection and flattened every keyframe on it.
    let mut state = with_path();
    let clip = state.selected_clip().expect("selected");
    let start = state
        .selected_model()
        .expect("selected")
        .timeline
        .start
        .value;
    state
        .add_param_keyframe_at(
            clip,
            ClipParam::Mask {
                mask: 0,
                param: MaskParam::Feather,
            },
            start,
        )
        .expect("key the feather");
    let from = mask_path_press(&state, Gesture::MaskVertex(0));
    drag(&mut state, Gesture::MaskVertex(0), from, [24.0, 12.0]);
    let mask = state.active_model_mask().cloned().expect("a mask");
    assert_ne!(*mask.path_at(0.0), MASK_PATH, "the drag changed nothing");
    assert!(
        matches!(mask.feather, Param::Keyframed { .. }),
        "the reshape flattened the feather: {:?}",
        mask.feather
    );
}

#[test]
fn a_path_on_another_clip_is_not_this_clips_draft() {
    let mut state = with_pen();
    pen_click(&mut state, [0.3, 0.3]);
    let other = aria_models::ClipId::from_raw(u64::MAX);
    if let Some(edit) = state.tool_session.mask_path.as_mut() {
        edit.clip = other;
    }
    assert!(!state.mask_path_drawing());
    assert!(
        state.drawn_mask_path().is_none(),
        "another clip's draft is drawn here"
    );
}

/// A Pen press off the active path starts a new path, and closing it adds a
/// mask. The new path draws unshifted and unturned whatever the active mask's
/// centre and rotation are, because `Mask::from_path` gives it neither: a draft
/// drawn in the active mask's space lands somewhere other than where it was
/// clicked.
#[test]
fn a_pen_press_off_the_active_path_adds_a_new_mask() {
    let mut state = with_path();
    state
        .set_selected_mask_geometry([0.1, 0.05], [1.0, 1.0], 30.0)
        .expect("shift and turn the path mask");
    state.tool = EditorTool::Pen;
    let watch = Watch::on(&state);
    // Each corner is clear of every segment of `MASK_PATH`.
    let corners = [[0.05, 0.05], [0.95, 0.05], [0.05, 0.95]];
    for at in corners {
        pen_click(&mut state, at);
    }
    assert!(watch.take().is_empty(), "the draft wrote an edit");
    state.finish_mask_path().expect("finish");
    assert_eq!(watch.take(), ["Add mask"]);

    let masks = &state.selected_model().expect("clip").masks;
    assert_eq!(masks.len(), 2, "the new path replaced the mask it missed");
    assert_eq!(
        masks[0].path,
        Param::Constant(MASK_PATH.to_vec()),
        "the press reshaped the active path"
    );
    assert_eq!(
        state.active_mask(),
        Some(1),
        "the new mask is not the active one"
    );
    let anchors: Vec<[f32; 2]> = masks[1]
        .path_at(0.0)
        .iter()
        .map(|vertex| vertex.anchor)
        .collect();
    assert!(
        anchors
            .iter()
            .zip(corners)
            .all(|(got, want)| near(*got, want, 1e-4)),
        "the new path landed at {anchors:?}, not where it was clicked"
    );

    // A vertex tool edits a path that exists and starts none.
    let mut state = with_path();
    state.tool = EditorTool::Pen;
    state.set_pen_variant(PenVariant::AddVertex);
    let watch = Watch::on(&state);
    pen_click(&mut state, [0.05, 0.05]);
    assert!(
        !state.mask_path_drawing(),
        "the Add Vertex tool started a path"
    );
    assert!(watch.take().is_empty());
}

/// The tick [`keyed_path`] keys the path at the second time.
const KEYED_LATER: i64 = 16;

/// [`with_path`] with the path keyframed at tick 0 and the playhead at
/// [`KEYED_LATER`], where no keyframe sits yet.
fn keyed_path() -> AppState {
    let mut state = with_path();
    let clip = state.selected_clip().expect("selected");
    state.set_playhead(0);
    state
        .animate_mask_path(clip, 0)
        .expect("the stopwatch keys the path at 0");
    state.set_playhead(KEYED_LATER);
    state
}

/// The ticks of the active mask's path keyframes and each one's vertex count.
fn path_keys(state: &AppState) -> Vec<(i64, usize)> {
    state
        .active_model_mask()
        .expect("a mask")
        .path
        .keyframes()
        .iter()
        .map(|key| (key.tick, key.value.len()))
        .collect()
}

#[test]
fn a_reshape_of_a_keyframed_path_keys_it_at_the_playhead_alone() {
    for (gesture, pen) in [
        (Gesture::MaskVertex(0), false),
        (Gesture::MaskTangent(1, TangentSide::Out), false),
        (Gesture::MaskVertexConvert(0), true),
    ] {
        let mut state = keyed_path();
        if pen {
            state.tool = EditorTool::Pen;
        }
        let watch = Watch::on(&state);
        let from = mask_path_press(&state, gesture);
        drag(&mut state, gesture, from, [24.0, 12.0]);
        assert_eq!(watch.take(), ["Set mask"], "{}", gesture.what());
        assert_eq!(
            path_keys(&state),
            [(0, 4), (KEYED_LATER, 4)],
            "{} keyed the path somewhere else",
            gesture.what()
        );
        let mask = state.active_model_mask().cloned().expect("a mask");
        assert_eq!(
            *mask.path_at(0.0),
            MASK_PATH,
            "{} moved the keyframe the playhead was not on",
            gesture.what()
        );
        assert_ne!(
            *mask.path_at(KEYED_LATER as f64),
            MASK_PATH,
            "{} changed nothing",
            gesture.what()
        );
        assert!(state.undo(), "{} left nothing to undo", gesture.what());
        assert_eq!(
            path_keys(&state),
            [(0, 4)],
            "one undo of {}",
            gesture.what()
        );
    }
}

#[test]
fn a_vertex_added_or_deleted_at_one_time_reaches_every_keyframe() {
    let mut state = keyed_path();
    // Two keyframes with different shapes.
    let from = mask_path_press(&state, Gesture::MaskVertex(0));
    drag(&mut state, Gesture::MaskVertex(0), from, [24.0, 12.0]);
    assert_eq!(path_keys(&state), [(0, 4), (KEYED_LATER, 4)]);

    // A Pen press between the keyframes splits the path there, keys it there,
    // and splits each keyframe on the same segment.
    state.tool = EditorTool::Pen;
    state.set_playhead(KEYED_LATER / 2);
    let watch = Watch::on(&state);
    let at = mask_path_press(&state, Gesture::MaskPathPen);
    drag(&mut state, Gesture::MaskPathPen, at, [6.0, 4.0]);
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(
        path_keys(&state),
        [(0, 5), (KEYED_LATER / 2, 5), (KEYED_LATER, 5)],
        "a keyframe kept its old vertex count"
    );
    let first = state
        .active_model_mask()
        .expect("a mask")
        .path_at(0.0)
        .into_owned();
    assert_eq!(
        [first[0], first[1], first[2], first[4]],
        MASK_PATH,
        "the split moved the first keyframe's vertices"
    );

    // A Ctrl+click on a vertex at the first keyframe takes it from all three.
    state.set_playhead(0);
    state.delete_mask_vertex(3).expect("delete");
    assert_eq!(watch.take(), ["Set mask"]);
    assert_eq!(
        path_keys(&state),
        [(0, 4), (KEYED_LATER / 2, 4), (KEYED_LATER, 4)]
    );
}
