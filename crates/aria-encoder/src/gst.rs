//! Linux backend: GStreamer H.264 / H.265 + AAC → mp4 behind
//! [`aria_core::VideoEncoder`].
//!
//! Hardware first (`vaapi*`, `nv*`) then the software encoder (`x264enc` /
//! `x265enc`). Missing plugins return [`EncodeError::Unsupported`], naming the
//! package that carries them. Aria never ships a codec.

use std::path::Path;

use aria_core::{
    AudioEncoderConfig, EncodeError, EncoderConfig, RationalTime, VideoCodec, VideoEncoder,
    VideoFrame,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;

use crate::EncoderChoice;
use crate::yuv::matrix_for;

#[path = "gst_element.rs"]
mod element;
#[path = "gst_frame.rs"]
mod frame;

pub(crate) use element::{await_codec_probe, probe_codecs, probed_codecs};
use element::{configure_encoder, pick_encoder};
use frame::{FrameLayout, open_buffer_pool, video_caps, write_frame_to_nv12};

const NS_PER_SEC: i64 = 1_000_000_000;
const AAC_BIT_RATE: i32 = 128_000;

static GST_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_init() -> Result<(), EncodeError> {
    let mut err = None;
    GST_INIT.call_once(|| {
        if let Err(e) = gst::init() {
            err = Some(e);
        }
    });
    match err {
        Some(e) => Err(EncodeError::Start(format!(
            "GStreamer failed to initialize: {e}. Install gstreamer1.0 and retry."
        ))),
        None => Ok(()),
    }
}

/// An H.264 (+ optional AAC) → mp4 encoder backed by GStreamer.
pub struct GstEncoder {
    pipeline: gst::Pipeline,
    video: gst_app::AppSrc,
    audio: Option<gst_app::AppSrc>,
    buffer_pool: Option<gst::BufferPool>,
    layout: FrameLayout,
    config: EncoderConfig,
    choice: EncoderChoice,
    finished: bool,
}

/// # Safety
/// The pipeline and AppSrcs are used from a single export worker. The encoder
/// is not shared across threads.
#[allow(unsafe_code)]
unsafe impl Send for GstEncoder {}

impl std::fmt::Debug for GstEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GstEncoder")
            .field("size", &self.config.size)
            .field("codec", &self.config.video_codec)
            .field("element", &self.choice.encoder)
            .field("has_audio", &self.audio.is_some())
            .field("finished", &self.finished)
            .finish()
    }
}

impl GstEncoder {
    pub fn open(path: &Path, config: EncoderConfig) -> Result<Self, EncodeError> {
        ensure_init()?;
        let (width, height) = config.size;
        if width == 0 || height == 0 {
            return Err(EncodeError::unsupported(
                "encoder size must be greater than zero.",
            ));
        }
        let (choice, encoder) = pick_encoder(config.video_codec)?;
        match Self::assemble(path, config, choice, &encoder) {
            Ok(opened) => Ok(opened),
            Err(err) => {
                // The picked element is open (`READY`), and GStreamer will not
                // dispose of an element in any state but `NULL`.
                let _ = encoder.set_state(gst::State::Null);
                Err(err)
            }
        }
    }

    /// The encode pipeline around the opened `encoder`, playing.
    fn assemble(
        path: &Path,
        config: EncoderConfig,
        choice: EncoderChoice,
        encoder: &gst::Element,
    ) -> Result<Self, EncodeError> {
        let (width, height) = config.size;
        let enc_name: &str = &choice.encoder;
        let location = path.to_string_lossy().into_owned();

        let pipeline = gst::Pipeline::new();
        let v_caps = video_caps(width, height, config.frame_rate)?;
        let layout = FrameLayout::new(width, height)?;
        let frame_bytes = layout.buffer_bytes;
        let buffer_pool = open_buffer_pool(&v_caps, frame_bytes);
        let video = gst_app::AppSrc::builder()
            .format(gst::Format::Time)
            .caps(&v_caps)
            .stream_type(gst_app::AppStreamType::Stream)
            .is_live(false)
            .block(true)
            .max_bytes((frame_bytes as u64).saturating_mul(8))
            .build();
        configure_encoder(encoder, &config, enc_name);
        let parse = make(match config.video_codec {
            VideoCodec::H264 => "h264parse",
            VideoCodec::H265 => "h265parse",
        })?;
        let video_queue = mux_queue(VIDEO_QUEUE_NS)?;
        let mux = make("mp4mux")?;
        let sink = gst::ElementFactory::make("filesink")
            .property("location", location.as_str())
            .property("sync", false)
            .build()
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        // `x265enc` takes I420, Y42B or Y444 and no NV12; every other
        // candidate takes the NV12 `appsrc` caps as they are.
        let convert = if enc_name == "x265enc" {
            Some(make("videoconvert")?)
        } else {
            None
        };
        let mut chain: Vec<&gst::Element> = vec![video.upcast_ref()];
        chain.extend(convert.as_ref());
        chain.extend([encoder, &parse, &video_queue]);
        pipeline
            .add_many(chain.iter().copied().chain([&mux, &sink]))
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        gst::Element::link_many(chain.iter().copied())
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        video_queue
            .link(&mux)
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        mux.link(&sink)
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        let audio = if let Some(audio_cfg) = config.audio {
            Some(add_audio_branch(&pipeline, &mux, audio_cfg)?)
        } else {
            None
        };

        if let Err(err) = pipeline.set_state(gst::State::Playing) {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(EncodeError::Start(err.to_string()));
        }

        Ok(Self {
            pipeline,
            video,
            audio,
            buffer_pool,
            layout,
            config,
            choice,
            finished: false,
        })
    }
}

