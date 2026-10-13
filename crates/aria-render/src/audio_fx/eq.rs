//! The parametric equalizer: a low shelf, six bells, a high shelf, a low cut
//! and a high cut, then an output gain.
//!
//! A band at 0 dB, a low cut at 0 Hz and a high cut at 24 kHz or above are
//! out of the signal path, so the effect at its defaults returns its input
//! unchanged. Coefficients follow automation every [`CONTROL`] frames.

use super::filter::{BUTTERWORTH, Biquad, Coefs};
use super::{Knob, db_to_gain};
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// Parameter names in slot order: three per band, then the cuts and the
/// output.
pub(super) const PARAMS: &[&str] = &[
    "band1_freq",
    "band1_gain",
    "band1_q",
    "band2_freq",
    "band2_gain",
    "band2_q",
    "band3_freq",
    "band3_gain",
    "band3_q",
    "band4_freq",
    "band4_gain",
    "band4_q",
    "band5_freq",
    "band5_gain",
    "band5_q",
    "band6_freq",
    "band6_gain",
    "band6_q",
    "band7_freq",
    "band7_gain",
    "band7_q",
    "band8_freq",
    "band8_gain",
    "band8_q",
    "low_cut",
    "high_cut",
    "output",
];

const BANDS: usize = 8;
const LOW_CUT: usize = BANDS * 3;
const HIGH_CUT: usize = LOW_CUT + 1;
const OUTPUT: usize = LOW_CUT + 2;

/// The high cut at or above this frequency is off.
const HIGH_CUT_OFF: f32 = 24_000.0;

/// The fallback for each band's frequency, matching the shipped defaults.
const BAND_FREQ: [f32; BANDS] = [80.0, 160.0, 320.0, 640.0, 1280.0, 2560.0, 5120.0, 10_000.0];

/// Frames between coefficient updates while a parameter is automated.
const CONTROL: usize = 32;

/// One filter in the chain: the values it was designed for, its
/// coefficients, and one memory per channel.
#[derive(Debug, Clone, Copy)]
struct Stage {
    design: Option<[f32; 3]>,
    coefs: Coefs,
    memory: [Biquad; 2],
}

impl Stage {
    fn new() -> Self {
        Self {
            design: None,
            coefs: Coefs::peaking(48_000, 1000.0, 1.0, 0.0),
            memory: [Biquad::default(); 2],
        }
    }

    /// Point the stage at `design`, or take it out of the path with `None`.
    fn set(&mut self, design: Option<[f32; 3]>, coefs: impl FnOnce([f32; 3]) -> Coefs) {
        if design == self.design {
            return;
        }
        match design {
            Some(values) => self.coefs = coefs(values),
            // A stage that leaves the path forgets its memory, so coming back
            // starts from silence rather than from a stale state.
            None => self.memory = [Biquad::default(); 2],
        }
        self.design = design;
    }
}

struct Equalizer {
    rate: u32,
    stages: [Stage; BANDS + 2],
}

impl Equalizer {
    fn design(&mut self, knobs: &[Knob<'_>], frame: usize) {
        let rate = self.rate;
        for band in 0..BANDS {
            let freq = knobs[band * 3].at(frame);
            let gain = knobs[band * 3 + 1].at(frame);
            let q = knobs[band * 3 + 2].at(frame);
            let design = (gain != 0.0).then_some([freq, gain, q]);
            self.stages[band].set(design, |[freq, gain, q]| match band {
                0 => Coefs::low_shelf(rate, freq, q, gain),
                b if b == BANDS - 1 => Coefs::high_shelf(rate, freq, q, gain),
                _ => Coefs::peaking(rate, freq, q, gain),
            });
        }
        let low_cut = knobs[LOW_CUT].at(frame);
        self.stages[BANDS].set(
            (low_cut > 0.0).then_some([low_cut, 0.0, 0.0]),
            |[freq, ..]| Coefs::high_pass(rate, freq, BUTTERWORTH),
        );
        let high_cut = knobs[HIGH_CUT].at(frame);
        let nyquist = rate as f32 / 2.0;
        let on = high_cut < HIGH_CUT_OFF && high_cut < nyquist;
        self.stages[BANDS + 1].set(on.then_some([high_cut, 0.0, 0.0]), |[freq, ..]| {
            Coefs::low_pass(rate, freq, BUTTERWORTH)
        });
    }
}

impl AudioProcessor for Equalizer {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let mut knobs = [Knob {
            signal: None,
            fallback: 0.0,
        }; PARAMS.len()];
        for (slot, knob) in knobs.iter_mut().enumerate() {
            let fallback = match slot {
                s if s < LOW_CUT => match s % 3 {
                    0 => BAND_FREQ[s / 3],
                    1 => 0.0,
                    _ => 1.0,
                },
                HIGH_CUT => HIGH_CUT_OFF,
                _ => 0.0,
            };
            *knob = Knob::of(block, slot, fallback);
        }
        let automated = knobs.iter().any(|knob| knob.constant().is_none());
        let (left, right) = block.channels();
        let frames = left.len();
        let mut start = 0;
        while start < frames {
            let end = if automated {
                (start + CONTROL).min(frames)
            } else {
                frames
            };
            self.design(&knobs, start);
            for stage in self
                .stages
                .iter_mut()
                .filter(|stage| stage.design.is_some())
            {
                let coefs = stage.coefs;
                let [memory_left, memory_right] = &mut stage.memory;
                for sample in &mut left[start..end] {
                    *sample = memory_left.run(&coefs, f64::from(*sample)) as f32;
                }
                for sample in &mut right[start..end] {
                    *sample = memory_right.run(&coefs, f64::from(*sample)) as f32;
                }
            }
            start = end;
        }
        let output = knobs[OUTPUT];
        for frame in 0..frames {
            let db = output.at(frame);
            if db != 0.0 {
                let scale = db_to_gain(db);
                left[frame] *= scale;
                right[frame] *= scale;
            }
        }
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.memory = [Biquad::default(); 2];
        }
    }
}

pub(super) fn make(rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Equalizer {
        rate,
        stages: [Stage::new(); BANDS + 2],
    })
}
