//! Linux backend: GStreamer H.264 / H.265 + AAC → mp4 behind
//! [`aria_core::VideoEncoder`].
//!
//! Hardware first (`vaapi*`, `nv*`) then the software encoder (`x264enc` /
//! `x265enc`). Missing plugins return [`EncodeError::Unsupported`], naming the
//! package that carries them. Aria never ships a codec.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use aria_core::{EncodeError, EncoderConfig, RationalTime, VideoCodec, VideoEncoder, VideoFrame};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::EncoderChoice;
use crate::yuv::matrix_for;

#[path = "gst_audio.rs"]
mod audio;
#[path = "gst_element.rs"]
mod element;
#[path = "gst_frame.rs"]
mod frame;

use audio::AudioBranch;
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
    output_path: PathBuf,
    pipeline: gst::Pipeline,
    video: gst_app::AppSrc,
    audio: Option<AudioBranch>,
    buffer_pool: Option<gst::BufferPool>,
    layout: FrameLayout,
    config: EncoderConfig,
    choice: EncoderChoice,
    /// The first error the pipeline posted, set from the thread that posted it.
    failure: Arc<OnceLock<String>>,
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
    /// Candidate GStreamer AAC encoder elements in priority order.
    pub const AAC_ENCODERS: &[&str] = &["avenc_aac", "fdkaacenc", "voaacenc"];

    /// Whether the GStreamer registry on this host provides the named AAC encoder.
    pub fn has_aac_encoder(name: &str) -> bool {
        ensure_init().is_ok() && gst::ElementFactory::find(name).is_some()
    }

    /// Open the encoder forcing a specific AAC element from [`Self::AAC_ENCODERS`].
    pub fn open_with_aac_encoder(
        path: &Path,
        config: EncoderConfig,
        aac_encoder: &str,
    ) -> Result<Self, EncodeError> {
        Self::open_inner(path, config, Some(aac_encoder))
    }

    pub fn open(path: &Path, config: EncoderConfig) -> Result<Self, EncodeError> {
        Self::open_inner(path, config, None)
    }

    fn open_inner(
        path: &Path,
        config: EncoderConfig,
        aac_override: Option<&str>,
    ) -> Result<Self, EncodeError> {
        ensure_init()?;
        let (width, height) = config.size;
        if width == 0 || height == 0 {
            return Err(EncodeError::unsupported(
                "encoder size must be greater than zero.",
            ));
        }
        let frames = video_caps(width, height, config.frame_rate, matrix_for(&config.color))?;
        let (choice, encoder) = pick_encoder(config.video_codec, &frames)?;
        match Self::assemble(path, config, choice, &encoder, &frames, aac_override) {
            Ok(opened) => Ok(opened),
            Err(err) => {
                // The picked element is open (`READY`), and GStreamer will not
                // dispose of an element in any state but `NULL`.
                let _ = encoder.set_state(gst::State::Null);
                Err(err)
            }
        }
    }

    /// The encode pipeline around the opened `encoder`, playing, taking
    /// `v_caps` frames.
    fn assemble(
        path: &Path,
        config: EncoderConfig,
        choice: EncoderChoice,
        encoder: &gst::Element,
        v_caps: &gst::Caps,
        aac_override: Option<&str>,
    ) -> Result<Self, EncodeError> {
        let (width, height) = config.size;
        let enc_name: &str = &choice.encoder;
        let location = path.to_string_lossy().into_owned();

        let pipeline = gst::Pipeline::new();
        let layout = FrameLayout::new(width, height)?;
        let frame_bytes = layout.buffer_bytes;
        let buffer_pool = open_buffer_pool(v_caps, frame_bytes);
        let video = gst_app::AppSrc::builder()
            .format(gst::Format::Time)
            .caps(v_caps)
            .stream_type(gst_app::AppStreamType::Stream)
            .is_live(false)
            .block(true)
            .max_bytes((frame_bytes as u64).saturating_mul(8))
            .build();
        configure_encoder(encoder, &config, enc_name)?;
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
            Some(AudioBranch::add(&pipeline, &mux, audio_cfg, aac_override)?)
        } else {
            None
        };

        let failure = end_sources_on_error(
            &pipeline,
            [Some(&video), audio.as_ref().map(|a| &a.src)]
                .into_iter()
                .flatten(),
        );
        if let Err(err) = pipeline.set_state(gst::State::Playing) {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(EncodeError::Start(err.to_string()));
        }

        Ok(Self {
            output_path: path.to_path_buf(),
            pipeline,
            video,
            audio,
            buffer_pool,
            layout,
            config,
            choice,
            failure,
            finished: false,
        })
    }

    /// The error a push or end of stream that `flow` refused reports: the
    /// pipeline's own error when one stopped it, which is why the source
    /// refused.
    fn push_failed(&self, flow: gst::FlowError) -> EncodeError {
        EncodeError::Encode(
            self.failure
                .get()
                .cloned()
                .unwrap_or_else(|| format!("GStreamer refused the buffer ({flow:?}).")),
        )
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
            .map_err(|flow| self.push_failed(flow))
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        if self.finished {
            return Err(EncodeError::Encode(
                "encoder already finished. Open a new encode session.".into(),
            ));
        }
        let Some(audio) = self.audio.as_mut() else {
            return Err(EncodeError::unsupported(
                "this encoder has no audio track (config.audio was None)",
            ));
        };
        if let Err(flow) = audio.push_samples(samples, pts) {
            return Err(self.push_failed(flow));
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        if self.finished {
            return Ok(());
        }
        self.video
            .end_of_stream()
            .map_err(|flow| self.push_failed(flow))?;
        if let Some(audio) = self.audio.as_mut()
            && let Err(flow) = audio.finish_stream()
        {
            return Err(self.push_failed(flow));
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
                        return Err(EncodeError::Encode(encode_failure(err)));
                    }
                    _ => {}
                }
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(pool) = &self.buffer_pool {
            let _ = pool.set_active(false);
        }
        if let Some(audio) = &self.audio {
            audio.patch_mp4_edit_list(&self.output_path)?;
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
                let _ = audio.src.end_of_stream();
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(pool) = &self.buffer_pool {
            let _ = pool.set_active(false);
        }
    }
}