impl VideoEncoder for GstEncoder {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.finished {
            return Err(EncodeError::Encode(
                "encoder already finished. Open a new encode session.".into(),
            ));
        }
        let (width, height) = self.config.size;
        let mut buffer = self
            .buffer_pool
            .as_ref()
            .and_then(|pool| pool.acquire_buffer(None).ok())
            .or_else(|| gst::Buffer::with_size(self.layout.buffer_bytes).ok())
            .ok_or_else(|| EncodeError::Encode("couldn't allocate an encode buffer.".into()))?;
        {
            let buffer = buffer
                .get_mut()
                .ok_or_else(|| EncodeError::Encode("couldn't write an encode buffer.".into()))?;
            let pts = gst::ClockTime::from_nseconds(frame.pts.in_units(NS_PER_SEC) as u64);
            buffer.set_pts(pts);
            buffer.set_dts(pts);
            if let Some(dur) = frame.duration {
                buffer.set_duration(gst::ClockTime::from_nseconds(
                    dur.in_units(NS_PER_SEC) as u64
                ));
            } else if self.config.frame_rate.is_valid() {
                let period = RationalTime::new(1, self.config.frame_rate);
                buffer.set_duration(gst::ClockTime::from_nseconds(
                    period.in_units(NS_PER_SEC) as u64
                ));
            }
            let mut map = buffer
                .map_writable()
                .map_err(|_| EncodeError::Encode("couldn't map an encode buffer.".into()))?;
            write_frame_to_nv12(
                frame,
                width,
                height,
                matrix_for(&self.config.color),
                map.as_mut_slice(),
            )?;
            drop(map);
            self.layout.describe(buffer)?;
        }
        self.video
            .push_buffer(buffer)
            .map(|_| ())
            .map_err(|e| EncodeError::Encode(e.to_string()))
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let Some(audio) = self.audio.as_ref() else {
            return Err(EncodeError::unsupported(
                "this encoder has no audio track (config.audio was None)",
            ));
        };
        if self.finished {
            return Err(EncodeError::Encode(
                "encoder already finished. Open a new encode session.".into(),
            ));
        }
        let bytes = bytemuck::cast_slice::<f32, u8>(samples).to_vec();
        let mut buffer = gst::Buffer::from_mut_slice(bytes);
        {
            let buffer = buffer.get_mut().ok_or_else(|| {
                EncodeError::Encode("couldn't write an audio encode buffer.".into())
            })?;
            buffer.set_pts(gst::ClockTime::from_nseconds(
                pts.in_units(NS_PER_SEC) as u64
            ));
        }
        audio
            .push_buffer(buffer)
            .map(|_| ())
            .map_err(|e| EncodeError::Encode(e.to_string()))
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        if self.finished {
            return Ok(());
        }
        self.video
            .end_of_stream()
            .map_err(|e| EncodeError::Encode(e.to_string()))?;
        if let Some(audio) = &self.audio {
            audio
                .end_of_stream()
                .map_err(|e| EncodeError::Encode(e.to_string()))?;
        }
        let bus = self
            .pipeline
            .bus()
            .ok_or_else(|| EncodeError::Encode("GStreamer pipeline has no bus.".into()))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let poll_slice = gst::ClockTime::from_mseconds(250);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(EncodeError::Encode(
                    "GStreamer encode timed out waiting for EOS. Check disk space and retry."
                        .into(),
                ));
            }
            if let Some(msg) = bus.timed_pop(poll_slice) {
                match msg.view() {
                    gst::MessageView::Eos(_) => break,
                    gst::MessageView::Error(err) => {
                        return Err(EncodeError::Encode(format!(
                            "GStreamer encode failed: {}. Check the output path and codec plugins.",
                            err.error()
                        )));
                    }
                    _ => {}
                }
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(pool) = &self.buffer_pool {
            let _ = pool.set_active(false);
        }
        self.finished = true;
        Ok(())
    }

    fn choice(&self) -> EncoderChoice {
        self.choice.clone()
    }

    fn accepts_cpu_nv12(&self) -> bool {
        true
    }
}

