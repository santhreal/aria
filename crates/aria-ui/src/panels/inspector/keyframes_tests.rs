//! The Keyframes block drawn: a row for each property of the selected clip
//! that holds a keyframe or a driver, and none for a constant one.
//!
//! Closes the class "a Keyframes row for a property with nothing to show".
//! The block drew a row for every animatable property of the clip, each with
//! no keys, a diamond and a Driver button, through every grade channel and
//! curve point. A constant property is keyed from its own row's stopwatch or
//! diamond, or from its timeline property row. The rows are read against
//! `AppState::key_params` at run time, so a property added to the catalog is
//! held to the rule without being listed here. Each row's name stands on the
//! label column of the keyed rows above it.
//!
//! Not covered: the None the block prints with no animated property, which
//! is text with no selector, and the Shape tab's copy of the block, which is
//! built by the same function.

use aria_models::ClipParam;
use gpui::TestAppContext;

use super::columns_tests::EPS;
use super::strip_tests::drawn_with;
use crate::surfaces::keyframe_rows::{self, Row};

/// The Transform tab drawn over the texture with its opacity keyed at the
/// playhead and its rotation driven in the Expressions language: one property
/// animated by keys, one by a driver, and every other property constant.
fn animated<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<crate::app::AppState>,
    &'a mut gpui::VisualTestContext,
) {
    drawn_with(cx, tag, "texture.png", "transform", |state| {
        state
            .ctx
            .mount(&aria_plugin::ExpressionsPlugin)
            .expect("mount Expressions");
        keyframe_rows::apply(
            state,
            Row::Key {
                param: ClipParam::Opacity,
                keyed: false,
            },
        )
        .expect("key the opacity at the playhead");
        keyframe_rows::apply(state, Row::Driver(ClipParam::Rotation)).expect("drive the rotation");
    })
}

#[gpui::test]
async fn the_keyframes_block_lists_the_animated_properties_and_no_other(cx: &mut TestAppContext) {
    let (state, visual) = animated(cx, "keyframes-animated");
    let rows: Vec<(String, bool)> = visual.read(|app| {
        let state = state.read(app);
        state
            .key_params()
            .iter()
            .map(|row| {
                let animated =
                    !row.marks.is_empty() || state.selected_driver(row.param()).is_some();
                (row.id.to_string(), animated)
            })
            .collect()
    });
    let animated: Vec<&str> = rows
        .iter()
        .filter(|(_, animated)| *animated)
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(
        animated.len(),
        2,
        "keying the opacity and driving the rotation animated {animated:?}"
    );
    assert!(
        rows.len() > animated.len(),
        "the texture has no constant property to leave out: {rows:?}"
    );
    for (id, animated) in &rows {
        let selector: &'static str = format!("kf-row-{id}").leak();
        assert_eq!(
            visual.debug_bounds(selector).is_some(),
            *animated,
            "the Keyframes block's row for {id}, which is {}animated",
            if *animated { "" } else { "not " }
        );
    }
}

#[gpui::test]
async fn every_keyframes_row_names_its_property_on_the_label_column(cx: &mut TestAppContext) {
    let (state, visual) = animated(cx, "keyframes-label-column");
    let ids: Vec<String> = visual.read(|app| {
        let state = state.read(app);
        [ClipParam::Opacity, ClipParam::Rotation]
            .into_iter()
            .map(|param| {
                state
                    .key_param(param)
                    .expect("the texture animates the property")
                    .id
                    .to_string()
            })
            .collect()
    });
    let column = visual
        .debug_bounds("insp-label-Opacity")
        .expect("the Transform tab draws the opacity row")
        .left();
    for id in ids {
        let selector: &'static str = format!("kf-label-{id}").leak();
        let name = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the Keyframes block draws no name for {id}"));
        assert!(
            (f32::from(name.left()) - f32::from(column)).abs() < EPS,
            "the Keyframes row of {id} starts its name at {:?}, off the label column at {column:?}",
            name.left()
        );
    }
}
