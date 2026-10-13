//! Time-based effects: the feedback delay and the reverb.

use super::{Knob, flush};
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// The longest delay time the line holds, in seconds. The shipped range ends
/// at two; a catalog file may raise it this far.
const DELAY_SECONDS: usize = 4;

/// A feedback delay per channel. The time may move while the line plays;
/// the read point is interpolated between frames.
struct Delay {
    rate: u32,
    lines: [Vec<f32>; 2],
    at: usize,
}

impl AudioProcessor for Delay {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let time = Knob::of(block, 0, 250.0);
        let feedback = Knob::of(block, 1, 0.35);
        let mix = Knob::of(block, 2, 0.35);
        let (left, right) = block.channels();
        let len = self.lines[0].len();
        let longest = (len - 2) as f32;
        for frame in 0..left.len() {
            let delay = (time.at(frame) * self.rate as f32 / 1000.0).clamp(1.0, longest);
            let whole = delay as usize;
            let part = delay - whole as f32;
            let feedback = feedback.at(frame).clamp(0.0, 0.99);
            let mix = mix.at(frame).clamp(0.0, 1.0);
            let newer = (self.at + len - whole) % len;
            let older = (newer + len - 1) % len;
            for (channel, sample) in [&mut left[frame], &mut right[frame]]
                .into_iter()
                .enumerate()
            {
                let line = &mut self.lines[channel];
                let echo = line[newer] * (1.0 - part) + line[older] * part;
                line[self.at] = flush(*sample + echo * feedback);
                *sample = *sample * (1.0 - mix) + echo * mix;
            }
            self.at = (self.at + 1) % len;
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.at = 0;
    }
}

pub(super) fn delay(rate: u32) -> Box<dyn AudioProcessor> {
    let len = rate as usize * DELAY_SECONDS + 2;
    Box::new(Delay {
        rate,
        lines: [vec![0.0; len], vec![0.0; len]],
        at: 0,
    })
}

/// Comb lengths at 44.1 kHz for the left channel; the right adds
/// [`SPREAD`].
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
/// All-pass lengths at 44.1 kHz.
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;
const INPUT_GAIN: f32 = 0.015;
const WET_SCALE: f32 = 3.0;
const ROOM_SCALE: f32 = 0.28;
const ROOM_OFFSET: f32 = 0.7;
const DAMP_SCALE: f32 = 0.4;
const PREDELAY_MS: usize = 200;

/// A lowpass-feedback comb filter.
struct Comb {
    line: Vec<f32>,
    at: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            line: vec![0.0; len.max(1)],
            at: 0,
            store: 0.0,
        }
    }

    fn run(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.line[self.at];
        self.store = flush(out * (1.0 - damp) + self.store * damp);
        self.line[self.at] = flush(input + self.store * feedback);
        self.at = (self.at + 1) % self.line.len();
        out
    }

    fn clear(&mut self) {
        self.line.fill(0.0);
        self.at = 0;
        self.store = 0.0;
    }
}

/// A Schroeder all-pass with a fixed feedback of one half.
struct Allpass {
    line: Vec<f32>,
    at: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            line: vec![0.0; len.max(1)],
            at: 0,
        }
    }

    fn run(&mut self, input: f32) -> f32 {
        let held = self.line[self.at];
        self.line[self.at] = flush(input + held * 0.5);
        self.at = (self.at + 1) % self.line.len();
        held - input
    }

    fn clear(&mut self) {
        self.line.fill(0.0);
        self.at = 0;
    }
}

/// The Schroeder-Moorer reverb as Freeverb tunes it: eight parallel combs and
/// four series all-passes per channel, the right channel's lines slightly
/// longer for width, fed by a mono pre-delay line.
struct Reverb {
    rate: u32,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    predelay: Vec<f32>,
    predelay_at: usize,
}

impl AudioProcessor for Reverb {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let size = Knob::of(block, 0, 0.5);
        let damping = Knob::of(block, 1, 0.5);
        let width = Knob::of(block, 2, 1.0);
        let mix = Knob::of(block, 3, 0.25);
        let predelay = Knob::of(block, 4, 0.0);
        let (left, right) = block.channels();
        let span = self.predelay.len();
        for frame in 0..left.len() {
            let feedback = size.at(frame).clamp(0.0, 1.0) * ROOM_SCALE + ROOM_OFFSET;
            let damp = damping.at(frame).clamp(0.0, 1.0) * DAMP_SCALE;
            let width = width.at(frame).clamp(0.0, 1.0);
            let mix = mix.at(frame).clamp(0.0, 1.0);
            let lag =
                ((predelay.at(frame).max(0.0) * self.rate as f32 / 1000.0) as usize).min(span - 1);

            self.predelay[self.predelay_at] = (left[frame] + right[frame]) * INPUT_GAIN;
            let input = self.predelay[(self.predelay_at + span - lag) % span];
            self.predelay_at = (self.predelay_at + 1) % span;

            let mut wet = [0.0f32; 2];
            for (channel, out) in wet.iter_mut().enumerate() {
                let mut sum = 0.0;
                for comb in &mut self.combs[channel] {
                    sum += comb.run(input, feedback, damp);
                }
                for allpass in &mut self.allpasses[channel] {
                    sum = allpass.run(sum);
                }
                *out = sum;
            }
            let direct = WET_SCALE * (width / 2.0 + 0.5);
            let cross = WET_SCALE * ((1.0 - width) / 2.0);
            let wet_left = wet[0] * direct + wet[1] * cross;
            let wet_right = wet[1] * direct + wet[0] * cross;
            left[frame] = left[frame] * (1.0 - mix) + wet_left * mix;
            right[frame] = right[frame] * (1.0 - mix) + wet_right * mix;
        }
    }

    fn reset(&mut self) {
        for comb in self.combs.iter_mut().flatten() {
            comb.clear();
        }
        for allpass in self.allpasses.iter_mut().flatten() {
            allpass.clear();
        }
        self.predelay.fill(0.0);
        self.predelay_at = 0;
    }
}

pub(super) fn reverb(rate: u32) -> Box<dyn AudioProcessor> {
    let scaled = |len: usize| (len as u64 * u64::from(rate) / 44_100) as usize;
    let side = |spread: usize| {
        (
            COMBS
                .iter()
                .map(|&len| Comb::new(scaled(len + spread)))
                .collect(),
            ALLPASSES
                .iter()
                .map(|&len| Allpass::new(scaled(len + spread)))
                .collect(),
        )
    };
    let (combs_left, allpasses_left) = side(0);
    let (combs_right, allpasses_right) = side(SPREAD);
    Box::new(Reverb {
        rate,
        combs: [combs_left, combs_right],
        allpasses: [allpasses_left, allpasses_right],
        predelay: vec![0.0; rate as usize * PREDELAY_MS / 1000 + 1],
        predelay_at: 0,
    })
}
