//! Contracts for how the preview is looked at.
//!
//! The class closed here is "the preview is a fixed window on the frame": there
//! was no way to fill the screen with the picture, no way to inspect a canvas
//! larger than the pane, and no safe-area or grid overlay to compose against.
//! Zoom closes a second class, "a zoom level that does not mean what it
//! reads": 100% drawn as the fitted size, a zoomed frame anchored to the
//! pane's top left, a pan that slid the picture out of the pane, and a step
//! from Fit that skipped the levels next to the fitted size.
//!
//! The rules: a viewing position is never an edit; a level is device pixels
//! per canvas pixel; a zoomed frame stands on the pane's center and pans only
//! as far as its edges reach the pane's; a zoom change holds the canvas point
//! at the center; a step moves one level past the zoom on screen and stops at
//! both ends; Esc gives fullscreen back once.
//!
//! Not covered: the drawn overlays and the drag itself, which need a render
//! surface. The panel reads these flags and calls `pan_preview` with a delta.

use super::AppState;
use super::premiere_tests::with_clip;
use crate::app::canvas_overlay::Rect;
use crate::app::preview_view::{ZOOM_STEPS, zoom_label};

/// A clip's state with the Viewer painted at `fraction` of the canvas's size
/// on a display of `ratio` device pixels per window pixel, and the canvas
/// size in pixels.
fn painted(fraction: f32, ratio: f32) -> (AppState, [f32; 2]) {
    let (mut state, _) = with_clip();
    state.preview_view.pixel_ratio = ratio;
    let canvas = state.canvas_size().map(|side| side as f32);
    state.record_overlay_frame(Rect::new(
        0.0,
        0.0,
        canvas[0] * fraction,
        canvas[1] * fraction,
    ));
    (state, canvas)
}

/// The painted pane and where the frame is drawn in it.
fn pane_and_frame(state: &AppState) -> (Rect, Rect) {
    let frame = state.overlay_frame;
    (
        frame.container.expect("the pane is painted"),
        frame.canvas.expect("the frame is placed"),
    )
}

/// The canvas point under the pane's center, as fractions of the canvas.
fn centered_point(state: &AppState) -> [f32; 2] {
    let (pane, frame) = pane_and_frame(state);
    [
        (pane.x + pane.w / 2.0 - frame.x) / frame.w,
        (pane.y + pane.h / 2.0 - frame.y) / frame.h,
    ]
}

/// Zoom to `zoom` and record the pane again, as the next paint does.
fn zoom_to(state: &mut AppState, zoom: Option<f32>) {
    state.set_preview_zoom(zoom);
    repaint(state);
}

fn repaint(state: &mut AppState) {
    let pane = state.overlay_frame.container.expect("the pane is painted");
    state.record_overlay_frame(pane);
}

#[test]
fn the_three_guides_are_independent_and_start_off() {
    let (mut state, _) = with_clip();
    let view = state.preview_view;
    assert!(!view.title_safe && !view.action_safe && !view.grid);

    state.toggle_title_safe();
    assert!(state.preview_view.title_safe);
    assert!(
        !state.preview_view.action_safe,
        "one guide is not the other"
    );
    assert!(!state.preview_view.grid);

    state.toggle_action_safe();
    state.toggle_preview_grid();
    assert!(state.preview_view.title_safe);
    assert!(state.preview_view.action_safe);
    assert!(state.preview_view.grid);

    state.toggle_title_safe();
    assert!(!state.preview_view.title_safe);
    assert!(state.preview_view.action_safe, "the others stay put");
}

#[test]
fn a_guide_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let depth = state.history_depth();
    state.toggle_title_safe();
    state.toggle_preview_grid();
    state.preview_zoom_in();
    state.toggle_preview_fullscreen();
    assert_eq!(
        state.history_depth(),
        depth,
        "how the preview is looked at is not undoable"
    );
}

#[test]
fn a_fitted_view_reports_fit_and_cannot_pan() {
    let (mut state, _) = painted(0.5, 1.0);
    assert_eq!(zoom_label(state.preview_view.zoom), "Fit");
    assert!(!state.preview_pannable());

    state.pan_preview(40.0, -20.0);
    assert_eq!(
        state.preview_view.pan,
        (0.0, 0.0),
        "a fitted frame has nowhere to pan to"
    );
}

