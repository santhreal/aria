//! Linux backend: GStreamer H.264 / H.265 + AAC → mp4 behind
//! [`aria_core::VideoEncoder`].
//!
//! Hardware first (`vaapi*`, `nv*`) then the software encoder (`x264enc` /
//! `x265enc`). Missing plugins return [`EncodeError::Unsupported`], naming the
//! package that carries them. Aria never ships a codec.

use std::path::Path;

use aria_core::{
    AudioEncoderConfig, EncodeError, EncoderConfig, PixelFormat, RationalTime, VideoCodec,
    VideoEncoder, VideoFrame,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use gstreamer_video as gst_video;

use crate::EncoderChoice;
use crate::yuv::{ChannelOrder, YuvMatrix, matrix_for, rgba_to_nv12_into, yuv420_len};

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
    config: EncoderConfig,
    choice: EncoderChoice,
    finished: bool,
}

/// # Safety
/// The pipeline and AppSrcs are used from a single export worker. The encoder
/// is not shared across threads.
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
        let video = gst_app::AppSrc::builder()
            .format(gst::Format::Time)
            .caps(&video_caps(width, height, config.frame_rate)?)
            .stream_type(gst_app::AppStreamType::Stream)
            .is_live(false)
            .block(true)
            .build();
        let convert = make("videoconvert")?;
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

        pipeline
            .add_many([
                video.upcast_ref(),
                &convert,
                encoder,
                &parse,
                &video_queue,
                &mux,
                &sink,
            ])
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        gst::Element::link_many([video.upcast_ref(), &convert, encoder, &parse, &video_queue])
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
        let nv12 = frame_to_nv12(frame, width, height, matrix_for(&self.config.color))?;
        let mut buffer = gst::Buffer::from_mut_slice(nv12);
        {
            let buffer = buffer
                .get_mut()
                .ok_or_else(|| EncodeError::Encode("couldn't write an encode buffer.".into()))?;
            let pts = gst::ClockTime::from_nseconds(rational_to_ns(frame.pts) as u64);
            buffer.set_pts(pts);
            buffer.set_dts(pts);
            if let Some(dur) = frame.duration {
                buffer.set_duration(gst::ClockTime::from_nseconds(rational_to_ns(dur) as u64));
            } else {
                let rate = self.config.frame_rate;
                if rate.num > 0 {
                    let ns = (NS_PER_SEC * i64::from(rate.den.max(1))) / i64::from(rate.num);
                    buffer.set_duration(gst::ClockTime::from_nseconds(ns as u64));
                }
            }
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
            buffer.set_pts(gst::ClockTime::from_nseconds(rational_to_ns(pts) as u64));
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
        self.finished = true;
        Ok(())
    }

    fn choice(&self) -> EncoderChoice {
        self.choice.clone()
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
    }
}

/// The encoder element for `codec`, built and opened: hardware first, software
/// last.
///
/// A registered factory is not a working encoder. NVENC stays in the registry
/// cache when the driver and its library disagree, and VAAPI registers without
/// a device to open, so each candidate is built and moved to `READY` (where a
/// video encoder opens its device). The first one that opens is returned still
/// open, so the pipeline it joins does not open the device a second time, with
/// why each earlier one did not; when none does, the error lists every
/// candidate's reason. A caller that does not keep the element sets it to
/// `NULL` before dropping it.
fn pick_encoder(codec: VideoCodec) -> Result<(EncoderChoice, gst::Element), EncodeError> {
    let hint = match codec {
        VideoCodec::H264 => "Install gstreamer1.0-plugins-ugly (x264enc) or a VAAPI/NVENC plugin.",
        VideoCodec::H265 => "Install gstreamer1.0-plugins-bad (x265enc) or a VAAPI/NVENC plugin.",
    };
    let (element, opened, passed_over) = first_opened(candidates(codec), open_encoder_element)
        .map_err(|reasons| {
            EncodeError::unsupported(format!(
                "no {} encoder opens here: {}. {hint}",
                codec.label(),
                reasons.join("; ")
            ))
        })?;
    let choice = EncoderChoice {
        encoder: element.into(),
        hardware: Some(!SOFTWARE_ENCODERS.contains(&element)),
        passed_over,
    };
    Ok((choice, opened))
}

/// The encoder elements for `codec`, in the order they are tried: NVENC, then
/// VAAPI, then software.
///
/// NVENC precedes VAAPI because a host with both is an NVIDIA card beside an
/// integrated GPU, and VAAPI drives the integrated encoder. Measured with
/// `gst-launch-1.0` on a Ryzen 9 9950X (integrated Radeon) with an RTX 5090,
/// 600 frames of 1080p60 NV12 less the 0.65 s the source takes alone, over two
/// runs: `nvh264enc` 2.00 to 2.06 s, `nvcudah264enc` 2.05 to 2.35 s,
/// `vaapih264enc` 4.58 to 5.15 s, `x264enc` 4.29 to 4.70 s.
fn candidates(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["nvh264enc", "nvcudah264enc", "vaapih264enc", "x264enc"],
        VideoCodec::H265 => &["nvh265enc", "nvcudah265enc", "vaapih265enc", "x265enc"],
    }
}

