//! The loudness normalizer: rides a gain so the short-term loudness of its
//! output sits at the target.
//!
//! Loudness is measured as ITU-R BS.1770 states it: K-weighted, summed over
//! both channels, over the last three seconds, measured every 100 ms. The gain
//! is the target minus that loudness, at most `max_gain` up and 40 dB down,
//! and moves toward it with a 0.5 s time constant going down and 2 s going up.
//! Sound quieter than [`GATE_LUFS`] holds the gain, so a pause is not raised
//! into its own room tone.

use super::filter::{Biquad, Coefs};
use super::{Knob, coefficient, db_to_gain};
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// Below this short-term loudness the gain holds.
const GATE_LUFS: f64 = -50.0;
/// Buckets in the window: thirty of 100 ms.
const BUCKETS: usize = 30;
/// Buckets measured before the gain first moves: 400 ms, BS.1770's momentary
/// window.
const FIRST_MEASURE: usize = 4;
/// The most the gain turns down, in dB.
const MAX_CUT_DB: f32 = 40.0;

/// The K-weighting pre-filter's two stages, as BS.1770 designs them.
fn k_weighting(rate: u32) -> [Coefs; 2] {
    [
        Coefs::high_shelf(rate, 1_681.974_5, 0.707_175_25, 3.999_843_8),
        Coefs::high_pass(rate, 38.135_47, 0.500_327),
    ]
}

struct LoudnessNormalizer {
    weighting: [Coefs; 2],
    /// Per channel, per stage.
    memory: [[Biquad; 2]; 2],
    bucket_len: usize,
    bucket_fill: usize,
    bucket_sum: f64,
    buckets: [f64; BUCKETS],
    bucket_at: usize,
    filled: usize,
    /// The gain being moved toward, and the gain applied, in dB.
    wanted_db: f32,
    gain_db: f32,
    fall: f32,
    rise: f32,
}

impl LoudnessNormalizer {
    fn clear(&mut self) {
        self.memory = [[Biquad::default(); 2]; 2];
        self.bucket_fill = 0;
        self.bucket_sum = 0.0;
        self.buckets = [0.0; BUCKETS];
        self.bucket_at = 0;
        self.filled = 0;
        self.wanted_db = 0.0;
        self.gain_db = 0.0;
    }

    /// Close a bucket and aim the gain at what the window now measures.
    fn measure(&mut self, target: f32, max_gain: f32) {
        self.buckets[self.bucket_at] = self.bucket_sum / self.bucket_len as f64;
        self.bucket_at = (self.bucket_at + 1) % BUCKETS;
        self.filled = (self.filled + 1).min(BUCKETS);
        self.bucket_sum = 0.0;
        self.bucket_fill = 0;
        if self.filled < FIRST_MEASURE {
            return;
        }
        let power = self.buckets.iter().sum::<f64>() / self.filled as f64;
        if power <= 0.0 {
            return;
        }
        let loudness = -0.691 + 10.0 * power.log10();
        if loudness < GATE_LUFS {
            return;
        }
        self.wanted_db =
            ((f64::from(target) - loudness) as f32).clamp(-MAX_CUT_DB, max_gain.max(0.0));
    }
}

impl AudioProcessor for LoudnessNormalizer {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let target = Knob::of(block, 0, -16.0);
        let max_gain = Knob::of(block, 1, 12.0);
        let (left, right) = block.channels();
        for frame in 0..left.len() {
            let mut power = 0.0;
            for (channel, sample) in [left[frame], right[frame]].into_iter().enumerate() {
                let [shelf, pass] = &mut self.memory[channel];
                let shaped = shelf.run(&self.weighting[0], f64::from(sample));
                let weighted = pass.run(&self.weighting[1], shaped);
                power += weighted * weighted;
            }
            self.bucket_sum += power;
            self.bucket_fill += 1;
            if self.bucket_fill == self.bucket_len {
                self.measure(target.at(frame), max_gain.at(frame));
            }
            let coef = if self.wanted_db < self.gain_db {
                self.fall
            } else {
                self.rise
            };
            self.gain_db = self.wanted_db + coef * (self.gain_db - self.wanted_db);
            if self.gain_db != 0.0 {
                let scale = db_to_gain(self.gain_db);
                left[frame] *= scale;
                right[frame] *= scale;
            }
        }
    }

    fn reset(&mut self) {
        self.clear();
    }
}

pub(super) fn make(rate: u32) -> Box<dyn AudioProcessor> {
    let mut made = LoudnessNormalizer {
        weighting: k_weighting(rate),
        memory: [[Biquad::default(); 2]; 2],
        bucket_len: (rate as usize / 10).max(1),
        bucket_fill: 0,
        bucket_sum: 0.0,
        buckets: [0.0; BUCKETS],
        bucket_at: 0,
        filled: 0,
        wanted_db: 0.0,
        gain_db: 0.0,
        fall: coefficient(500.0, rate),
        rise: coefficient(2000.0, rate),
    };
    made.clear();
    Box::new(made)
}
