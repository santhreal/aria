//! What an operator does to an audio insert chain: the selected clip's in the
//! inspector's Audio block, a lane's or the master's in the mixer.
//!
//! Both panels draw the same chain from the projection, so both press the same
//! gestures, and every gesture names the chain it acts on. An index past the end
//! of the chain is inert rather than an error: an undo can shorten the chain
//! between the frame that drew the row and the click that pressed it.

use aria_engine::EngineError;
use aria_models::{AudioInsertTarget, TrackId};

use crate::app::AppState;

/// One thing an operator does to an insert chain.
pub enum Gesture {
    /// Show or hide the chain's Add list.
    TogglePicker(AudioInsertTarget),
    /// Append an effect from the Add list.
    Add {
        target: AudioInsertTarget,
        effect_id: String,
    },
    /// Take the insert at `index` out of the chain.
    Remove {
        target: AudioInsertTarget,
        index: usize,
    },
    /// Move the insert one step earlier in the signal path.
    MoveUp {
        target: AudioInsertTarget,
        index: usize,
    },
    /// Move it one step later.
    MoveDown {
        target: AudioInsertTarget,
        index: usize,
    },
    /// Bypass the insert, or bring it back.
    Bypass {
        target: AudioInsertTarget,
        index: usize,
    },
    /// Show or hide the insert's parameters.
    Open {
        target: AudioInsertTarget,
        index: usize,
    },
    /// One step of one parameter's stepper.
    Nudge {
        target: AudioInsertTarget,
        index: usize,
        slot: usize,
        by: f32,
    },
    /// Key the insert from a lane, or, with `None`, from nothing.
    Key {
        target: AudioInsertTarget,
        index: usize,
        key: Option<TrackId>,
    },
}

impl Gesture {
    /// What the operator did, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::TogglePicker(_) => "an insert chain's Add list shown or hidden",
            Self::Add { .. } => "an audio effect added from the Add list",
            Self::Remove { .. } => "an audio effect removed",
            Self::MoveUp { .. } => "an audio effect moved earlier",
            Self::MoveDown { .. } => "an audio effect moved later",
            Self::Bypass { .. } => "an audio effect bypassed or brought back",
            Self::Open { .. } => "an audio effect's parameters shown or hidden",
            Self::Nudge { .. } => "an audio effect's parameter stepped",
            Self::Key { .. } => "an audio effect's key lane picked",
        }
    }

    /// The chain this gesture acts on, and the insert in it, when it names one.
    fn insert(&self) -> Option<(AudioInsertTarget, usize)> {
        match self {
            Self::TogglePicker(_) | Self::Add { .. } => None,
            Self::Remove { target, index }
            | Self::MoveUp { target, index }
            | Self::MoveDown { target, index }
            | Self::Bypass { target, index }
            | Self::Open { target, index }
            | Self::Nudge { target, index, .. }
            | Self::Key { target, index, .. } => Some((*target, *index)),
        }
    }
}

/// Apply one gesture.
pub fn apply(state: &mut AppState, gesture: Gesture) -> Result<(), EngineError> {
    // The model's setters refuse an out-of-range index loudly, which is what the
    // assistant path needs. A row the operator can no longer see is dropped here.
    let gone = gesture
        .insert()
        .is_some_and(|(target, index)| index >= state.audio_insert_count(target));
    if gone {
        return Ok(());
    }
    match gesture {
        Gesture::TogglePicker(target) => {
            state.toggle_insert_picker(target);
            Ok(())
        }
        Gesture::Add { target, effect_id } => state.add_audio_insert(target, effect_id),
        Gesture::Remove { target, index } => state.remove_audio_insert(target, index),
        Gesture::MoveUp { target, index } => match index.checked_sub(1) {
            Some(to) => state.move_audio_insert(target, index, to),
            None => Ok(()),
        },
        Gesture::MoveDown { target, index } => state.move_audio_insert(target, index, index + 1),
        Gesture::Bypass { target, index } => state.toggle_audio_insert_bypass(target, index),
        Gesture::Open { target, index } => {
            state.toggle_open_insert(target, index);
            Ok(())
        }
        Gesture::Nudge {
            target,
            index,
            slot,
            by,
        } => state.nudge_audio_insert_param(target, index, slot, by),
        Gesture::Key { target, index, key } => state.set_audio_insert_key(target, index, key),
    }
}
