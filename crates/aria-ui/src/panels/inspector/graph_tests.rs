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

use aria_models::ClipParam;
use gpui::{TestAppContext, VisualTestContext};

use super::keyframes_tests::animated;
use super::strip_tests::drawn_with;
use crate::app::AppState;
use crate::surfaces::keyframe_rows::{self, Row};

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
