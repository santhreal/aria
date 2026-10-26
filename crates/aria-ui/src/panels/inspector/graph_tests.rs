//! The Graph block drawn: a chip for each property of the selected clip that
//! holds a keyframe, and for the graphed property, and no block while no
//! property holds a keyframe.
//!
//! Closes the class "a graph offering what it has no curve for". The block
//! drew a chip for every animatable property of the clip, through every grade
//! channel and curve point, and graphed the first of them, Position, while
//! only Opacity held keyframes; with no keyframe at all it still drew a flat
//! line. The chips are read against `AppState::key_params` at run time, so a
//! property added to the catalog is held to the rule without being listed
//! here. A driver is not a keyframe, so a driven property has no chip.
//!
//! Not covered: the chip order, which is the catalog order, and the axis
//! chips, which follow the graphed property.
//!
//! The graph scrolled into view is also drawn, pressed and dragged through
//! the window's own events: the marks the window paints are read for the
//! handle's color, for the cut at the graph's edge, and for where a dragged
//! mark is drawn while the button is held.

use aria_models::{ClipParam, Easing};
use gpui::{
    Background, Bounds, Hsla, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, VisualTestContext, point, px,
};

use super::blocks::curve_editor::picker::{HANDLE_DOT_SIZE, KEYFRAME_DOT_SIZE};
use super::blocks::curve_editor::{CurveDragTarget, CurvePolyline};
use super::keyframes_tests::animated;
use super::strip_tests::drawn_with;
use crate::app::AppState;
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;

/// The id of `param`'s row on the selected clip.
fn id_of(state: &gpui::Entity<AppState>, visual: &VisualTestContext, param: ClipParam) -> String {
    visual.read(|app| {
        state
            .read(app)
            .key_param(param)
            .unwrap_or_else(|| panic!("the texture offers no {param:?} row"))
            .id
            .to_string()
    })
}

