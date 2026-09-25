//! The audio decode/encode contract.
//!
//! The audio counterpart to [`crate::decode`] / [`crate::encode`]: the codec
//! call is platform-native (AVFoundation, MediaCodec, …) while *control* —
//! pull PCM, seek, mix, push PCM — stays in Rust behind these types.
//!
//! Samples are **interleaved `f32`** throughout: each frame is `channels`
//! consecutive samples, one buffer carrying `frames * channels` values. This
//! is the unit the export mixer sums in and the format every platform reader
//! resamples/downmixes to, so the pipeline never reasons about codec-native
//! layouts (planar, S16, surround) above the backend.

use crate::decode::DecodeError;

/// A pull-based audio decoder over a single source, delivering interleaved
/// `f32` at a fixed output rate and channel count chosen at open time.
///
/// Positions are expressed in **output sample frames** since the start of the
/// source (`frame / out_rate` seconds) — the unit the mixer does all its span
/// math in. `Send` so a reader can be owned by an export/decode worker thread.
pub trait AudioReader: Send {
    /// Fill `out` with up to `out.len() / channels` interleaved sample frames,
    /// advancing the position. Returns the number of **sample frames** written
    /// (`< requested` at end of stream). `out.len()` must be a multiple of the
    /// channel count.
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError>;

    /// Position the next [`read`](AudioReader::read) at output sample `frame`.
    /// A no-op when already there; small forward gaps may decode-and-discard
    /// rather than pay a container seek.
    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError>;

    /// Output-frame position of the next sample [`read`](AudioReader::read)
    /// will emit, or `None` before the first decode establishes the anchor.
    fn position(&self) -> Option<i64>;
}

/// Output configuration for the audio track of an encode session.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioEncoderConfig {
    /// Output sample rate in hertz (e.g. 48 000).
    pub sample_rate: u32,
    /// Output channel count (interleaved); 2 for stereo.
    pub channels: u16,
}

impl AudioEncoderConfig {
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// Validate sample rate and channel bounds.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.sample_rate == 0 {
            return Err("audio sample_rate must be non-zero");
        }
        if self.channels == 0 {
            return Err("audio channels must be non-zero");
        }
        if self.channels > 8 {
            return Err("audio channels must not exceed 8");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_encoder_config_construction_and_validation() {
        let stereo = AudioEncoderConfig::new(48_000, 2);
        assert_eq!(stereo.sample_rate, 48_000);
        assert_eq!(stereo.channels, 2);
        assert!(stereo.validate().is_ok());

        let invalid_rate = AudioEncoderConfig::new(0, 2);
        assert!(invalid_rate.validate().is_err());

        let invalid_ch = AudioEncoderConfig::new(44_100, 0);
        assert!(invalid_ch.validate().is_err());

        let excessive_ch = AudioEncoderConfig::new(48_000, 16);
        assert!(excessive_ch.validate().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn audio_encoder_config_serde_roundtrip() {
        let cfg = AudioEncoderConfig::new(48_000, 2);
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AudioEncoderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, parsed);
    }
}
