//! Tests for the curve editor: polyline resolution against model samples, drag
//! mapping through the state that owns it, and easing preservation.
//!
//! # Class of Defect Closed
//! - Polyline rendering diverging from model sampler evaluation (true-to-playback guarantee).
//! - A drag nothing carries: the panel built a fresh drag state per press, so a
//!   press started a gesture no move continued and no release committed. The
//!   graph could be drawn and hit-tested, and no keyframe could be moved.
//! - A keyframe moved in time costing two undo entries instead of one.
//! - A pointer read in graph pixels while it arrives in window pixels.
//! - Easing selection corrupting neighbouring keyframe easings or values.
//! - A graph that can only draw one property, while the model animates 58.
//! - Empty or non-animated properties crashing during curve editor rendering.
//! - A graph drawing a constant property while another holds keyframes.
//! - A keyframe plotted on the graph's border, where its rounded corner cuts the
//!   mark.
//! - A handle mirrored across its segment, off the curve's tangent, and a
//!   handle drag bending the curve away from the pointer.
//! - A handle taking the press meant for the keyframe under it.
//! - A handle's tip painted with no lever to the keyframe it bends.

use aria_models::{ClipParam, EASING_PRESETS, Easing, ParamKind, TemporalEase, clip_params_for};

use super::picker::{GraphInk, GraphMark, HANDLE_DOT_SIZE, KEYFRAME_DOT_SIZE, graph_marks};
use crate::app::AppState;
use crate::app::curve_drag::{CurveDragTarget, HIT_RADIUS};
use crate::app::curve_geometry::{BezierHandleKind, build_curve_polyline, sample_param_scalar};
use crate::app::premiere_tests::with_clip;

/// Where the panel drew the graph. Origin is deliberately not `[0, 0]`: a
/// pointer arrives in window coordinates, and a conversion that forgets the
/// origin still passes when the graph starts at the window's corner.
const FRAME: [f32; 4] = [10.0, 20.0, 300.0, 150.0];

/// A state with a selected clip and a measured graph.
fn graphed() -> (AppState, i64, i64) {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    assert!(state.record_curve_frame(FRAME), "the first measure lands");
    (state, start, end)
}

/// Add a keyframe on `param` at `tick` carrying `value`.
fn key(state: &mut AppState, param: ClipParam, tick: i64, value: f32) {
    state.set_playhead(tick);
    state.add_keyframe_at_playhead(param).expect("add keyframe");
    state
        .set_keyframe_scalar(param, tick, value)
        .expect("write value");
}

/// A graph pixel as the window pixel the panel would report for it.
fn window_pixel(state: &AppState, param: ClipParam, tick: i64, value: f32) -> (f32, f32) {
    let bounds = state.curve_bounds(param, None).expect("bounds");
    let (x, y) = bounds.to_pixel(tick, value);
    (x + FRAME[0], y + FRAME[1])
}

/// What a press at graph pixel `(x, y)` on `param` grabs, released at no cost.
fn grabbed(state: &mut AppState, param: ClipParam, x: f32, y: f32) -> Option<CurveDragTarget> {
    state.begin_curve_drag(param, None, x + FRAME[0], y + FRAME[1]);
    let target = state.curve_dragging().map(|draft| draft.target);
    state.cancel_curve_drag();
    target
}

/// Every easing a segment can leave a keyframe with: the named ones, each
/// shipped preset, and temporal ones with and without a hold.
fn easings() -> Vec<Easing> {
    let mut easings = vec![
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Hold,
        Easing::EaseOut.with_temporal_ease(None, Some(TemporalEase::EASY_EASE), false),
        Easing::Linear.with_temporal_ease(None, Some(TemporalEase::new(0.0, 0.75)), false),
        Easing::Hold.with_temporal_ease(None, Some(TemporalEase::EASY_EASE), true),
    ];
    easings.extend(EASING_PRESETS.iter().map(|preset| Easing::Bezier {
        points: preset.points,
    }));
    easings
}