/// The software encoders in the candidate lists. Every other candidate runs on
/// a GPU or media engine.
const SOFTWARE_ENCODERS: &[&str] = &["x264enc", "x265enc"];

/// The first candidate `open` succeeds on, in order, with the reasons of the
/// ones before it, or every reason when none succeeds.
///
/// Candidates after the first success are not tried, so a working hardware
/// encoder never pays for probing the software one.
fn first_opened<T>(
    candidates: &[&'static str],
    mut open: impl FnMut(&'static str) -> Result<T, String>,
) -> Result<(&'static str, T, Vec<String>), Vec<String>> {
    let mut reasons = Vec::with_capacity(candidates.len());
    for &name in candidates {
        match open(name) {
            Ok(opened) => return Ok((name, opened, reasons)),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(reasons)
}

/// Build `name` and open it, returning it in `READY` state.
///
/// Closing `nvh264enc` takes 250 ms on an RTX 5090 and opening it again 75 ms,
/// so the element is kept open for the pipeline rather than closed and opened
/// again there. It opens on the device contexts the decoders keep, so an
/// export runs one CUDA context rather than one for the encoder beside the
/// decoders' one, and a decoder opened after it reuses the encoder's.
fn open_encoder_element(name: &'static str) -> Result<gst::Element, String> {
    if gst::ElementFactory::find(name).is_none() {
        return Err(format!("{name} is not installed"));
    }
    let element = gst::ElementFactory::make(name)
        .build()
        .map_err(|e| format!("{name} failed to load ({e})"))?;
    if !aria_decoder::open_on_kept_contexts(&element) {
        let _ = element.set_state(gst::State::Null);
        return Err(format!("{name} could not open its device"));
    }
    Ok(element)
}

/// Whether this host can encode `codec` at all: one of its elements opens.
///
/// The one owner of that answer: a picker offering a codec the pipeline would
/// then refuse is worse than a picker with one entry.
///
/// Initialises GStreamer first. A registry lookup on an uninitialised library
/// panics, and this is called from the export sheet, which may run in a process
/// that has not encoded anything yet.
pub(crate) fn can_encode(codec: VideoCodec) -> bool {
    ensure_init().is_ok()
        && pick_encoder(codec).is_ok_and(|(_, opened)| {
            let _ = opened.set_state(gst::State::Null);
            true
        })
}

/// Apply what the caller chose: the keyframe spacing and the bitrate.
///
/// Both are set through `find_property` rather than by element name, because
/// every encoder here spells the rate control differently and only the software
/// ones take the tuning knobs. A property an element does not have is left
/// alone rather than refused: the encode is still the codec that was asked for.
fn configure_encoder(encoder: &gst::Element, config: &EncoderConfig, name: &str) {
    if name == "x264enc" || name == "x265enc" {
        encoder.set_property_from_str("speed-preset", "veryfast");
    }
    if name == "x264enc" {
        encoder.set_property_from_str("tune", "zerolatency");
    }
    if let Some(interval) = config.keyframe_interval {
        // x264enc / x265enc take a frame count; the hardware elements do not
        // carry this property at all.
        if encoder.find_property("key-int-max").is_some() {
            encoder.set_property("key-int-max", interval);
        }
    }
    if let Some(kbps) = config.bitrate_kbps {
        set_bitrate(encoder, name, kbps);
    }
}

/// Write `kbps` into whichever rate property this element carries.
///
/// Every encoder here spells rate control differently: `x264enc` takes an
/// `i32` of kilobits, `x265enc` and the VAAPI elements a `u32`, and the NVENC
/// elements need their rate-control mode moved off constant-quality or the
/// bitrate is a cap they never approach. The property's own type is read rather
/// than guessed from the element name, so a plugin that changes it does not
/// turn into a panic.
fn set_bitrate(encoder: &gst::Element, name: &str, kbps: u32) {
    if name.starts_with("nv") && encoder.find_property("rc-mode").is_some() {
        encoder.set_property_from_str("rc-mode", "cbr");
    }
    let Some(spec) = encoder.find_property("bitrate") else {
        return;
    };
    if spec.value_type() == gst::glib::Type::U32 {
        encoder.set_property("bitrate", kbps);
    } else if spec.value_type() == gst::glib::Type::I32 {
        encoder.set_property("bitrate", kbps as i32);
    }
}

fn add_audio_branch(
    pipeline: &gst::Pipeline,
    mux: &gst::Element,
    audio: AudioEncoderConfig,
) -> Result<gst_app::AppSrc, EncodeError> {
    if gst::ElementFactory::find("avenc_aac").is_none()
        && gst::ElementFactory::find("voaacenc").is_none()
        && gst::ElementFactory::find("fdkaacenc").is_none()
    {
        return Err(EncodeError::unsupported(
            "no AAC encoder plugin found. Install gstreamer1.0-libav or gstreamer1.0-plugins-bad.",
        ));
    }
    let aac_name = ["fdkaacenc", "voaacenc", "avenc_aac"]
        .into_iter()
        .find(|n| gst::ElementFactory::find(n).is_some())
        .expect("checked above");

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

fn video_caps(
    width: u32,
    height: u32,
    rate: aria_core::Rational,
) -> Result<gst::Caps, EncodeError> {
    if rate.num <= 0 || rate.den <= 0 {
        return Err(EncodeError::unsupported(
            "encoder frame rate must be greater than zero.",
        ));
    }
    Ok(gst::Caps::builder("video/x-raw")
        .field("format", gst_video::VideoFormat::Nv12.to_str())
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(rate.num, rate.den))
        .build())
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

fn frame_to_nv12(
    frame: &VideoFrame,
    width: u32,
    height: u32,
    matrix: YuvMatrix,
) -> Result<Vec<u8>, EncodeError> {
    let cpu = frame.cpu().ok_or_else(|| {
        EncodeError::unsupported("Linux encoder accepts CPU frames only. Export from a CPU render.")
    })?;
    match frame.format {
        PixelFormat::Nv12 => packed_nv12(&cpu.planes, width, height),
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
            let plane = cpu
                .planes
                .first()
                .ok_or_else(|| EncodeError::Encode("RGB frame has no plane.".into()))?;
            let order = if frame.format == PixelFormat::Bgra8 {
                ChannelOrder::Bgra
            } else {
                ChannelOrder::Rgba
            };
            // GStreamer takes ownership of the buffer, so this one allocation
            // per frame is the handoff itself — the frame is no longer cloned
            // to re-order channels, and no I420 intermediate is built.
            let mut nv12 = vec![0u8; yuv420_len(width, height)];
            rgba_to_nv12_into(
                &plane.data,
                width,
                height,
                plane.stride,
                matrix,
                order,
                &mut nv12,
            )?;
            Ok(nv12)
        }
        other => Err(EncodeError::unsupported(format!(
            "Linux encoder cannot take {other:?} frames. Export RGBA or NV12."
        ))),
    }
}

fn packed_nv12(
    planes: &[aria_core::Plane],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, EncodeError> {
    if planes.len() < 2 {
        return Err(EncodeError::Encode("NV12 frame is missing a plane.".into()));
    }
    let y_stride = width as usize;
    let uv_stride = width as usize;
    let y_rows = height as usize;
    let uv_rows = height.div_ceil(2) as usize;
    let mut out = vec![0u8; y_stride * y_rows + uv_stride * uv_rows];
    copy_plane(&planes[0], &mut out[..y_stride * y_rows], y_stride, y_rows);
    copy_plane(
        &planes[1],
        &mut out[y_stride * y_rows..],
        uv_stride,
        uv_rows,
    );
    Ok(out)
}

fn copy_plane(plane: &aria_core::Plane, dest: &mut [u8], dest_stride: usize, rows: usize) {
    let copy_w = dest_stride.min(plane.stride);
    for row in 0..rows.min(plane.rows) {
        let src = &plane.data[row * plane.stride..row * plane.stride + copy_w];
        dest[row * dest_stride..row * dest_stride + copy_w].copy_from_slice(src);
    }
}

fn rational_to_ns(t: RationalTime) -> i64 {
    if t.rate.num <= 0 {
        return 0;
    }
    let den = i128::from(t.rate.den.max(1));
    let num = i128::from(t.rate.num);
    let ns = (i128::from(t.value) * den * i128::from(NS_PER_SEC)) / num;
    ns.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::Rational;

    #[test]
    fn one_second_is_onee9_ns() {
        assert_eq!(
            rational_to_ns(RationalTime::new(24, Rational::new(24, 1))),
            NS_PER_SEC
        );
    }

    #[test]
    fn one_hour_timestamp_converts_exactly() {
        let one_hour_ns = 3_600 * NS_PER_SEC;
        let t = RationalTime::new(one_hour_ns, Rational::new(1_000_000_000, 1));
        assert_eq!(rational_to_ns(t), one_hour_ns);

        let t_48k = RationalTime::new(3_600 * 48_000, Rational::new(48_000, 1));
        assert_eq!(rational_to_ns(t_48k), one_hour_ns);
    }

    /// The walk returns the first candidate that opens and stops there; a
    /// candidate that fails (a registered NVENC whose driver does not answer)
    /// is passed over rather than returned for the pipeline to fail on, and its
    /// reason is kept with the pick.
    #[test]
    fn the_first_candidate_that_opens_is_picked_and_later_ones_are_not_tried() {
        let mut tried = Vec::new();
        let picked = first_opened(&["hw-a", "hw-b", "sw", "never"], |name| {
            tried.push(name);
            if name == "sw" {
                Ok(7)
            } else {
                Err(format!("{name} could not open its device"))
            }
        });
        assert_eq!(
            picked,
            Ok((
                "sw",
                7,
                vec![
                    "hw-a could not open its device".to_string(),
                    "hw-b could not open its device".to_string(),
                ]
            ))
        );
        assert_eq!(tried, ["hw-a", "hw-b", "sw"]);
    }

    #[test]
    fn when_nothing_opens_every_reason_is_kept_in_order() {
        let picked = first_opened::<()>(&["hw", "sw"], |name| Err(format!("{name} failed")));
        assert_eq!(
            picked,
            Err(vec!["hw failed".to_string(), "sw failed".to_string()])
        );
    }

    /// The pick a host makes from each codec's real candidate list, with
    /// `opens` standing in for which of its encoders open. A hybrid host (an
    /// NVIDIA card beside an integrated GPU) opens both NVENC and VAAPI and
    /// must export through NVENC, the faster of the two; a host whose NVENC
    /// does not open takes VAAPI over software; software is the last resort.
    #[test]
    fn nvenc_is_preferred_then_vaapi_then_software() {
        fn pick(codec: VideoCodec, opens: impl Fn(&str) -> bool) -> &'static str {
            first_opened(candidates(codec), |name| {
                if opens(name) {
                    Ok(())
                } else {
                    Err(format!("{name} shut"))
                }
            })
            .expect("one candidate opens")
            .0
        }
        for codec in VideoCodec::ALL {
            let picked = pick(codec, |_| true);
            assert!(
                picked.starts_with("nv"),
                "{codec:?}: hybrid host picked {picked}"
            );
            let picked = pick(codec, |name| !name.starts_with("nv"));
            assert!(
                picked.starts_with("vaapi"),
                "{codec:?}: no NVENC picked {picked}"
            );
            let picked = pick(codec, |name| SOFTWARE_ENCODERS.contains(&name));
            assert_eq!(Some(&picked), candidates(codec).last(), "{codec:?}");
        }
    }

    /// Whatever the picker hands to the pipeline is open on this machine, for
    /// every codec. Before the picker opened its candidates it returned the
    /// first registered name, which on a host with a stale NVENC registration
    /// could not even be built; before it kept the winner open, the pipeline
    /// paid for closing the device and opening it again.
    #[test]
    fn the_picked_encoder_opens_on_this_machine() {
        ensure_init().expect("GStreamer initialises");
        for codec in [VideoCodec::H264, VideoCodec::H265] {
            let Ok((choice, element)) = pick_encoder(codec) else {
                continue;
            };
            let name = &choice.encoder;
            assert_eq!(
                element.current_state(),
                gst::State::Ready,
                "{name} was picked for {codec:?} but is not open"
            );
            let _ = element.set_state(gst::State::Null);
        }
    }

    /// WHY: the picked encoder opened alone, with no bus to ask for a device
    /// context on, so every export made a CUDA context of its own beside the
    /// decoders' one: about 68 ms of each NVENC open on an RTX 5090, against
    /// 5 ms on a kept context. Holds that once one encoder has opened, every
    /// later pick opens on one device context, the one the process kept. The
    /// second and third picks are compared: another test in this binary can
    /// open an encoder beside the first pick and have its context kept
    /// instead of the first pick's. Checks nothing on a host whose picked
    /// encoder holds no device context, such as a software encoder.
    #[test]
    fn encoders_picked_in_a_row_open_on_one_device_context() {
        ensure_init().expect("GStreamer initialises");
        for codec in [VideoCodec::H264, VideoCodec::H265] {
            let Ok((choice, first)) = pick_encoder(codec) else {
                continue;
            };
            let _ = first.set_state(gst::State::Null);
            let held = || {
                let (_, element) = pick_encoder(codec).expect("the encoder opens again");
                let contexts = element.contexts();
                let _ = element.set_state(gst::State::Null);
                contexts
            };
            let second = held();
            let third = held();
            for context in &second {
                assert!(
                    third
                        .iter()
                        .any(|again| again.structure() == context.structure()),
                    "{} opened on a new {} context",
                    choice.encoder,
                    context.context_type()
                );
            }
        }
    }
}
