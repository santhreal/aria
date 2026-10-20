//! Animation and effect presets (`.ariapreset`) registered by native plugins.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Specification of an animation and effect preset (`.ariapreset`) registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetSpec {
    /// The preset format version: 1.
    pub version: u32,
    /// Display name, trimmed, at most 128 characters.
    pub name: String,
    /// Effect instances, at most 64.
    #[serde(default)]
    pub effects: Vec<serde_json::Value>,
    /// Audio effect instances, at most 64.
    #[serde(default)]
    pub audio_effects: Vec<serde_json::Value>,
    /// `{ "id": <clip property row id>, "curve": <constant or keyframes> }`
    /// objects, at most 256, each curve at most 4096 keyframes.
    #[serde(default)]
    pub properties: Vec<serde_json::Value>,
}

impl PresetSpec {
    /// Build a version-1 preset with `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: 1,
            name: name.into(),
            effects: Vec::new(),
            audio_effects: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Append a visual effect instance with default parameters.
    pub fn with_effect(mut self, effect_id: impl Into<String>) -> Self {
        self.effects.push(json!({ "effect_id": effect_id.into() }));
        self
    }

    /// Append a constant scalar property curve (`id` from `clip_param_catalog`, e.g. `"opacity"`).
    pub fn with_scalar_property(mut self, id: impl Into<String>, value: f32) -> Self {
        self.properties.push(json!({
            "id": id.into(),
            "curve": {
                "type": "Constant",
                "value": { "scalar": value }
            }
        }));
        self
    }
}
