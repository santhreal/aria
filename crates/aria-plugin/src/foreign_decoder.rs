//! Decoders whose calls cross a plugin runtime boundary: the WASM component
//! and C ABI tiers. A runtime implements [`DecoderCalls`] with plain values;
//! [`ForeignDecoder`] checks each value and builds the frames, audio reads,
//! and roll-forward seeking of an [`ExternalDecoder`] from them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_core::{
    AudioReader, ColorSpace, CpuImage, DecodeError, FrameData, PixelFormat, Plane, Rational,
    RationalTime, Rect, Rotation, SourceInfo, VideoDecoder, VideoFrame, resample,
};
use aria_decoder::{
    ExternalDecoder, MediaProbe, SeekStats, frame_at_nearest_rolling, frame_at_rolling,
};

/// Largest frame width or height a plugin decoder may report.
pub const MAX_FOREIGN_FRAME_SIDE: u32 = 16_384;

/// What a runtime reports about a video stream it opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamInfo {
    /// Frame width in pixels: even, nonzero, at most [`MAX_FOREIGN_FRAME_SIDE`].
    pub width: u32,
    /// Frame height in pixels, on the terms of `width`.
    pub height: u32,
    /// [`PixelFormat::Nv12`] or [`PixelFormat::Yuv420p`].
    pub format: PixelFormat,
    pub color: ColorSpace,
    pub frame_rate: Rational,
    /// Ticks per second of every pts the stream reports.
    pub time_base: Rational,
    /// Length in `time_base` ticks, when known.
    pub duration: Option<i64>,
}

impl StreamInfo {
    /// Minimum bytes per row and rows of plane `index`.
    pub fn plane_shape(&self, index: usize) -> (usize, usize) {
        let (width, height) = (self.width as usize, self.height as usize);
        match (self.format, index) {
            (_, 0) => (width, height),
            (PixelFormat::Nv12, _) => (width, height / 2),
            _ => (width / 2, height / 2),
        }
    }

    /// Why the host cannot present this stream, if it cannot.
    fn refusal(&self) -> Option<String> {
        if !matches!(self.format, PixelFormat::Nv12 | PixelFormat::Yuv420p) {
            return Some(format!(
                "the stream is {:?}; a plugin decoder returns Nv12 or Yuv420p",
                self.format
            ));
        }
        let side = 1..=MAX_FOREIGN_FRAME_SIDE;
        if !side.contains(&self.width)
            || !side.contains(&self.height)
            || self.width % 2 != 0
            || self.height % 2 != 0
        {
            return Some(format!(
                "the frame size {}x{} is not even and between 2 and {MAX_FOREIGN_FRAME_SIDE}",
                self.width, self.height
            ));
        }
        for (what, rate) in [
            ("time base", self.time_base),
            ("frame rate", self.frame_rate),
        ] {
            if !rate.is_valid() {
                return Some(format!(
                    "the {what} {}/{} is not positive",
                    rate.num, rate.den
                ));
            }
        }
        None
    }

    fn source_info(&self) -> SourceInfo {
        let size = (self.width, self.height);
        SourceInfo {
            coded_size: size,
            display_size: size,
            rotation: Rotation::None,
            pixel_format: self.format,
            color: self.color,
            frame_rate: self.frame_rate,
            time_base: self.time_base,
            duration: self
                .duration
                .map(|ticks| RationalTime::new(ticks, self.time_base)),
        }
    }