/// The ids of the properties whose Graph chip is drawn, sorted, out of every
/// property the selection offers.
fn drawn_chips(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Vec<String> {
    let ids: Vec<String> = visual.read(|app| {
        state
            .read(app)
            .key_params()
            .iter()
            .map(|row| row.id.to_string())
            .collect()
    });
    assert!(
        ids.len() > 2,
        "the texture offers too few properties: {ids:?}"
    );
    let mut drawn: Vec<String> = ids
        .into_iter()
        .filter(|id| {
            let selector: &'static str = format!("curve-param-{id}").leak();
            visual.debug_bounds(selector).is_some()
        })
        .collect();
    drawn.sort();
    drawn
}

#[gpui::test]
async fn the_graph_offers_the_keyframed_properties_and_the_graphed_one(cx: &mut TestAppContext) {
    let (state, visual) = animated(cx, "graph-keyframed");
    let opacity = id_of(&state, visual, ClipParam::Opacity);
    let position = id_of(&state, visual, ClipParam::Position);

    assert_eq!(
        visual.read(|app| state.read(app).curve_param()),
        Some(ClipParam::Opacity),
        "with nothing chosen, the graph draws a constant property before the keyframed opacity"
    );
    assert!(
        visual.debug_bounds("curve-editor-graph").is_some(),
        "the Transform tab draws no graph with the opacity keyframed"
    );
    assert_eq!(
        drawn_chips(&state, visual),
        vec![opacity.clone()],
        "with the opacity keyframed and the rotation driven, the Graph block offers"
    );

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.set_curve_param(ClipParam::Position);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let mut both = vec![opacity, position];
    both.sort();
    assert_eq!(
        drawn_chips(&state, visual),
        both,
        "with the constant position graphed, the Graph block offers"
    );
}

#[gpui::test]
async fn no_graph_is_drawn_while_no_property_holds_a_keyframe(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(cx, "graph-driven", "texture.png", "transform", |state| {
        state
            .ctx
            .mount(&aria_plugin::ExpressionsPlugin)
            .expect("mount Expressions");
        keyframe_rows::apply(state, Row::Driver(ClipParam::Rotation)).expect("drive the rotation");
    });
    let rotation = id_of(&state, visual, ClipParam::Rotation);
    let row: &'static str = format!("kf-row-{rotation}").leak();
    assert!(
        visual.debug_bounds(row).is_some(),
        "the Transform tab draws no Keyframes row for the driven rotation"
    );
    assert!(
        visual.debug_bounds("curve-editor-graph").is_none(),
        "the Graph block is drawn with no property holding a keyframe"
    );
    assert_eq!(
        drawn_chips(&state, visual),
        Vec::<String>::new(),
        "with no property holding a keyframe, the Graph block offers"
    );
}

/// Scroll the Inspector to its end and return the window bounds of the
/// element `selector` names, held to lie inside the Inspector's body.
pub(super) fn scrolled_to(
    visual: &mut VisualTestContext,
    selector: &'static str,
) -> Bounds<Pixels> {
    let body = visual
        .debug_bounds("inspector-body")
        .expect("the Inspector draws its body");
    visual.simulate_mouse_move(body.center(), None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position: body.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
    let drawn = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("the Inspector draws no {selector}"));
    assert!(
        body.contains(&drawn.origin) && body.contains(&drawn.bottom_right()),
        "the Inspector scrolled to its end draws {selector} at {drawn:?} outside its body {body:?}"
    );
    drawn
}

/// The Transform tab drawn over the texture with its opacity keyed at a
/// quarter and at three quarters of the clip, both at full opacity, and the
/// Inspector scrolled to its end so the Graph block is in view. Returns the
/// graph's window bounds.
fn keyed_twice<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<AppState>,
    &'a mut VisualTestContext,
    Bounds<Pixels>,
) {
    let (state, visual) = drawn_with(cx, tag, "texture.png", "transform", |state| {
        let clip = state.selected_ui_clip().expect("the texture is selected");
        let span = clip.timeline_end_ticks - clip.timeline_start_ticks;
        for tick in [span / 4, span * 3 / 4] {
            state.set_playhead(clip.timeline_start_ticks + tick);
            state
                .add_keyframe_at_playhead(ClipParam::Opacity)
                .expect("key the opacity");
        }
    });
    // The marks are read at rest: a panel that fades in paints them at no
    // opacity until the fade ends.
    visual.update(|_, app| app.set_reduce_motion(true));
    let graph = scrolled_to(visual, "curve-editor-graph");
    (state, visual, graph)
}

/// Shape the fixture's curve: the first keyframe at full opacity, leaving it
/// with `easing`, and the second lowered to 0.2, so the segment falls and its
/// handles stand apart from its keyframes.
fn shaped(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext, easing: Easing) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let ticks: Vec<i64> = state
                .keyframes_of(ClipParam::Opacity)
                .iter()
                .map(|mark| mark.tick)
                .collect();
            let [first, second] = ticks[..] else {
                panic!("the fixture keys the opacity twice, not at {ticks:?}");
            };
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, first, 1.0, easing)
                .expect("shape the first segment");
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, second, 0.2, Easing::Linear)
                .expect("lower the second keyframe");
            cx.notify();
        });
    });
    visual.run_until_parked();
}

/// The opacity's curve as the graph draws it, in graph pixels.
fn drawn(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> CurvePolyline {
    visual.read(|app| {
        state
            .read(app)
            .curve_polyline(ClipParam::Opacity, None)
            .expect("the opacity's curve")
    })
}

/// One quad the last frame painted, in window pixels: its center, its size,
/// and the mask it is cut to, `[left, top, right, bottom]`.
#[derive(Debug, Clone, Copy)]
struct Painted {
    center: Point<Pixels>,
    size: [f32; 2],
    mask: [f32; 4],
}

/// Every quad the last frame painted in `color` with its center inside
/// `within`, left to right.
fn painted(visual: &mut VisualTestContext, color: Hsla, within: Bounds<Pixels>) -> Vec<Painted> {
    let color = Background::from(color);
    let mut quads: Vec<Painted> = visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .filter(|quad| quad.background == color)
            .map(|quad| {
                let (b, m) = (quad.bounds, quad.content_mask.bounds);
                Painted {
                    center: point(
                        px((b.origin.x.0 + b.size.width.0 * 0.5) / scale),
                        px((b.origin.y.0 + b.size.height.0 * 0.5) / scale),
                    ),
                    size: [b.size.width.0 / scale, b.size.height.0 / scale],
                    mask: [
                        m.origin.x.0 / scale,
                        m.origin.y.0 / scale,
                        (m.origin.x.0 + m.size.width.0) / scale,
                        (m.origin.y.0 + m.size.height.0) / scale,
                    ],
                }
            })
            .filter(|quad| within.contains(&quad.center))
            .collect()
    });
    quads.sort_by(|a, b| f32::from(a.center.x).total_cmp(&f32::from(b.center.x)));
    quads
}

