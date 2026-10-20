//! What a channel strip does, as gestures rather than closures.
//!
//! The strip is drawn from data already: `MixerStrip` carries the buses a lane
//! can be sent to and the outputs it can point at, so the panel keeps no list of
//! either. What it did keep was the meaning of each control, sealed in a gpui
//! listener: the chip that points a lane at a bus, the chip that cycles a send
//! through post, pre and off, and the three pointer phases of riding a fader.
//! Nothing could enumerate those, so five of the model's routing commands sat
//! outside every measurement.
//!
//! A gesture here is a payload plus a place, and one function says what it means.
//! The fader is three gestures rather than one, because that is what the pointer
//! is: a press grabs, a move rides, a release writes. A ride writes nothing, so
//! taking a fader from silence to unity is one undo entry however many pixels the
//! pointer crossed.

use aria_engine::EngineError;
use aria_models::{AutomationMode, TrackId};

use crate::app::AppState;
use crate::app::mixer::FaderTarget;

/// One gesture on the mixer.
pub enum Gesture {
    /// Point a lane at a bus, or back at the master.
    PointOutput {
        track: TrackId,
        /// `None` is the master.
        to: Option<TrackId>,
    },
    /// Click a send chip: off becomes post-fader, post becomes pre-fader, pre
    /// becomes off. The cycle is the model's, not the panel's.
    CycleSend { track: TrackId, to: TrackId },
    /// Press a fader. `at` is the pointer's position on the strip and `travel`
    /// the height of the whole fader, so the value moves by how far the pointer
    /// comes rather than jumping to where it landed.
    GrabFader {
        target: FaderTarget,
        at: f32,
        travel: f32,
    },
    /// Move the pointer while a fader is held.
    RideFader { at: f32 },
    /// Let a fader go, which is where the one command is written.
    ReleaseFader,
    /// Add a submix bus to the project.
    AddBus,
    /// Toggle mute on a track strip (`Some(track)`) or the master strip (`None`).
    ToggleMute { track: Option<TrackId> },
    /// Toggle solo-in-place on a track strip.
    ToggleSolo { track: TrackId },
    /// Set the automation mode on a track strip (`Some(track)`) or the master strip (`None`).
    SetAutomation {
        track: Option<TrackId>,
        mode: AutomationMode,
    },
}

impl Gesture {
    /// The gesture, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::PointOutput { .. } => "a lane pointed at a bus",
            Self::CycleSend { .. } => "a send chip clicked",
            Self::GrabFader { .. } => "a fader grabbed",
            Self::RideFader { .. } => "a fader ridden",
            Self::ReleaseFader => "a fader let go",
            Self::AddBus => "a submix bus added",
            Self::ToggleMute { .. } => "a strip mute toggled",
            Self::ToggleSolo { .. } => "a strip solo toggled",
            Self::SetAutomation { .. } => "an automation mode chosen",
        }
    }
}

/// Apply one gesture.
///
/// The bool is whether the picture changed, which the panel needs and the engine
/// does not: a ride that lands on the same gain must not cost a repaint at
/// pointer rate. A release with nothing held, and a ride with nothing held, are
/// both inert rather than errors, because a pointer that entered the strip
/// mid-drag is a state the window can be in.
pub fn apply(state: &mut AppState, gesture: Gesture) -> Result<bool, EngineError> {
    match gesture {
        Gesture::PointOutput { track, to } => {
            state.set_track_output(track, to)?;
            Ok(true)
        }
        Gesture::CycleSend { track, to } => {
            state.cycle_send(track, to)?;
            Ok(true)
        }
        Gesture::GrabFader { target, at, travel } => {
            state.begin_fader_drag(target, at, travel);
            Ok(true)
        }
        Gesture::RideFader { at } => Ok(state.drag_fader_to_pointer(at)),
        Gesture::ReleaseFader => {
            if state.fader_dragging().is_none() {
                return Ok(false);
            }
            state.commit_fader_drag()?;
            Ok(true)
        }
        Gesture::AddBus => {
            state.add_submix_lane()?;
            Ok(true)
        }
        Gesture::ToggleMute { track } => {
            state.toggle_strip_mute(track)?;
            Ok(true)
        }
        Gesture::ToggleSolo { track } => {
            state.toggle_strip_solo(track)?;
            Ok(true)
        }
        Gesture::SetAutomation { track, mode } => {
            state.set_strip_automation_mode(track, mode)?;
            Ok(true)
        }
    }
}