/// Whether the graph draws handles on a segment leaving `easing`. Exhaustive
/// on purpose: a new easing variant stops this compiling until it is placed.
fn draws_handles(easing: Easing) -> bool {
    match easing {
        Easing::Linear
        | Easing::EaseIn
        | Easing::EaseOut
        | Easing::EaseInOut
        | Easing::Bezier { .. }
        | Easing::Temporal { hold: false, .. } => true,
        Easing::Hold | Easing::Temporal { hold: true, .. } => false,
    }
}

#[test]
fn the_polyline_equals_the_resolvers_samples_at_the_same_ticks() {
    let (mut state, start, _) = graphed();
    let span = state
        .selected_model()
        .expect("model")
        .timeline
        .duration
        .value;

    key(&mut state, ClipParam::Opacity, start, 0.2);
    let t1 = start + span / 4;
    key(&mut state, ClipParam::Opacity, t1, 0.8);
    state
        .set_keyframe_easing(ClipParam::Opacity, start, Easing::Linear)
        .expect("linear");
    let t2 = start + span / 2;
    key(&mut state, ClipParam::Opacity, t2, 0.4);
    state
        .set_keyframe_easing(ClipParam::Opacity, t1, Easing::EaseInOut)
        .expect("ease in out");
    let t3 = start + (span * 3) / 4;
    key(&mut state, ClipParam::Opacity, t3, 1.0);
    state
        .set_keyframe_easing(
            ClipParam::Opacity,
            t2,
            Easing::Bezier {
                points: [0.25, 0.1, 0.25, 1.0],
            },
        )
        .expect("bezier");

    let polyline = state
        .curve_polyline(ClipParam::Opacity, None)
        .expect("polyline");
    let clip = state.selected_model().expect("model");
    let spec = &state.key_param(ClipParam::Opacity).expect("row").spec;
    for &(_px, _py, tick, val) in &polyline.samples {
        let local = (tick - start as f64).clamp(0.0, span as f64);
        let expected = sample_param_scalar(spec, clip, None, local).expect("sample");
        assert!(
            (val - expected).abs() < 1e-5,
            "polyline value {val} disagrees with model sampler {expected} at tick {tick}"
        );
    }
}

#[test]
fn a_drag_moves_the_keyframe_the_pointer_names_and_one_undo_puts_it_back() {
    let (mut state, start, end) = graphed();
    let origin_tick = start + 10;
    key(&mut state, ClipParam::Opacity, origin_tick, 0.3);
    state
        .set_keyframe_easing(ClipParam::Opacity, origin_tick, Easing::EaseInOut)
        .expect("easing to preserve");

    let (grab_x, grab_y) = window_pixel(&state, ClipParam::Opacity, origin_tick, 0.3);
    assert!(
        state.begin_curve_drag(ClipParam::Opacity, None, grab_x, grab_y),
        "the press grabs the keyframe under it"
    );

    let target_tick = start + (end - start) / 2;
    let target_val = 0.75_f32;
    let (to_x, to_y) = window_pixel(&state, ClipParam::Opacity, target_tick, target_val);
    let before = state.engine().lock().revision();
    assert!(state.update_curve_drag(to_x, to_y), "the draft follows");
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a pointer move is a draft: nothing reaches the engine"
    );

    state.commit_curve_drag().expect("commit");
    let marks = state.keyframes_of(ClipParam::Opacity);
    assert_eq!(marks.len(), 1, "the keyframe moved rather than multiplied");
    assert_eq!(marks[0].tick, target_tick);
    let sampled = sample_param_scalar(
        &state.key_param(ClipParam::Opacity).expect("row").spec,
        state.selected_model().expect("model"),
        None,
        (target_tick - start) as f64,
    )
    .expect("sample");
    assert!(
        (sampled - target_val).abs() < 1e-2,
        "moved keyframe carries {sampled}, not {target_val}"
    );
    // The shape of the segment leaving the keyframe belongs to the keyframe.
    assert_eq!(
        state.keyframe_easing_at(ClipParam::Opacity, target_tick),
        Some(Easing::EaseInOut)
    );

    // A remove plus a write is one gesture, so it is one undo.
    assert!(state.undo(), "undo the move");
    let restored = state.keyframes_of(ClipParam::Opacity);
    assert_eq!(restored.len(), 1);
    assert_eq!(
        restored[0].tick, origin_tick,
        "one undo left the keyframe half-moved"
    );
}

