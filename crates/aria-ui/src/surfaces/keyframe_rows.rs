//! What the keyframe list does to one property's curve.
//!
//! The list's rows come from the model: `key_params` answers with the properties
//! the selected clip can animate, so a plugin's effect is keyframable by the same
//! route as a shipped one. The buttons on each row were six gpui listeners, and
//! one of them carried a decision the panel should never have owned: the key
//! button adds a keyframe or removes the one already under the playhead, and that
//! choice is product behaviour rather than a detail of how a row is drawn.
//!
//! Two of the six move the playhead instead of editing the project.
//! [`Row::edits`] says which, so a census can hold an editing button to sending a
//! command and a navigating one to moving the playhead, without either being an
//! exception someone remembered to write down.

use aria_engine::EngineError;
use aria_models::{ClipParam, PiecewiseEasingPreset};

use crate::app::AppState;

/// One press on one property's keyframe row.
pub enum Row {
    /// Take a keyframe at the playhead, or remove the one the row was drawn
    /// with. `keyed` is what the operator saw, not what is there now: a press
    /// answers the dot that was under the pointer.
    Key { param: ClipParam, keyed: bool },
    /// Shape the segment leaving the keyframe under the playhead.
    Easing {
        param: ClipParam,
        preset: &'static str,
    },
    /// Approximate a bounce, an elastic or an overshoot with several keyframes.
    Piecewise {
        param: ClipParam,
        preset: PiecewiseEasingPreset,
    },
    /// Drop the curve and keep the value under the playhead.
    Flatten(ClipParam),
    /// Move the playhead to the previous keyframe on this property.
    Previous(ClipParam),
    /// Move it to the next one.
    Next(ClipParam),
}

impl Row {
    /// What the operator pressed, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Key { .. } => "the key button",
            Self::Easing { .. } => "an easing preset",
            Self::Piecewise { .. } => "a piecewise easing preset",
            Self::Flatten(_) => "the flatten button",
            Self::Previous(_) => "the previous-keyframe arrow",
            Self::Next(_) => "the next-keyframe arrow",
        }
    }

    /// Whether this press changes the project. The two arrows move the playhead
    /// and leave the curve alone, so they are not undo entries.
    pub fn edits(&self) -> bool {
        match self {
            Self::Key { .. } | Self::Easing { .. } | Self::Piecewise { .. } | Self::Flatten(_) => {
                true
            }
            Self::Previous(_) | Self::Next(_) => false,
        }
    }
}

/// Apply one keyframe-row press.
pub fn apply(state: &mut AppState, row: Row) -> Result<(), EngineError> {
    match row {
        // A toggle acts on the state the operator saw, so the row hands over
        // what it drew rather than the registry re-reading it.
        Row::Key { param, keyed } => {
            if keyed {
                state.remove_keyframe_at_playhead(param)
            } else {
                state.add_keyframe_at_playhead(param)
            }
        }
        Row::Easing { param, preset } => state.set_keyframe_easing_preset(param, preset),
        Row::Piecewise { param, preset } => state.apply_piecewise_preset(param, preset),
        Row::Flatten(param) => state.flatten_param(param),
        Row::Previous(param) => {
            state.go_to_prev_keyframe(param);
            Ok(())
        }
        Row::Next(param) => {
            state.go_to_next_keyframe(param);
            Ok(())
        }
    }
}
