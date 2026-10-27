//! Linux media probe: one demux-and-parse pass that reads a file's streams at
//! their caps and plugs no decoder.
//!
//! The caps a demuxer and parser state for a stream carry the frame size,
//! frame rate, and codec the decoder later opens with, and the demuxer states
//! the duration. A probe that plugged a decoder also loaded its plugin: the
//! CUDA plugin takes about 1.5 s to load on an RTX 5090, and GStreamer loads
//! one plugin at a time, so that probe waited for the load, or for the decode
//! device warm-up ([`crate::gst_context::warm_up`]) loading it on its own
//! thread. Read at the caps, a video file probes in about 2 ms.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use aria_core::{DecodeError, SourceInfo, VideoDecoder};
use gstreamer as gst;
use gstreamer::prelude::*;

use crate::OutputMode;
use crate::gst::{PipelineGuard, bus_or, ensure_init, gst_err, path_uri};
use crate::probe::{MediaProbe, Tracks};

/// How long the pass may take to reach every stream before the probe fails.
const PROBE_WAIT: gst::ClockTime = gst::ClockTime::from_seconds(8);

/// The first video stream and the first audio stream the pass exposed, at the
/// caps they were exposed with.
#[derive(Default)]
struct Exposed {
    video: Option<gst::Caps>,
    audio: Option<gst::Caps>,
}

/// Probe `path` in one demux-and-parse pass.
pub(crate) fn probe(path: &Path) -> Result<MediaProbe, DecodeError> {
    ensure_init()?;
    let uri = path_uri(path)?;
    let pipeline = PipelineGuard::new(gst::Pipeline::new());
    let src = gst::ElementFactory::make("uridecodebin")
        .property("uri", uri.as_str())
        .build()
        .map_err(|e| {
            DecodeError::Open(format!(
                "couldn't build the GStreamer probe (uridecodebin: {e}). Install gstreamer1.0-plugins-base."
            ))
        })?;
    pipeline
        .add(&src)
        .map_err(|e| DecodeError::Open(format!("couldn't build the GStreamer probe: {e}")))?;
    crate::gst::plug_only(&src, plugs_before_decoding)?;
    let exposed = Arc::new(Mutex::new(Exposed::default()));
    let taken = Arc::clone(&exposed);
    src.connect_pad_added(move |src, pad| take_stream(&taken, src, pad));

    pipeline
        .set_state(gst::State::Paused)
        .map_err(|e| bus_or(&pipeline, gst_err(DecodeError::Open, e)))?;
    match pipeline.state(PROBE_WAIT).0 {
        Ok(gst::StateChangeSuccess::Success | gst::StateChangeSuccess::NoPreroll) => {}
        Ok(_) => {
            return Err(bus_or(
                &pipeline,
                DecodeError::Open(format!(
                    "GStreamer did not reach the streams of this file within {} s. Pick another file.",
                    PROBE_WAIT.seconds()
                )),
            ));
        }
        Err(e) => return Err(bus_or(&pipeline, gst_err(DecodeError::Open, e))),
    }
    let duration = crate::gst::stream_duration(&pipeline);
    let exposed = std::mem::take(&mut *exposed.lock().unwrap_or_else(PoisonError::into_inner));
    drop(pipeline);

    let audio_codec = exposed.audio.as_deref().and_then(codec_description);
    match (exposed.video, duration) {
        (Some(video), duration) => {
            let mut info = video_info(path, &video)?;
            info.duration = duration;
            Ok(MediaProbe::video(
                &info,
                Tracks {
                    has_audio: exposed.audio.is_some(),
                    video_codec: codec_description(&video),
                    audio_codec,
                },
            ))
        }
        (None, Some(duration)) if exposed.audio.is_some() => {
            Ok(MediaProbe::audio_only(duration, audio_codec))
        }
        (None, _) => Err(DecodeError::Open(
            "GStreamer found no video stream and no audio stream with a duration in this file. Pick another file.".into(),
        )),
    }
}

/// Whether the pass plugs `factory`: every element up to a decoder, which it
/// exposes the stream in front of.
fn plugs_before_decoding(_caps: &gst::CapsRef, factory: &gst::ElementFactory) -> bool {
    !factory
        .metadata(gst::ELEMENT_METADATA_KLASS)
        .is_some_and(|klass| klass.contains("Decoder"))
}

/// Keep `pad` when it is the first video or the first audio stream `src`
/// exposed, and link it to a sink of its own so the pass prerolls on its first
/// buffer. Any other stream stays unlinked.
fn take_stream(exposed: &Mutex<Exposed>, src: &gst::Element, pad: &gst::Pad) {
    let Some(caps) = pad.current_caps() else {
        return;
    };
    let mut exposed = exposed.lock().unwrap_or_else(PoisonError::into_inner);
    let slot = match caps.structure(0).map(|stream| stream.name()) {
        Some(name) if name.starts_with("video/") => &mut exposed.video,
        Some(name) if name.starts_with("audio/") => &mut exposed.audio,
        _ => return,
    };
    if slot.is_some() {
        return;
    }
    let Some(bin) = src.parent().and_downcast::<gst::Bin>() else {
        return;
    };
    let Ok(sink) = gst::ElementFactory::make("fakesink").build() else {
        return;
    };
    if bin.add(&sink).is_err() {
        return;
    }
    let _ = sink.sync_state_with_parent();
    if let Some(sink_pad) = sink.static_pad("sink")
        && pad.link(&sink_pad).is_ok()
    {
        *slot = Some(caps);
    }
}

/// The frame size and rate of the video stream exposed at `caps`. A stream
/// whose caps state no frame size is opened with its decoder, which reads one
/// from the first frame.
fn video_info(path: &Path, caps: &gst::CapsRef) -> Result<SourceInfo, DecodeError> {
    match stated_frame(caps) {
        Some(info) => Ok(info),
        None => Ok(crate::GstDecoder::open(path, OutputMode::Cpu)?
            .info()
            .clone()),
    }
}

/// The [`SourceInfo`] `caps` state: `None` without a positive width and
/// height.
fn stated_frame(caps: &gst::CapsRef) -> Option<SourceInfo> {
    let stream = caps.structure(0)?;
    let side = |field: &str| {
        stream
            .get::<i32>(field)
            .ok()
            .and_then(|side| u32::try_from(side).ok())
            .filter(|side| *side > 0)
    };
    let (width, height) = (side("width")?, side("height")?);
    let fps = stream.get::<gst::Fraction>("framerate").ok();
    Some(crate::gst::source_info(width, height, fps))
}

/// The description GStreamer gives a stream's caps, e.g. `H.264 (High
/// Profile)`. `None` when it has none, so an unknown codec is stated rather
/// than named wrongly.
fn codec_description(caps: &gst::CapsRef) -> Option<String> {
    let described = gstreamer_pbutils::functions::pb_utils_get_codec_description(caps).to_string();
    (!described.is_empty()).then_some(described)
}

#[cfg(test)]
#[path = "gst_probe_tests.rs"]
mod tests;
