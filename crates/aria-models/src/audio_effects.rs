//! Audio effects as data.
//!
//! An audio effect is a processor the mixer runs: an id, a label, scalar
//! parameters, the latency it adds, and whether it reads a sidechain key. The
//! shipped set is read from `data/audio_effects.toml`; a plugin registers more
//! at run time. A chain of [`AudioEffectInstance`]s sits on a clip, on a lane,
//! and on the master, addressed by [`AudioInsertTarget`]; the mixer in
//! `aria-render` runs each chain in order. The model holds no processing code,
//! only what a chain is and the rules an edit to it follows.
//!
//! Keyframes on a clip insert are clip-relative ticks, like every other clip
//! curve, so they move with the clip. Keyframes on a lane or master insert are
//! sequence ticks.
//!
//! An instance whose effect is not registered stays in the chain with its
//! parameters, and the mixer passes its input through.

use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use crate::Map;
use crate::effects::{EffectParamKind, EffectParamSpec, check_params, range_check};
use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::param::Param;

/// The most latency a processor may report: one second of sample frames at
/// the mix rate (48 kHz).
pub const MAX_AUDIO_LATENCY: u32 = 48_000;

/// The most inserts a lane or the master holds: the slots a mixer strip draws.
/// A clip chain has no limit.
pub const STRIP_INSERTS: usize = 5;

/// One audio effect: what a processor declares about itself.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AudioEffectSpec {
    /// Stable id. A registered id is namespaced, such as `example.gain`; a
    /// shipped id is not.
    pub id: String,
    pub label: String,
    /// Scalar parameters in slot order. A processor reads them by slot.
    #[serde(default)]
    pub params: Vec<EffectParamSpec>,
    /// Sample frames at 48 kHz by which the processor's output trails its
    /// input. The mixer delays every parallel path to match, so lanes stay
    /// aligned.
    #[serde(default)]
    pub latency: u32,
    /// Whether the processor reads a key signal from another lane.
    #[serde(default)]
    pub sidechain: bool,
}

impl AudioEffectSpec {
    /// The parameter named `name`, or `None`.
    pub fn param(&self, name: &str) -> Option<&EffectParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }
}

/// The rules every registered audio effect follows beyond an id and a label:
/// the effect parameter rules, scalar parameters only, defaults inside their
/// ranges, and a latency within [`MAX_AUDIO_LATENCY`].
pub(crate) fn check_audio_effect(spec: &AudioEffectSpec) -> Result<(), String> {
    check_params(&spec.id, &spec.params)?;
    for param in &spec.params {
        if param.kind != EffectParamKind::Scalar {
            return Err(format!(
                "{}.{} is a {:?} parameter; an audio effect takes scalar parameters only",
                spec.id, param.name, param.kind
            ));
        }
        if !param.default.is_finite() || param.default < param.min || param.default > param.max {
            return Err(format!(
                "{}.{} defaults to {} outside its range [{}, {}]",
                spec.id, param.name, param.default, param.min, param.max
            ));
        }
    }
    if spec.latency > MAX_AUDIO_LATENCY {
        return Err(format!(
            "{} reports {} frames of latency; the most is {MAX_AUDIO_LATENCY}",
            spec.id, spec.latency
        ));
    }
    Ok(())
}

/// The shipped catalog's text, compiled in.
const SHIPPED_TEXT: &str = include_str!("../data/audio_effects.toml");

/// The shipped audio effects, read once. Shipped set at
/// `data/audio_effects.toml`; a file of the same name in the catalog directory
/// replaces it.
static SHIPPED: LazyLock<Vec<Arc<AudioEffectSpec>>> = LazyLock::new(|| {
    crate::catalog::load("audio_effects", SHIPPED_TEXT, check_audio_catalog)
        .into_iter()
        .map(Arc::new)
        .collect()
});

/// The audio effects that ship with the app, without registered ones.
pub fn shipped_audio_effect_catalog() -> &'static [Arc<AudioEffectSpec>] {
    &SHIPPED
}

/// The audio effect catalog's own rule: unique ids, every entry valid as a
/// registered one would be, and an entry with a shipped id keeping the shape
/// its processor reads. A processor reads parameters by slot and reports a
/// fixed latency, so a renamed or reordered parameter, a changed latency, or a
/// changed key would feed it the wrong values or misalign the mix.
fn check_audio_catalog(entries: &[AudioEffectSpec]) -> Result<(), String> {
    crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
    entries.iter().try_for_each(check_audio_effect)?;
    #[derive(Deserialize)]
    struct File {
        entry: Vec<AudioEffectSpec>,
    }
    let shipped: File = toml::from_str(SHIPPED_TEXT).map_err(|err| err.to_string())?;
    for entry in entries {
        let Some(original) = shipped.entry.iter().find(|held| held.id == entry.id) else {
            continue;
        };
        let names = |spec: &AudioEffectSpec| -> Vec<String> {
            spec.params.iter().map(|param| param.name.clone()).collect()
        };
        if names(entry) != names(original) {
            return Err(format!(
                "{} must keep the parameters {:?} in that order; its processor reads them by slot",
                entry.id,
                names(original)
            ));
        }
        if entry.latency != original.latency || entry.sidechain != original.sidechain {
            return Err(format!(
                "{} must keep latency = {} and sidechain = {}; its processor reports them",
                entry.id, original.latency, original.sidechain
            ));
        }
    }
    Ok(())
}