#[test]
fn each_step_moves_one_level_and_stops_at_both_ends() {
    let (mut state, _) = painted(0.5, 1.0);
    state.set_preview_zoom(ZOOM_STEPS.first().copied());
    let mut climbed = vec![state.preview_view.zoom];
    for _ in 0..ZOOM_STEPS.len() + 2 {
        state.preview_zoom_in();
        climbed.push(state.preview_view.zoom);
    }
    climbed.dedup();
    assert_eq!(
        climbed,
        ZOOM_STEPS.map(Some),
        "zooming in visits every level once and stops at the top"
    );

    let mut fell = vec![state.preview_view.zoom];
    for _ in 0..ZOOM_STEPS.len() + 2 {
        state.preview_zoom_out();
        fell.push(state.preview_view.zoom);
    }
    fell.dedup();
    let mut down = ZOOM_STEPS.map(Some);
    down.reverse();
    assert_eq!(
        fell, down,
        "zooming out visits every level once and stops at the bottom"
    );
}

/// From Fit, a step lands on the next level past the size the fitted frame
/// is drawn at, whatever that size is, and a fitted frame past either end
/// stays fitted.
#[test]
fn a_step_from_fit_lands_on_the_level_next_to_the_fitted_size() {
    for fraction in [0.05, 0.3, 0.5, 1.0, 2.5, 10.0] {
        for ratio in [1.0, 2.0] {
            let fitted = fraction * ratio;
            let above = ZOOM_STEPS.into_iter().find(|step| *step > fitted);
            let below = ZOOM_STEPS.into_iter().rev().find(|step| *step < fitted);

            let (mut state, _) = painted(fraction, ratio);
            let fit = state.preview_fit_zoom().expect("the pane is painted");
            assert!(
                (fit - fitted).abs() < 1e-5,
                "a pane {fraction} of the canvas at {ratio}x fits at {fitted}, not {fit}"
            );
            state.preview_zoom_in();
            assert_eq!(
                state.preview_view.zoom, above,
                "zoom in from a fit at {fitted}"
            );

            let (mut state, _) = painted(fraction, ratio);
            state.preview_zoom_out();
            assert_eq!(
                state.preview_view.zoom, below,
                "zoom out from a fit at {fitted}"
            );
        }
    }
}

#[test]
fn a_hundred_percent_draws_one_canvas_pixel_per_device_pixel() {
    for ratio in [1.0, 1.5, 2.0] {
        let (mut state, canvas) = painted(0.5, ratio);
        zoom_to(&mut state, Some(1.0));
        let (_, frame) = pane_and_frame(&state);
        assert!(
            (frame.w - canvas[0] / ratio).abs() < 1e-3
                && (frame.h - canvas[1] / ratio).abs() < 1e-3,
            "at {ratio}x a {canvas:?} canvas at 100% is drawn {} by {}",
            frame.w,
            frame.h
        );
    }
}

#[test]
fn every_level_stands_the_frame_on_the_pane_center() {
    let (mut state, _) = painted(0.5, 1.0);
    for level in ZOOM_STEPS {
        zoom_to(&mut state, Some(level));
        let (pane, frame) = pane_and_frame(&state);
        let off = [
            frame.x + frame.w / 2.0 - (pane.x + pane.w / 2.0),
            frame.y + frame.h / 2.0 - (pane.y + pane.h / 2.0),
        ];
        assert!(
            off[0].abs() < 1e-3 && off[1].abs() < 1e-3,
            "at {level} the frame's center is {off:?} off the pane's"
        );
    }
}

#[test]
fn the_pan_stops_where_the_frame_edge_meets_the_pane_edge() {
    let (mut state, _) = painted(0.5, 1.0);
    zoom_to(&mut state, Some(1.0));
    assert!(state.preview_pannable());

    state.pan_preview(1.0e6, -1.0e6);
    repaint(&mut state);
    let (pane, frame) = pane_and_frame(&state);
    assert!(
        (frame.x - pane.x).abs() < 1e-3,
        "panned right as far as it goes, the frame's left edge {} meets the pane's {}",
        frame.x,
        pane.x
    );
    assert!(
        (frame.y + frame.h - (pane.y + pane.h)).abs() < 1e-3,
        "panned up as far as it goes, the frame's bottom edge meets the pane's"
    );

    state.pan_preview(-1.0e6, 1.0e6);
    repaint(&mut state);
    let (pane, frame) = pane_and_frame(&state);
    assert!((frame.x + frame.w - (pane.x + pane.w)).abs() < 1e-3);
    assert!((frame.y - pane.y).abs() < 1e-3);
}

