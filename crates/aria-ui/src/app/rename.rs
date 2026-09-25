//! Inline typed entry: names for markers, tracks, the project, a template slot
//! and a title, plus the timeline's shift offset.
//!
//! Typing is key-routed exactly like the command palette instead of focused
//! into a GPUI text field, so one path serves every target and there is no second
//! text-entry convention to keep in sync. `ShiftFrames` is a number rather than a
//! name and rides the same path for that reason.
//!
//! Closes `SetTrackName` and `SetProjectName`, which had no UI call site, and
//! gives `SetMarker`'s rename an operator path.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::TrackId;

use super::AppState;
use crate::types::RenameTarget;

/// Longest accepted name. Long enough for a real label, short enough that a
/// held key cannot grow the draft without bound.
pub const RENAME_MAX_LEN: usize = 64;

impl AppState {
    /// Begin renaming `target`, seeding the draft with its current name.
    ///
    /// Inert when the target no longer exists, so a stale menu cannot open a
    /// rename over nothing.
    pub fn begin_rename(&mut self, target: RenameTarget) {
        let Some(current) = self.current_name(target) else {
            return;
        };
        self.rename_draft = current;
        self.rename = Some(target);
    }

    /// Begin renaming the marker under the playhead. Inert when there is none.
    pub fn begin_rename_marker_at_playhead(&mut self) {
        let Some(id) = self.marker_at_playhead().map(|marker| marker.id) else {
            self.status_message = Some("No marker at the playhead".into());
            return;
        };
        self.begin_rename(RenameTarget::Marker(id));
    }

    fn current_name(&self, target: RenameTarget) -> Option<String> {
        match target {
            RenameTarget::Marker(id) => self
                .store
                .sequence
                .markers
                .iter()
                .find(|marker| marker.id == id)
                .map(|marker| marker.name.to_string()),
            RenameTarget::MarkerComment(id) => self
                .store
                .sequence
                .markers
                .iter()
                .find(|marker| marker.id == id)
                .map(|marker| marker.comment.to_string()),
            RenameTarget::Track(id) => self
                .store
                .sequence
                .tracks
                .iter()
                .find(|track| track.id == id)
                .map(|track| track.name.to_string()),
            RenameTarget::Clip(id) => self
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .find(|clip| clip.id == id)
                .map(|clip| clip.name.to_string()),
            RenameTarget::Project => Some(self.store.project_name.to_string()),
            // An unset hint still opens a draft: the slot exists, the hint is
            // just empty.
            RenameTarget::SlotLabel => self
                .selected_slot()
                .map(|slot| slot.label.map(|hint| hint.to_string()).unwrap_or_default()),
            RenameTarget::ClipText => self
                .selected_ui_clip()
                .and_then(|clip| clip.generator)
                .filter(|generator| generator.kind == "text")
                .map(|generator| generator.content.to_string()),
            // A number, not a name: the draft starts empty every time, because the
            // last offset is not a property of anything on the timeline.
            RenameTarget::ShiftFrames => Some(String::new()),
        }
    }

    pub fn cancel_rename(&mut self) {
        self.rename = None;
        self.rename_draft.clear();
    }

    pub fn rename_push(&mut self, ch: char) {
        if self.rename.is_none() || self.rename_draft.chars().count() >= RENAME_MAX_LEN {
            return;
        }
        self.rename_draft.push(ch);
    }

    pub fn rename_pop(&mut self) {
        self.rename_draft.pop();
    }

    /// Apply the draft. An unchanged draft cancels instead of dispatching, so
    /// committing twice is not two undo entries. An empty draft also cancels,
    /// except on a target that accepts one: clearing a marker's note is a real
    /// edit, and a name is not allowed to be blank.
    pub fn commit_rename(&mut self) -> Result<(), EngineError> {
        let Some(target) = self.rename else {
            return Ok(());
        };
        let name = self.rename_draft.trim().to_string();
        let unchanged = self
            .current_name(target)
            .is_some_and(|current| current == name);
        if unchanged || (name.is_empty() && !target.allows_empty()) {
            self.cancel_rename();
            return Ok(());
        }
        let result = match target {
            RenameTarget::Marker(id) => self.rename_marker(id, name),
            RenameTarget::MarkerComment(id) => self.set_marker_comment(id, name),
            RenameTarget::Track(id) => self
                .apply_command(Command::Edit(EditCommand::SetTrackName {
                    track: TrackId::from_raw(id),
                    name,
                }))
                .map(|_| ()),
            RenameTarget::Project => self
                .apply_command(Command::Edit(EditCommand::SetProjectName { name }))
                .map(|_| ()),
            // An empty draft is a value: it clears the operator's name so the
            // card takes the source's name again.
            RenameTarget::Clip(id) => self
                .apply_command(Command::Edit(EditCommand::SetClipName {
                    clip: aria_models::ClipId::from_raw(id),
                    name: (!name.is_empty()).then_some(name),
                }))
                .map(|_| ()),
            RenameTarget::SlotLabel => self.set_selected_slot_label(&name),
            RenameTarget::ClipText => self.set_selected_text(name),
            // A leading `+` reads as an intention, not a number, so it is accepted
            // and dropped; anything else that is not an integer cancels rather than
            // shifting by a guess.
            RenameTarget::ShiftFrames => match name.trim_start_matches('+').parse::<i64>() {
                Ok(frames) => self.shift_selection(frames),
                Err(_) => {
                    self.status_message = Some(format!("{name} is not a whole number of frames"));
                    Ok(())
                }
            },
        };
        self.cancel_rename();
        result
    }
}
