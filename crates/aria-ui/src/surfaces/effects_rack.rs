//! The effects rack's gestures: what an operator does to a clip's effect chain.
//!
//! The rack's rows come from the model: the inspector walks each effect's
//! `EffectSpec::params` in display order, so a plugin's effect draws its own
//! fields. The writes did not. Add, remove, reorder, apply a
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
use crate::app::clip_effects::effect_param;

/// One update to an effect's parameter slot.
#[derive(Clone, Debug)]
pub enum SlotEdit {
    /// Reset the slot to its catalog default.
    Reset,
    /// Step an integer slot by `by`.
    NudgeInt(i32),
    /// Flip a boolean slot.
    ToggleBool,
    /// Pick a choice option by id.
    Choice(String),
    /// Step one axis (`0` = X, `1` = Y, `2` = Z) of a 3D point slot.
    NudgePoint3 { axis: usize, by: f32 },
    /// Replace a 1D curve slot's control points.
    Curve(Vec<aria_models::EffectCurvePoint>),
    /// Replace a gradient slot's color stops.
    Gradient(Vec<aria_models::EffectGradientStop>),
    /// Set a text, font, or file slot.
    Text(String),
    /// Set a layer reference slot (`None` clears it).
    Layer(Option<aria_models::ClipId>),
}

/// One thing an operator does to the effect chain.
pub enum Gesture {
    /// Append an effect from the browse list.
    Add(String),
    /// Take the effect at this index out of the chain.
    Remove(usize),
    /// Switch the effect at this index off, or back on.
    Bypass(usize),
    /// Move the effect one step earlier in the render order.
    MoveUp(usize),
    /// Move it one step later.
    MoveDown(usize),
    /// Drag the effect at `from` to just above the effect at `before`, or to
    /// the end of the chain when `before` is `None`.
    Drop { from: usize, before: Option<usize> },
    /// Put every parameter of the effect back at its default.
    Reset(usize),
    /// Apply a saved or plugin preset to the selected clips.
    Preset(String),
    /// Save the selected parts of the clip under the name typed in the draft.
    SavePreset,
    /// Toggle whether an effect is included in the open preset draft.
    TogglePresetEffect(usize),
    /// Toggle whether audio inserts are included in the open preset draft.
    TogglePresetAudioEffects,
    /// Toggle whether a keyframed property is included in the open preset draft.
    TogglePresetProperty(String),
    /// Forget a saved user preset by name.
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
    /// Update one effect parameter slot (toggle, choice, int, point3, curve,
    /// gradient, text, font, file, layer, or reset to default).
    EditSlot {
        index: usize,
        slot: usize,
        edit: SlotEdit,
    },
    /// Collapse or expand a parameter group on an effect.
    ToggleGroup { index: usize, group: String },
}

impl Gesture {
    /// What the operator did, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Add(_) => "an effect added from the browse list",
            Self::Remove(_) => "an effect removed",
            Self::Bypass(_) => "an effect switched off or back on",
            Self::MoveUp(_) => "an effect moved earlier",
            Self::MoveDown(_) => "an effect moved later",
            Self::Drop { .. } => "an effect dragged to another place in the chain",
            Self::Reset(_) => "an effect put back at its defaults",
            Self::Preset(_) => "a saved preset applied",
            Self::SavePreset => "the built chain saved as a preset",
            Self::TogglePresetEffect(_) => "an effect toggled in the preset draft",
            Self::TogglePresetAudioEffects => "audio inserts toggled in the preset draft",
            Self::TogglePresetProperty(_) => "a property toggled in the preset draft",
            Self::DeletePreset(_) => "a saved preset deleted",
            Self::Color { .. } => "an effect's colour slot set",
            Self::NudgeScalar { .. } => "an effect's number stepped",
            Self::NudgeVec2 { .. } => "one axis of an effect's vector stepped",
            Self::EditSlot { .. } => "an effect's parameter slot updated",
            Self::ToggleGroup { .. } => "an effect's parameter group toggled",
        }
    }

    /// The effect this gesture names, when it names one by index.
    fn index(&self) -> Option<usize> {
        match self {
            Self::Add(_)
            | Self::Preset(_)
            | Self::SavePreset
            | Self::TogglePresetEffect(_)
            | Self::TogglePresetAudioEffects
            | Self::TogglePresetProperty(_)
            | Self::DeletePreset(_) => None,
            Self::Remove(index)
            | Self::Bypass(index)
            | Self::MoveUp(index)
            | Self::MoveDown(index)
            | Self::Reset(index)
            | Self::Drop { from: index, .. } => Some(*index),
            Self::Color { index, .. }
            | Self::NudgeScalar { index, .. }
            | Self::NudgeVec2 { index, .. }
            | Self::EditSlot { index, .. }
            | Self::ToggleGroup { index, .. } => Some(*index),
        }
    }
}