#[test]
fn a_drag_that_lands_where_it_started_writes_nothing() {
    let (mut state, start, _) = graphed();
    let tick = start + 8;
    key(&mut state, ClipParam::Opacity, tick, 0.5);

    let (x, y) = window_pixel(&state, ClipParam::Opacity, tick, 0.5);
    assert!(state.begin_curve_drag(ClipParam::Opacity, None, x, y));
    let before = state.engine().lock().revision();
    assert!(
        !state.update_curve_drag(x, y),
        "a move onto the same tick and value changes nothing"
    );
    state.commit_curve_drag().expect("release");
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a click on a keyframe is not an undo step"
    );
    assert!(state.curve_dragging().is_none());
}

#[test]
fn a_press_on_empty_graph_grabs_nothing_and_a_cancel_costs_nothing() {
    let (mut state, start, _) = graphed();
    let tick = start + 8;
    key(&mut state, ClipParam::Opacity, tick, 0.5);

    // Far from the only keyframe, and outside the graph entirely.
    let (x, y) = window_pixel(&state, ClipParam::Opacity, start, 1.0);
    assert!(!state.begin_curve_drag(ClipParam::Opacity, None, x + 120.0, y + 60.0));
    assert!(state.curve_dragging().is_none());
    assert!(
        !state.update_curve_drag(x, y),
        "no drag in flight, no draft to move"
    );

    let before = state.engine().lock().revision();
    let (gx, gy) = window_pixel(&state, ClipParam::Opacity, tick, 0.5);
    assert!(state.begin_curve_drag(ClipParam::Opacity, None, gx, gy));
    assert!(state.update_curve_drag(gx + 40.0, gy - 20.0));
    state.cancel_curve_drag();
    assert!(state.curve_dragging().is_none());
    state.commit_curve_drag().expect("nothing to commit");
    assert_eq!(state.engine().lock().revision(), before);
}

#[test]
fn a_pointer_is_read_against_where_the_graph_was_drawn() {
    let (mut state, start, _) = graphed();
    let tick = start + 12;
    key(&mut state, ClipParam::Opacity, tick, 0.4);

    let bounds = state
        .curve_bounds(ClipParam::Opacity, None)
        .expect("bounds");
    let (graph_x, graph_y) = bounds.to_pixel(tick, 0.4);
    // The same pixel in graph coordinates is not the same pixel in window
    // coordinates: reading one as the other is how a grab misses.
    assert!(!state.begin_curve_drag(ClipParam::Opacity, None, graph_x, graph_y));
    assert!(state.begin_curve_drag(
        ClipParam::Opacity,
        None,
        graph_x + FRAME[0],
        graph_y + FRAME[1]
    ));
    state.cancel_curve_drag();
}

#[test]
fn a_graph_with_no_measured_frame_hit_tests_nothing() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    let tick = ui.timeline_start_ticks + 4;
    key(&mut state, ClipParam::Opacity, tick, 0.6);
    assert!(state.curve_bounds(ClipParam::Opacity, None).is_none());
    assert!(state.curve_polyline(ClipParam::Opacity, None).is_none());
    assert!(!state.begin_curve_drag(ClipParam::Opacity, None, 0.0, 0.0));
}

