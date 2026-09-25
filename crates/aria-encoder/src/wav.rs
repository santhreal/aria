//! The sound-only export sink: 16-bit PCM in a WAV container.
//!
//! Every platform video backend needs a frame size to open a file at all, so a
//! sound intermediate cannot be written through one. It is written here
//! instead, in pure Rust, which is also why a sound export works on a machine
//! whose video encoder is missing.
//!
//! PCM is the honest choice for an intermediate: it is what the mixer already
//! produced, so the file is the mix rather than a lossy pass over it, and every
//! decoder on every platform reads it (GStreamer `wavparse`, Media Foundation,
//! AVFoundation). The cost is size, which is the right trade for a file the
//! project made for itself and can delete.
//!
//! What it refuses:
//!
//! - **A block that does not start where the last one ended.** PCM carries no
//!   timestamps: a sample's position in the file *is* its time. A gap would be
//!   written as a shift, silently, for the rest of the file.
//! - **Passing the RIFF size limit.** The header counts bytes in a `u32`, so a
//!   file over 4 GiB cannot describe itself. A refusal names the limit; a
//!   truncated size field would produce a file that plays for four minutes and
//!   claims to be finished.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use aria_core::{AudioEncoder, AudioEncoderConfig, EncodeError, RationalTime};

/// Bytes per sample per channel: 16-bit PCM.
const BYTES_PER_SAMPLE: u64 = 2;

/// The fixed header this writer emits, before any samples.
const HEADER_BYTES: u64 = 44;

/// Largest `data` chunk that the RIFF header can describe. `riff_size` is
/// `HEADER_BYTES - 8 + data_bytes` and both size fields are `u32`.
const MAX_DATA_BYTES: u64 = u32::MAX as u64 - (HEADER_BYTES - 8);

/// A 16-bit PCM WAV writer behind [`AudioEncoder`].
///
/// Open with [`open_audio_encoder`](crate::open_audio_encoder). The header is
/// written up front with placeholder sizes and patched by
/// [`finish`](AudioEncoder::finish), so an unfinished file is visibly zero
/// length rather than plausible and wrong.
pub struct WavEncoder {
    file: BufWriter<File>,
    channels: u16,
    sample_rate: u32,
    data_bytes: u64,
    finished: bool,
}

impl std::fmt::Debug for WavEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WavEncoder")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("data_bytes", &self.data_bytes)
            .field("finished", &self.finished)
            .finish()
    }
}

impl WavEncoder {
    /// Create `path` and write the WAV header for `config`.
    pub fn create(path: &Path, config: AudioEncoderConfig) -> Result<Self, EncodeError> {
        if config.channels == 0 {
            return Err(EncodeError::unsupported("a WAV file needs a channel"));
        }
        if config.sample_rate == 0 {
            return Err(EncodeError::unsupported("a WAV file needs a sample rate"));
        }
        let file = File::create(path)
            .map_err(|e| EncodeError::Start(format!("{}: {e}", path.display())))?;
        let mut sink = Self {
            file: BufWriter::new(file),
            channels: config.channels,
            sample_rate: config.sample_rate,
            data_bytes: 0,
            finished: false,
        };
        sink.write_header()?;
        Ok(sink)
    }

    /// Bytes one frame of every channel occupies.
    fn block_align(&self) -> u64 {
        self.channels as u64 * BYTES_PER_SAMPLE
    }

    /// How many frames are in the file so far, which is also the sample time
    /// the next block must start at.
    fn frames_written(&self) -> i64 {
        (self.data_bytes / self.block_align()) as i64
    }

    fn write_header(&mut self) -> Result<(), EncodeError> {
        let byte_rate = self.sample_rate as u64 * self.block_align();
        let mut header = Vec::with_capacity(HEADER_BYTES as usize);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&riff_size(0).to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
        header.extend_from_slice(&self.channels.to_le_bytes());
        header.extend_from_slice(&self.sample_rate.to_le_bytes());
        header.extend_from_slice(&(byte_rate as u32).to_le_bytes());
        header.extend_from_slice(&(self.block_align() as u16).to_le_bytes());
        header.extend_from_slice(&((BYTES_PER_SAMPLE * 8) as u16).to_le_bytes());
        header.extend_from_slice(b"data");
        header.extend_from_slice(&0u32.to_le_bytes());
        debug_assert_eq!(header.len() as u64, HEADER_BYTES);
        self.write_all(&header)
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.file
            .write_all(bytes)
            .map_err(|e| EncodeError::Encode(e.to_string()))
    }