/// Apply one rack gesture to the selected clip.
///
/// With several clips selected, a gesture on an effect reaches every selected
/// clip holding the same effect at the same place in its chain, and an effect
/// added reaches every clip on the same kind of lane, as one undo step. A step
/// or a scrub moves the drawn clip and gives the others the value it lands on,
/// and a switch flipped on the drawn clip is set the same way on the others.
/// Presets and the preset draft are the selection's already.
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
        Gesture::Add(id) => {
            let peers = state.peers_sharing_effect(None);
            state.edit_each(peers, |state| state.add_effect_to_selected(id.as_str()))
        }
        Gesture::Remove(index) => {
            let peers = state.peers_sharing_effect(Some(index));
            state.edit_each(peers, |state| state.remove_effect_at(index))
        }
        Gesture::Bypass(index) => {
            let peers = state.peers_sharing_effect(Some(index));
            let Some(bypass) = state.selected_effect_bypass(index) else {
                return Ok(());
            };
            state.edit_each(peers, |state| {
                state.set_selected_effect_bypass(index, !bypass)
            })
        }
        Gesture::MoveUp(index) => {
            let peers = state.peers_sharing_effect(Some(index));
            state.edit_each(peers, |state| state.move_selected_effect_up(index))
        }
        Gesture::MoveDown(index) => {
            let peers = state.peers_sharing_effect(Some(index));
            state.edit_each(peers, |state| state.move_selected_effect_down(index))
        }
        Gesture::Drop { from, before } => {
            let peers = state.peers_sharing_effect(Some(from));
            state.edit_each(peers, |state| state.drop_selected_effect(from, before))
        }
        Gesture::Reset(index) => {
            let peers = state.peers_sharing_effect(Some(index));
            state.edit_each(peers, |state| state.reset_selected_effect(index))
        }
        Gesture::Preset(name) => state.apply_preset(&name),
        Gesture::SavePreset => state.commit_preset_draft().map_err(EngineError::Refused),
        Gesture::TogglePresetEffect(index) => {
            state.toggle_preset_draft_effect(index);
            Ok(())
        }
        Gesture::TogglePresetAudioEffects => {
            state.toggle_preset_draft_audio_effects();
            Ok(())
        }
        Gesture::TogglePresetProperty(id) => {
            state.toggle_preset_draft_property(&id);
            Ok(())
        }
        Gesture::DeletePreset(name) => state.delete_preset(&name).map_err(EngineError::Refused),
        Gesture::Color { index, slot, rgba } => {
            let peers = state.peers_sharing_effect(Some(index));
            state.edit_each(peers, |state| {
                state.set_selected_effect_color(index, slot, rgba)
            })
        }
        Gesture::NudgeScalar { index, slot, by } => {
            let peers = state.peers_sharing_effect(Some(index));
            state.land_each(peers, &[(effect_param(index, slot), None)], |state| {
                state.nudge_selected_effect_param(index, slot, by)
            })
        }
        Gesture::NudgeVec2 {
            index,
            slot,
            axis,
            by,
        } => {
            let peers = state.peers_sharing_effect(Some(index));
            let landed = [(effect_param(index, slot), Some(axis as u8))];
            state.land_each(peers, &landed, |state| {
                state.nudge_selected_effect_vec2(index, slot, axis, by)
            })
        }
        Gesture::EditSlot { index, slot, edit } => {
            let peers = state.peers_sharing_effect(Some(index));
            edit_slot(state, peers, index, slot, edit)
        }
        Gesture::ToggleGroup { index, group } => {
            state.toggle_effect_group(index, &group);
            Ok(())
        }
    }
}

/// One slot edit on the effect at `index`, reaching `peers` as well.
fn edit_slot(
    state: &mut AppState,
    peers: Vec<aria_models::ClipId>,
    index: usize,
    slot: usize,
    edit: SlotEdit,
) -> Result<(), EngineError> {
    let param = effect_param(index, slot);
    match edit {
        SlotEdit::NudgeInt(_) => state.land_each(peers, &[(param, None)], |state| {
            state.edit_selected_effect_slot(index, slot, edit)
        }),
        SlotEdit::NudgePoint3 { axis, .. } => {
            state.land_each(peers, &[(param, Some(axis as u8))], |state| {
                state.edit_selected_effect_slot(index, slot, edit)
            })
        }
        SlotEdit::ToggleBool => {
            let Some(on) = state.selected_effect_bool(index, slot) else {
                return Ok(());
            };
            state.edit_each(peers, |state| {
                state.set_selected_effect_bool(index, slot, !on)
            })
        }
        SlotEdit::Reset
        | SlotEdit::Choice(_)
        | SlotEdit::Curve(_)
        | SlotEdit::Gradient(_)
        | SlotEdit::Text(_)
        | SlotEdit::Layer(_) => state.edit_each(peers, |state| {
            state.edit_selected_effect_slot(index, slot, edit.clone())
        }),
    }
}