/// The centers of the `side` pixel dots painted in `color` inside `within`,
/// left to right.
fn dots(
    visual: &mut VisualTestContext,
    color: Hsla,
    side: f32,
    within: Bounds<Pixels>,
) -> Vec<Point<Pixels>> {
    painted(visual, color, within)
        .into_iter()
        .filter(|quad| quad.size.iter().all(|edge| (edge - side).abs() < 0.5))
        .map(|quad| quad.center)
        .collect()
}

/// The drawn graph paints its handles in the color the timeline graph draws a
/// handle in, and cuts a lever bent past its top edge at that edge instead of
/// painting it over the rows above.
///
/// Closes the class "a mark painted outside its graph": the graph painted in
/// the Inspector's mask, so a handle bent past the frame was drawn over the
/// controls above it. The first segment's easing puts its handle further above
/// the graph than the graph is tall. Every quad painted in the handle color
/// inside the Inspector is held to the graph's bounds by the mask it is drawn
/// in. Does not cover the lever's line, which
/// `curve_editor::tests::every_handle_is_painted_as_a_lever_from_its_keyframe_to_its_tip`
/// covers without a window.
#[gpui::test]
async fn a_lever_past_the_graph_is_cut_at_its_edge_in_the_handle_color(cx: &mut TestAppContext) {
    let (state, visual, graph) = keyed_twice(cx, "graph-levers");
    let overshoot = Easing::Bezier {
        points: [0.3, -1.5, 0.7, 1.0],
    };
    shaped(&state, visual, overshoot);
    assert!(
        drawn(&state, visual)
            .handles
            .iter()
            .any(|handle| handle.handle_y < 0.0),
        "the fixture's handle is not past the graph's top edge"
    );

    let body = visual
        .debug_bounds("inspector-body")
        .expect("the Inspector draws its body");
    let handle = visual.read(|app| Theme::of(app).warning);
    let marks = painted(visual, handle, body);
    assert!(
        !marks.is_empty(),
        "the graph at {graph:?} paints no mark in the handle color"
    );
    let [left, top] = [f32::from(graph.origin.x), f32::from(graph.origin.y)];
    let [right, bottom] = [
        f32::from(graph.bottom_right().x),
        f32::from(graph.bottom_right().y),
    ];
    for Painted {
        center,
        mask: [x0, y0, x1, y1],
        ..
    } in marks
    {
        assert!(
            x0 >= left - 0.5 && y0 >= top - 0.5 && x1 <= right + 0.5 && y1 <= bottom + 0.5,
            "the handle mark at {center:?} is cut to [{x0}, {y0}, {x1}, {y1}], past the graph \
             at {graph:?}"
        );
    }
}

/// Every number a curve is drawn from, so two curves compare within a
/// tolerance.
fn numbers(curve: &CurvePolyline) -> Vec<f32> {
    let samples = curve
        .samples
        .iter()
        .flat_map(|&(x, y, tick, value)| [x, y, tick as f32, value]);
    let keys = curve
        .key_points
        .iter()
        .flat_map(|key| [key.tick as f32, key.value, key.pixel_x, key.pixel_y]);
    let handles = curve.handles.iter().flat_map(|handle| {
        [
            handle.anchor_x,
            handle.anchor_y,
            handle.handle_x,
            handle.handle_y,
        ]
    });
    samples.chain(keys).chain(handles).collect()
}