    /// Patch the two size fields the header could not know up front.
    fn patch_sizes(&mut self) -> Result<(), EncodeError> {
        let io = |e: std::io::Error| EncodeError::Encode(e.to_string());
        self.file.flush().map_err(io)?;
        self.file.seek(SeekFrom::Start(4)).map_err(io)?;
        self.file
            .write_all(&riff_size(self.data_bytes).to_le_bytes())
            .map_err(io)?;
        self.file.seek(SeekFrom::Start(40)).map_err(io)?;
        self.file
            .write_all(&(self.data_bytes as u32).to_le_bytes())
            .map_err(io)?;
        self.file.flush().map_err(io)?;
        self.file.get_ref().sync_all().map_err(io)?;
        Ok(())
    }
}

/// The `RIFF` chunk size for a `data` chunk of `data_bytes`.
fn riff_size(data_bytes: u64) -> u32 {
    (HEADER_BYTES - 8 + data_bytes) as u32
}

/// Where the `data` chunk would end after adding `added` bytes, or a refusal
/// naming the limit.
///
/// Its own function because the limit is the one thing about this writer that
/// cannot be exercised by writing a file.
fn checked_data_end(data_bytes: u64, added: u64) -> Result<u64, EncodeError> {
    let end = data_bytes.saturating_add(added);
    if end > MAX_DATA_BYTES {
        return Err(EncodeError::unsupported(format!(
            "a WAV file holds at most {MAX_DATA_BYTES} bytes of audio and this export reached {end}"
        )));
    }
    Ok(end)
}

