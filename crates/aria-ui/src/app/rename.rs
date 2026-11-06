//! Inline typed entry: names for markers, tracks, clips, effects, the project, a
//! template slot and a title, plus the timeline's shift offset.
//!
//! Typing is key-routed exactly like the command palette instead of focused
//! into a GPUI text field, so one path serves every target and there is no second
//! text-entry convention to keep in sync. `ShiftFrames` is a number rather than a
//! name and rides the same path for that reason.
//!
//! The draft opens with its whole name selected, so typing replaces the name
//! and an arrow key keeps it. The draft is a [`LineText`], so the caret,
//! selection and undo keys are those of every other box.
//!
//! Closes `SetTrackName` and `SetProjectName`, which had no UI call site, and
//! gives `SetMarker`'s rename a UI path.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::TrackId;

use super::AppState;
use super::decline::Decline;
use super::line_text::{LineRule, LineText};
use crate::types::RenameTarget;

/// Longest accepted name. Long enough for a real label, short enough that a
/// held key cannot grow the draft without bound.
pub(crate) const RENAME_MAX_LEN: usize = 64;

/// What a rename draft takes.
pub(crate) const RENAME_RULE: LineRule = LineRule::chars(RENAME_MAX_LEN);

/// An open rename as a surface draws it: its target, its draft, and the
/// draft's `(anchor, caret)` in characters.
pub(crate) type OpenDraft = (RenameTarget, String, (usize, usize));

impl AppState {
    /// Begin renaming `target`, seeding the draft with its current name, all of
    /// it selected.
    ///
    /// Inert when the target no longer exists, so a stale menu cannot open a
    /// rename over nothing.
    pub fn begin_rename(&mut self, target: RenameTarget) {
        self.open_rename(target, false);
    }

    /// Begin renaming `target` in the row that lists it, which draws the draft
    /// in place of the rename bar. Inert, as [`Self::begin_rename`] is, when
    /// the target no longer exists.
    pub fn begin_rename_in_row(&mut self, target: RenameTarget) {
        self.open_rename(target, true);
    }

    fn open_rename(&mut self, target: RenameTarget, in_row: bool) {
        let Some(current) = self.current_name(target) else {
            return;
        };
        self.rename_draft = LineText::selecting_all(current);
        self.rename = Some(target);
        self.rename_in_row = in_row;
    }

    /// The open rename as a surface draws it, or `None` while none is open.
    pub(crate) fn open_draft(&self) -> Option<OpenDraft> {
        let draft = &self.rename_draft;
        self.rename
            .map(|target| (target, draft.to_string(), draft.selection()))
    }

    /// Begin renaming the marker under the playhead. Inert when there is none.
    pub fn begin_rename_marker_at_playhead(&mut self) {
        let Some(id) = self.marker_at_playhead().map(|marker| marker.id) else {
            self.decline(Decline::NoMarkerAtPlayhead);
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
            RenameTarget::Project => Some(self.project_display_name().to_string()),
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
            RenameTarget::Effect { clip, index } => self
                .effect_on(aria_models::ClipId::from_raw(clip), index)
                .map(|effect| effect.display_name()),
        }
    }

    pub fn cancel_rename(&mut self) {
        self.rename = None;
        self.rename_draft.clear();
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
            // An empty draft is a value: it clears the given name so the
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
            // On the drawn clip, every selected clip holding the same effect
            // there takes the name.
            RenameTarget::Effect { clip, index } => self.effect_verb(
                aria_models::ClipId::from_raw(clip),
                index,
                None,
                |state, clip| state.rename_effect(clip, index, &name),
            ),
        };
        self.cancel_rename();
        result
    }
}
