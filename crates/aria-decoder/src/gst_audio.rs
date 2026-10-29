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

use crate::gst::{
    NS_PER_SEC, PipelineGuard, bus_or, ensure_init, failed_change_or, gst_err, path_uri,
};
use crate::pending::{PendingSamples, PendingSource, read_frames};

const PULL_TIMEOUT: Duration = Duration::from_secs(8);

/// An audio source decoded to interleaved `f32` at a fixed output rate/channels.
pub struct GstAudioReader {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    out_rate: u32,
    channels: usize,
    pending: PendingSamples,
    position: Option<i64>,
    eos: bool,
}

// SAFETY: pipeline and sink are owned by one export/decode worker.
#[allow(unsafe_code)]
unsafe impl Send for GstAudioReader {}

impl std::fmt::Debug for GstAudioReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GstAudioReader")
            .field("out_rate", &self.out_rate)
            .field("channels", &self.channels)
            .field("position", &self.position)
            .field("eos", &self.eos)
            .finish_non_exhaustive()
    }
}

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
            "uridecodebin name=src uri=\"{uri}\" ! audioconvert ! audioresample ! audio/x-raw,format={fmt},channels={channels},rate={out_rate},layout=interleaved ! appsink name=out sync=false max-buffers=8 drop=false"
        );
        let pipeline = PipelineGuard::new(
            gst::parse::launch(&desc)
                .map_err(|e| {
                    DecodeError::Open(format!(
                        "couldn't build the GStreamer audio pipeline: {e}. Install gstreamer1.0-plugins-base."
                    ))
                })?
                .downcast::<gst::Pipeline>()
                .map_err(|_| {
                    DecodeError::Open(
                        "GStreamer did not return an audio pipeline. Retry the install.".into(),
                    )
                })?,
        );
        let src = pipeline
            .by_name("src")
            .ok_or_else(|| DecodeError::Open("audio pipeline is missing uridecodebin.".into()))?;
        // Every stream but the audio is exposed undecoded and links to
        // nothing, as a decoded video pad did, so reading a file's sound
        // creates no video parser or decoder: on a host with a CUDA decoder,
        // each one opened a GPU context of its own.
        crate::gst::plug_only(&src, plugs_for_audio)?;
        let sink = pipeline
            .by_name("out")
            .ok_or_else(|| DecodeError::Open("audio pipeline is missing appsink.".into()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| DecodeError::Open("audio sink is not an appsink.".into()))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| failed_change_or(&pipeline, gst_err(DecodeError::Open, e)))?;
        let not_opened = || {
            DecodeError::Open(
                "GStreamer could not open an audio track. Install codec plugins or pick another file.".into(),
            )
        };
        match pipeline.state(gst::ClockTime::from_seconds(8)).0 {
            Ok(gst::StateChangeSuccess::Success | gst::StateChangeSuccess::NoPreroll) => {}
            Ok(_) => return Err(bus_or(&pipeline, not_opened())),
            Err(_) => return Err(failed_change_or(&pipeline, not_opened())),
        }

        Ok(Self {
            pipeline: pipeline.take(),
            sink,
            out_rate,
            channels: usize::from(channels),
            pending: PendingSamples::default(),
            position: Some(0),
            eos: false,
        })
    }
}

impl PendingSource for GstAudioReader {
    fn pending(&mut self) -> &mut PendingSamples {
        &mut self.pending
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
                let samples = self.pending.refill();
                samples.extend(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
                let filled = !samples.is_empty();
                if self.position.is_none()
                    && let Some(ns) = crate::gst::stream_time_ns(&sample, buffer)
                {
                    self.position =
                        Some(ns.saturating_mul(i64::from(self.out_rate)) / 1_000_000_000);
                }
                Ok(filled)
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

/// Whether the audio pipeline plugs `factory` on a stream of `caps`: every
/// element on an audio stream, and the demuxer that splits the container the
/// audio is in.
fn plugs_for_audio(caps: &gst::CapsRef, factory: &gst::ElementFactory) -> bool {
    caps.structure(0)
        .is_some_and(|stream| stream.name().starts_with("audio/"))
        || factory
            .metadata(gst::ELEMENT_METADATA_KLASS)
            .is_some_and(|klass| klass.contains("Demuxer"))
}

impl AudioReader for GstAudioReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let produced = read_frames(self, out, self.channels)?;
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
        let ns = start.in_units(NS_PER_SEC);
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::ClockTime::from_nseconds(ns as u64),
            )
            .map_err(|_| {
                DecodeError::Seek(format!("GStreamer could not seek audio to sample {frame}."))
            })?;
        self.pending.clear();
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

#[cfg(test)]
#[path = "gst_audio_tests.rs"]
mod tests;
