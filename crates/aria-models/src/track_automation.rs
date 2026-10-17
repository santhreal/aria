//! Mixer strip pan, automation modes, and volume/pan envelopes for lanes and
//! the master strip.

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::TrackId;
use crate::param::Param;
use crate::timeline::Timeline;
use crate::track::Track;
use crate::track_routing::AudioSend;

/// Bounds for lane and master pan: hard left (`-1.0`) through hard right (`1.0`).
pub const MIN_TRACK_PAN: f32 = -1.0;
pub const MAX_TRACK_PAN: f32 = 1.0;

/// How a mixer strip handles its volume and pan automation envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    /// Envelopes are bypassed; the strip plays at its static fader and pan.
    #[default]
    Off,
    /// Envelopes are evaluated at the sequence playhead; moves do not write.
    Read,
    /// Fader and pan moves during playback write keyframes across the pass.
    Write,
    /// Moves during playback write while held and return to the curve on release.
    Touch,
    /// Moves during playback write while held and latch the last value on release.
    Latch,
}

impl AutomationMode {
    /// Every automation mode in strip selector order.
    pub const ALL: [Self; 5] = [Self::Off, Self::Read, Self::Write, Self::Touch, Self::Latch];

    /// Whether this is [`Self::Off`]. Takes a reference for serde's
    /// `skip_serializing_if`.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub const fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }

    /// Whether playback and export evaluate the strip's keyframed envelopes.
    pub const fn reads_envelope(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether fader and pan moves during playback write keyframes onto the
    /// strip's envelopes.
    pub const fn writes_envelope(self) -> bool {
        matches!(self, Self::Write | Self::Touch | Self::Latch)
    }

    /// Full operator-facing mode name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::Touch => "Touch",
            Self::Latch => "Latch",
        }
    }

    /// Compact chip label on a narrow mixer strip.
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::Touch => "Touch",
            Self::Latch => "Latch",
        }
    }
}

/// Refuse a pan position outside `[-1.0, 1.0]`, naming what it was.
pub fn check_track_pan(what: &str, pan: f32) -> Result<f32, ModelError> {
    if !pan.is_finite() || !(MIN_TRACK_PAN..=MAX_TRACK_PAN).contains(&pan) {
        return Err(ModelError::InvalidParam(format!(
            "{what} {pan} is outside {MIN_TRACK_PAN}..={MAX_TRACK_PAN}"
        )));
    }
    Ok(pan)
}

/// Validate a strip volume automation envelope (`0.0..=AudioSend::MAX_LEVEL`,
/// strictly sorted keyframes, no spatial tangents).
pub fn check_track_volume_envelope(what: &str, envelope: &Param<f32>) -> Result<(), ModelError> {
    envelope.validate_shape()?;
    if envelope.keyframes().iter().any(|kf| kf.tangents.is_some()) {
        return Err(ModelError::InvalidParam(format!(
            "{what} is a scalar curve and cannot carry spatial tangents"
        )));
    }
    envelope.for_each_value(|&v| AudioSend::check_level(what, v).map(|_| ()))
}

/// Validate a strip pan automation envelope (`-1.0..=1.0`, strictly sorted
/// keyframes, no spatial tangents).
pub fn check_track_pan_envelope(what: &str, envelope: &Param<f32>) -> Result<(), ModelError> {
    envelope.validate_shape()?;
    if envelope.keyframes().iter().any(|kf| kf.tangents.is_some()) {
        return Err(ModelError::InvalidParam(format!(
            "{what} is a scalar curve and cannot carry spatial tangents"
        )));
    }
    envelope.for_each_value(|&v| check_track_pan(what, v).map(|_| ()))
}

impl Track {
    /// Effective linear gain at sequence `tick`: samples [`Self::volume_envelope`]
    /// when [`Self::automation`] reads envelopes and the curve is keyframed;
    /// otherwise returns [`Self::gain`].
    pub fn gain_at(&self, tick: i64) -> f32 {
        if self.automation.reads_envelope() && self.volume_envelope.is_animated() {
            self.volume_envelope.sample(tick)
        } else {
            self.gain
        }
    }

    /// Effective stereo pan (`-1.0..=1.0`) at sequence `tick`: samples
    /// [`Self::pan_envelope`] when [`Self::automation`] reads envelopes and the
    /// curve is keyframed; otherwise returns [`Self::pan`].
    pub fn pan_at(&self, tick: i64) -> f32 {
        if self.automation.reads_envelope() && self.pan_envelope.is_animated() {
            self.pan_envelope.sample(tick)
        } else {
            self.pan
        }
    }
}

impl Timeline {
    /// Set a lane's stereo pan (`-1.0` hard left .. `1.0` hard right).
    pub fn set_track_pan(&mut self, track: TrackId, pan: f32) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        let pan = check_track_pan("lane pan", pan)?;
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        lane.pan = pan;
        Ok(())
    }

    /// Set a lane's automation mode and its volume and pan envelopes.
    pub fn set_track_automation(
        &mut self,
        track: TrackId,
        mode: AutomationMode,
        volume: Param<f32>,
        pan: Param<f32>,
    ) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        check_track_volume_envelope("lane volume envelope", &volume)?;
        check_track_pan_envelope("lane pan envelope", &pan)?;
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        lane.automation = mode;
        lane.volume_envelope = volume;
        lane.pan_envelope = pan;
        Ok(())
    }

    /// Set the master strip's fader, pan, mute, automation mode, and envelopes.
    pub fn set_master_mix(
        &mut self,
        gain: f32,
        pan: f32,
        muted: bool,
        automation: AutomationMode,
        volume: Param<f32>,
        pan_envelope: Param<f32>,
    ) -> Result<(), ModelError> {
        let gain = AudioSend::check_level("master gain", gain)?;
        let pan = check_track_pan("master pan", pan)?;
        check_track_volume_envelope("master volume envelope", &volume)?;
        check_track_pan_envelope("master pan envelope", &pan_envelope)?;
        self.set_master_mix_unchecked(gain, pan, muted, automation, volume, pan_envelope);
        Ok(())
    }
}
