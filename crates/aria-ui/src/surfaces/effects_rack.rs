//! The effects rack's gestures: what an operator does to a clip's effect chain.
//!
//! The rack's rows already come from the model: `clip_effects::scalar_params`,
//! `vec2_params` and `color_params` say what an effect exposes, so a plugin's
//! effect draws its own fields. The writes did not. Add, remove, reorder, apply a
//! preset, pick a colour and step a number were six gpui listeners, so five of
//! the model's commands were reachable only from inside a closure and nothing
//! could enumerate them.
//!
//! Each gesture names an effect by its index in the chain, which is what the row
//! the operator pressed knows. An index past the end of the chain is inert
//! rather than an error, because a chain can be shortened by an undo between the
//! frame that drew the row and the click that pressed it.

use aria_engine::EngineError;

use crate::app::AppState;

/// One thing an operator does to the effect chain.
pub enum Gesture {
    /// Append an effect from the browse list.
    Add(String),
    /// Take the effect at this index out of the chain.
    Remove(usize),
    /// Move the effect one step earlier in the render order.
    MoveUp(usize),
    /// Move it one step later.
    MoveDown(usize),
    /// Replace the whole chain with a saved preset.
    Preset(String),
    /// Save the chain the operator built under the name they typed.
    SavePreset,
    /// Forget a saved preset by name.
    DeletePreset(String),
    /// A swatch in one effect's colour slot.
    Color {
        index: usize,
        slot: usize,
        rgba: [u8; 4],
    },
    /// One step of a scalar slot's stepper.
    NudgeScalar { index: usize, slot: usize, by: f32 },
    /// One step of one axis of a vec2 slot. `axis` is 0 for x and 1 for y.
    NudgeVec2 {
        index: usize,
        slot: usize,
        axis: usize,
        by: f32,
    },
}

impl Gesture {
    /// What the operator did, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Add(_) => "an effect added from the browse list",
            Self::Remove(_) => "an effect removed",
            Self::MoveUp(_) => "an effect moved earlier",
            Self::MoveDown(_) => "an effect moved later",
            Self::Preset(_) => "a saved preset applied",
            Self::SavePreset => "the built chain saved as a preset",
            Self::DeletePreset(_) => "a saved preset deleted",
            Self::Color { .. } => "an effect's colour slot set",
            Self::NudgeScalar { .. } => "an effect's number stepped",
            Self::NudgeVec2 { .. } => "one axis of an effect's vector stepped",
        }
    }

    /// The effect this gesture names, when it names one by index.
    fn index(&self) -> Option<usize> {
        match self {
            Self::Add(_) | Self::Preset(_) | Self::SavePreset | Self::DeletePreset(_) => None,
            Self::Remove(index) | Self::MoveUp(index) | Self::MoveDown(index) => Some(*index),
            Self::Color { index, .. }
            | Self::NudgeScalar { index, .. }
            | Self::NudgeVec2 { index, .. } => Some(*index),
        }
    }
}

/// Apply one rack gesture to the selected clip.
pub fn apply(state: &mut AppState, gesture: Gesture) -> Result<(), EngineError> {
    // A row the operator can no longer see is not a bug report. The chain can be
    // shortened by an undo between the frame that drew the row and the press, so
    // the surface drops the press. The model's setters keep refusing an
    // out-of-range index loudly, which is what the assistant path needs.
    let chain = state
        .selected_ui_clip_ref()
        .map_or(0, |clip| clip.effects.len());
    if gesture.index().is_some_and(|index| index >= chain) {
        return Ok(());
    }
    match gesture {
        Gesture::Add(id) => state.add_effect_to_selected(id),
        Gesture::Remove(index) => state.remove_effect_at(index),
        Gesture::MoveUp(index) => state.move_selected_effect_up(index),
        Gesture::MoveDown(index) => state.move_selected_effect_down(index),
        Gesture::Preset(name) => state.apply_effect_preset(&name),
        Gesture::SavePreset => state
            .commit_effect_preset_draft()
            .map_err(EngineError::Refused),
        Gesture::DeletePreset(name) => state
            .delete_effect_preset(&name)
            .map_err(EngineError::Refused),
        Gesture::Color { index, slot, rgba } => state.set_selected_effect_color(index, slot, rgba),
        Gesture::NudgeScalar { index, slot, by } => {
            state.nudge_selected_effect_param(index, slot, by)
        }
        Gesture::NudgeVec2 {
            index,
            slot,
            axis,
            by,
        } => state.nudge_selected_effect_vec2(index, slot, axis, by),
    }
}
