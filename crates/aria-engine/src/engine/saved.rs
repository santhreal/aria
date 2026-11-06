//! Whether the session matches the file on disk.
//!
//! The saved state is a history step id: the step on top of the undo stack
//! when the session was last saved, opened or loaded. Undoing or redoing back
//! to that step reads clean again. A change that records no history step
//! clears the saved id, so no history position matches the file until the next
//! save.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::Engine;
use crate::error::EngineError;

impl Engine {
    /// True when the session differs from the state last saved, opened, or
    /// loaded: the current history position is not the saved step, an open
    /// group holds uncommitted edits, or a change that records no history step
    /// landed since.
    pub fn is_dirty(&self) -> bool {
        self.history.group_len() > 0 || self.saved_step_id != Some(self.history.top_undo_id())
    }

    /// Declare the session unsaved without editing it.
    ///
    /// A recovered autosave is content the file on disk does not hold, so the
    /// session has to read dirty even though this process made no edit.
    pub fn mark_unsaved(&mut self) {
        self.unhistoried_change();
    }

    /// Point the session at `path` after a recovery load, adopt its file stem
    /// when the recovered document is still `"untitled"`, and mark the session
    /// unsaved.
    pub fn adopt_recovered_path(&mut self, path: Option<PathBuf>) {
        if let Some(target) = &path {
            crate::action::project::sync_project_name_from_path(
                Arc::make_mut(&mut self.project),
                target,
            );
        }
        self.project_path = path;
        self.unhistoried_change();
    }

    /// Set the project's per-project AI agent rules (`ProjectMetadata`).
    /// Metadata, not timeline state: it bypasses the command layer and is
    /// not undoable by design (like `RelinkMedia`), but it does dirty the
    /// session so the rules save with the project.
    pub fn set_agent_rules(&mut self, rules: String) {
        if self.project.metadata().agent_rules == rules {
            return;
        }
        Arc::make_mut(&mut self.project).metadata_mut().agent_rules = rules;
        self.unhistoried_change();
    }

    /// Write `value` under `key` in `plugin_id`'s per-project store, returning `true` when changed.
    pub fn set_plugin_storage(&mut self, plugin_id: &str, key: &str, value: Vec<u8>) -> bool {
        let changed = Arc::make_mut(&mut self.project).plugin_storage_set(plugin_id, key, value);
        if changed {
            self.unhistoried_change();
        }
        changed
    }

    /// Remove `key` from `plugin_id`'s per-project store, returning `true` when removed.
    pub fn delete_plugin_storage(&mut self, plugin_id: &str, key: &str) -> bool {
        if self.project.plugin_storage_get(plugin_id, key).is_none() {
            return false;
        }
        let removed = Arc::make_mut(&mut self.project).plugin_storage_delete(plugin_id, key);
        if removed {
            self.unhistoried_change();
        }
        removed
    }

    /// Persist mutations made during a save notification (`ProjectSaved`) to `path` and mark clean.
    pub fn finish_post_save(&mut self, path: &Path) -> Result<(), EngineError> {
        if !self.is_dirty() || self.project_path.as_deref() != Some(path) {
            return Ok(());
        }
        self.project.save_to_file(path)?;
        self.mark_saved();
        Ok(())
    }

    /// The session now matches the file on disk at the current history step.
    pub(super) fn mark_saved(&mut self) {
        self.saved_step_id = Some(self.history.top_undo_id());
    }

    /// A project change that records no history step: no undo or redo returns
    /// to the saved state, so the session reads dirty until the next save.
    pub(super) fn unhistoried_change(&mut self) {
        self.saved_step_id = None;
        self.revision += 1;
    }
}