/// Whether two curves draw the same marks, within a thousandth of a pixel
/// and of a value.
fn same_curve(a: &CurvePolyline, b: &CurvePolyline) -> bool {
    let (a, b) = (numbers(a), numbers(b));
    a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-3)
}

/// Every mark the graph draws, pressed where it is painted, follows a drag
/// made through the window's own events: its dot is painted under the
/// pointer while the button is held, the release writes the curve drawn
/// then, and one undo puts the curve back.
///
/// Closes the class "a mark that does not follow its drag". The graph read a
/// pointer against a frame recorded one graph height below the graph it
/// painted, so a press on a painted mark grabbed nothing, and a grabbed mark
/// stayed where it was committed until the release. The marks are read off
/// the painted graph, every keyframe dot and every handle tip, so a mark the
/// curve gains is dragged here too. Each is carried halfway to the graph's
/// center, which moves a keyframe in time and in value and a handle on both
/// axes. The first segment leaves its keyframe with an ease, so a drag drawn
/// with another easing than the release writes is caught.
///
/// Each mark is pressed on the graph as it is drawn then, scrolled into view
/// again: a keyframe carried off the playhead drops its row's easing presets,
/// and the undo that brings them back moves the graph below them.
///
/// Not covered: a keyframe carried onto another keyframe's tick.
#[gpui::test]
async fn every_mark_on_the_graph_follows_a_drag_and_lands_where_it_was_drawn(
    cx: &mut TestAppContext,
) {
    let (state, visual, _) = keyed_twice(cx, "graph-drag");
    let ease = Easing::Bezier {
        points: [0.42, 0.0, 0.58, 1.0],
    };
    shaped(&state, visual, ease);
    let (key, handle) = visual.read(|app| {
        let theme = Theme::of(app);
        (theme.text, theme.warning)
    });
    let committed = drawn(&state, visual);
    let [first, second] = [committed.key_points[0], committed.key_points[1]];
    // A keyframe lands on a whole tick, so its dot sits up to half a tick
    // from the pointer across the graph. A quad is painted on whole pixels,
    // so every dot sits up to a pixel from the pointer.
    let half_tick = 0.5 * (second.pixel_x - first.pixel_x) / (second.tick - first.tick) as f32;
    let kinds = [
        (
            "keyframe",
            key,
            KEYFRAME_DOT_SIZE,
            committed.key_points.len(),
            half_tick + 1.0,
        ),
        (
            "handle",
            handle,
            HANDLE_DOT_SIZE,
            committed.handles.len(),
            1.0,
        ),
    ];
    for (kind, color, side, count, reach) in kinds {
        for index in 0..count {
            let graph = scrolled_to(visual, "curve-editor-graph");
            let center = graph.center();
            let marks = dots(visual, color, side, graph);
            assert_eq!(
                marks.len(),
                count,
                "the graph paints {marks:?} for its {count} {kind} marks"
            );
            let at = marks[index];
            let to = point((at.x + center.x) * 0.5, (at.y + center.y) * 0.5);
            visual.simulate_mouse_move(at, None, Modifiers::none());
            visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
            assert!(
                visual.read(|app| state.read(app).curve_dragging().is_some()),
                "a press on {kind} {index}, painted at {at:?}, grabs nothing"
            );
            visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
            let held = dots(visual, color, side, graph);
            assert!(
                held.iter().any(|dot| {
                    f32::from(dot.x - to.x).abs() <= reach && f32::from(dot.y - to.y).abs() <= 1.0
                }),
                "{kind} {index} carried to {to:?} is painted at {held:?}"
            );
            let during = drawn(&state, visual);
            visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
            visual.run_until_parked();
            assert!(
                visual.read(|app| state.read(app).curve_dragging().is_none()),
                "the release of {kind} {index} left the drag in flight"
            );
            let written = drawn(&state, visual);
            assert!(
                same_curve(&during, &written),
                "{kind} {index}: the release wrote {written:?}, not the {during:?} drawn while \
                 the button was held"
            );
            assert!(
                !same_curve(&written, &committed),
                "{kind} {index} carried to {to:?} wrote nothing"
            );
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    assert!(
                        state.undo(),
                        "the drag of {kind} {index} left nothing to undo"
                    );
                    cx.notify();
                });
            });
            visual.run_until_parked();
            assert!(
                same_curve(&drawn(&state, visual), &committed),
                "one undo left {kind} {index} moved"
            );
        }
    }
}