/// The error an encode that stopped on `err` reports.
fn encode_failure(err: &gst::message::Error) -> String {
    format!(
        "GStreamer encode failed: {}. Check the output path, the free disk space, and the codec plugins.",
        err.error().message().trim_end_matches('.')
    )
}

/// Record the first error `pipeline` posts and end each of `sources`, from the
/// thread that posts it. Returns where the error is recorded.
///
/// Each source blocks a push while it is full. A pipeline stopped by an error
/// (an encoder that refuses the frame size, a full disk) takes no more
/// buffers, so with nothing else to wake it the next push waits forever and
/// the error waits on the bus for a `finish` that never comes. An ended source
/// wakes the blocked push, which then fails with the recorded error. The
/// sources are held weakly, as the pipeline holds the bus that holds this
/// handler.
fn end_sources_on_error<'a>(
    pipeline: &gst::Pipeline,
    sources: impl IntoIterator<Item = &'a gst_app::AppSrc>,
) -> Arc<OnceLock<String>> {
    let failure = Arc::new(OnceLock::new());
    let Some(bus) = pipeline.bus() else {
        return failure;
    };
    let sources: Vec<_> = sources
        .into_iter()
        .map(|source| source.downgrade())
        .collect();
    let failed = Arc::clone(&failure);
    bus.set_sync_handler(move |_, message| {
        if let gst::MessageView::Error(err) = message.view() {
            let _ = failed.set(encode_failure(err));
            for source in sources.iter().filter_map(|source| source.upgrade()) {
                let _ = source.end_of_stream();
            }
        }
        gst::BusSyncReply::Pass
    });
    failure
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

fn make(factory: &str) -> Result<gst::Element, EncodeError> {
    gst::ElementFactory::make(factory).build().map_err(|e| {
        EncodeError::unsupported(format!(
            "GStreamer element {factory} is missing ({e}). Install gstreamer1.0-plugins-base / -good."
        ))
    })
}