    /// `decoded` as a frame of this stream, or why its planes do not fit.
    fn frame(&self, decoded: DecodedPlanes) -> Result<VideoFrame, String> {
        let want = self.format.plane_count();
        if decoded.planes.len() != want {
            return Err(format!(
                "a {:?} frame has {want} planes, the decoder returned {}",
                self.format,
                decoded.planes.len()
            ));
        }
        for (index, plane) in decoded.planes.iter().enumerate() {
            let (row_bytes, rows) = self.plane_shape(index);
            if plane.stride < row_bytes || plane.rows < rows || !plane.is_well_formed() {
                return Err(format!(
                    "plane {index} holds {} bytes at stride {} over {} rows; it needs a stride of at least {row_bytes} over {rows} rows",
                    plane.data.len(),
                    plane.stride,
                    plane.rows
                ));
            }
        }
        Ok(VideoFrame::new(
            RationalTime::new(decoded.pts, self.time_base),
            self.format,
            self.color,
            (self.width, self.height),
            Rect::new(0, 0, self.width, self.height),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(decoded.planes)),
        ))
    }
}

/// One decoded frame: its pts in time-base ticks and its planes in the
/// format's plane order.
#[derive(Debug)]
pub struct DecodedPlanes {
    pub pts: i64,
    pub planes: Vec<Plane>,
}

/// Why a decoder call across a runtime boundary failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The decoder does not read this file; the next decoder is tried.
    Unsupported(String),
    /// The call failed. Reported when no later decoder reads the file.
    Failed(String),
}

impl CallError {
    fn into_decode(self, failed: fn(String) -> DecodeError) -> DecodeError {
        match self {
            Self::Unsupported(reason) => DecodeError::Unsupported(reason),
            Self::Failed(reason) => failed(reason),
        }
    }
}

/// The calls a plugin runtime makes into one registered decoder. Every call
/// on an instance receives the path the instance was opened on.
pub trait DecoderCalls: Send + Sync + 'static {
    fn probe(&self, path: &Path) -> Result<MediaProbe, CallError>;

    /// Open the video stream of `path`: an instance and its stream.
    fn open_video(&self, path: &Path) -> Result<(u64, StreamInfo), CallError>;

    /// Seek to the sync frame at or before `pts`, in time-base ticks.
    fn seek(&self, path: &Path, instance: u64, pts: i64) -> Result<(), CallError>;

    /// The next frame, or `None` at end of stream. `stream` is what
    /// `open_video` returned, for sizing the planes.
    fn next_frame(
        &self,
        path: &Path,
        instance: u64,
        stream: &StreamInfo,
    ) -> Result<Option<DecodedPlanes>, CallError>;

    /// Open the audio of `path`, resampled to `sample_rate` and `channels`.
    fn open_audio(&self, path: &Path, sample_rate: u32, channels: u16) -> Result<u64, CallError>;

    /// Fill `out` with interleaved samples and return how many were written:
    /// whole frames, fewer than `out` holds only at end of stream.
    fn read_audio(&self, path: &Path, instance: u64, out: &mut [f32]) -> Result<usize, CallError>;

    /// Position the next read at output frame `frame`.
    fn seek_audio(&self, path: &Path, instance: u64, frame: i64) -> Result<(), CallError>;

    /// Drop an instance `open_video` or `open_audio` returned. Never panics.
    fn close(&self, path: &Path, instance: u64);
}

/// An [`ExternalDecoder`] over a runtime's [`DecoderCalls`].
pub struct ForeignDecoder<C: DecoderCalls> {
    calls: Arc<C>,
}

impl<C: DecoderCalls> std::fmt::Debug for ForeignDecoder<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForeignDecoder").finish_non_exhaustive()
    }
}

impl<C: DecoderCalls> ForeignDecoder<C> {
    pub fn new(calls: C) -> Self {
        Self {
            calls: Arc::new(calls),
        }
    }

    fn opened(&self, path: &Path, instance: u64) -> Opened<C> {
        Opened {
            calls: Arc::clone(&self.calls),
            path: path.to_path_buf(),
            instance,
        }
    }
}

