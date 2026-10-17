//! Property drivers in the inspector: Alt+click on a stopwatch adds or removes
//! one, a driven row shows its source under the value, and the source is
//! edited in a key-routed box (`aria.field.driver`). The pick whip inserts a
//! reference to the next property clicked, on this clip or another, in the
//! driver's own language.
//!
//! Every write is one `SetParamDriver` command, so each is one undo step and
//! a cycle is refused by the engine before it reaches the project.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, DriverCode, MAX_DRIVER_SOURCE};

use super::AppState;

/// The language a new driver is written in, when a plugin provides it.
pub const DEFAULT_DRIVER_LANGUAGE: &str = "aria.expressions";

/// Driver source being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDraft {
    pub clip: ClipId,
    pub param: ClipParam,
    pub language: String,
    pub text: String,
    /// The next property clicked in the inspector inserts a reference to
    /// itself instead of changing its value.
    pub picking: bool,
}

impl AppState {
    /// The driver on `param` of the selected clip.
    pub fn selected_driver(&self, param: ClipParam) -> Option<&DriverCode> {
        self.selected_model()?.driver(param)
    }

    /// Alt+click on a stopwatch. Adds a driver to `param` of the selected
    /// clip, in [`DEFAULT_DRIVER_LANGUAGE`] when it is installed and else in
    /// the first installed language, starting from the language's template,
    /// and opens its source for editing. Removes the driver when the property
    /// has one.
    pub fn toggle_driver(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if self.selected_driver(param).is_some() {
            self.set_driver(clip, param, None)?;
            if self
                .driver_draft
                .as_ref()
                .is_some_and(|draft| draft.clip == clip && draft.param == param)
            {
                self.driver_draft = None;
            }
            return Ok(());
        }
        let languages = aria_render::driver_languages();
        let language = if languages.iter().any(|id| id == DEFAULT_DRIVER_LANGUAGE) {
            DEFAULT_DRIVER_LANGUAGE.to_owned()
        } else {
            languages.into_iter().next().ok_or_else(|| {
                EngineError::Refused(
                    "no driver language is installed; enable the Expressions plugin in \
                     Settings > Plugins"
                        .into(),
                )
            })?
        };
        let text = aria_render::driver_template(&language).unwrap_or_default();
        self.set_driver(
            clip,
            param,
            Some(DriverCode::new(language.clone(), text.clone())),
        )?;
        self.driver_draft = Some(DriverDraft {
            clip,
            param,
            language,
            text,
            picking: false,
        });
        Ok(())
    }

    /// Open the source of the selected clip's driver on `param` for editing.
    /// Inert when the property has no driver.
    pub fn open_driver_draft(&mut self, param: ClipParam) {
        let (Some(clip), Some(code)) = (self.selected_clip(), self.selected_driver(param)) else {
            return;
        };
        self.driver_draft = Some(DriverDraft {
            clip,
            param,
            language: code.language.clone(),
            text: code.source.clone(),
            picking: false,
        });
    }

    /// The open driver draft, if any.
    pub fn driver_draft(&self) -> Option<&DriverDraft> {
        self.driver_draft.as_ref()
    }

    /// Whether keys type into the draft: while its clip is selected, or while
    /// a pick is made on another clip. With another clip selected the draft is
    /// not drawn, so keys reach the editor's shortcuts.
    pub fn driver_draft_open(&self) -> bool {
        self.driver_draft
            .as_ref()
            .is_some_and(|draft| draft.picking || Some(draft.clip) == self.selected_clip())
    }

    pub fn driver_draft_text(&self) -> String {
        self.driver_draft
            .as_ref()
            .map(|draft| draft.text.clone())
            .unwrap_or_default()
    }

    /// Append `ch`, within [`MAX_DRIVER_SOURCE`] bytes.
    pub fn driver_draft_push(&mut self, ch: char) {
        if let Some(draft) = self.driver_draft.as_mut()
            && draft.text.len() + ch.len_utf8() <= MAX_DRIVER_SOURCE
        {
            draft.text.push(ch);
        }
    }

    pub fn driver_draft_pop(&mut self) {
        if let Some(draft) = self.driver_draft.as_mut() {
            draft.text.pop();
        }
    }

    /// Esc: end a pick, else close the draft without writing it.
    pub fn escape_driver_draft(&mut self) {
        match self.driver_draft.as_mut() {
            Some(draft) if draft.picking => draft.picking = false,
            _ => self.driver_draft = None,
        }
    }

    /// Enter: write the draft as the property's driver. A refused write, such
    /// as a cycle, keeps the draft open so it can be corrected.
    pub fn commit_driver_draft(&mut self) -> Result<(), EngineError> {
        let Some(draft) = self.driver_draft.take() else {
            return Ok(());
        };
        let code = DriverCode::new(draft.language.clone(), draft.text.clone());
        if let Err(err) = self.set_driver(draft.clip, draft.param, Some(code)) {
            self.driver_draft = Some(draft);
            return Err(err);
        }
        Ok(())
    }

    /// The pick whip: the next property row clicked inserts a reference.
    /// Pressing it again ends the pick.
    pub fn toggle_driver_pick(&mut self) {
        if let Some(draft) = self.driver_draft.as_mut() {
            draft.picking = !draft.picking;
        }
    }

    /// Whether a pick is waiting for a property row.
    pub fn driver_picking(&self) -> bool {
        self.driver_draft
            .as_ref()
            .is_some_and(|draft| draft.picking)
    }

    /// Insert a reference to `param` of the selected clip into the draft
    /// being picked for, and select the driven clip again when the pick was
    /// made on another one.
    pub fn pick_driver_input(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(source) = self.selected_clip() else {
            return Ok(());
        };
        let Some(draft) = self.driver_draft.as_ref().filter(|draft| draft.picking) else {
            return Ok(());
        };
        let input = {
            let engine = self.engine.lock();
            aria_render::driver_input(engine.project(), draft.clip, source, param)
        }
        .ok_or_else(|| EngineError::Refused("a driver cannot read this property".into()))?;
        let reference =
            aria_render::driver_reference(&draft.language, &input).ok_or_else(|| {
                EngineError::Refused(format!(
                    "{} has no syntax for reading this property; type the reference",
                    draft.language
                ))
            })?;
        let driven = draft.clip;
        if let Some(draft) = self.driver_draft.as_mut() {
            if draft.text.len() + reference.len() > MAX_DRIVER_SOURCE {
                return Err(EngineError::Refused(format!(
                    "driver source is limited to {MAX_DRIVER_SOURCE} bytes"
                )));
            }
            draft.text.push_str(&reference);
            draft.picking = false;
        }
        if source != driven {
            self.select_clip(driven);
        }
        Ok(())
    }

    fn set_driver(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        code: Option<DriverCode>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetParamDriver {
            clip,
            param,
            code,
        }))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "drivers_tests.rs"]
mod tests;
