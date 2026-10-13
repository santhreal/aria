//! The pitch shifter: two read heads sweep a delay line at the pitch ratio,
//! half a window apart, each faded in and out with a sine-squared window so
//! one is always at full level while the other wraps.
//!
//! At a ratio of one both heads rest, the live head at half a window, so the
//! effect is a pure delay of half a window: the catalog's 1024 frames.

use std::f64::consts::PI;

use super::Knob;
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// The window the heads sweep, in frames at 48 kHz.
const WINDOW: usize = 2048;

struct PitchShifter {
    window: f64,
    lines: [Vec<f32>; 2],
    mask: usize,
    at: usize,
    /// Where the first head is in its sweep, in `[0, 1)`.
    phase: f64,
}

impl PitchShifter {
    /// The sample `delay` frames before the newest one in `line`.
    fn tap(&self, channel: usize, delay: f64) -> f32 {
        let whole = delay as usize;
        let part = (delay - whole as f64) as f32;
        let line = &self.lines[channel];
        let newer = line[self.at.wrapping_sub(whole) & self.mask];
        let older = line[self.at.wrapping_sub(whole + 1) & self.mask];
        newer * (1.0 - part) + older * part
    }
}

impl AudioProcessor for PitchShifter {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let semitones = Knob::of(block, 0, 0.0);
        let cents = Knob::of(block, 1, 0.0);
        let (left, right) = block.channels();
        for frame in 0..left.len() {
            let steps = f64::from(semitones.at(frame)) + f64::from(cents.at(frame)) / 100.0;
            let ratio = 2f64.powf(steps / 12.0).clamp(0.25, 4.0);
            if ratio == 1.0 {
                self.phase = 0.0;
            }
            self.at = (self.at + 1) & self.mask;
            self.lines[0][self.at] = left[frame];
            self.lines[1][self.at] = right[frame];

            let first = self.phase * self.window;
            let second = (self.phase + 0.5).fract() * self.window;
            let fade = (PI * self.phase).sin();
            let first_weight = (fade * fade) as f32;
            let rest = (PI * self.phase).cos();
            let second_weight = (rest * rest) as f32;
            left[frame] = self.tap(0, first) * first_weight + self.tap(0, second) * second_weight;
            right[frame] = self.tap(1, first) * first_weight + self.tap(1, second) * second_weight;

            self.phase = (self.phase - (ratio - 1.0) / self.window).rem_euclid(1.0);
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.at = 0;
        self.phase = 0.0;
    }
}

pub(super) fn make(rate: u32) -> Box<dyn AudioProcessor> {
    let window = (WINDOW as u64 * u64::from(rate) / 48_000).max(4) as usize;
    let len = (window * 2).next_power_of_two();
    Box::new(PitchShifter {
        window: window as f64,
        lines: [vec![0.0; len], vec![0.0; len]],
        mask: len - 1,
        at: 0,
        phase: 0.0,
    })
}