#[test]
fn the_graphed_property_is_chosen_and_falls_back_when_it_leaves() {
    let (mut state, start, _) = graphed();
    let rows = state.key_params();
    assert!(rows.len() > 1, "a clip offers more than one property");
    let first = rows[0].param();
    assert_eq!(state.curve_param(), Some(first));

    let other = rows
        .iter()
        .map(|row| row.param())
        .find(|param| *param != first)
        .expect("a second property");
    state.set_curve_param(other);
    assert_eq!(state.curve_param(), Some(other));

    // A property the selection does not offer is not graphed: the block falls
    // back to the first row rather than drawing an unreachable route. A media
    // clip has no shape geometry, so this route is absent on it.
    let absent = ClipParam::Shape {
        param: aria_models::ShapeParam::Width,
    };
    assert!(
        rows.iter().all(|row| row.param() != absent),
        "the fixture clip unexpectedly animates a shape"
    );
    state.set_curve_param(absent);
    assert_eq!(state.curve_param(), Some(first));

    // With no reachable choice, a keyframed property is graphed before a
    // constant one, and a reachable choice still wins over it.
    let keyed = ClipParam::Opacity;
    assert_ne!(keyed, first, "the fixture's first property is the opacity");
    key(&mut state, keyed, start, 0.5);
    assert_eq!(state.curve_param(), Some(keyed));
    state.set_curve_param(first);
    assert_eq!(state.curve_param(), Some(first));
}

#[test]
fn a_two_dimensional_property_is_graphed_one_axis_at_a_time() {
    let (mut state, start, _) = graphed();
    let param = ClipParam::Position;
    assert_eq!(state.curve_component(param), Some(0));
    state.set_curve_axis(1);
    assert_eq!(state.curve_component(param), Some(1));
    // A scalar has one curve, so it has no axis.
    assert_eq!(state.curve_component(ClipParam::Opacity), None);

    let tick = start + 6;
    state.set_playhead(tick);
    state.add_keyframe_at_playhead(param).expect("add keyframe");
    let before = axes(&state, param, tick - start);
    state
        .set_keyframe_value_at(param, Some(1), tick, before[1] + 0.25, Easing::Linear)
        .expect("write the y axis");
    let after = axes(&state, param, tick - start);
    assert!(
        (after[0] - before[0]).abs() < 1e-5,
        "editing one axis moved the other: {before:?} -> {after:?}"
    );
    assert!((after[1] - before[1] - 0.25).abs() < 1e-3, "{after:?}");
}

/// Both axes of a two-dimensional property at a clip-local tick.
fn axes(state: &AppState, param: ClipParam, local: i64) -> [f32; 2] {
    let clip = state.selected_model().expect("model");
    let spec = clip_params_for(clip)
        .into_iter()
        .find(|spec| spec.param == param)
        .expect("row");
    match spec.sample(clip, local).expect("sample") {
        aria_models::ParamValue::Vec2(axes) => axes,
        other => panic!("expected a two-dimensional value, got {other:?}"),
    }
}

#[test]
fn picking_an_easing_writes_that_easing_and_leaves_neighbours_alone() {
    let (mut state, start, _) = graphed();
    let (k1, k2, k3) = (start + 5, start + 15, start + 25);

    state.set_playhead(k1);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("k1");
    state
        .set_keyframe_easing(ClipParam::Opacity, k1, Easing::Linear)
        .expect("linear");
    state.set_playhead(k2);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("k2");
    state
        .set_keyframe_easing(ClipParam::Opacity, k2, Easing::EaseIn)
        .expect("ease in");
    state.set_playhead(k3);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("k3");
    state
        .set_keyframe_easing(ClipParam::Opacity, k3, Easing::EaseOut)
        .expect("ease out");

    // Every preset the flyout can offer is written, and the row is read from
    // the catalog rather than named here: adding a preset covers it, and a
    // preset that stops being writable fails this test.
    assert!(!aria_models::EASING_PRESETS.is_empty(), "no easing presets");
    for preset in aria_models::EASING_PRESETS.iter() {
        let id = preset.id.as_str();
        state.set_playhead(k2);
        state
            .set_keyframe_easing_preset(ClipParam::Opacity, id)
            .expect("preset");

        let clip = state.selected_model().expect("model");
        let spec = clip_params_for(clip)
            .into_iter()
            .find(|spec| spec.param == ClipParam::Opacity)
            .expect("row");
        let keyframes = spec.keyframes(clip);
        assert_eq!(keyframes.len(), 3, "{id} added or dropped a keyframe");
        assert_eq!(keyframes[0].1, Easing::Linear, "{id} touched its neighbor");
        assert_eq!(keyframes[1].1.preset_id(), Some(id));
        assert_eq!(keyframes[2].1, Easing::EaseOut, "{id} touched its neighbor");
    }

    // An id no catalog row carries is inert: the keyframe keeps the last
    // preset written rather than losing its shape to a typo.
    let last = aria_models::EASING_PRESETS
        .last()
        .expect("a preset")
        .id
        .as_str();
    state.set_playhead(k2);
    state
        .set_keyframe_easing_preset(ClipParam::Opacity, "no_such_easing")
        .expect("unknown preset");
    let clip = state.selected_model().expect("model");
    let spec = clip_params_for(clip)
        .into_iter()
        .find(|spec| spec.param == ClipParam::Opacity)
        .expect("row");
    let keyframes = spec.keyframes(clip);
    assert_eq!(keyframes.len(), 3);
    assert_eq!(keyframes[1].1.preset_id(), Some(last));
}

