//! Mixer routing arguments: a lane's fader, where its sum goes, and the
//! copies of it other buses take.
//!
//! Levels are linear gain, the same unit `set_clip_audio` takes, so one number
//! means one thing across the vocabulary: `0` is silence, `1` is unity, `4` is
//! `+12 dB` and is the most the model accepts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Move a lane's fader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTrackGain {
    /// The lane to ride. Audio lanes, video lanes that carry their own sound,
    /// and submix buses have a fader; other lanes have nothing to route.
    pub track: u64,
    /// Linear gain: `0` silences the lane, `1` is unity, `4` is `+12 dB`.
    pub gain: f64,
}

/// Point a lane's output at a bus, or back at the master.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTrackOutput {
    /// The lane whose output moves.
    pub track: u64,
    /// The submix bus to feed, or `null` to send the lane straight to the
    /// master. Only a submix lane can be a destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
}

/// Add a send: a copy of a lane, at its own level, added to a bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddSend {
    /// The lane being copied.
    pub track: u64,
    /// The submix bus the copy is added to. One send per lane per bus: a
    /// second copy of the same lane on the same bus is a level, not a route.
    pub to: u64,
    /// Linear gain of the copy. Omit for unity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f64>,
    /// Take the copy before the lane's own fader, so riding the lane down does
    /// not thin the send — how a reverb is fed. Omit for post-fader, which
    /// follows the lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_fader: Option<bool>,
}

/// Change an existing send, addressed by its position on the lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSend {
    /// The lane holding the send.
    pub track: u64,
    /// Which of the lane's sends to change, counting from 0 in the order they
    /// were added. `describe_project` lists them.
    pub index: u32,
    /// New linear gain of the copy. Omit to keep the level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f64>,
    /// Whether the copy is taken before the lane's fader. Omit to keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_fader: Option<bool>,
}

/// Drop a send off a lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveSend {
    /// The lane holding the send.
    pub track: u64,
    /// Which of the lane's sends to drop, counting from 0.
    pub index: u32,
}
