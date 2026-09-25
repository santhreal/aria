//! Look and grade commands for the selected clip: blend mode, denoise, pitch
//! lock, and stabilization.
//!
//! Each of these existed in `EditCommand` with no UI call site. They share this
//! module because they all read the selected clip, mutate one field of its look,
//! and dispatch one command.
//!
//! The eleven colour adjustments are not here. `SetClipAdjustments` carries all
//! of them, so a per-slider setter had to rebuild the block from the projection
//! and flattened every grade keyframe on the way; a slider edit now addresses
//! its own property through `clip_params.rs`.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{BlendMode, ColorAdjustments, StabilizeLevel};

use super::AppState;

impl AppState {
    pub fn set_selected_blend_mode(&mut self, mode: BlendMode) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipBlendMode { clip, mode }))?;
        Ok(())
    }

    pub fn toggle_selected_denoise(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipDenoise {
            clip,
            denoise: !ui.denoise,
        }))?;
        Ok(())
    }

    /// Toggle pitch preservation. Unlocked pitch rides the clip's speed.
    pub fn toggle_selected_pitch_lock(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipPitch {
            clip,
            preserve_pitch: !ui.preserve_pitch,
        }))?;
        Ok(())
    }

    /// Whether the selection can be stabilized (visual video media only).
    pub fn selected_can_stabilize(&self) -> bool {
        self.selected_ui_clip()
            .is_some_and(|clip| clip.can_stabilize)
    }

    /// Set (or with `None` clear) stabilization.
    pub fn set_selected_stabilize(
        &mut self,
        stabilize: Option<StabilizeLevel>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.selected_can_stabilize() {
            self.status_message = Some("This clip has no motion to stabilize".into());
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetClipStabilize {
            clip,
            stabilize,
        }))?;
        Ok(())
    }

    /// Return every slider to neutral, which is also how the grade is cleared.
    pub fn clear_selected_adjustments(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipAdjustments {
            clip,
            adjust: ColorAdjustments::default(),
        }))?;
        self.status_message = Some("Cleared grade".into());
        Ok(())
    }
}
