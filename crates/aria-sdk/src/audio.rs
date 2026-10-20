//! Audio effects: the specification, the processor instance, and the blocks
//! and parameter signals it processes.

use serde::{Deserialize, Serialize};

/// Scalar parameter specification for an audio effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioParamSpec {
    /// Parameter key, unique within the effect.
    pub name: String,
    /// The parameter's row label.
    pub label: String,
    /// Value of a new insert, inside `min..=max`.
    pub default: f32,
    /// Lowest value.
    pub min: f32,
    /// Highest value, not below `min`.
    pub max: f32,
}

impl AudioParamSpec {
    /// Define a scalar parameter with default and inclusive `[min, max]` range.
    pub fn scalar(
        name: impl Into<String>,
        label: impl Into<String>,
        default: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            default,
            min,
            max,
        }
    }
}

/// Specification of an audio effect registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEffectSpec {
    /// Namespaced id with at least one `.`, such as `com.example.drive`. A
    /// shipped id or an id already registered is refused.
    pub id: String,
    /// The name the Effect menu and the mixer inserts list. Not empty.
    pub label: String,
    /// The Effect submenu and Effects browser group the effect is listed
    /// under. Empty lists it under Other Audio.
    #[serde(default)]
    pub category: String,
    /// Scalar parameters in slot order: the order the processor receives their
    /// values.
    #[serde(default)]
    pub params: Vec<AudioParamSpec>,
    /// Sample frames at 48 kHz by which the output trails the input, at most
    /// 48000. The mixer delays every parallel path by the same amount.
    #[serde(default)]
    pub latency: u32,
    /// The effect reads a key signal: a mixer strip shows a Key from row, and
    /// each audio block includes the key lane.
    #[serde(default)]
    pub sidechain: bool,
}

/// One parameter signal across an [`AudioBlock`].
#[derive(Debug, Clone, Copy)]
pub struct AudioParam<'a> {
    pub(crate) constant: f32,
    pub(crate) per_frame: Option<&'a [f32]>,
}

impl<'a> AudioParam<'a> {
    /// The constant value for the block, or the first frame's value when automated.
    pub fn constant(&self) -> f32 {
        self.constant
    }

    /// Per-frame values when the parameter is automated across the block.
    pub fn per_frame(&self) -> Option<&'a [f32]> {
        self.per_frame
    }

    /// Parameter value at `frame`.
    pub fn at(&self, frame: usize) -> f32 {
        match self.per_frame {
            Some(values) => values
                .get(frame)
                .or_else(|| values.last())
                .copied()
                .unwrap_or(self.constant),
            None => self.constant,
        }
    }
}

/// One block of planar stereo audio samples handed to [`AudioProcessor::process`].
#[derive(Debug)]
pub struct AudioBlock<'a> {
    pub(crate) left: &'a mut [f32],
    pub(crate) right: &'a mut [f32],
    pub(crate) position: i64,
    pub(crate) sample_rate: u32,
    pub(crate) sidechain: Option<(&'a [f32], &'a [f32])>,
}

impl<'a> AudioBlock<'a> {
    /// Number of sample frames in the block.
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    /// Timeline sample position of the block's first frame.
    pub fn position(&self) -> i64 {
        self.position
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Mutable planar stereo channels `(left, right)`.
    pub fn channels(&mut self) -> (&mut [f32], &mut [f32]) {
        (&mut *self.left, &mut *self.right)
    }

    /// Optional planar stereo sidechain key signal.
    pub fn sidechain(&self) -> Option<(&'a [f32], &'a [f32])> {
        self.sidechain
    }
}

/// Instance of a plugin audio effect processing stereo planar blocks.
pub trait AudioProcessor: Send {
    /// Process `block` in place using `params`.
    fn process(&mut self, params: &[AudioParam<'_>], block: &mut AudioBlock<'_>);

    /// Reset internal state (delay lines, filter state, envelopes) after a seek.
    fn reset(&mut self) {}
}
