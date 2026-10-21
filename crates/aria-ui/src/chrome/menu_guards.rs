//! Why a menu row cannot run: one function per condition, shared by every row
//! that has it.
//!
//! A guard reads the state the row would act on and returns the reason the row
//! is drawn disabled, or `None` when the row can run. The reason is the
//! disabled row's tooltip, and the status line when the row is chosen anyway
//! from the keyboard or the palette. Each reads what the row's own code reads,
//! so a row is not drawn enabled over a state it would refuse.

use crate::app::AppState;

/// The row can always run.
pub fn ready(_: &str, _: &AppState) -> Option<&'static str> {
    None
}

/// The row acts on the selected clips.
pub fn selection(_: &str, state: &AppState) -> Option<&'static str> {
    state.selection.is_empty().then_some("No clip is selected.")
}

/// The row acts on the selected clip's active mask.
pub fn mask(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .active_mask()
        .is_none()
        .then_some("The selected clip has no mask.")
}

/// The row acts on selected clips that carry sound.
pub fn sound(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_has_sound()).then_some("Select a clip with sound.")
}

/// The row imports a file that is still where it was imported from.
pub fn import_recent(id: &str, _: &AppState) -> Option<&'static str> {
    let path = id.strip_prefix(super::menu_file::IMPORT_RECENT_PREFIX)?;
    (!std::path::Path::new(path).exists()).then_some("The file is missing.")
}

/// The row empties an Import Recent list that holds a file.
pub fn import_recents(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .import_recents()
        .is_empty()
        .then_some("No file has been imported yet.")
}

/// The row acts on two or more selected clips.
pub fn several(_: &str, state: &AppState) -> Option<&'static str> {
    (state.selection.len() < 2).then_some("Select two or more clips.")
}

/// The row acts on a selection that is linked.
pub fn linked(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_is_linked()).then_some("The selected clips are not linked.")
}

/// The row acts on a selection that forms a clip group.
pub fn grouped(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_is_grouped()).then_some("The selected clips are not a group.")
}

/// The row pastes what Copy or Cut put on the clipboard.
pub fn clipboard(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .clipboard
        .is_none()
        .then_some("Nothing has been copied or cut.")
}

/// The row steps back through the history.
pub fn undo(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.can_undo).then_some("Nothing to undo.")
}

/// The row steps forward through the history.
pub fn redo(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.can_redo).then_some("Nothing to redo.")
}

/// The row acts on the sequence In or Out point.
pub fn marks(_: &str, state: &AppState) -> Option<&'static str> {
    (state.mark_in.is_none() && state.mark_out.is_none())
        .then_some("Neither an In nor an Out point is set.")
}

/// The row acts on the sequence In point.
pub fn mark_in(_: &str, state: &AppState) -> Option<&'static str> {
    state.mark_in.is_none().then_some("No In point is set.")
}

/// The row acts on the sequence Out point.
pub fn mark_out(_: &str, state: &AppState) -> Option<&'static str> {
    state.mark_out.is_none().then_some("No Out point is set.")
}

/// The row acts on the sequence's markers.
pub fn markers(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .store
        .sequence
        .markers
        .is_empty()
        .then_some("The sequence has no markers.")
}

/// The row acts on the clips of the sequence.
pub fn clips(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .all(|track| track.clips.is_empty())
        .then_some("The sequence has no clips.")
}

/// The row acts on the span between the In and Out points.
pub fn range(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .marked_range()
        .is_none()
        .then_some("Mark an In and an Out point first.")
}

/// The row acts on the selected keyframes, or on the keyframes under the
/// playhead on the selected clip.
pub fn keyframes(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .ease_target_keyframes()
        .is_empty()
        .then_some("Select a keyframe, or put the playhead on one.")
}

/// The row acts on the property rows selected in the timeline outline.
pub fn property_rows(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .with_prop_rows(|rows| rows.graph_params.is_empty())
        .then_some("Select a property row in the timeline.")
}

/// The row acts on two or more selected keyframes of one property.
pub fn keyframe_span(_: &str, state: &AppState) -> Option<&'static str> {
    let selected = state.selected_timeline_keyframes();
    let paired = selected.iter().enumerate().any(|(i, key)| {
        selected[i + 1..]
            .iter()
            .any(|other| other.clip == key.clip && other.param == key.param)
    });
    (!paired).then_some("Select two or more keyframes on one property.")
}

/// The row acts on a selected property row of the selected clip.
pub fn clip_property_row(_: &str, state: &AppState) -> Option<&'static str> {
    let clip = state.selected_clip();
    let picked = state.with_prop_rows(|rows| {
        rows.graph_params
            .iter()
            .any(|(owner, _)| Some(*owner) == clip)
    });
    (!picked).then_some("Select a property row of the selected clip.")
}

/// The row acts on the selected edit point.
pub fn edge(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .selected_edge
        .is_none()
        .then_some("Select an edit point first.")
}

/// The row reopens the saved file over unsaved edits.
pub fn revertible(_: &str, state: &AppState) -> Option<&'static str> {
    if state.project_path.is_none() {
        return Some("The project has never been saved.");
    }
    (!state.store.is_dirty).then_some("There are no changes since the last save.")
}

/// The row acts on the marked range of the source in the Source monitor.
pub fn source_range(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .source_range()
        .is_none()
        .then_some("Open a source in the Source monitor first.")
}

/// The row places the media selected in the pool.
pub fn pool_selection(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.pool_selection_online()).then_some("No media is selected in the pool.")
}

/// The row acts on the source in the Source monitor.
pub fn source(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .monitor_source()
        .is_none()
        .then_some("Open a source in the Source monitor first.")
}

/// The row acts on the comparison the Viewer draws.
pub fn comparing(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.viewer_compare.comparing()).then_some("Nothing is being compared.")
}

/// The row applies the effect applied last.
pub fn last_effect(_: &str, state: &AppState) -> Option<&'static str> {
    if state.last_effect.is_none() {
        return Some("No effect has been applied yet.");
    }
    selection("", state)
}

/// The row pastes the copied effect onto the selected clips.
pub fn copied_effect(_: &str, state: &AppState) -> Option<&'static str> {
    if state.copied_effect.is_none() {
        return Some("No effect has been copied.");
    }
    selection("", state)
}

/// The row acts on the effects of the selected clips.
pub fn effects(_: &str, state: &AppState) -> Option<&'static str> {
    if state.selection.is_empty() {
        return Some("No clip is selected.");
    }
    let any = state.selection.iter().any(|clip| {
        state
            .ui_clip_ref(*clip)
            .is_some_and(|ui| !ui.effects.is_empty())
    });
    (!any).then_some("The selected clips have no effects.")
}

/// The row opens a project from the recent list, by the path in its id.
pub fn recent(id: &str, state: &AppState) -> Option<&'static str> {
    let path = id.strip_prefix(super::menu_file::RECENT_PREFIX)?;
    state
        .recents()
        .iter()
        .any(|view| view.missing && view.path.as_os_str() == path)
        .then_some("The project file is missing.")
}

/// The row exports through a timeline exporter.
pub fn exporters(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .ctx
        .exporters()
        .is_empty()
        .then_some("No timeline exporter is mounted.")
}

/// The row deletes tracks that hold no clips.
pub fn empty_tracks(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .empty_track_ids()
        .is_empty()
        .then_some("Every track holds a clip.")
}
