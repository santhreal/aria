use aria_models::{AudioEffectInstance, AudioInsertTarget, Param, TrackId};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// One gesture on one insert chain.
///
/// Every gesture snapshots the whole chain first, so the inverse is one write
/// through [`aria_models::Project::set_audio_inserts`]: a removed insert comes
/// back at its index with its values, which a per-field inverse could not
/// promise once a later insert had moved.
#[derive(Debug, Clone)]
pub(crate) enum InsertEdit {
    Add {
        effect_id: String,
    },
    Remove {
        index: usize,
    },
    Move {
        from_index: usize,
        to_index: usize,
    },
    Param {
        index: usize,
        param: usize,
        value: Param<f32>,
    },
    Bypass {
        index: usize,
        bypass: bool,
    },
    Sidechain {
        index: usize,
        key: Option<TrackId>,
    },
}

/// The undoable unit for an insert chain: the whole chain as it was.
pub(crate) struct SetAudioInsertsAction {
    target: AudioInsertTarget,
    inserts: Vec<AudioEffectInstance>,
}

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    target: AudioInsertTarget,
    edit: InsertEdit,
) -> Result<Box<dyn EditAction>, EngineError> {
    let project = &mut *ctx.project;
    let inverse = Box::new(SetAudioInsertsAction {
        target,
        inserts: project.audio_inserts(target)?.to_vec(),
    });
    match edit {
        InsertEdit::Add { effect_id } => {
            project.add_audio_effect(target, &effect_id)?;
        }
        InsertEdit::Remove { index } => {
            project.remove_audio_effect(target, index)?;
        }
        InsertEdit::Move {
            from_index,
            to_index,
        } => project.move_audio_effect(target, from_index, to_index)?,
        InsertEdit::Param {
            index,
            param,
            value,
        } => project.set_audio_effect_param(target, index, param, value)?,
        InsertEdit::Bypass { index, bypass } => {
            project.set_audio_effect_bypass(target, index, bypass)?;
        }
        InsertEdit::Sidechain { index, key } => {
            project.set_audio_effect_sidechain(target, index, key)?;
        }
    }
    Ok(inverse)
}

impl EditAction for SetAudioInsertsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let inverse = Box::new(SetAudioInsertsAction {
            target: self.target,
            inserts: ctx.project.audio_inserts(self.target)?.to_vec(),
        });
        ctx.project.set_audio_inserts(self.target, self.inserts)?;
        Ok(inverse)
    }
}
