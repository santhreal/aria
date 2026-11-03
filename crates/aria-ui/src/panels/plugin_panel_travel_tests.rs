//! Every plugin panel slider lights the stretch from its travel's origin to its
//! value: from zero when its bounds run from below zero to above it, and from
//! its lower bound otherwise.
//!
//! The class closed here is a slider filled from its start whatever its bounds:
//! a slider from -1 to 1 at zero drew half its track lit, as a value halfway up
//! an all-positive slider does. One panel draws a slider for each pair of
//! bounds, all-positive, through zero evenly and unevenly, and all-negative, at
//! each quarter of its travel, and each fill is read against the stretch worked
//! out here from the bounds alone.
//!
//! Not caught: the thumb, which has no debug selector; and a slider held in a
//! drag, which reads its value from the drag rather than from the tree.

use aria_plugin::{PanelEvent, PanelLocation, PanelSpec, PanelView, PluginContext, UiNode};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

use crate::app::plugin_panels_tests::settle;
use crate::interned::intern;
use crate::test_support::drawn_shell;

/// All-positive, through zero evenly, through zero unevenly, all-negative,
/// and positive from above zero.
const BOUNDS: [(f64, f64); 5] = [
    (0.0, 10.0),
    (-1.0, 1.0),
    (-10.0, 30.0),
    (-5.0, -1.0),
    (2.0, 8.0),
];
/// Quarters of the travel, both ends among them.
const QUARTERS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn id(bound: usize, quarter: usize) -> String {
    format!("travel-{bound}-{quarter}")
}

/// One slider per pair of bounds per quarter, at that quarter of its travel.
struct Sliders;

impl PanelView for Sliders {
    fn view(&self, _: &PluginContext) -> UiNode {
        let mut rows = Vec::with_capacity(BOUNDS.len() * QUARTERS.len());
        for (bound, (min, max)) in BOUNDS.into_iter().enumerate() {
            for (quarter, at) in QUARTERS.into_iter().enumerate() {
                rows.push(UiNode::Slider {
                    id: id(bound, quarter).into(),
                    label: "".into(),
                    value: min + at * (max - min),
                    min,
                    max,
                    step: 0.01,
                });
            }
        }
        UiNode::column(rows)
    }

    fn event(&self, _: &PluginContext, _: &PanelEvent) -> Result<(), String> {
        Ok(())
    }
}

/// A box's start and width, left to right.
fn across(visual: &mut VisualTestContext, selector: &str) -> (f32, f32) {
    let bounds: Bounds<Pixels> = visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the panel draws no {selector}"));
    (f32::from(bounds.left()), f32::from(bounds.size.width))
}

#[gpui::test]
fn every_plugin_slider_lights_from_its_origin_to_its_value(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "plugin-slider-travel");
    visual.update(|window, app| {
        app.set_reduce_motion(true);
        state.update(app, |state, _| {
            let registration = state
                .ctx
                .register_panel(
                    PanelSpec {
                        id: "test.travel".into(),
                        title: "Travel".into(),
                        location: PanelLocation::Sidebar,
                    },
                    Sliders,
                )
                .unwrap_or_else(|err| panic!("the travel panel registers: {err:?}"));
            state._services.push(registration);
            settle(state);
        });
        window.refresh();
    });
    visual.run_until_parked();
    for (bound, (min, max)) in BOUNDS.into_iter().enumerate() {
        let origin = if min < 0.0 && max > 0.0 {
            -min / (max - min)
        } else {
            0.0
        };
        for (quarter, at) in QUARTERS.into_iter().enumerate() {
            let slider = format!("plugin-slider-{}", id(bound, quarter));
            let (start, width) = across(visual, &slider);
            let (fill_start, fill_width) = across(visual, &format!("{slider}-fill"));
            let (from, to) = (origin.min(at) as f32, origin.max(at) as f32);
            let (want_start, want_width) = (start + from * width, (to - from) * width);
            assert!(
                (fill_start - want_start).abs() < 1.0 && (fill_width - want_width).abs() < 1.0,
                "the slider from {min} to {max} at {at} of its travel is lit {fill_width} px \
                 from {fill_start} px, and covers {from}..{to} of its {width} px from {start} \
                 px: {want_width} px from {want_start} px"
            );
        }
    }
}