/// A handle drag writes the control point under the pointer, measured in the
/// segment's own box, and leaves the segment's other handle where it was drawn.
///
/// Closes the class "a handle drag bending the curve away from the pointer":
/// the value fraction was read as one minus the pointer's, so a handle pulled
/// toward the next keyframe's value wrote a curve that lagged behind it. Starts
/// on a linear segment, whose handles are drawn a third of the way along it
/// rather than on its keyframes.
#[test]
fn a_handle_drag_writes_the_point_under_the_pointer() {
    let (mut state, start, end) = graphed();
    let eighth = (end - start) / 8;
    let (k1, k2) = (start, start + 4 * eighth);
    key(&mut state, ClipParam::Opacity, k1, 0.0);
    key(&mut state, ClipParam::Opacity, k2, 1.0);

    let poly = state
        .curve_polyline(ClipParam::Opacity, None)
        .expect("polyline");
    let handle = poly
        .handles
        .iter()
        .find(|handle| handle.kind == BezierHandleKind::Control1)
        .copied()
        .expect("a linear segment has handles");
    let (third_x, third_y) = poly
        .bounds
        .to_pixel_f64(k1 as f64 + (k2 - k1) as f64 / 3.0, 1.0 / 3.0);
    assert!(
        (handle.handle_x - third_x).abs() < 1e-3 && (handle.handle_y - third_y).abs() < 1e-3,
        "a linear segment's first handle is drawn at ({}, {}), not a third of the way along \
         it at ({third_x}, {third_y})",
        handle.handle_x,
        handle.handle_y,
    );
    assert!(state.begin_curve_drag(
        ClipParam::Opacity,
        None,
        handle.handle_x + FRAME[0],
        handle.handle_y + FRAME[1],
    ));
    assert!(
        matches!(
            state.curve_dragging().map(|draft| draft.target),
            Some(CurveDragTarget::BezierHandle {
                kind: BezierHandleKind::Control1,
                ..
            })
        ),
        "the press grabs the handle under it"
    );

    let (to_x, to_y) = window_pixel(&state, ClipParam::Opacity, k1 + eighth, 0.8);
    assert!(state.update_curve_drag(to_x, to_y), "the handle follows");
    state.commit_curve_drag().expect("commit the handle");

    let expected = [0.25, 0.8, 2.0 / 3.0, 2.0 / 3.0];
    match state.keyframe_easing_at(ClipParam::Opacity, k1) {
        Some(Easing::Bezier { points }) => assert!(
            points
                .iter()
                .zip(expected)
                .all(|(got, want)| (got - want).abs() < 1e-3),
            "the drag wrote {points:?}, not the pointer's point beside the untouched handle \
             {expected:?}"
        ),
        other => panic!("expected a bezier easing, got {other:?}"),
    }
}

