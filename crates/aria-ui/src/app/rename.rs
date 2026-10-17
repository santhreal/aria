//! Inline typed entry: names for markers, tracks, clips, effects, the project, a
//! template slot and a title, plus the timeline's shift offset.
//!
//! Typing is key-routed exactly like the command palette instead of focused
//! into a GPUI text field, so one path serves every target and there is no second
//! text-entry convention to keep in sync. `ShiftFrames` is a number rather than a
//! name and rides the same path for that reason.
//!
//! The draft opens with its whole name selected, so typing replaces the name
//! and an arrow key keeps it. The arrows, Home and End move the caret, Shift
//! extends the selection, and Ctrl+Z restores the name the rename opened with.
//!
//! Closes `SetTrackName` and `SetProjectName`, which had no UI call site, and
//! gives `SetMarker`'s rename an operator path.

use std::ops::Range;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::TrackId;

use super::AppState;
use super::clip_text::text_caret::geometry::{byte_at, word_left, word_right};
use crate::keys::fields::CaretKey;
use crate::types::RenameTarget;

/// Longest accepted name. Long enough for a real label, short enough that a
/// held key cannot grow the draft without bound.
pub const RENAME_MAX_LEN: usize = 64;

/// The caret and selection in the rename draft, in characters.
///
/// Read through [`AppState::rename_selection`], which clamps both ends to the
/// draft, so a draft set without moving the caret cannot leave it past the end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RenameCaret {
    /// The fixed end of the selection; the caret when nothing is selected.
    anchor: usize,
    /// Characters of the draft before the caret.
    caret: usize,
    /// The draft an undo replaced with the opening name, for a redo.
    redo: Option<String>,
}

impl AppState {
    /// Begin renaming `target`, seeding the draft with its current name, all of
    /// it selected.
    ///
    /// Inert when the target no longer exists, so a stale menu cannot open a
    /// rename over nothing.
    pub fn begin_rename(&mut self, target: RenameTarget) {
        let Some(current) = self.current_name(target) else {
            return;
        };
        self.rename_caret = RenameCaret {
            anchor: 0,
            caret: current.chars().count(),
            redo: None,
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
            RenameTarget::SaveWorkspace => Some(String::new()),
            RenameTarget::Effect { clip, index } => self
                .effect_on(aria_models::ClipId::from_raw(clip), index)
                .map(|effect| effect.display_name()),
        }
    }

    pub fn cancel_rename(&mut self) {
        self.rename = None;
        self.rename_draft.clear();
        self.rename_caret = RenameCaret::default();
    }

    /// Type `ch` over the selection, or at the caret.
    pub fn rename_push(&mut self, ch: char) {
        if self.rename.is_none() {
            return;
        }
        let (anchor, caret) = self.rename_selection();
        self.replace_rename_text(
            anchor.min(caret)..anchor.max(caret),
            ch.encode_utf8(&mut [0; 4]),
        );
    }

    /// Backspace: the selection, or the character before the caret.
    pub fn rename_pop(&mut self) {
        let (anchor, caret) = self.rename_selection();
        let range = if anchor == caret {
            caret.saturating_sub(1)..caret
        } else {
            anchor.min(caret)..anchor.max(caret)
        };
        self.replace_rename_text(range, "");
    }

    /// `(anchor, caret)` in the draft, in characters.
    pub fn rename_selection(&self) -> (usize, usize) {
        let len = self.rename_draft.chars().count();
        (
            self.rename_caret.anchor.min(len),
            self.rename_caret.caret.min(len),
        )
    }

    /// The selected part of the draft, `None` with nothing selected.
    pub fn rename_selected_text(&self) -> Option<String> {
        let (anchor, caret) = self.rename_selection();
        (anchor != caret).then(|| {
            let text = &self.rename_draft;
            text[byte_at(text, anchor.min(caret))..byte_at(text, anchor.max(caret))].to_owned()
        })
    }

    /// Splice `text` over characters `range` of the draft and put the caret
    /// after it: a keystroke, a paste, or an IME commit. Control characters are
    /// dropped, and the draft stops at `RENAME_MAX_LEN` characters.
    pub fn replace_rename_text(&mut self, range: Range<usize>, text: &str) {
        let len = self.rename_draft.chars().count();
        let end = range.end.min(len);
        let start = range.start.min(end);
        let room = RENAME_MAX_LEN.saturating_sub(len - (end - start));
        let insert: String = text
            .chars()
            .filter(|ch| !ch.is_control())
            .take(room)
            .collect();
        if start == end && insert.is_empty() {
            return;
        }
        let span = byte_at(&self.rename_draft, start)..byte_at(&self.rename_draft, end);
        self.rename_draft.replace_range(span, &insert);
        let caret = start + insert.chars().count();
        self.rename_caret = RenameCaret {
            anchor: caret,
            caret,
            redo: None,
        };
    }

    /// Move, select, delete forward, or undo in the draft. `extend` is Shift
    /// held: the caret moves and the anchor stays.
    pub fn rename_caret_key(&mut self, key: CaretKey, extend: bool) {
        let Some(target) = self.rename else {
            return;
        };
        let (anchor, caret) = self.rename_selection();
        let text = &self.rename_draft;
        let len = text.chars().count();
        let (low, high) = (anchor.min(caret), anchor.max(caret));
        let collapse = !extend && anchor != caret;
        let to = match key {
            CaretKey::Delete => {
                let range = if anchor == caret {
                    caret..(caret + 1).min(len)
                } else {
                    low..high
                };
                self.replace_rename_text(range, "");
                return;
            }
            CaretKey::SelectAll => {
                self.rename_caret.anchor = 0;
                self.rename_caret.caret = len;
                return;
            }
            // The draft is not in the edit history: undo is the name the
            // rename opened with, and redo is the draft that undo replaced.
            CaretKey::Undo => {
                let Some(opening) = self.current_name(target).filter(|name| name != text) else {
                    return;
                };
                let typed = std::mem::replace(&mut self.rename_draft, opening);
                self.rename_caret = RenameCaret {
                    anchor: 0,
                    caret: self.rename_draft.chars().count(),
                    redo: Some(typed),
                };
                return;
            }
            CaretKey::Redo => {
                let Some(typed) = self.rename_caret.redo.take() else {
                    return;
                };
                let end = typed.chars().count();
                self.rename_draft = typed;
                self.rename_caret.anchor = end;
                self.rename_caret.caret = end;
                return;
            }
            CaretKey::Left if collapse => low,
            CaretKey::Right if collapse => high,
            CaretKey::Left => caret.saturating_sub(1),
            CaretKey::Right => (caret + 1).min(len),
            CaretKey::WordLeft => word_left(text, caret),
            CaretKey::WordRight => word_right(text, caret),
            // One line: up and down go to its ends.
            CaretKey::LineStart | CaretKey::TextStart | CaretKey::Up => 0,
            CaretKey::LineEnd | CaretKey::TextEnd | CaretKey::Down => len,
        };
        self.rename_caret.caret = to;
        if !extend {
            self.rename_caret.anchor = to;
        }
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
            RenameTarget::SaveWorkspace => self.save_named_workspace(&name),
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