/// One `f32` sample as 16-bit PCM.
///
/// `32767` rather than `32768` so full scale is representable and a `+1.0`
/// sample does not wrap to the most negative value.
fn pcm_sample(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

impl AudioEncoder for WavEncoder {
    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        if self.finished {
            return Err(EncodeError::Encode(
                "the WAV file is already finished".to_string(),
            ));
        }
        if samples.len() as u64 % self.channels as u64 != 0 {
            return Err(EncodeError::Encode(format!(
                "{} interleaved samples is not a whole number of {}-channel frames",
                samples.len(),
                self.channels
            )));
        }
        if pts.rate.num != self.sample_rate as i32 || pts.rate.den != 1 {
            return Err(EncodeError::Encode(format!(
                "a block timed at {}/{} cannot be placed in a file counting samples at {}",
                pts.rate.num, pts.rate.den, self.sample_rate
            )));
        }
        let expected = self.frames_written();
        if pts.value != expected {
            return Err(EncodeError::Encode(format!(
                "PCM has no timestamps, so a block must start where the last one ended: expected sample {expected}, got {}",
                pts.value
            )));
        }
        let added = samples.len() as u64 * BYTES_PER_SAMPLE;
        let end = checked_data_end(self.data_bytes, added)?;

        let mut bytes = Vec::with_capacity(added as usize);
        for sample in samples {
            bytes.extend_from_slice(&pcm_sample(*sample).to_le_bytes());
        }
        self.write_all(&bytes)?;
        self.data_bytes = end;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        if self.finished {
            return Err(EncodeError::Encode(
                "the WAV file is already finished".to_string(),
            ));
        }
        self.patch_sizes()?;
        self.finished = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::Rational;

    fn stereo() -> AudioEncoderConfig {
        AudioEncoderConfig::new(48_000, 2)
    }

    fn at(sample: i64) -> RationalTime {
        RationalTime::new(sample, Rational::new(48_000, 1))
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        // A flat file per test, like the encoder refusal probes above it: the
        // name is unique, so nothing has to make or sweep a directory.
        let path = std::env::temp_dir().join(format!("aria-wav-{name}"));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
    }

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("two bytes"))
    }

    #[test]
    fn the_header_describes_the_samples_that_followed() {
        let path = scratch("header.wav");
        let mut sink = WavEncoder::create(&path, stereo()).expect("create");
        sink.push_audio(&[0.0; 8], at(0)).expect("first block");
        sink.push_audio(&[0.0; 4], at(4)).expect("second block");
        sink.finish().expect("finish");

        let bytes = std::fs::read(&path).expect("read back");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u16_at(&bytes, 20), 1, "PCM");
        assert_eq!(u16_at(&bytes, 22), 2, "channels");
        assert_eq!(u32_at(&bytes, 24), 48_000, "sample rate");
        assert_eq!(u32_at(&bytes, 28), 48_000 * 4, "byte rate");
        assert_eq!(u16_at(&bytes, 32), 4, "block align");
        assert_eq!(u16_at(&bytes, 34), 16, "bit depth");
        // 12 samples of 16-bit PCM.
        assert_eq!(u32_at(&bytes, 40), 24, "data size");
        assert_eq!(u32_at(&bytes, 4), 36 + 24, "riff size");
        assert_eq!(bytes.len(), 44 + 24, "header plus samples");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_sample_is_written_at_full_scale_without_wrapping() {
        let path = scratch("scale.wav");
        let mut sink =
            WavEncoder::create(&path, AudioEncoderConfig::new(48_000, 1)).expect("create");
        sink.push_audio(&[1.0, -1.0, 0.0, 2.0, -2.0], at(0))
            .expect("block");
        sink.finish().expect("finish");

        let bytes = std::fs::read(&path).expect("read back");
        let samples: Vec<i16> = bytes[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(samples, vec![32767, -32767, 0, 32767, -32767]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_block_out_of_sequence_is_refused() {
        let path = scratch("gap.wav");
        let mut sink = WavEncoder::create(&path, stereo()).expect("create");
        sink.push_audio(&[0.0; 8], at(0)).expect("first block");
        let err = sink
            .push_audio(&[0.0; 8], at(9))
            .expect_err("a gap is not writable");
        assert!(
            err.to_string().contains("expected sample 4"),
            "the refusal names where the block had to start: {err}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_block_at_another_rate_is_refused() {
        let path = scratch("rate.wav");
        let mut sink = WavEncoder::create(&path, stereo()).expect("create");
        let err = sink
            .push_audio(&[0.0; 8], RationalTime::new(0, Rational::new(44_100, 1)))
            .expect_err("another rate is not writable");
        assert!(
            err.to_string().contains("44100/1"),
            "the refusal names the rate it was given: {err}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_partial_frame_is_refused() {
        let path = scratch("partial.wav");
        let mut sink = WavEncoder::create(&path, stereo()).expect("create");
        let err = sink
            .push_audio(&[0.0; 3], at(0))
            .expect_err("half a frame is not writable");
        assert!(
            err.to_string().contains("whole number"),
            "the refusal says what is wrong with the block: {err}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_riff_size_limit_is_refused_rather_than_truncated() {
        assert!(checked_data_end(0, 1024).is_ok());
        assert!(checked_data_end(MAX_DATA_BYTES - 4, 4).is_ok());
        let err = checked_data_end(MAX_DATA_BYTES - 4, 8).expect_err("over the limit");
        assert!(
            err.to_string().contains(&MAX_DATA_BYTES.to_string()),
            "the refusal names the limit: {err}"
        );
        // The size field can hold everything up to the limit, which is what
        // makes refusing at it correct rather than conservative.
        assert_eq!(riff_size(MAX_DATA_BYTES), u32::MAX);
    }

    #[test]
    fn finishing_twice_is_refused() {
        let path = scratch("twice.wav");
        let mut sink = WavEncoder::create(&path, stereo()).expect("create");
        sink.finish().expect("finish");
        assert!(sink.finish().is_err(), "the file is closed");
        assert!(
            sink.push_audio(&[0.0; 2], at(0)).is_err(),
            "a finished file takes no more audio"
        );
        std::fs::remove_file(&path).ok();
    }
}
