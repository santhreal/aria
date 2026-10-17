//! Media decoders a native plugin registers with
//! [`Host::register_decoder`](crate::Host::register_decoder).
//!
//! Aria opens every file whose extension a registered decoder claims through
//! that decoder, for import, playback, export, and analysis, before the
//! platform decoder.

use std::path::Path;

use aria_plugin_abi::{
    ARIA_COLOR_BT601, ARIA_COLOR_BT709, ARIA_COLOR_BT2020_PQ, ARIA_COLOR_SRGB, ARIA_FORMAT_NV12,
    ARIA_FORMAT_YUV420P, AriaVideoInfo,
};
use serde::{Deserialize, Serialize};

use crate::Host;

/// What a decoder reads, registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderSpec {
    pub id: String,
    pub name: String,
    /// File extensions without the dot: ASCII letters and digits.
    pub extensions: Vec<String>,
    /// Among decoders claiming one extension, a higher priority is tried
    /// first.
    #[serde(default)]
    pub priority: i32,
}

/// What [`Decoder::probe`] reports about a file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderProbe {
    pub width: u32,
    pub height: u32,
    pub frame_rate_num: i32,
    pub frame_rate_den: i32,
    /// Length in whole frames at the frame rate, 0 when unknown.
    pub frame_count: i64,
    pub has_audio: bool,
    /// The file is a still image: one frame, no time base.
    pub is_image: bool,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

/// Pixel layout of the frames a [`VideoStream`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    /// A full-size Y plane, then one plane of interleaved U and V at half
    /// width and half height.
    Nv12,
    /// Y, U, and V planes; U and V at half width and half height.
    Yuv420p,
}

/// Color space of the frames a [`VideoStream`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameColor {
    Bt709,
    Bt601,
    Bt2020Pq,
    Srgb,
}

/// What [`Decoder::open_video`] reports about the stream it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    /// Frame width in pixels: even, at most 16384.
    pub width: u32,
    /// Frame height in pixels: even, at most 16384.
    pub height: u32,
    pub format: FrameFormat,
    pub color: FrameColor,
    pub frame_rate_num: i32,
    pub frame_rate_den: i32,
    /// Every pts the stream reports counts ticks at
    /// `time_base_num / time_base_den` ticks per second.
    pub time_base_num: i32,
    pub time_base_den: i32,
    /// Length in time-base ticks, when known.
    pub duration: Option<i64>,
}

impl VideoInfo {
    pub(crate) fn to_raw(self) -> AriaVideoInfo {
        AriaVideoInfo {
            width: self.width,
            height: self.height,
            format: match self.format {
                FrameFormat::Nv12 => ARIA_FORMAT_NV12,
                FrameFormat::Yuv420p => ARIA_FORMAT_YUV420P,
            },
            color: match self.color {
                FrameColor::Bt709 => ARIA_COLOR_BT709,
                FrameColor::Bt601 => ARIA_COLOR_BT601,
                FrameColor::Bt2020Pq => ARIA_COLOR_BT2020_PQ,
                FrameColor::Srgb => ARIA_COLOR_SRGB,
            },
            frame_rate_num: self.frame_rate_num,
            frame_rate_den: self.frame_rate_den,
            time_base_num: self.time_base_num,
            time_base_den: self.time_base_den,
            duration: self.duration.unwrap_or(-1),
        }
    }
}

/// One plane of the frame [`VideoStream::next_frame`] fills: `rows` rows of
/// `stride` bytes.
#[derive(Debug)]
pub struct FramePlane<'a> {
    pub data: &'a mut [u8],
    pub stride: usize,
    pub rows: usize,
}

/// Why a decoder call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeFailure {
    /// The decoder does not read this file. Aria tries the next decoder.
    Unsupported(String),
    /// The decoder reads this file and failed.
    Failed(String),
}

impl From<String> for DecodeFailure {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

/// An open video stream.
pub trait VideoStream: Send {
    /// Position the stream so the next frame is the sync frame at or before
    /// `pts`, in time-base ticks.
    fn seek(&mut self, pts: i64) -> Result<(), String>;

    /// Write the next frame into `planes`, in plane order, and return its
    /// pts, or `None` at end of stream.
    fn next_frame(&mut self, planes: &mut [FramePlane<'_>]) -> Result<Option<i64>, String>;
}

/// An open audio stream, resampled to the rate and channel count it was
/// opened with.
pub trait AudioStream: Send {
    /// Write interleaved samples into `out` and return how many samples it
    /// wrote: a multiple of the channel count, fewer than `out` holds only at
    /// end of stream.
    fn read(&mut self, out: &mut [f32]) -> Result<usize, String>;

    /// Position the next read at sample frame `frame`.
    fn seek(&mut self, frame: i64) -> Result<(), String>;
}

/// A media decoder registered with
/// [`Host::register_decoder`](crate::Host::register_decoder).
pub trait Decoder: Send + Sync + 'static {
    /// Describe `path`.
    fn probe(&self, host: &Host, path: &Path) -> Result<DecoderProbe, DecodeFailure>;

    /// Open the video stream of `path`.
    fn open_video(
        &self,
        host: &Host,
        path: &Path,
    ) -> Result<(VideoInfo, Box<dyn VideoStream>), DecodeFailure>;

    /// Open the audio of `path`, resampled to `sample_rate` and `channels`.
    fn open_audio(
        &self,
        _host: &Host,
        path: &Path,
        _sample_rate: u32,
        _channels: u16,
    ) -> Result<Box<dyn AudioStream>, DecodeFailure> {
        Err(DecodeFailure::Unsupported(format!(
            "{} has no audio this decoder reads",
            path.display()
        )))
    }
}
