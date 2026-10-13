//! What a shipped key binding does, when the answer is more than one call.
//!
//! Moved out of [`super::bindings`] with the row tables: two row files call these
//! and the table file holds neither, so one action has one owner and no row
//! reimplements a neighbour's behaviour.

use crate::app::DialogKind;

pub(super) fn zoom_in(state: &mut crate::app::AppState) {
    state.set_zoom(state.zoom * crate::transport::ZOOM_STEP, true);
}

pub(super) fn zoom_out(state: &mut crate::app::AppState) {
    state.set_zoom(state.zoom / crate::transport::ZOOM_STEP, true);
}

pub(super) fn delete_selected(state: &mut crate::app::AppState) {
    if let Err(err) = state.delete_selected() {
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

/// A step key nudges the selected clip, or moves the playhead when nothing is
/// selected. One owner for both directions and both step sizes.
pub(super) fn step(state: &mut crate::app::AppState, frames: i64) {
    if state.selected_clip().is_some() {
        if let Err(err) = state.nudge_selected(frames) {
            state.report_error(&err);
        }
    } else {
        state.set_playhead(state.playhead.value + frames);
    }
}
