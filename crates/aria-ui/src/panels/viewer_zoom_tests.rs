//! The Viewer's zoom controls as they are drawn and pressed.
//!
//! The class closed here: a control in the Viewer's row whose width follows
//! the zoom, so the controls after it move as the zoom changes and a second
//! press on `+` lands on whatever slid under the pointer. The zoom readout
//! was such a control: it grew from `Fit` to `150%` and pushed `+` and the
//! resolution menu along. Every `CONTROL_TIPS` control the row draws is
//! measured at Fit and at every level, so a control added to the row is held
//! in place without an edit here. The zoom menu is pressed through: its list
//! sets the level it reads and closes.
//!
//! Not covered: a Viewer narrow enough to wrap the row, where a wrap point may
//! move, and the drawn picture, which `preview_view_tests` places by the same
//! rectangle the overlays use.

use gpui::{Modifiers, Pixels, TestAppContext, VisualTestContext, px};

use super::viewer_row_height_tests::{measure, press, viewer_with_take};
use crate::app::AppState;
use crate::app::preview_view::{ZOOM_STEPS, zoom_label};

const EPS: f32 = 0.5;

fn zoom_to(visual: &mut VisualTestContext, state: &gpui::Entity<AppState>, zoom: Option<f32>) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.preview_view.zoom = zoom;
            cx.notify();
        })
    });
    visual.run_until_parked();
}

#[gpui::test]
fn every_row_control_holds_its_place_at_every_zoom(cx: &mut TestAppContext) {
    let (state, visual) = viewer_with_take(cx, "viewer-zoom-place");
    let (_, fitted) = measure(visual);
    assert!(
        fitted.len() >= 10,
        "the scan found too few controls: {fitted:?}"
    );
    for level in ZOOM_STEPS {
        zoom_to(visual, &state, Some(level));
        let (_, zoomed) = measure(visual);
        assert_eq!(
            zoomed.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            fitted.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            "at {level} the row draws other controls"
        );
        for ((name, at), (_, was)) in zoomed.iter().zip(&fitted) {
            let moved = [
                at.origin.x - was.origin.x,
                at.origin.y - was.origin.y,
                at.size.width - was.size.width,
            ];
            assert!(
                moved.iter().all(|delta| delta.abs() < px(EPS)),
                "at {level} {name} stands at {at:?}, not {was:?} as at Fit"
            );
        }
    }
}

/// The class: a zoom level counted in window pixels, so 100% on a HiDPI
/// display drew each sequence pixel over two or more display pixels.
#[gpui::test]
fn a_hundred_percent_counts_the_display_pixels_of_the_window(cx: &mut TestAppContext) {
    let (state, visual) = viewer_with_take(cx, "viewer-zoom-actual");
    zoom_to(visual, &state, Some(1.0));
    let ratio = visual.update(|window, _| window.scale_factor());
    let (canvas, drawn) = visual.read(|app| {
        let state = state.read(app);
        (state.canvas_size(), state.overlay_frame.canvas)
    });
    let drawn = drawn.expect("the Viewer places its frame");
    assert!(
        (drawn.w - canvas[0] as f32 / ratio).abs() < 1e-3
            && (drawn.h - canvas[1] as f32 / ratio).abs() < 1e-3,
        "at {ratio}x a {canvas:?} canvas at 100% is drawn {} by {} window pixels",
        drawn.w,
        drawn.h
    );
}

/// The center of the text `label` drawn above the Viewer's control row
/// nearest the zoom chip: a row of the list the chip opens over the frame.
fn listed(visual: &mut VisualTestContext, label: &str) -> gpui::Point<Pixels> {
    let row = visual
        .debug_bounds("viewer-row")
        .expect("the Viewer draws its control row");
    let chip = visual
        .debug_bounds("preview-zoom")
        .expect("the Viewer draws its zoom chip")
        .center();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let far = |at: &gpui::Point<Pixels>| f32::from((at.x - chip.x).abs() + (at.y - chip.y).abs());
    runs.iter()
        .filter(|run| run.text.as_ref() == label)
        .map(|run| run.bounds.center())
        .filter(|at| at.y < row.top())
        .min_by(|a, b| far(a).total_cmp(&far(b)))
        .unwrap_or_else(|| panic!("no {label} is listed above the control row"))
}

#[gpui::test]
fn the_zoom_menu_sets_the_level_it_lists(cx: &mut TestAppContext) {
    let (state, visual) = viewer_with_take(cx, "viewer-zoom-menu");
    let zoom = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            (state.preview_view.zoom, state.viewer_menu)
        })
    };

    for level in ZOOM_STEPS.map(Some).into_iter().chain([None]) {
        press(visual, "preview-zoom");
        let label = zoom_label(level);
        let at = listed(visual, &label);
        visual.simulate_click(at, Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            zoom(visual),
            (level, None),
            "a press on {label} sets it and closes the list"
        );
    }
}
