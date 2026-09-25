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
        let enc_name = pick_encoder(config.video_codec)?;
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
        let encoder = gst::ElementFactory::make(enc_name.as_str())
            .build()
            .map_err(|e| {
                EncodeError::unsupported(format!(
                    "couldn't create {enc_name} ({e}). Install gstreamer1.0-plugins-ugly or a VAAPI/NVENC plugin."
                ))
            })?;
        configure_encoder(&encoder, &config, &enc_name);
        let parse = make(match config.video_codec {
            VideoCodec::H264 => "h264parse",
            VideoCodec::H265 => "h265parse",
        })?;
        let mux = make("mp4mux")?;
        let sink = gst::ElementFactory::make("filesink")
            .property("location", location.as_str())
            .property("sync", false)
            .build()
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        pipeline
            .add_many([video.upcast_ref(), &convert, &encoder, &parse, &mux, &sink])
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        gst::Element::link_many([video.upcast_ref(), &convert, &encoder, &parse])
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        parse
            .link(&mux)
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        mux.link(&sink)
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        let audio = if let Some(audio_cfg) = config.audio {
            Some(add_audio_branch(&pipeline, &mux, audio_cfg)?)
        } else {
            None
        };

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        Ok(Self {
            pipeline,
            video,
            audio,
            config,
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

/// The encoder element for `codec`: hardware first, software last.
fn pick_encoder(codec: VideoCodec) -> Result<String, EncodeError> {
    let (candidates, hint): (&[&str], &str) = match codec {
        VideoCodec::H264 => (
            &["vaapih264enc", "nvh264enc", "nvcudah264enc", "x264enc"],
            "Install gstreamer1.0-plugins-ugly (x264enc) or a VAAPI/NVENC plugin.",
        ),
        VideoCodec::H265 => (
            &["vaapih265enc", "nvh265enc", "nvcudah265enc", "x265enc"],
            "Install gstreamer1.0-plugins-bad (x265enc) or a VAAPI/NVENC plugin.",
        ),
    };
    for name in candidates {
        if gst::ElementFactory::find(name).is_some() {
            return Ok((*name).to_string());
        }
    }
    Err(EncodeError::unsupported(format!(
        "no {} encoder plugin found. {hint}",
        codec.label()
    )))
}

/// Whether this host can encode `codec` at all — one of its elements exists.
///
/// The one owner of that answer: a picker offering a codec the pipeline would
/// then refuse is worse than a picker with one entry.
///
/// Initialises GStreamer first. A registry lookup on an uninitialised library
/// panics, and this is called from the export sheet, which may run in a process
/// that has not encoded anything yet.
pub(crate) fn can_encode(codec: VideoCodec) -> bool {
    ensure_init().is_ok() && pick_encoder(codec).is_ok()
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

    pipeline
        .add_many([src.upcast_ref(), &convert, &resample, &encoder, &parse])
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    gst::Element::link_many([src.upcast_ref(), &convert, &resample, &encoder, &parse])
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    parse
        .link(mux)
        .map_err(|e| EncodeError::Start(e.to_string()))?;
    Ok(src)
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
}
