//! Mixer strip pan, automation modes, and volume/pan envelopes for lanes and
//! the master strip.

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::TrackId;
use crate::param::Param;
use crate::timeline::Timeline;
use crate::track::Track;
use crate::track_routing::{AudioSend, TrackOutput, check_send_level};

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
}

/// Refuse a pan position outside `[-1.0, 1.0]`, naming what it was.
pub fn check_track_pan(what: &str, pan: f32) -> Result<f32, ModelError> {
    if !pan.is_finite() || !(MIN_TRACK_PAN..=MAX_TRACK_PAN).contains(&pan) {
        return Err(ModelError::InvalidParam(format!(
            "{what} {pan} is outside {MIN_TRACK_PAN} to {MAX_TRACK_PAN}"
        )));
    }
    Ok(pan)
}

/// Validate a strip volume automation envelope (`0.0..=AudioSend::MAX_LEVEL`,
/// strictly sorted keyframes, no spatial tangents).
pub fn check_track_volume_envelope(what: &str, envelope: &Param<f32>) -> Result<(), ModelError> {
    check_scalar_envelope(what, envelope, |v| {
        AudioSend::check_level(what, v).map(|_| ())
    })
}

/// Validate a strip pan automation envelope (`-1.0..=1.0`, strictly sorted
/// keyframes, no spatial tangents).
pub fn check_track_pan_envelope(what: &str, envelope: &Param<f32>) -> Result<(), ModelError> {
    check_scalar_envelope(what, envelope, |v| check_track_pan(what, v).map(|_| ()))
}

/// Strictly sorted keyframes, no spatial tangents, and every value accepted
/// by `check_value`.
fn check_scalar_envelope(
    what: &str,
    envelope: &Param<f32>,
    check_value: impl Fn(f32) -> Result<(), ModelError>,
) -> Result<(), ModelError> {
    envelope.validate_shape()?;
    if envelope.keyframes().iter().any(|kf| kf.tangents.is_some()) {
        return Err(ModelError::InvalidParam(format!(
            "{what} is a scalar curve and cannot carry spatial tangents"
        )));
    }
    envelope.for_each_value(|&v| check_value(v))
}

/// A strip's fader, pan and envelopes, each checked as its setter checks it,
/// labelled `{strip} gain` and so on. `Err` holds the name of the field that
/// stores the rejected value and the rejection.
fn check_strip(
    labels: [&str; 4],
    fields: [&'static str; 4],
    gain: f32,
    pan: f32,
    volume: &Param<f32>,
    pan_envelope: &Param<f32>,
) -> Result<(), (&'static str, ModelError)> {
    let [gain_field, pan_field, volume_field, pan_envelope_field] = fields;
    AudioSend::check_level(labels[0], gain).map_err(|err| (gain_field, err))?;
    check_track_pan(labels[1], pan).map_err(|err| (pan_field, err))?;
    check_track_volume_envelope(labels[2], volume).map_err(|err| (volume_field, err))?;
    check_track_pan_envelope(labels[3], pan_envelope).map_err(|err| (pan_envelope_field, err))
}

const LANE_LABELS: [&str; 4] = [
    "lane gain",
    "lane pan",
    "lane volume envelope",
    "lane pan envelope",
];
const MASTER_LABELS: [&str; 4] = [
    "master gain",
    "master pan",
    "master volume envelope",
    "master pan envelope",
];
const MASTER_FIELDS: [&str; 4] = [
    "master_gain",
    "master_pan",
    "master_volume_envelope",
    "master_pan_envelope",
];

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

    /// `Ok` iff every mix value of this lane is one its setters accept: the
    /// fader, the pan, both envelopes, each send's level, and no output or
    /// send back into the lane itself. A file reaches the lane through the
    /// deserializer instead, so the loader reads this
    /// ([`Timeline::layout_fault`]). A loop through other lanes is reported
    /// when the mix reads the routing ([`Timeline::audio_buses`]).
    pub(crate) fn check_mix_values(&self) -> Result<(), ModelError> {
        check_strip(
            LANE_LABELS,
            ["gain", "pan", "volume_envelope", "pan_envelope"],
            self.gain,
            self.pan,
            &self.volume_envelope,
            &self.pan_envelope,
        )
        .map_err(|(_, err)| err)?;
        if self.output == TrackOutput::Bus(self.id) || self.sends.iter().any(|s| s.to == self.id) {
            return Err(ModelError::RoutingCycle(self.id));
        }
        self.sends
            .iter()
            .try_for_each(|send| check_send_level(send.level).map(drop))
    }
}

impl Timeline {
    /// Set a lane's stereo pan (`-1.0` hard left .. `1.0` hard right).
    pub fn set_track_pan(&mut self, track: TrackId, pan: f32) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        let pan = check_track_pan(LANE_LABELS[1], pan)?;
        let lane = self.require_track_mut(track)?;
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
        check_track_volume_envelope(LANE_LABELS[2], &volume)?;
        check_track_pan_envelope(LANE_LABELS[3], &pan)?;
        let lane = self.require_track_mut(track)?;
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
        check_strip(
            MASTER_LABELS,
            MASTER_FIELDS,
            gain,
            pan,
            &volume,
            &pan_envelope,
        )
        .map_err(|(_, err)| err)?;
        self.set_master_mix_unchecked(gain, pan, muted, automation, volume, pan_envelope);
        Ok(())
    }

    /// The master strip's stored values as [`Self::set_master_mix`] checks
    /// them. `Err` holds the timeline field that stores the rejected value.
    pub(crate) fn check_master_mix_values(&self) -> Result<(), (&'static str, ModelError)> {
        check_strip(
            MASTER_LABELS,
            MASTER_FIELDS,
            self.master_gain(),
            self.master_pan(),
            self.master_volume_envelope(),
            self.master_pan_envelope(),
        )
    }
}
