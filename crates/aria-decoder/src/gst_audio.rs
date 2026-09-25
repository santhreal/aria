//! Linux audio decode: GStreamer `uridecodebin` → interleaved `f32`.
//!
//! `audioconvert` + `audioresample` do the mix/resample to the mixer's rate
//! and channel count, matching AVFoundation's LPCM request.

use std::path::Path;
use std::time::Duration;

use aria_core::{AudioReader, DecodeError, Rational, RationalTime};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;

use crate::gst::{ensure_init, gst_err, path_uri, rational_to_ns};

const PULL_TIMEOUT: Duration = Duration::from_secs(8);

/// An audio source decoded to interleaved `f32` at a fixed output rate/channels.
pub struct GstAudioReader {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    out_rate: u32,
    channels: usize,
    pending: Vec<f32>,
    pending_cursor: usize,
    position: Option<i64>,
    eos: bool,
}

// SAFETY: pipeline and sink are owned by one export/decode worker.
unsafe impl Send for GstAudioReader {}

impl GstAudioReader {
    pub fn open(path: &Path, out_rate: u32, channels: u16) -> Result<Self, DecodeError> {
        ensure_init()?;
        if out_rate == 0 || channels == 0 {
            return Err(DecodeError::unsupported(
                "audio output rate and channel count must be greater than zero.",
            ));
        }
        let uri = path_uri(path)?;
        let fmt = gst_audio::AUDIO_FORMAT_F32.to_str();
        let desc = format!(
            "uridecodebin uri=\"{uri}\" ! audioconvert ! audioresample ! audio/x-raw,format={fmt},channels={channels},rate={out_rate},layout=interleaved ! appsink name=out sync=false max-buffers=8 drop=false"
        );
        let pipeline = gst::parse::launch(&desc)
            .map_err(|e| {
                DecodeError::Open(format!(
                    "couldn't build the GStreamer audio pipeline: {e}. Install gstreamer1.0-plugins-base."
                ))
            })?
            .downcast::<gst::Pipeline>()
            .map_err(|_| {
                DecodeError::Open("GStreamer did not return an audio pipeline. Retry the install.".into())
            })?;
        let sink = pipeline
            .by_name("out")
            .ok_or_else(|| DecodeError::Open("audio pipeline is missing appsink.".into()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| DecodeError::Open("audio sink is not an appsink.".into()))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| gst_err(DecodeError::Open, e))?;
        let (state_res, ..) = pipeline.state(gst::ClockTime::from_seconds(8));
        if !matches!(
            state_res,
            Ok(gst::StateChangeSuccess::Success) | Ok(gst::StateChangeSuccess::NoPreroll)
        ) {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(DecodeError::Open(
                "GStreamer could not open an audio track. Install codec plugins or pick another file.".into(),
            ));
        }

        Ok(Self {
            pipeline,
            sink,
            out_rate,
            channels: usize::from(channels),
            pending: Vec::new(),
            pending_cursor: 0,
            position: Some(0),
            eos: false,
        })
    }

    fn fill_pending(&mut self) -> Result<bool, DecodeError> {
        if self.eos {
            return Ok(false);
        }
        match self
            .sink
            .try_pull_sample(gst::ClockTime::from_nseconds(PULL_TIMEOUT.as_nanos() as u64))
        {
            Some(sample) => {
                let buffer = sample.buffer().ok_or_else(|| {
                    DecodeError::Decode("GStreamer audio sample has no buffer.".into())
                })?;
                let map = buffer.map_readable().map_err(|_| {
                    DecodeError::Decode("couldn't map decoded audio. Try another file.".into())
                })?;
                let bytes = map.as_slice();
                if !bytes.len().is_multiple_of(4) {
                    return Err(DecodeError::Decode(
                        "decoded audio is not aligned f32. Try another file.".into(),
                    ));
                }
                self.pending.clear();
                self.pending.reserve(bytes.len() / 4);
                for chunk in bytes.chunks_exact(4) {
                    self.pending
                        .push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                }
                self.pending_cursor = 0;
                if self.position.is_none() {
                    if let Some(pts) = buffer.pts() {
                        let ns = pts.nseconds() as i64;
                        self.position =
                            Some(ns.saturating_mul(i64::from(self.out_rate)) / 1_000_000_000);
                    }
                }
                Ok(!self.pending.is_empty())
            }
            None if self.sink.is_eos() => {
                self.eos = true;
                Ok(false)
            }
            None => {
                if self.sink.is_eos() {
                    self.eos = true;
                    return Ok(false);
                }
                Err(DecodeError::Decode(
                    "GStreamer timed out waiting for audio. Try another file or reinstall codec plugins.".into(),
                ))
            }
        }
    }
}

impl AudioReader for GstAudioReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let channels = self.channels;
        debug_assert_eq!(out.len() % channels, 0, "buffer not a frame multiple");
        let want_frames = out.len() / channels;
        let mut produced = 0;

        while produced < want_frames {
            if self.pending_cursor >= self.pending.len() {
                if self.eos {
                    break;
                }
                if !self.fill_pending()? {
                    break;
                }
                continue;
            }
            let avail_frames = (self.pending.len() - self.pending_cursor) / channels;
            let take = (want_frames - produced).min(avail_frames);
            if take == 0 {
                self.pending_cursor = self.pending.len();
                continue;
            }
            let src = &self.pending[self.pending_cursor..self.pending_cursor + take * channels];
            out[produced * channels..(produced + take) * channels].copy_from_slice(src);
            self.pending_cursor += take * channels;
            produced += take;
        }

        if let Some(pos) = self.position.as_mut() {
            *pos += produced as i64;
        }
        Ok(produced)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        if self.position == Some(frame) {
            return Ok(());
        }
        let start = RationalTime::new(frame.max(0), Rational::new(self.out_rate as i32, 1));
        let ns = rational_to_ns(start);
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::ClockTime::from_nseconds(ns as u64),
            )
            .map_err(|_| {
                DecodeError::Seek(format!("GStreamer could not seek audio to sample {frame}."))
            })?;
        self.pending.clear();
        self.pending_cursor = 0;
        self.position = Some(frame.max(0));
        self.eos = false;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        self.position
    }
}

impl Drop for GstAudioReader {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
