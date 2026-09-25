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

use aria_models::{ClipParam, Easing, clip_params_for};

use crate::app::AppState;
use crate::app::curve_geometry::{build_curve_polyline, sample_param_scalar};
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
    let (mut state, _, _) = graphed();
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
        assert_eq!(keyframes[0].1, Easing::Linear, "{id} touched its neighbour");
        assert_eq!(keyframes[1].1.preset_id(), Some(id));
        assert_eq!(
            keyframes[2].1,
            Easing::EaseOut,
            "{id} touched its neighbour"
        );
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

#[test]
fn a_bezier_handle_drag_writes_the_segments_easing() {
    let (mut state, start, _) = graphed();
    let (k1, k2) = (start, start + 20);
    key(&mut state, ClipParam::Opacity, k1, 0.0);
    key(&mut state, ClipParam::Opacity, k2, 1.0);

    let poly = state
        .curve_polyline(ClipParam::Opacity, None)
        .expect("polyline");
    let handle = poly
        .handles
        .first()
        .copied()
        .expect("a linear segment has handles");
    assert!(state.begin_curve_drag(
        ClipParam::Opacity,
        None,
        handle.handle_x + FRAME[0],
        handle.handle_y + FRAME[1],
    ));

    let (to_x, to_y) = window_pixel(&state, ClipParam::Opacity, start + 5, 0.8);
    assert!(state.update_curve_drag(to_x, to_y), "the handle follows");
    state.commit_curve_drag().expect("commit the handle");

    match state.keyframe_easing_at(ClipParam::Opacity, k1) {
        Some(Easing::Bezier { points }) => {
            assert!(
                points.iter().all(|c| c.is_finite()),
                "handle drag wrote {points:?}"
            );
            assert!(
                points[0] >= 0.0 && points[0] <= 1.0,
                "the time coordinate left its box: {points:?}"
            );
        }
        other => panic!("expected a bezier easing, got {other:?}"),
    }

    // Every sample between the two keyframes stays finite and in the property's
    // own range: a handle drag shapes the segment, it does not break it.
    let after = state
        .curve_polyline(ClipParam::Opacity, None)
        .expect("polyline after");
    for &(_, _, tick, val) in &after.samples {
        if tick >= k1 as f64 && tick <= k2 as f64 {
            assert!(
                val.is_finite() && (0.0..=1.0).contains(&val),
                "{val} at {tick}"
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