impl Drop for GstEncoder {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.video.end_of_stream();
            if let Some(audio) = &self.audio {
                let _ = audio.end_of_stream();
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(pool) = &self.buffer_pool {
            let _ = pool.set_active(false);
        }
    }
}

fn add_audio_branch(
    pipeline: &gst::Pipeline,
    mux: &gst::Element,
    audio: AudioEncoderConfig,
) -> Result<gst_app::AppSrc, EncodeError> {
    let Some(aac_name) = ["fdkaacenc", "voaacenc", "avenc_aac"]
        .into_iter()
        .find(|n| gst::ElementFactory::find(n).is_some())
    else {
        return Err(EncodeError::unsupported(
            "no AAC encoder plugin found. Install gstreamer1.0-libav or gstreamer1.0-plugins-bad.",
        ));
    };

    let src = gst_app::AppSrc::builder()
        .format(gst::Format::Time)
        .caps(&audio_caps(audio)?)
        .stream_type(gst_app::AppStreamType::Stream)
        .is_live(false)
        .block(true)
        .build();
    let convert = make("audioconvert")?;
    let resample = make("audioresample")?;
    let encoder = gst::ElementFactory::make(aac_name)
        .build()
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    if encoder.find_property("bitrate").is_some() {
        encoder.set_property("bitrate", AAC_BIT_RATE);
    }
    let parse = make("aacparse")?;
    let queue = mux_queue(AUDIO_QUEUE_NS)?;

    pipeline
        .add_many([
            src.upcast_ref(),
            &convert,
            &resample,
            &encoder,
            &parse,
            &queue,
        ])
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    gst::Element::link_many([
        src.upcast_ref(),
        &convert,
        &resample,
        &encoder,
        &parse,
        &queue,
    ])
    .map_err(|e| EncodeError::Start(e.to_string()))?;
    queue
        .link(mux)
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    Ok(src)
}

/// How much coded video the video branch holds in front of the muxer: well
/// past the coder delay of AAC, which is a few thousand samples at any frame
/// rate.
const VIDEO_QUEUE_NS: u64 = 2 * NS_PER_SEC as u64;

/// How much coded audio the audio branch holds in front of the muxer: no
/// limit. The video coder delay is a count of frames (`x265enc` holds its
/// lookahead, 15 frames or more, before its first output), so at a low frame
/// rate it spans many seconds of audio, and any time limit here is a frame
/// rate below which the export wedges. Coded AAC at [`AAC_BIT_RATE`] is a few
/// kilobytes a second, so holding a whole delay costs little.
const AUDIO_QUEUE_NS: u64 = 0;

/// A `queue` between one coded branch and `mp4mux`, bounded by `max_time_ns`
/// of stream only (0 is no bound).
///
/// `mp4mux` takes a buffer from a pad only once every pad has one, and the
/// caller pushes video and audio from one thread with `block` set on both
/// sources. Without a queue the video branch stalls in the muxer waiting for
/// AAC, whose first frame needs more samples than one video frame carries, and
/// the caller stalls pushing the next video frame before it can push that
/// audio. Likewise the audio branch stalls in the muxer until the video coder
/// emits its first frame, and the caller stalls pushing audio before it can
/// push the frames that coder is waiting for. The queue gives each branch its
/// own thread and room for the coder delay of the other.
fn mux_queue(max_time_ns: u64) -> Result<gst::Element, EncodeError> {
    gst::ElementFactory::make("queue")
        .property("max-size-buffers", 0u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", max_time_ns)
        .build()
        .map_err(|e| {
            EncodeError::unsupported(format!(
                "GStreamer element queue is missing ({e}). Install gstreamer1.0-plugins-base."
            ))
        })
}

fn audio_caps(audio: AudioEncoderConfig) -> Result<gst::Caps, EncodeError> {
    Ok(gst::Caps::builder("audio/x-raw")
        .field("format", gst_audio::AUDIO_FORMAT_F32.to_str())
        .field("layout", "interleaved")
        .field("rate", audio.sample_rate as i32)
        .field("channels", i32::from(audio.channels))
        .build())
}

fn make(factory: &str) -> Result<gst::Element, EncodeError> {
    gst::ElementFactory::make(factory).build().map_err(|e| {
        EncodeError::unsupported(format!(
            "GStreamer element {factory} is missing ({e}). Install gstreamer1.0-plugins-base / -good."
        ))
    })
}
