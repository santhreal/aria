//! What a shipped key binding does, when the answer is more than one call.
//!
//! Moved out of [`super::bindings`] with the row tables: two row files call these
//! and the table file holds neither, so one action has one owner and no row
//! reimplements a neighbour's behaviour.

use crate::app::DialogKind;

pub(super) fn zoom_in(state: &mut crate::app::AppState) {
    state.zoom_around_playhead(crate::transport::ZOOM_STEP);
}

pub(super) fn zoom_out(state: &mut crate::app::AppState) {
    state.zoom_around_playhead(1.0 / crate::transport::ZOOM_STEP);
}

pub(super) fn clear_selected(state: &mut crate::app::AppState) {
    if let Err(err) = state.clear_selected() {
        state.report_error(&err);
    }
}

pub(super) fn ripple_delete(state: &mut crate::app::AppState) {
    if let Err(err) = state.ripple_delete_selected() {
        state.report_error(&err);
    }
}

pub(super) fn redo(state: &mut crate::app::AppState) {
    state.redo();
}

pub(super) fn new_project(state: &mut crate::app::AppState) {
    state.request_new_project();
}

pub(super) fn open_settings(state: &mut crate::app::AppState) {
    state.open_dialog(DialogKind::Settings);
}

/// Left, Right, Shift+Left and Shift+Right: the playhead by `frames`. A
/// selected clip stays where it is; Ctrl+arrow moves or trims the selection.
pub(super) fn step(state: &mut crate::app::AppState, frames: i64) {
    state.step_here(frames);
}

/// `]`, `[`, Shift+`]` and Shift+`[`: the selected clips' gain by `decibels`.
pub(super) fn gain(state: &mut crate::app::AppState, decibels: f32) {
    if let Err(err) = state.step_selected_gain(decibels) {
        state.report_error(&err);
    }
}

/// Ctrl+arrow and Ctrl+Shift+arrow: trim the selected edges, or move the
/// selected clips, by `frames`.
pub(super) fn trim_edges(state: &mut crate::app::AppState, frames: i64) {
    if let Err(err) = state.trim_selected_edges(frames) {
        state.report_error(&err);
    }
}

/// An arrow in the Viewer: move the selected clips by `dx`, `dy` pixels.
pub(super) fn viewer_arrow(state: &mut crate::app::AppState, dx: f32, dy: f32) {
    if let Err(err) = state.viewer_arrow(dx, dy) {
        state.report_error(&err);
    }
}
