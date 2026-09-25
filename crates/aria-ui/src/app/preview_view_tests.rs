//! Contracts for how the preview is looked at.
//!
//! The class closed here is "the preview is a fixed window on the frame": there
//! was no way to fill the screen with the picture, no way to inspect a canvas
//! larger than the pane, and no safe-area or grid overlay to compose against.
//!
//! The rules that matter: a viewing position is never an edit, a fitted view has
//! no pan, zoom walks a fixed ladder rather than multiplying without bound, and
//! Esc gives fullscreen back exactly once.
//!
//! Not covered: the drawn overlays and the drag itself, which need a render
//! surface. The panel reads these flags and calls `pan_preview` with a delta.

use super::premiere_tests::with_clip;
use crate::app::preview_view::ZOOM_STEPS;

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
    let (mut state, _) = with_clip();
    assert_eq!(state.preview_zoom_label(), "Fit");
    assert!(!state.preview_view.pannable());

    state.pan_preview(40.0, -20.0);
    assert_eq!(
        state.preview_view.pan,
        (0.0, 0.0),
        "a fitted frame has nowhere to pan to"
    );
}

#[test]
fn zoom_walks_the_ladder_and_stops_at_both_ends() {
    let (mut state, _) = with_clip();
    state.preview_zoom_actual();
    assert_eq!(state.preview_zoom_label(), "100%");

    state.preview_zoom_in();
    assert_eq!(state.preview_view.zoom, Some(1.5));
    for _ in 0..10 {
        state.preview_zoom_in();
    }
    assert_eq!(
        state.preview_view.zoom,
        Some(ZOOM_STEPS[ZOOM_STEPS.len() - 1]),
        "zooming in must stop at the top step"
    );

    for _ in 0..20 {
        state.preview_zoom_out();
    }
    assert_eq!(
        state.preview_view.zoom,
        Some(ZOOM_STEPS[0]),
        "zooming out must stop at the bottom step"
    );
}

#[test]
fn the_first_zoom_in_from_fit_is_one_step() {
    let (mut state, _) = with_clip();
    state.preview_zoom_in();
    assert_eq!(
        state.preview_view.zoom,
        Some(1.5),
        "fit enters the ladder at 100%"
    );
}

#[test]
fn a_zoomed_view_pans_and_fit_clears_it() {
    let (mut state, _) = with_clip();
    state.preview_zoom_actual();
    state.preview_zoom_in();
    assert!(state.preview_view.pannable());

    state.pan_preview(30.0, -10.0);
    state.pan_preview(5.0, 5.0);
    assert_eq!(state.preview_view.pan, (35.0, -5.0), "pans accumulate");

    state.preview_zoom_fit();
    assert_eq!(state.preview_view.zoom, None);
    assert_eq!(state.preview_view.pan, (0.0, 0.0));
    assert_eq!(state.preview_zoom_label(), "Fit");
}

#[test]
fn zooming_back_below_one_drops_the_pan() {
    let (mut state, _) = with_clip();
    state.preview_zoom_actual();
    state.preview_zoom_in();
    state.pan_preview(50.0, 50.0);
    assert_ne!(state.preview_view.pan, (0.0, 0.0));

    state.preview_zoom_out();
    assert_eq!(
        state.preview_view.pan,
        (0.0, 0.0),
        "a frame that fits again must not stay offset"
    );
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
fn the_zoom_label_is_a_whole_percentage() {
    let (mut state, _) = with_clip();
    for step in ZOOM_STEPS {
        state.preview_view.zoom = Some(step);
        let label = state.preview_zoom_label();
        assert!(label.ends_with('%'), "{label} is not a percentage");
        assert!(
            !label.contains('.'),
            "{label} shows a fraction of a percent"
        );
    }
}