/// Every audio effect the model knows about now: the shipped set, then
/// registered ones in registration order.
pub fn audio_effect_catalog() -> crate::Catalog<AudioEffectSpec> {
    crate::registered::audio_effect_catalog()
}

/// The audio effect shipped or registered as `id`, or `None`.
pub fn audio_effect_spec(id: &str) -> Option<Arc<AudioEffectSpec>> {
    crate::registered::audio_effect_spec(id)
}

/// Where an insert chain sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioInsertTarget {
    /// One clip's own sound, before its volume, fades and pan.
    Clip(ClipId),
    /// A lane's sum, before its fader and its sends.
    Track(TrackId),
    /// The programme mix, before the output clamp.
    Master,
}

impl AudioInsertTarget {
    /// The clip this chain sits on, if it is a clip chain.
    pub fn clip(self) -> Option<ClipId> {
        match self {
            Self::Clip(clip) => Some(clip),
            Self::Track(_) | Self::Master => None,
        }
    }

    /// The lane this chain sits on, if it is a lane chain.
    pub fn track(self) -> Option<TrackId> {
        match self {
            Self::Track(track) => Some(track),
            Self::Clip(_) | Self::Master => None,
        }
    }

    /// How many inserts the chain may hold, if it is bounded.
    pub fn limit(self) -> Option<usize> {
        match self {
            Self::Clip(_) => None,
            Self::Track(_) | Self::Master => Some(STRIP_INSERTS),
        }
    }
}

/// An audio effect placed in a chain. Only parameters that differ from their
/// default are stored, so a fresh instance serializes to its id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEffectInstance {
    pub effect_id: String,
    /// Explicitly set parameters by name, constant or keyframed.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub params: Map<String, Param<f32>>,
    /// Skipped by the mixer while set: the input passes through and the insert
    /// adds no latency.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
    /// The lane whose sound keys this insert, for an effect that reads one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<TrackId>,
}

impl AudioEffectInstance {
    /// A new instance of `effect_id` with every parameter at its default.
    pub fn new(effect_id: impl Into<String>) -> Self {
        Self {
            effect_id: effect_id.into(),
            params: Map::default(),
            bypass: false,
            sidechain: None,
        }
    }

    /// The registered effect, or `None` when it is not registered.
    pub fn spec(&self) -> Option<Arc<AudioEffectSpec>> {
        audio_effect_spec(&self.effect_id)
    }

    /// The registered effect, or the error an edit reports.
    fn require_spec(&self) -> Result<Arc<AudioEffectSpec>, ModelError> {
        self.spec()
            .ok_or_else(|| ModelError::UnknownAudioEffect(self.effect_id.clone()))
    }

    /// Parameter slot `slot`'s curve: the stored one, or the default as a
    /// constant. `None` when the slot does not exist.
    pub fn param_curve(&self, spec: &AudioEffectSpec, slot: usize) -> Option<Param<f32>> {
        let pspec = spec.params.get(slot)?;
        Some(
            self.params
                .get(&pspec.name)
                .cloned()
                .unwrap_or(Param::Constant(pspec.default)),
        )
    }

    /// Parameter `name` at fractional `tick`, falling back to its default.
    /// `None` when the effect is not registered or has no such parameter.
    pub fn sample_param(&self, name: &str, tick: f64) -> Option<f32> {
        let spec = self.spec()?;
        let pspec = spec.param(name)?;
        Some(
            self.params
                .get(name)
                .map_or(pspec.default, |curve| curve.sample_at(tick)),
        )
    }

    /// Set parameter slot `slot` to `value`, constant or keyframed. Every
    /// stored value is checked against the slot's range.
    pub fn set_param(&mut self, slot: usize, value: Param<f32>) -> Result<(), ModelError> {
        let spec = self.require_spec()?;
        let pspec = spec.params.get(slot).ok_or_else(|| {
            ModelError::InvalidParam(format!(
                "audio effect '{}' has no parameter at slot {slot}",
                self.effect_id
            ))
        })?;
        value.validate_shape()?;
        value.for_each_value(|v| range_check(pspec, *v))?;
        if value.constant() == Some(pspec.default) {
            self.params.remove(&pspec.name);
        } else {
            self.params.insert(pspec.name.clone(), value);
        }
        Ok(())
    }

    /// Key this insert from `key`, or stop keying it. Refused for an effect
    /// that reads no sidechain; the lane itself is checked by the caller, which
    /// has the routing graph.
    pub(crate) fn set_sidechain(&mut self, key: Option<TrackId>) -> Result<(), ModelError> {
        if key.is_some() && !self.require_spec()?.sidechain {
            return Err(ModelError::InvalidParam(format!(
                "audio effect '{}' reads no sidechain",
                self.effect_id
            )));
        }
        self.sidechain = key;
        Ok(())
    }
}

#[cfg(test)]
#[path = "audio_effects_tests.rs"]
mod tests;
