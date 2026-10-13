//! Dynamics: the compressor, the lookahead limiter, and the de-esser.

use std::collections::VecDeque;

use super::filter::{BUTTERWORTH, Biquad, Coefs};
use super::{Knob, Timing, coefficient, db_to_gain, gain_to_db};
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// The limiter's lookahead in frames at 48 kHz: 5 ms, which is also its
/// latency.
pub(super) const LIMITER_LOOKAHEAD: u32 = 240;

/// The compressor's static curve: the output level for input level `x`, all
/// in decibels, with a soft knee `knee` dB wide centred on the threshold.
fn curve(x: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let over = x - threshold;
    if knee > 0.0 && 2.0 * over.abs() <= knee {
        let into = over + knee / 2.0;
        x + (1.0 / ratio - 1.0) * into * into / (2.0 * knee)
    } else if over <= 0.0 {
        x
    } else {
        threshold + over / ratio
    }
}

/// A feed-forward compressor, keyed by the sidechain when the insert has one
/// and by its own input otherwise. The gain reduction is smoothed in
/// decibels, with the attack while it deepens and the release while it
/// recovers.
struct Compressor {
    rate: u32,
    /// The smoothed gain change, in dB, at most zero.
    envelope: f32,
    attack: Timing,
    release: Timing,
}

impl AudioProcessor for Compressor {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let threshold = Knob::of(block, 0, -18.0);
        let ratio = Knob::of(block, 1, 4.0);
        let attack = Knob::of(block, 2, 10.0);
        let release = Knob::of(block, 3, 100.0);
        let knee = Knob::of(block, 4, 6.0);
        let makeup = Knob::of(block, 5, 0.0);
        let key = block.sidechain();
        let (left, right) = block.channels();
        for frame in 0..left.len() {
            let level = match key {
                Some((key_left, key_right)) => key_left[frame].abs().max(key_right[frame].abs()),
                None => left[frame].abs().max(right[frame].abs()),
            };
            let x = gain_to_db(level);
            let target = curve(
                x,
                threshold.at(frame),
                ratio.at(frame).max(1.0),
                knee.at(frame).max(0.0),
            ) - x;
            let coef = if target < self.envelope {
                self.attack.at(attack.at(frame), self.rate)
            } else {
                self.release.at(release.at(frame), self.rate)
            };
            self.envelope = target + coef * (self.envelope - target);
            let change = self.envelope + makeup.at(frame);
            if change != 0.0 {
                let scale = db_to_gain(change);
                left[frame] *= scale;
                right[frame] *= scale;
            }
        }
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
    }
}

pub(super) fn compressor(rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Compressor {
        rate,
        envelope: 0.0,
        attack: Timing::new(10.0, rate),
        release: Timing::new(100.0, rate),
    })
}

/// A lookahead limiter: no sample leaves above the ceiling.
///
/// The gain a sample needs is the ceiling over its peak. The gain applied is
/// the least needed over the lookahead window, recovered at the release rate,
/// then averaged over the window; every average covering a sample's output
/// time includes that sample's own requirement, so the gain has reached it by
/// the time the delayed sample leaves, without a step.
struct Limiter {
    rate: u32,
    lookahead: usize,
    /// The input, delayed by the lookahead, one line per channel.
    delayed: [Vec<f32>; 2],
    delayed_at: usize,
    /// Indices and needed gains, increasing, over the window.
    window: VecDeque<(u64, f32)>,
    frame: u64,
    recovered: f32,
    averaged: Vec<f64>,
    averaged_at: usize,
    sum: f64,
    release: Timing,
}

impl Limiter {
    fn clear(&mut self) {
        for line in &mut self.delayed {
            line.fill(0.0);
        }
        self.delayed_at = 0;
        self.window.clear();
        self.frame = 0;
        self.recovered = 1.0;
        self.averaged.fill(1.0);
        self.averaged_at = 0;
        self.sum = self.averaged.len() as f64;
    }
}

impl AudioProcessor for Limiter {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let ceiling = Knob::of(block, 0, -1.0);
        let release = Knob::of(block, 1, 50.0);
        let input = Knob::of(block, 2, 0.0);
        let (left, right) = block.channels();
        let span = self.averaged.len();
        for frame in 0..left.len() {
            let ceiling = db_to_gain(ceiling.at(frame).min(0.0));
            let drive = db_to_gain(input.at(frame));
            let (in_left, in_right) = (left[frame] * drive, right[frame] * drive);
            let peak = in_left.abs().max(in_right.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };

            while self.window.back().is_some_and(|&(_, held)| held >= needed) {
                self.window.pop_back();
            }
            self.window.push_back((self.frame, needed));
            while self
                .window
                .front()
                .is_some_and(|&(at, _)| at + (self.lookahead as u64) < self.frame)
            {
                self.window.pop_front();
            }
            let least = self.window.front().map_or(1.0, |&(_, held)| held);
            let coef = self.release.at(release.at(frame), self.rate);
            self.recovered = if least < self.recovered {
                least
            } else {
                least + (self.recovered - least) * coef
            };

            self.sum += f64::from(self.recovered) - self.averaged[self.averaged_at];
            self.averaged[self.averaged_at] = f64::from(self.recovered);
            self.averaged_at += 1;
            if self.averaged_at == span {
                self.averaged_at = 0;
                // Re-add from scratch once per window, so rounding in the
                // running sum cannot drift.
                self.sum = self.averaged.iter().sum();
            }
            let gain = (self.sum / span as f64) as f32;

            let out_left = std::mem::replace(&mut self.delayed[0][self.delayed_at], in_left);
            let out_right = std::mem::replace(&mut self.delayed[1][self.delayed_at], in_right);
            self.delayed_at = (self.delayed_at + 1) % self.lookahead;
            left[frame] = (out_left * gain).clamp(-ceiling, ceiling);
            right[frame] = (out_right * gain).clamp(-ceiling, ceiling);
            self.frame += 1;
        }
    }

    fn reset(&mut self) {
        self.clear();
    }
}