/// Every keyframe draws inside the graph's frame, clear of its border by half a
/// keyframe mark, and a press on it grabs that keyframe.
///
/// Closes the class "a mark plotted on the frame's border": the plotted range
/// spanned the whole canvas, so an opacity of 1 drew on the top border and the
/// frame's rounded corner cut its mark. Every scalar property the selection
/// offers is keyed at the low end of its range on the clip's first tick and at
/// the high end on its last, which reaches all four edges; a property with an
/// open side is keyed ten units inside the bound it declares. Does not cover a
/// two-dimensional property's axes, or the handles, which a curve may bend
/// outside the range.
#[test]
fn every_keyframe_draws_and_takes_a_press_inside_the_frame() {
    let (mut state, start, end) = graphed();
    let half = KEYFRAME_DOT_SIZE * 0.5;
    let rows = state.key_params();
    let scalars: Vec<_> = rows
        .iter()
        .filter(|row| row.spec.kind == ParamKind::Scalar)
        .collect();
    assert!(
        scalars.iter().any(|row| row.param() == ClipParam::Opacity),
        "the fixture clip offers its opacity"
    );
    for row in scalars {
        let param = row.param();
        let (low, high) = match (row.spec.range.min, row.spec.range.max) {
            (Some(low), Some(high)) => (low, high),
            (Some(low), None) => (low, low + 10.0),
            (None, Some(high)) => (high - 10.0, high),
            (None, None) => (-10.0, 10.0),
        };
        key(&mut state, param, start, low);
        key(&mut state, param, end - 1, high);
        let poly = state
            .curve_polyline(param, None)
            .unwrap_or_else(|| panic!("{}: a keyed property draws", row.id));
        for point in &poly.key_points {
            assert!(
                (half..=FRAME[2] - half).contains(&point.pixel_x)
                    && (half..=FRAME[3] - half).contains(&point.pixel_y),
                "{}: the keyframe at tick {} holding {} draws at ({}, {}), on the border of \
                 the {} by {} graph",
                row.id,
                point.tick,
                point.value,
                point.pixel_x,
                point.pixel_y,
                FRAME[2],
                FRAME[3],
            );
            let target = grabbed(&mut state, param, point.pixel_x, point.pixel_y);
            assert!(
                matches!(
                    target,
                    Some(CurveDragTarget::Keyframe { origin_tick, .. }) if origin_tick == point.tick
                ),
                "{}: a press on the keyframe at tick {} grabs {target:?}",
                row.id,
                point.tick,
            );
        }
    }
}

/// Every handle the graph draws lies on the curve's tangent at the keyframe it
/// belongs to, for every easing that draws handles, on a rising and on a
/// falling segment, and a hold draws none.
///
/// Closes the class "a handle mirrored across its segment": a handle's height
/// was one minus its control point's, so an Ease In, which leaves its keyframe
/// flat, drew its first handle level with the next keyframe. The tangent is
/// read from the stored easing's own curve ([`Easing::apply`]) a thousandth of
/// the segment from each end, independent of the graph's mapping. Does not
/// cover a handle of no horizontal length, whose slope is not a number.
#[test]
fn every_handle_lies_on_the_curves_tangent_at_its_keyframe() {
    const STEP: f32 = 1e-3;
    let (mut state, start, end) = graphed();
    let span = end - start;
    let (k0, k1) = (start + span / 4, start + span * 3 / 4);
    for (from, to) in [(0.2_f32, 0.9_f32), (0.9, 0.2)] {
        for easing in easings() {
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, k0, from, easing)
                .expect("the leaving keyframe");
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, k1, to, Easing::Linear)
                .expect("the arriving keyframe");
            // The model rewrites a temporal segment's control points from its
            // eases, so the curve drawn is the stored easing's.
            let stored = state
                .keyframe_easing_at(ClipParam::Opacity, k0)
                .expect("the stored easing");
            let poly = state
                .curve_polyline(ClipParam::Opacity, None)
                .expect("polyline");
            let [a, b] = [poly.key_points[0], poly.key_points[1]];
            let drawn: Vec<_> = poly
                .handles
                .iter()
                .filter(|handle| handle.segment_index == 0)
                .collect();
            if !draws_handles(stored) {
                assert!(drawn.is_empty(), "{stored:?} draws handles {drawn:?}");
                continue;
            }
            assert_eq!(drawn.len(), 2, "{stored:?} draws both handles");
            let slope = (b.pixel_y - a.pixel_y) / (b.pixel_x - a.pixel_x);
            for handle in drawn {
                let (anchor, eased) = match handle.kind {
                    BezierHandleKind::Control1 => (a, stored.apply(STEP) / STEP),
                    BezierHandleKind::Control2 => (b, (1.0 - stored.apply(1.0 - STEP)) / STEP),
                };
                let run = handle.handle_x - anchor.pixel_x;
                let rise = handle.handle_y - anchor.pixel_y;
                if run.abs() < 1.0 {
                    continue;
                }
                let (actual, expected) = (rise / run, eased * slope);
                assert!(
                    (actual - expected).abs() <= 0.03 * expected.abs().max(1.0),
                    "{stored:?} from {from} to {to}: the {:?} handle leaves its keyframe at \
                     slope {actual}, the curve at {expected}",
                    handle.kind,
                );
            }
        }
    }
}

