//! A tone written by the sink the export writes with, so no fixture file is
//! checked in and decoding runs against a real container. Shared by the test
//! binaries that mix sound as a `#[path]` module.

use std::path::Path;

use aria_core::{AudioEncoderConfig, RationalTime};
use aria_models::Rational;

/// The export's sample rate and channel count.
pub(crate) const RATE: u32 = 48_000;
pub(crate) const CHANNELS: usize = 2;

/// Write `millis` of a `hz` sine at 0.8 peak, the same in both channels.
pub(crate) fn write_tone(path: &Path, millis: i64, hz: f32) {
    let mut sink =
        aria_encoder::open_audio_encoder(path, AudioEncoderConfig::new(RATE, CHANNELS as u16))
            .expect("a sound sink");
    let frames = millis * RATE as i64 / 1000;
    let mut block = vec![0.0f32; CHANNELS * RATE as usize / 100];
    let per_block = (block.len() / CHANNELS) as i64;
    let mut written = 0;
    while written < frames {
        let count = (frames - written).min(per_block);
        block.truncate(count as usize * CHANNELS);
        for frame in 0..count {
            let t = (written + frame) as f32 / RATE as f32;
            let sample = (std::f32::consts::TAU * hz * t).sin() * 0.8;
            for channel in 0..CHANNELS {
                block[frame as usize * CHANNELS + channel] = sample;
            }
        }
        sink.push_audio(
            &block,
            RationalTime::new(written, Rational::new(RATE as i32, 1)),
        )
        .expect("a block");
        written += count;
    }
    sink.finish().expect("finish the source");
}
