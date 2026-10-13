//! Gain and the channel tools: swap, fill one side from the other, and mono.

use super::{Knob, db_to_gain};
use crate::audio_processor::{AudioBlock, AudioProcessor};

/// Scales both channels by slot 0, in decibels.
struct Gain;

impl AudioProcessor for Gain {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let gain = Knob::of(block, 0, 0.0);
        let (left, right) = block.channels();
        match gain.constant() {
            Some(db) => {
                let scale = db_to_gain(db);
                if scale != 1.0 {
                    for sample in left.iter_mut().chain(right.iter_mut()) {
                        *sample *= scale;
                    }
                }
            }
            None => {
                for (frame, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                    let scale = db_to_gain(gain.at(frame));
                    *left *= scale;
                    *right *= scale;
                }
            }
        }
    }

    fn reset(&mut self) {}
}

pub(super) fn gain(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Gain)
}

/// What a channel tool does to one frame.
#[derive(Debug, Clone, Copy)]
enum Route {
    Swap,
    FillLeft,
    FillRight,
    Mono,
}

struct Channels(Route);

impl AudioProcessor for Channels {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let (left, right) = block.channels();
        match self.0 {
            Route::Swap => left.swap_with_slice(right),
            Route::FillLeft => left.copy_from_slice(right),
            Route::FillRight => right.copy_from_slice(left),
            Route::Mono => {
                for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                    let mid = (*left + *right) * 0.5;
                    *left = mid;
                    *right = mid;
                }
            }
        }
    }

    fn reset(&mut self) {}
}

pub(super) fn swap(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Channels(Route::Swap))
}

pub(super) fn fill_left(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Channels(Route::FillLeft))
}

pub(super) fn fill_right(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Channels(Route::FillRight))
}

pub(super) fn mono(_rate: u32) -> Box<dyn AudioProcessor> {
    Box::new(Channels(Route::Mono))
}