/// The distance from `point` to the line from `a` to `b`.
fn off_lever(point: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let along = if length > 0.0 {
        (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point.0 - a.0 - dx * along).hypot(point.1 - a.1 - dy * along)
}

/// Every handle the graph places is painted as a lever from its keyframe to
/// its tip, and the tip, in the handle's color, for every easing on a rising
/// and a falling segment; nothing else is painted in that color, so a hold
/// paints no handle.
///
/// Closes the class "a tip with no lever": the graph painted each tip alone,
/// so nothing showed which keyframe a tip bends or which way it leaves it. The
/// marks are the list the canvas paints, read without a window. Does not cover
/// where the canvas lands in the window, which the press tests cover through
/// the frame the graph records, or the cut at the graph's edge.
#[test]
fn every_handle_is_painted_as_a_lever_from_its_keyframe_to_its_tip() {
    let ink = GraphInk {
        rule: gpui::hsla(0.0, 0.0, 0.5, 1.0),
        curve: gpui::hsla(0.6, 1.0, 0.5, 1.0),
        handle: gpui::hsla(0.1, 1.0, 0.5, 1.0),
        key: gpui::hsla(0.0, 0.0, 1.0, 1.0),
    };
    let [_, _, width, height] = FRAME;
    let (mut state, start, end) = graphed();
    let span = end - start;
    let (k0, k1) = (start + span / 4, start + span * 3 / 4);
    for (from, to) in [(0.2_f32, 0.9_f32), (0.9, 0.2)] {
        for easing in easings() {
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, k0, from, easing)
                .expect("the leaving keyframe");
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, k1, to, Easing::Linear)
                .expect("the arriving keyframe");
            let poly = state
                .curve_polyline(ClipParam::Opacity, None)
                .expect("polyline");
            let handles: Vec<GraphMark> = graph_marks(Some(&poly), width, height, ink)
                .filter(|(_, color)| *color == ink.handle)
                .collect();
            let painted = |(x, y): (f32, f32)| {
                handles.iter().any(|([left, top, w, h], _)| {
                    (*left..=left + w).contains(&x) && (*top..=top + h).contains(&y)
                })
            };
            let at = format!("{easing:?} from {from} to {to}");
            assert_eq!(
                handles.is_empty(),
                poly.handles.is_empty(),
                "{at}: paints {} handle marks for {} handles",
                handles.len(),
                poly.handles.len()
            );
            for handle in &poly.handles {
                let (anchor, tip) = (
                    (handle.anchor_x, handle.anchor_y),
                    (handle.handle_x, handle.handle_y),
                );
                for step in 0..=20 {
                    let along = step as f32 / 20.0;
                    let point = (
                        anchor.0 + (tip.0 - anchor.0) * along,
                        anchor.1 + (tip.1 - anchor.1) * along,
                    );
                    assert!(
                        painted(point),
                        "{at}: the {:?} lever from {anchor:?} to {tip:?} is not painted at \
                         {point:?}",
                        handle.kind,
                    );
                }
                assert!(
                    handles.iter().any(|([left, top, w, h], _)| {
                        [*w, *h] == [HANDLE_DOT_SIZE; 2]
                            && (left + w * 0.5 - tip.0).abs() < 1e-3
                            && (top + h * 0.5 - tip.1).abs() < 1e-3
                    }),
                    "{at}: the {:?} tip at {tip:?} is not painted",
                    handle.kind,
                );
            }
            for ([left, top, w, h], _) in &handles {
                let center = (left + w * 0.5, top + h * 0.5);
                assert!(
                    poly.handles.iter().any(|handle| {
                        off_lever(
                            center,
                            (handle.anchor_x, handle.anchor_y),
                            (handle.handle_x, handle.handle_y),
                        ) <= HANDLE_DOT_SIZE
                    }),
                    "{at}: a handle mark at {center:?} is on no lever"
                );
            }
        }
    }
}