impl<C: DecoderCalls> ExternalDecoder for ForeignDecoder<C> {
    fn probe(&self, path: &Path) -> Result<MediaProbe, DecodeError> {
        self.calls
            .probe(path)
            .map_err(|err| err.into_decode(DecodeError::Open))
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        let (instance, stream) = self
            .calls
            .open_video(path)
            .map_err(|err| err.into_decode(DecodeError::Open))?;
        // Dropped on refusal, which closes the instance.
        let opened = self.opened(path, instance);
        if let Some(reason) = stream.refusal() {
            return Err(DecodeError::Open(reason));
        }
        Ok(Box::new(ForeignVideo {
            info: stream.source_info(),
            stream,
            opened,
            last_pts: None,
            stats: SeekStats::default(),
        }))
    }

    fn open_audio(
        &self,
        path: &Path,
        out_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AudioReader>, DecodeError> {
        let instance = self
            .calls
            .open_audio(path, out_rate, channels)
            .map_err(|err| err.into_decode(DecodeError::Open))?;
        Ok(Box::new(ForeignAudio {
            opened: self.opened(path, instance),
            channels: usize::from(channels.max(1)),
            position: 0,
        }))
    }
}

/// An open instance, closed when dropped.
struct Opened<C: DecoderCalls> {
    calls: Arc<C>,
    path: PathBuf,
    instance: u64,
}

impl<C: DecoderCalls> Drop for Opened<C> {
    fn drop(&mut self) {
        self.calls.close(&self.path, self.instance);
    }
}

struct ForeignVideo<C: DecoderCalls> {
    opened: Opened<C>,
    stream: StreamInfo,
    info: SourceInfo,
    /// The pts of the last frame `next_frame` returned since the last seek.
    last_pts: Option<RationalTime>,
    stats: SeekStats,
}

impl<C: DecoderCalls> VideoDecoder for ForeignVideo<C> {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        self.last_pts = None;
        let pts = resample(target, self.stream.time_base).value;
        let Opened {
            calls,
            path,
            instance,
        } = &self.opened;
        calls
            .seek(path, *instance, pts)
            .map_err(|err| err.into_decode(DecodeError::Seek))
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let Opened {
            calls,
            path,
            instance,
        } = &self.opened;
        let decoded = calls
            .next_frame(path, *instance, &self.stream)
            .map_err(|err| err.into_decode(DecodeError::Decode))?;
        let Some(decoded) = decoded else {
            return Ok(None);
        };
        let frame = self.stream.frame(decoded).map_err(DecodeError::Decode)?;
        self.last_pts = Some(frame.pts);
        Ok(Some(frame))
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let (last_pts, mut stats) = (self.last_pts, self.stats);
        let result = frame_at_rolling(self, last_pts, &mut stats, target);
        self.stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let (last_pts, mut stats) = (self.last_pts, self.stats);
        let result = frame_at_nearest_rolling(self, last_pts, &mut stats, target);
        self.stats = stats;
        result
    }
}

struct ForeignAudio<C: DecoderCalls> {
    opened: Opened<C>,
    channels: usize,
    /// The output frame the next read starts at.
    position: i64,
}

impl<C: DecoderCalls> AudioReader for ForeignAudio<C> {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let Opened {
            calls,
            path,
            instance,
        } = &self.opened;
        let written = calls
            .read_audio(path, *instance, out)
            .map_err(|err| err.into_decode(DecodeError::Decode))?;
        if written > out.len() || written % self.channels != 0 {
            return Err(DecodeError::Decode(format!(
                "the decoder wrote {written} samples into a block of {}; samples come in whole frames of {} channels",
                out.len(),
                self.channels
            )));
        }
        let frames = written / self.channels;
        self.position += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        if frame == self.position {
            return Ok(());
        }
        let Opened {
            calls,
            path,
            instance,
        } = &self.opened;
        calls
            .seek_audio(path, *instance, frame)
            .map_err(|err| err.into_decode(DecodeError::Seek))?;
        self.position = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.position)
    }
}

#[cfg(test)]
#[path = "foreign_decoder_tests.rs"]
mod tests;
