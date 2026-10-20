//! What the keyframe list does to one property's curve.
//!
//! The list's rows come from the model: `key_params` answers with the properties
//! the selected clip can animate, so a plugin's effect is keyframable by the same
//! route as a shipped one. The buttons on each row were six gpui listeners, and
//! one of them carried a decision the panel should never have owned: the key
//! button adds a keyframe or removes the one already under the playhead, and that
//! choice is product behaviour rather than a detail of how a row is drawn.
//!
//! Two of them move the playhead instead of editing the project.
//! [`Row::edits`] says which, so a census can hold an editing button to sending a
//! command and a navigating one to moving the playhead, without either being an
//! exception someone remembered to write down.

use aria_engine::EngineError;
use aria_models::{ClipParam, PiecewiseEasingPreset, TemporalEase};

use crate::app::AppState;

/// One press on one property's keyframe row.
#[derive(Clone, Copy)]
pub enum Row {
    /// Take a keyframe at the playhead, or remove the one the row was drawn
    /// with. `keyed` is what the row showed, not what is there now: a press
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
    /// Add a driver to this property, or remove the one it has. The same
    /// toggle as Alt+click on the property's stopwatch.
    Driver(ClipParam),
    /// Set incoming/outgoing temporal ease and/or hold state on the keyframe
    /// under the playhead.
    TemporalEase {
        param: ClipParam,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    },
    /// Toggle roving across time on the interior keyframe under the playhead.
    Roving { param: ClipParam, roving: bool },
    /// Return the property to its default: a keyframe at the playhead when it
    /// is animated, a constant otherwise.
    Reset(ClipParam),
}

impl Row {
    /// The pressed control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Key { .. } => "the key button",
            Self::Easing { .. } => "an easing preset",
            Self::Piecewise { .. } => "a piecewise easing preset",
            Self::Flatten(_) => "the flatten button",
            Self::Previous(_) => "the previous-keyframe arrow",
            Self::Next(_) => "the next-keyframe arrow",
            Self::Driver(_) => "the driver button",
            Self::TemporalEase { .. } => "a temporal ease handle",
            Self::Roving { .. } => "the roving keyframe toggle",
            Self::Reset(_) => "the reset button",
        }
    }

    /// Whether this press changes the project. The two arrows move the playhead
    /// and leave the curve alone, so they are not undo entries.
    pub fn edits(&self) -> bool {
        match self {
            Self::Key { .. }
            | Self::Easing { .. }
            | Self::Piecewise { .. }
            | Self::Flatten(_)
            | Self::Driver(_)
            | Self::TemporalEase { .. }
            | Self::Roving { .. }
            | Self::Reset(_) => true,
            Self::Previous(_) | Self::Next(_) => false,
        }
    }

    /// The property the row was pressed on.
    pub fn param(&self) -> ClipParam {
        match *self {
            Self::Key { param, .. }
            | Self::Easing { param, .. }
            | Self::Piecewise { param, .. }
            | Self::TemporalEase { param, .. }
            | Self::Roving { param, .. }
            | Self::Flatten(param)
            | Self::Previous(param)
            | Self::Next(param)
            | Self::Driver(param)
            | Self::Reset(param) => param,
        }
    }
}

/// Apply one keyframe-row press.
///
/// A press that edits reaches every selected clip that has the property, as
/// one undo step. Each clip takes the press as the row drew it: the key button
/// removes a keyframe at the playhead on every clip when the drawn clip was
/// keyed there and adds one on every clip otherwise, and the driver button
/// adds a driver only where the drawn clip had none and removes one only where
/// it had one. A clip the press does not fit is left alone, as the drawn clip
/// would be. The arrows move the playhead, which every clip shares.
pub fn apply(state: &mut AppState, row: Row) -> Result<(), EngineError> {
    let peers = match row {
        Row::Driver(param) => state.peers_driven_alike(param),
        Row::Key { param, .. }
        | Row::Easing { param, .. }
        | Row::Piecewise { param, .. }
        | Row::TemporalEase { param, .. }
        | Row::Roving { param, .. }
        | Row::Flatten(param)
        | Row::Reset(param) => state.peers_sharing(param),
        Row::Previous(_) | Row::Next(_) => Vec::new(),
    };
    state.edit_each(peers, |state| press(state, row))
}

/// One press on the selected clip.
fn press(state: &mut AppState, row: Row) -> Result<(), EngineError> {
    match row {
        // A toggle acts on the drawn state, so the row hands over
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
        Row::Driver(param) => state.toggle_driver(param),
        Row::TemporalEase {
            param,
            in_ease,
            out_ease,
            hold,
        } => state.set_keyframe_temporal_ease_at_playhead(param, in_ease, out_ease, hold),
        Row::Roving { param, roving } => state.set_keyframe_roving_at_playhead(param, roving),
        Row::Reset(param) => state.reset_param(param),
    }
}