/// A press grabs the mark nearest it, and a keyframe over a handle at the same
/// distance.
///
/// Closes the class "a keyframe under its own handle": handles were hit-tested
/// before keyframes, so a handle drawn within the hit radius of its keyframe
/// took every press meant for the keyframe, and a handle of no length left its
/// keyframe immovable. Covers a short handle from both of its ends and a
/// handle of no length.
#[test]
fn a_press_grabs_the_nearest_mark_and_a_keyframe_over_its_handle() {
    let (mut state, start, end) = graphed();
    let span = end - start;
    let (k0, k1) = (start + span / 4, start + span * 3 / 4);
    for points in [[0.02, 0.02, 0.98, 0.98], [0.0, 0.0, 0.2, 1.0]] {
        state
            .set_keyframe_value_at(ClipParam::Opacity, None, k0, 0.0, Easing::Bezier { points })
            .expect("the leaving keyframe");
        state
            .set_keyframe_value_at(ClipParam::Opacity, None, k1, 1.0, Easing::Linear)
            .expect("the arriving keyframe");
        let poly = state
            .curve_polyline(ClipParam::Opacity, None)
            .expect("polyline");
        let key = poly.key_points[0];
        let handle = poly
            .handles
            .iter()
            .find(|handle| handle.kind == BezierHandleKind::Control1)
            .copied()
            .expect("the first handle");
        let length = (handle.handle_x - key.pixel_x).hypot(handle.handle_y - key.pixel_y);
        assert!(
            length < HIT_RADIUS,
            "{points:?}: the handle is {length} px from its keyframe, outside the hit radius"
        );
        let target = grabbed(&mut state, ClipParam::Opacity, key.pixel_x, key.pixel_y);
        assert!(
            matches!(target, Some(CurveDragTarget::Keyframe { origin_tick, .. }) if origin_tick == k0),
            "{points:?}: a press on the keyframe grabs {target:?}"
        );
        if length > 0.0 {
            let target = grabbed(
                &mut state,
                ClipParam::Opacity,
                handle.handle_x,
                handle.handle_y,
            );
            assert!(
                matches!(
                    target,
                    Some(CurveDragTarget::BezierHandle {
                        kind: BezierHandleKind::Control1,
                        ..
                    })
                ),
                "{points:?}: a press on the handle grabs {target:?}"
            );
        }
    }
}

#[test]
fn an_empty_property_draws_nothing_rather_than_panicking() {
    let (state, start, end) = graphed();
    let bounds = state
        .curve_bounds(ClipParam::Opacity, None)
        .expect("bounds");
    assert_eq!((bounds.min_tick, bounds.max_tick), (start, end));
    let clip = state.selected_model().expect("model");
    let spec = &state.key_param(ClipParam::Opacity).expect("row").spec;
    assert!(
        build_curve_polyline(spec, clip, None, bounds, 50).is_none(),
        "a constant property has no curve to draw"
    );
    assert!(state.curve_polyline(ClipParam::Opacity, None).is_none());
}
