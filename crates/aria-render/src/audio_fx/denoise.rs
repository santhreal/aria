//! Noise reduction: RNNoise on each channel, mixed with the dry signal by
//! `amount`.
//!
//! RNNoise takes whole frames of [`FRAME`] samples and returns each one a frame
//! late. The input waits for a whole frame, and the output starts a frame
//! behind, so every block can be answered in full: the latency is two frames.

use std::collections::VecDeque;

use nnnoiseless::DenoiseState;

use super::Knob;
use crate::audio_dsp::I16_SCALE;
use crate::audio_processor::{AudioBlock, AudioProcessor};

const FRAME: usize = DenoiseState::FRAME_SIZE;

struct NoiseReduction {
    states: [Box<DenoiseState<'static>>; 2],
    /// Input not yet a whole frame, at RNNoise scale.
    pending: [Vec<f32>; 2],
    /// Denoised output waiting to leave.
    wet: [VecDeque<f32>; 2],
    /// The input, delayed to line up with `wet`.
    dry: [VecDeque<f32>; 2],
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
}

impl NoiseReduction {
    fn new() -> Self {
        let mut made = Self {
            states: [DenoiseState::new(), DenoiseState::new()],
            pending: [Vec::with_capacity(FRAME), Vec::with_capacity(FRAME)],
            wet: [
                VecDeque::with_capacity(2 * FRAME + 1),
                VecDeque::with_capacity(2 * FRAME + 1),
            ],
            dry: [
                VecDeque::with_capacity(2 * FRAME + 1),
                VecDeque::with_capacity(2 * FRAME + 1),
            ],
            frame_in: [0.0; FRAME],
            frame_out: [0.0; FRAME],
        };
        made.prime();
        made
    }

    /// Empty every buffer and start the output its latency behind the input.
    fn prime(&mut self) {
        for channel in 0..2 {
            self.pending[channel].clear();
            self.wet[channel].clear();
            self.wet[channel].extend(std::iter::repeat_n(0.0, FRAME));
            self.dry[channel].clear();
            self.dry[channel].extend(std::iter::repeat_n(0.0, 2 * FRAME));
        }
    }

    /// Take one input sample on `channel` and return the output sample.
    fn step(&mut self, channel: usize, sample: f32, amount: f32) -> f32 {
        self.pending[channel].push(sample * I16_SCALE);
        self.dry[channel].push_back(sample);
        if self.pending[channel].len() == FRAME {
            self.frame_in.copy_from_slice(&self.pending[channel]);
            self.pending[channel].clear();
            self.states[channel].process_frame(&mut self.frame_out, &self.frame_in);
            self.wet[channel].extend(self.frame_out.iter().map(|value| value / I16_SCALE));
        }
        let wet = self.wet[channel].pop_front().unwrap_or(0.0);
        let dry = self.dry[channel].pop_front().unwrap_or(0.0);
        wet * amount + dry * (1.0 - amount)
    }
}

impl AudioProcessor for NoiseReduction {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let amount = Knob::of(block, 0, 1.0);
        let (left, right) = block.channels();
        for frame in 0..left.len() {
            let amount = amount.at(frame).clamp(0.0, 1.0);
            left[frame] = self.step(0, left[frame], amount);
            right[frame] = self.step(1, right[frame], amount);
        }
    }

    fn reset(&mut self) {
        self.states = [DenoiseState::new(), DenoiseState::new()];
        self.prime();
    }
}

pub(super) fn make(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(NoiseReduction::new())
}
