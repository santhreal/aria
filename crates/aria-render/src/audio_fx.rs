//! The audio effects that ship with the app.
//!
//! Each one is an [`AudioProcessor`] on the contract a plugin's processor
//! follows, listed in [`SHIPPED`] under the id the model's shipped catalog
//! (`aria-models/data/audio_effects.toml`) gives it. The catalog declares the
//! parameter slots each processor reads and the latency each one adds; the
//! tests fail when a processor disagrees with either.

mod channels;
mod denoise;
mod dynamics;
mod eq;
mod filter;
mod loudness;
mod pitch;
mod space;

use crate::audio_processor::{AudioBlock, AudioProcessor, ParamSignal};

/// One shipped audio effect: its id and its constructor.
pub(crate) struct Shipped {
    pub(crate) id: &'static str,
    pub(crate) make: fn(u32) -> Box<dyn AudioProcessor>,
}

/// Every shipped audio effect.
pub(crate) static SHIPPED: &[Shipped] = &[
    Shipped {
        id: "gain",
        make: channels::gain,
    },
    Shipped {
        id: "parametric_eq",
        make: eq::make,
    },
    Shipped {
        id: "compressor",
        make: dynamics::compressor,
    },
    Shipped {
        id: "limiter",
        make: dynamics::limiter,
    },
    Shipped {
        id: "deesser",
        make: dynamics::deesser,
    },
    Shipped {
        id: "noise_reduction",
        make: denoise::make,
    },
    Shipped {
        id: "reverb",
        make: space::reverb,
    },
    Shipped {
        id: "delay",
        make: space::delay,
    },
    Shipped {
        id: "pitch_shifter",
        make: pitch::make,
    },
    Shipped {
        id: "swap_channels",
        make: channels::swap,
    },
    Shipped {
        id: "fill_left",
        make: channels::fill_left,
    },
    Shipped {
        id: "fill_right",
        make: channels::fill_right,
    },
    Shipped {
        id: "mono",
        make: channels::mono,
    },
    Shipped {
        id: "loudness_normalizer",
        make: loudness::make,
    },
];

/// The shipped effect `id`, or `None`.
pub(crate) fn shipped(id: &str) -> Option<&'static Shipped> {
    SHIPPED.iter().find(|entry| entry.id == id)
}

/// Decibels as a linear gain. `0.0` dB is exactly `1.0`.
fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A linear level in decibels, floored at -240 dB for silence.
fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-12).log10()
}

/// The one-pole coefficient for a time constant of `ms` at `rate`.
fn coefficient(ms: f32, rate: u32) -> f32 {
    let samples = ms.max(0.001) * 0.001 * rate as f32;
    (-1.0 / samples).exp()
}

/// A one-pole coefficient, recomputed only when its time constant moves.
#[derive(Debug, Clone, Copy)]
struct Timing {
    ms: f32,
    coefficient: f32,
}

impl Timing {
    fn new(ms: f32, rate: u32) -> Self {
        Self {
            ms,
            coefficient: coefficient(ms, rate),
        }
    }

    fn at(&mut self, ms: f32, rate: u32) -> f32 {
        if ms != self.ms {
            *self = Self::new(ms, rate);
        }
        self.coefficient
    }
}

/// One parameter slot across a block, with the value to read when the block
/// carries no such slot.
#[derive(Debug, Clone, Copy)]
struct Knob<'a> {
    signal: Option<ParamSignal<'a>>,
    fallback: f32,
}

impl<'a> Knob<'a> {
    fn of(block: &AudioBlock<'a>, slot: usize, fallback: f32) -> Self {
        Self {
            signal: block.param(slot),
            fallback,
        }
    }

    fn at(&self, frame: usize) -> f32 {
        let value = match &self.signal {
            Some(signal) => signal.at(frame),
            None => self.fallback,
        };
        if value.is_finite() {
            value
        } else {
            self.fallback
        }
    }

    /// The value, when it holds for the whole block.
    fn constant(&self) -> Option<f32> {
        match &self.signal {
            Some(signal) => signal.constant().filter(|value| value.is_finite()),
            None => Some(self.fallback),
        }
    }
}

/// Zero for a value too small to hear, so a decaying feedback loop does not
/// run on subnormal floats.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-25 { 0.0 } else { value }
}

#[cfg(test)]
#[path = "audio_fx_tests.rs"]
mod tests;
