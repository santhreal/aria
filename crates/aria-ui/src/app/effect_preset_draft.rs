//! Naming an effect preset: the field the rack's Save button opens.
//!
//! # Why
//!
//! A preset could be applied and never made. The rack drew a row per saved
//! preset, and the only thing that ever saved one was a test, so on a fresh
//! install the Presets block was empty and stayed empty however many chains an
//! operator built.
//!
//! Saving needs a name, and a name needs somewhere to type it. This is that
//! somewhere: a draft, opened over the chain being saved, typed into through the
//! key-routed field table like every other box in the editor, and committed as
//! one save. A refused name (empty, or one already taken) keeps the draft open
//! with what was typed still in it, because the operator's next act is to change
//! the name rather than to type the whole thing again.

use super::AppState;

/// The longest name a preset can be given.
///
/// The same ceiling as a pool folder's: a name is a row label, and a row label
/// that does not fit is a row nobody can read.
pub const PRESET_NAME_MAX_LEN: usize = 48;

impl AppState {
    /// Open the name field over the selected clip's chain.
    ///
    /// Refused where the operator is looking when nothing is selected: a name
    /// field that cannot commit is worse than a button that says why.
    pub fn open_effect_preset_draft(&mut self) {
        if self.selected_clip().is_none() {
            self.status_message = Some("Select a clip to save its effects as a preset".into());
            return;
        }
        self.effect_preset_draft = Some(String::new());
    }

    /// Leave the field, keeping the presets as they are.
    pub fn cancel_effect_preset_draft(&mut self) {
        self.effect_preset_draft = None;
    }

    /// Whether the name field is open.
    pub fn effect_preset_draft_open(&self) -> bool {
        self.effect_preset_draft.is_some()
    }

    /// What the field holds, empty when it is closed.
    pub fn effect_preset_draft_text(&self) -> String {
        self.effect_preset_draft.clone().unwrap_or_default()
    }

    pub fn effect_preset_draft_push(&mut self, ch: char) {
        if let Some(draft) = self.effect_preset_draft.as_mut()
            && draft.chars().count() < PRESET_NAME_MAX_LEN
        {
            draft.push(ch);
        }
    }

    pub fn effect_preset_draft_pop(&mut self) {
        if let Some(draft) = self.effect_preset_draft.as_mut() {
            draft.pop();
        }
    }

    /// Save the chain under the typed name.
    ///
    /// The draft closes only on success. A refusal is returned with the field
    /// still open and still holding what was typed.
    pub fn commit_effect_preset_draft(&mut self) -> Result<(), String> {
        let Some(name) = self.effect_preset_draft.as_deref() else {
            return Ok(());
        };
        let name = name.to_owned();
        self.save_selected_effect_preset(name)?;
        self.effect_preset_draft = None;
        Ok(())
    }
}
