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
//! The graph scrolled into view is also drawn with a handle bent past its top
//! edge, so the marks the window paints are read for the handle's color and
//! for the cut at the graph's edge.

use aria_models::{ClipParam, Easing};
use gpui::{
    Background, Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};

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
    let graph = visual
        .debug_bounds("curve-editor-graph")
        .expect("the Transform tab draws the graph");
    assert!(
        body.contains(&graph.origin) && body.contains(&graph.bottom_right()),
        "the Inspector scrolled to its end draws the graph at {graph:?} outside its body {body:?}"
    );
    (state, visual, graph)
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
            let overshoot = Easing::Bezier {
                points: [0.3, -1.5, 0.7, 1.0],
            };
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, first, 1.0, overshoot)
                .expect("bend the first segment past the top");
            state
                .set_keyframe_value_at(ClipParam::Opacity, None, second, 0.2, Easing::Linear)
                .expect("lower the second keyframe");
            cx.notify();
        });
    });
    visual.run_until_parked();
    let bent = visual.read(|app| {
        state
            .read(app)
            .curve_polyline(ClipParam::Opacity, None)
            .is_some_and(|poly| poly.handles.iter().any(|handle| handle.handle_y < 0.0))
    });
    assert!(
        bent,
        "the fixture's handle is not past the graph's top edge"
    );

    let body = visual
        .debug_bounds("inspector-body")
        .expect("the Inspector draws its body");
    let handle = Background::from(visual.read(|app| Theme::of(app).warning));
    // Each handle-colored quad in the Inspector: its center, and the mask it is
    // cut to, in window pixels.
    let marks: Vec<([f32; 2], [f32; 4])> = visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .filter(|quad| quad.background == handle)
            .map(|quad| {
                let (b, m) = (quad.bounds, quad.content_mask.bounds);
                (
                    [
                        (b.origin.x.0 + b.size.width.0 * 0.5) / scale,
                        (b.origin.y.0 + b.size.height.0 * 0.5) / scale,
                    ],
                    [
                        m.origin.x.0 / scale,
                        m.origin.y.0 / scale,
                        (m.origin.x.0 + m.size.width.0) / scale,
                        (m.origin.y.0 + m.size.height.0) / scale,
                    ],
                )
            })
            .filter(|([x, y], _)| body.contains(&point(px(*x), px(*y))))
            .collect()
    });
    assert!(
        !marks.is_empty(),
        "the graph at {graph:?} paints no mark in the handle color"
    );
    let [left, top] = [f32::from(graph.origin.x), f32::from(graph.origin.y)];
    let [right, bottom] = [
        f32::from(graph.bottom_right().x),
        f32::from(graph.bottom_right().y),
    ];
    for (center, [x0, y0, x1, y1]) in &marks {
        assert!(
            *x0 >= left - 0.5 && *y0 >= top - 0.5 && *x1 <= right + 0.5 && *y1 <= bottom + 0.5,
            "the handle mark at {center:?} is cut to [{x0}, {y0}, {x1}, {y1}], past the graph \
             at {graph:?}"
        );
    }
}
