//! Media format encoders: the specification, the session a plugin opens per
//! export, and the frames and audio blocks the session receives.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Host;

/// Specification of a media format encoder registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderSpec {
    /// Not empty, and unique among encoders.
    pub id: String,
    /// The format as the Export dialog lists it, such as `Apple ProRes 422`.
    pub name: String,
    /// The extension of the file it writes, without the dot: ASCII letters and
    /// digits, stored lowercase. Selecting the encoder sets the destination's
    /// extension.
    pub extension: String,
    /// The encoder takes audio through [`EncoderSession::push_audio`]. When
    /// `false`, the Export dialog hides the Audio row and the export is video
    /// only.
    #[serde(default)]
    pub audio: bool,
}

/// Configuration passed when opening an [`EncoderSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frame rate numerator: `fps_num / fps_den` frames per second.
    pub fps_num: i64,
    /// Frame rate denominator.
    pub fps_den: i32,
    /// Requested video bitrate in kilobits per second, or 0 when the export
    /// requests none.
    pub video_bitrate_kbps: u32,
    /// Requested audio bitrate in kilobits per second. The host passes 0.
    pub audio_bitrate_kbps: u32,
}

/// One RGBA video frame handed to [`EncoderSession::push_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderFrame<'a> {
    /// Straight-alpha RGBA8 pixels: `height` rows of `stride` bytes, top row
    /// first.
    pub rgba: &'a [u8],
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Bytes per row, at least `width * 4`.
    pub stride: u32,
    /// Presentation time in ticks of `pts_rate`: the frame starts
    /// `pts_value / pts_rate` seconds into the file.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// One interleaved `f32` audio block handed to [`EncoderSession::push_audio`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderAudio<'a> {
    /// Interleaved samples, `channels` per sample frame.
    pub samples: &'a [f32],
    /// Sample frames per second.
    pub sample_rate: u32,
    /// Channels per sample frame.
    pub channels: u32,
    /// Presentation time of the first sample frame in ticks of `pts_rate`.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// Active media encoding session created by [`Encoder::open`] or [`Plugin::encoder_open`](crate::Plugin::encoder_open).
pub trait EncoderSession: Send {
    /// Encode one RGBA video frame.
    fn push_frame(&mut self, frame: &EncoderFrame<'_>) -> Result<(), String>;

    /// Encode one interleaved `f32` audio block.
    fn push_audio(&mut self, _audio: &EncoderAudio<'_>) -> Result<(), String> {
        Ok(())
    }

    /// Finalize the output container and flush all pending bytes to disk.
    fn finish(self: Box<Self>) -> Result<(), String>;
}

/// Media encoder factory registered with [`Host::register_encoder`](crate::Host::register_encoder).
pub trait Encoder: Send + Sync + 'static {
    /// Open an [`EncoderSession`] writing to `path` with `config`.
    fn open(
        &self,
        host: &Host,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn EncoderSession>, String>;
}

impl<F> Encoder for F
where
    F: Fn(&Host, &Path, &EncoderConfig) -> Result<Box<dyn EncoderSession>, String>
        + Send
        + Sync
        + 'static,
{
    fn open(
        &self,
        host: &Host,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn EncoderSession>, String> {
        self(host, path, config)
    }
}