/// Where the keyframe drag in flight holds its keyframe, `(tick, value)`, or
/// `None` with no keyframe drag in flight.
fn dragged_to(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> Option<(i64, f32)> {
    visual.read(|app| match state.read(app).curve_dragging()?.target {
        CurveDragTarget::Keyframe {
            current_tick,
            current_val,
            ..
        } => Some((current_tick, current_val)),
        CurveDragTarget::BezierHandle { .. } => None,
    })
}

/// The second keyframe the graph draws, `(tick, value)`.
fn second_key(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> (i64, f32) {
    let key = drawn(state, visual).key_points[1];
    (key.tick, key.value)
}

/// A keyframe dragged off the graph keeps following the pointer, held to the
/// clip and to the property's range, and a release outside the graph, or
/// outside the window, writes it and ends the drag.
///
/// Closes the class "a drag held by the element it started on": the graph
/// took the pointer's moves and its release only while the pointer was over
/// it, so a drag that left it stopped where it left, and a release outside it
/// left the drag in flight, moving the keyframe with the button up. Past the
/// right edge the keyframe lands on the clip's last frame: the plotted span
/// ends on the clip's end, the first tick after the clip, where the model
/// refuses a keyframe. A release outside the window reaches the window as a
/// move with no button held.
#[gpui::test]
async fn a_keyframe_dragged_off_the_graph_follows_the_pointer_and_lands_on_its_release(
    cx: &mut TestAppContext,
) {
    let (state, visual, graph) = keyed_twice(cx, "graph-drag-out");
    let key = visual.read(|app| Theme::of(app).text);
    let last = visual.read(|app| {
        let clip = state
            .read(app)
            .selected_ui_clip()
            .expect("the texture is selected");
        clip.timeline_end_ticks - 1
    });
    let at = dots(visual, key, KEYFRAME_DOT_SIZE, graph)[1];
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(
        at + point(px(0.0), px(20.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    let off = point(graph.right() + px(40.0), graph.bottom() + px(30.0));
    visual.simulate_mouse_move(off, MouseButton::Left, Modifiers::none());
    assert_eq!(
        dragged_to(&state, visual),
        Some((last, 0.0)),
        "the pointer past the graph's bottom right corner holds the keyframe on the clip's \
         last frame at the opacity's floor"
    );
    visual.simulate_mouse_up(off, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        dragged_to(&state, visual),
        None,
        "a release outside the graph left the drag in flight"
    );
    assert_eq!(
        second_key(&state, visual),
        (last, 0.0),
        "a release outside the graph writes where the drag held the keyframe"
    );

    let at = dots(visual, key, KEYFRAME_DOT_SIZE, graph)[1];
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    let up = at - point(px(0.0), px(30.0));
    visual.simulate_mouse_move(up, MouseButton::Left, Modifiers::none());
    let (tick, value) = dragged_to(&state, visual).expect("the second drag outlived its move");
    assert!(
        value > 0.1,
        "30 px up the graph holds the keyframe at {value}"
    );
    visual.simulate_mouse_move(up, None, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        dragged_to(&state, visual),
        None,
        "a move with the button up left the drag in flight"
    );
    let (written, at_value) = second_key(&state, visual);
    assert!(
        written == tick && (at_value - value).abs() < 1e-4,
        "a release outside the window writes ({written}, {at_value}) for the ({tick}, {value}) \
         the drag held"
    );
}