pub(super) fn limiter(rate: u32) -> Box<dyn AudioProcessor> {
    let lookahead = ((u64::from(LIMITER_LOOKAHEAD) * u64::from(rate) / 48_000) as usize).max(1);
    let mut limiter = Limiter {
        rate,
        lookahead,
        delayed: [vec![0.0; lookahead], vec![0.0; lookahead]],
        delayed_at: 0,
        window: VecDeque::with_capacity(lookahead + 2),
        frame: 0,
        recovered: 1.0,
        averaged: vec![1.0; lookahead + 1],
        averaged_at: 0,
        sum: 0.0,
        release: Timing::new(50.0, rate),
    };
    limiter.clear();
    Box::new(limiter)
}

/// Frames between updates of the de-esser's shelf.
const DEESS_CONTROL: u64 = 16;

/// A de-esser as a dynamic high shelf. A high-pass at `frequency` measures
/// the sibilance; by how far its level rises over the threshold, at 4:1 and
/// by at most `range` dB, a high shelf at the same frequency turns the top
/// down. A shelf at 0 dB has the identity's coefficients, so with nothing to
/// reduce the input passes unchanged.
struct DeEsser {
    rate: u32,
    detected_at: f32,
    detect: Coefs,
    detector: [Biquad; 2],
    /// The frequency and the reduction the shelf is designed for.
    shelved: (f32, f32),
    shelf_coefs: Coefs,
    shelf: [Biquad; 2],
    /// The detected level, in dB.
    envelope: f32,
    attack: f32,
    release: f32,
    /// Frames processed since the last reset; the shelf updates on a grid of
    /// these, so block boundaries do not move it.
    count: u64,
}

impl DeEsser {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            detected_at: 6000.0,
            detect: Coefs::high_pass(rate, 6000.0, BUTTERWORTH),
            detector: [Biquad::default(); 2],
            shelved: (6000.0, 0.0),
            shelf_coefs: Coefs::high_shelf(rate, 6000.0, BUTTERWORTH, 0.0),
            shelf: [Biquad::default(); 2],
            envelope: -240.0,
            attack: coefficient(0.5, rate),
            release: coefficient(40.0, rate),
            count: 0,
        }
    }
}

impl AudioProcessor for DeEsser {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let frequency = Knob::of(block, 0, 6000.0);
        let threshold = Knob::of(block, 1, -30.0);
        let range = Knob::of(block, 2, 12.0);
        let (left, right) = block.channels();
        for frame in 0..left.len() {
            let freq = frequency.at(frame);
            if freq != self.detected_at {
                self.detected_at = freq;
                self.detect = Coefs::high_pass(self.rate, freq, BUTTERWORTH);
            }
            let (x_left, x_right) = (f64::from(left[frame]), f64::from(right[frame]));
            let high_left = self.detector[0].run(&self.detect, x_left);
            let high_right = self.detector[1].run(&self.detect, x_right);
            let level = gain_to_db(high_left.abs().max(high_right.abs()) as f32);
            let coef = if level > self.envelope {
                self.attack
            } else {
                self.release
            };
            self.envelope = level + coef * (self.envelope - level);
            let over = self.envelope - threshold.at(frame);
            let reduction = if over > 0.0 {
                (over * 0.75).min(range.at(frame).max(0.0))
            } else {
                0.0
            };
            if self.count.is_multiple_of(DEESS_CONTROL) && (freq, reduction) != self.shelved {
                self.shelved = (freq, reduction);
                self.shelf_coefs = Coefs::high_shelf(self.rate, freq, BUTTERWORTH, -reduction);
            }
            self.count += 1;
            left[frame] = self.shelf[0].run(&self.shelf_coefs, x_left) as f32;
            right[frame] = self.shelf[1].run(&self.shelf_coefs, x_right) as f32;
        }
    }

    fn reset(&mut self) {
        *self = Self::new(self.rate);
    }
}

pub(super) fn deesser(rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(DeEsser::new(rate))
}