#[test]
fn a_zoom_change_holds_the_canvas_point_at_the_pane_center() {
    let (mut state, canvas) = painted(0.5, 1.0);
    zoom_to(&mut state, Some(1.0));
    state.pan_preview(canvas[0] / 8.0, -canvas[1] / 8.0);
    repaint(&mut state);
    let held = centered_point(&state);
    assert_ne!(held, [0.5, 0.5], "the pan moved the frame");

    for step in [
        AppState::preview_zoom_in,
        AppState::preview_zoom_in,
        AppState::preview_zoom_out,
        AppState::preview_zoom_out,
        AppState::preview_zoom_out,
    ] {
        step(&mut state);
        repaint(&mut state);
        let at = centered_point(&state);
        assert!(
            (at[0] - held[0]).abs() < 1e-4 && (at[1] - held[1]).abs() < 1e-4,
            "at {:?} the pane's center shows {at:?}, not {held:?}",
            state.preview_view.zoom
        );
    }
}

#[test]
fn a_frame_no_larger_than_the_pane_stays_centered() {
    let (mut state, _) = painted(0.5, 1.0);
    zoom_to(&mut state, Some(1.0));
    state.pan_preview(50.0, 50.0);
    assert_ne!(state.preview_view.pan, (0.0, 0.0));

    zoom_to(&mut state, Some(0.5));
    assert_eq!(
        state.preview_view.pan,
        (0.0, 0.0),
        "a frame that fits the pane again must not stay offset"
    );
    assert!(!state.preview_pannable());
    state.pan_preview(50.0, 50.0);
    assert_eq!(state.preview_view.pan, (0.0, 0.0));
}

#[test]
fn a_resized_pane_pulls_the_pan_back_inside_the_frame() {
    let (mut state, canvas) = painted(0.5, 1.0);
    zoom_to(&mut state, Some(1.0));
    state.pan_preview(1.0e6, 1.0e6);
    assert_eq!(state.preview_view.pan, (canvas[0] / 4.0, canvas[1] / 4.0));

    let wider = Rect::new(0.0, 0.0, canvas[0] * 0.75, canvas[1] * 0.75);
    assert!(state.record_overlay_frame(wider), "the pan moved");
    assert_eq!(state.preview_view.pan, (canvas[0] / 8.0, canvas[1] / 8.0));
}

#[test]
fn fit_drops_the_zoom_and_the_pan() {
    let (mut state, _) = painted(0.5, 1.0);
    zoom_to(&mut state, Some(2.0));
    state.pan_preview(30.0, -10.0);
    state.pan_preview(5.0, 5.0);
    assert_eq!(state.preview_view.pan, (35.0, -5.0), "pans accumulate");

    state.preview_zoom_fit();
    assert_eq!(state.preview_view.zoom, None);
    assert_eq!(state.preview_view.pan, (0.0, 0.0));
}

#[test]
fn fullscreen_toggles_and_takes_the_viewer() {
    let (mut state, _) = with_clip();
    state.active_panel = crate::types::ActivePanel::Ai;
    state.toggle_preview_fullscreen();
    assert!(state.preview_view.fullscreen);
    assert_eq!(state.active_panel, crate::types::ActivePanel::Preview);

    state.toggle_preview_fullscreen();
    assert!(!state.preview_view.fullscreen);
}

#[test]
fn escape_gives_fullscreen_back_once() {
    let (mut state, _) = with_clip();
    assert!(
        !state.leave_preview_fullscreen(),
        "there is nothing to leave"
    );

    state.toggle_preview_fullscreen();
    assert!(state.leave_preview_fullscreen());
    assert!(!state.preview_view.fullscreen);
    assert!(
        !state.leave_preview_fullscreen(),
        "a second Esc must fall through to the shell's other meanings"
    );
}

#[test]
fn every_level_reads_as_its_own_whole_percentage() {
    assert_eq!(zoom_label(None), "Fit");
    let labels: Vec<String> = ZOOM_STEPS.map(|step| zoom_label(Some(step))).to_vec();
    for (label, step) in labels.iter().zip(ZOOM_STEPS) {
        assert_eq!(*label, format!("{}%", (step * 100.0).round()));
    }
    let mut distinct = labels.clone();
    distinct.dedup();
    assert_eq!(distinct, labels, "two levels read the same");
    assert!(
        ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]),
        "the levels ascend"
    );
}
