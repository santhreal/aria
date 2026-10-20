//! Timeline → video export: composite every frame `0..duration` and push it to
//! a [`VideoEncoder`].
//!
//! [`export`] is the encoder-agnostic core (the "composite every frame → mux"
//! path the roadmap calls for): it renders each tick of the timeline, wraps the
//! composited [`RgbaImage`] as a packed-RGBA [`VideoFrame`], and feeds the
//! [`aria_core::VideoEncoder`] trait. The codec/container is whatever encoder
//! you pass. [`export_to_file`] is the batteries-included path: it opens the
//! platform-native encoder (H.264→mp4 on Apple) in a sibling staging directory
//! and atomically publishes a complete file, preserving any prior destination
//! on failure or cancellation. For a portable, codec-free output there is
//! [`PngSequenceEncoder`] below, which writes one PNG per frame (handy for
//! tests, previews, and CI).

use std::path::Path;

use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncodeError, EncoderChoice, EncoderConfig,
    ExportFormat, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect, RgbaImage, Rotation,
    VideoCodec, VideoEncoder, VideoFrame, resample,
};
use aria_models::Project;

use crate::error::{EmptyExport, ExportFormatError, MIN_EXPORT_DIM, RenderError};
use crate::export_audio::feed::AudioFeed;
use crate::export_audio::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, sample_boundary,
};
use crate::render::{PendingFrame, Renderer};
use crate::resolve::canvas_size;
use crate::transactional_file::TransactionalFile;
use aria_compositor::FrameSink;

/// Output shape of an export: pixel size, frame sampling rate, what part of the
/// timeline to render, and how the encoder is configured. The defaults
/// ([`ExportSettings::for_project`]) reproduce the project exactly; overrides
/// drive the pickers in the shells' export sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSettings {
    /// Output size in pixels. When it differs from the project canvas, each
    /// frame is uniformly scaled and centered (letterboxed on an aspect
    /// mismatch) — see [`Renderer::render_frame_sized`].
    pub size: (u32, u32),
    /// Frame sampling rate over the exported span.
    pub frame_rate: Rational,
    /// Maximum keyframe spacing in frames. `None` leaves the codec's default.
    pub keyframe_interval: Option<u32>,
    /// Whether the file carries an audio track. `false` writes video only, even
    /// when the project has audible audio.
    pub audio: bool,
    /// The span to render, as a half-open `[from, to)` on the timeline. `None`
    /// renders the whole timeline. The output always starts at zero: the span is
    /// what is rendered, not where the file begins.
    pub range: Option<(RationalTime, RationalTime)>,
    /// Video codec to write. Defaults to H.264, which every backend writes.
    pub video_codec: VideoCodec,
    /// Target average video bitrate in kilobits per second. `None` leaves the
    /// encoder's own quality-driven default.
    pub bitrate_kbps: Option<u32>,
}

/// Round both dimensions down to even, floor 2.
///
/// H.264 encoders reject an odd dimension, so every size that reaches an
/// encoder passes through here — the resolved settings on the way to a render,
/// and the export sheet's own size row, which has to name the size the file
/// will have rather than the one the scale produced.
pub fn even_export_size((w, h): (u32, u32)) -> (u32, u32) {
    let even = |v: u32| (v & !1).max(2);
    (even(w), even(h))
}

impl ExportSettings {
    /// The project's native output: resolved canvas size at the timeline rate,
    /// whole timeline, audio when the project has any.
    pub fn for_project(project: &Project) -> Self {
        Self {
            size: canvas_size(project),
            frame_rate: project.timeline().frame_rate,
            keyframe_interval: None,
            audio: true,
            range: None,
            video_codec: VideoCodec::default(),
            bitrate_kbps: None,
        }
    }

    /// The project's output with a chosen [`ExportFormat`] applied, or a
    /// refusal naming the choice this machine cannot honour.
    ///
    /// One owner of two rules that used to live in three places — the export
    /// sheet, the CLI's flag parsing, and nowhere at all on the command path:
    /// what an override means, and which overrides the installed encoder can
    /// actually write. A codec or a bitrate that would be quietly swapped for
    /// something that happens to work is refused instead, because a file that
    /// is not what was asked for has no way of announcing itself.
    pub fn resolved(project: &Project, format: ExportFormat) -> Result<Self, RenderError> {
        let capability = aria_encoder::capability();
        if !capability.available {
            return Err(ExportFormatError::NoEncoder.into());
        }
        if let Some(codec) = format.video_codec
            && !capability.codecs.contains(&codec)
        {
            return Err(ExportFormatError::Codec {
                asked: codec,
                backend: capability.backend,
                available: capability.codecs.iter().map(|c| c.label()).collect(),
            }
            .into());
        }
        if format.bitrate_kbps.is_some() && !capability.bitrate {
            return Err(ExportFormatError::Bitrate {
                backend: capability.backend,
            }
            .into());
        }
        Self::resolved_for_encoder(project, format, true)
    }

    /// Resolve `format` against `project` for a caller-supplied encoder (such as
    /// a registered plugin encoder) without checking the platform encoder's
    /// codec/bitrate capability table.
    ///
    /// When `encoder_supports_audio` is `false`, `audio` is forced to `false`.
    pub fn resolved_for_encoder(
        project: &Project,
        format: ExportFormat,
        encoder_supports_audio: bool,
    ) -> Result<Self, RenderError> {
        if let Some((w, h)) = format.size
            && (w < MIN_EXPORT_DIM || h < MIN_EXPORT_DIM)
        {
            return Err(ExportFormatError::Size { asked: (w, h) }.into());
        }
        if let Some(rate) = format.frame_rate
            && (rate.num <= 0 || rate.den <= 0)
        {
            return Err(ExportFormatError::Rate { asked: rate }.into());
        }
        let base = Self::for_project(project);
        if let Some((from, to)) = format.range {
            let rate = format.frame_rate.unwrap_or(base.frame_rate);
            let from_val = resample(from, rate).value;
            let to_val = resample(to, rate).value;
            if from_val > to_val {
                return Err(EmptyExport::Range.into());
            }
        }
        let settings = Self {
            size: format.size.unwrap_or(base.size),
            frame_rate: format.frame_rate.unwrap_or(base.frame_rate),
            keyframe_interval: format.keyframe_interval,
            audio: encoder_supports_audio && format.audio.unwrap_or(base.audio),
            range: format.range,
            video_codec: format.video_codec.unwrap_or(base.video_codec),
            bitrate_kbps: format.bitrate_kbps,
        }
        .evened();
        // An empty span is refused here rather than at the frame loop, so a
        // caller that asked for a range learns it covers nothing before a
        // staging directory exists.
        export_frame_count(project, settings)?;
        Ok(settings)
    }

    /// The same settings with both dimensions rounded down to even (min 2).
    pub fn evened(self) -> Self {
        Self {
            size: even_export_size(self.size),
            ..self
        }
    }

    /// Render only `[from, to)`.
    pub fn with_range(mut self, from: RationalTime, to: RationalTime) -> Self {
        self.range = Some((from, to));
        self
    }

    /// Cap keyframe spacing at `frames`.
    pub fn with_keyframe_interval(mut self, frames: u32) -> Self {
        self.keyframe_interval = Some(frames);
        self
    }

    /// Write `codec` instead of H.264.
    pub fn with_codec(mut self, codec: VideoCodec) -> Self {
        self.video_codec = codec;
        self
    }

    /// Target `kbps` kilobits per second of video.
    pub fn with_bitrate_kbps(mut self, kbps: u32) -> Self {
        self.bitrate_kbps = Some(kbps);
        self
    }

    /// Write a video-only file.
    pub fn without_audio(mut self) -> Self {
        self.audio = false;
        self
    }

    /// The half-open frame span this renders, at `self.frame_rate`, clamped to
    /// the project's duration.
    ///
    /// One owner of the arithmetic, because the frame loop, the progress total,
    /// and the encoder all have to agree on how many frames a range is.
    pub fn frame_span(&self, project: &Project) -> (i64, i64) {
        let rate = self.frame_rate;
        let duration = resample(project.timeline().duration(), rate).value.max(0);
        match self.range {
            None => (0, duration),
            Some((from, to)) => {
                let start = resample(from, rate).value.clamp(0, duration);
                let end = resample(to, rate).value.clamp(0, duration);
                (start, end.max(start))
            }
        }
    }
}

/// Per-frame export observer: called with `(frames_done, frames_total)` before
/// each frame is rendered and once more after the encoder is finalized.
/// Returning `false` cancels the export ([`RenderError::Cancelled`]).
/// [`export_to_file_observed`] discards its private staging output and leaves
/// the requested destination untouched. With [`export_observed`], the
/// caller-owned encoder's unfinalized output remains caller-managed.
pub type ExportObserver<'a> = &'a mut dyn FnMut(u64, u64) -> bool;

/// Opens a [`VideoEncoder`] writing `config` to `path`.
pub type EncoderOpener<'a> =
    &'a mut dyn FnMut(&Path, EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncodeError>;

/// How many frames an export will write, or a refusal naming why there are
/// none.
///
/// One owner of "is there anything to export". Three callers:
/// [`export_to_file_observed`] checks before it creates a staging directory or
/// opens an encoder, [`export_observed`] checks again because a caller-owned
/// encoder reaches the frame loop without passing through the first, and a
/// shell calls it as a pre-flight so an export sheet refuses in place instead
/// of spawning a job that dies on its first frame.
pub fn export_frame_count(project: &Project, settings: ExportSettings) -> Result<u64, RenderError> {
    let (start, end) = settings.frame_span(project);
    let frames = (end - start).max(0) as u64;
    if frames > 0 {
        return Ok(frames);
    }
    let duration = resample(project.timeline().duration(), settings.frame_rate)
        .value
        .max(0);
    Err(RenderError::Empty(if duration == 0 {
        EmptyExport::Sequence
    } else {
        EmptyExport::Range
    }))
}

/// Composite the whole timeline and encode it to a video file at `path`, using
/// the platform-native encoder (H.264/mp4 on Apple). Returns the frame count.
///
/// Convenience over [`export`]: builds the right [`EncoderConfig`] from the
/// project and opens the native encoder for you. Publication is transactional:
/// encoding happens in a unique sibling staging directory, then a complete,
/// synchronized file atomically replaces `path`. Cancellation or failure leaves
/// an existing destination byte-for-byte unchanged (or an absent destination
/// absent). On platforms without a native encoder this returns
/// [`RenderError::Encode`].
pub fn export_to_file(
    renderer: &mut Renderer,
    project: &Project,
    path: &Path,
) -> Result<u64, RenderError> {
    export_to_file_observed(
        renderer,
        project,
        path,
        ExportSettings::for_project(project),
        &mut |_, _| true,
    )
}

/// [`export_to_file`] with explicit output settings and a progress/cancel
/// observer — the export-job path behind the shells' export sheets. It has the
/// same transactional destination semantics as [`export_to_file`].
pub fn export_to_file_observed(
    renderer: &mut Renderer,
    project: &Project,
    path: &Path,
    settings: ExportSettings,
    observer: ExportObserver<'_>,
) -> Result<u64, RenderError> {
    export_to_file_with_encoder_observed(
        renderer,
        project,
        path,
        settings,
        &mut aria_encoder::open_encoder,
        observer,
    )
    .map(|exported| exported.frames)
}

/// A finished file export: the frames written and the encoder that wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub frames: u64,
    pub encoder: EncoderChoice,
}

/// [`export_to_file_observed`] with a caller-supplied encoder opener (such as a
/// registered plugin encoder). Returns the frame count and the encoder the
/// opener opened.
///
/// Encoding writes to the same sibling `TransactionalFile` staging directory
/// as [`export_to_file_observed`], and atomically replaces `path` only after
/// the encoder finishes cleanly. Cancellation or failure drops the encoder and
/// discards the staging directory, leaving `path` untouched.
pub fn export_to_file_with_encoder_observed(
    renderer: &mut Renderer,
    project: &Project,
    path: &Path,
    settings: ExportSettings,
    open_encoder: EncoderOpener<'_>,
    observer: ExportObserver<'_>,
) -> Result<Exported, RenderError> {
    let settings = settings.evened();
    export_frame_count(project, settings)?;
    let staging = TransactionalFile::new(path)?;
    let mut encoder = open_encoder(
        staging.staging_path(),
        export_config_with(project, settings),
    )?;
    let choice = encoder.choice();
    let frames = export_observed(renderer, project, encoder.as_mut(), settings, observer)?;
    drop(encoder);
    staging.publish()?;
    Ok(Exported {
        frames,
        encoder: choice,
    })
}

/// Composite every frame of `project` and push it to `encoder`, returning the
/// number of frames written.
///
/// Frames are rendered at the timeline frame rate over `0..duration`. When the
/// project has audible audio *and* the encoder has an audio track, the mixed
/// stereo audio for each frame's wall-clock span is pushed right after the
/// video frame (running sample cursor, so non-integer NTSC rates stay in sync).
/// Encoders without an audio track (e.g. [`PngSequenceEncoder`]) report
/// [`EncodeError::Unsupported`] from the first `push_audio`, after which audio
/// is skipped for the rest of the export. The encoder is finalized before
/// returning.
pub fn export(
    renderer: &mut Renderer,
    project: &Project,
    encoder: &mut dyn VideoEncoder,
) -> Result<u64, RenderError> {
    export_observed(
        renderer,
        project,
        encoder,
        ExportSettings::for_project(project),
        &mut |_, _| true,
    )
}

/// [`export`] with explicit output settings and a progress/cancel observer.
///
/// The observer is called with `(frames_done, frames_total)` before each frame
/// (so `(0, total)` announces the length up front) and once more with
/// `(total, total)` after the encoder is finalized. Returning `false` stops
/// the export with [`RenderError::Cancelled`] *without* finalizing the
/// encoder. Because this generic API does not own the encoder or its output,
/// cancellation cleanup remains the caller's responsibility. The transactional
/// staging guarantees of [`export_to_file_observed`] do not apply here.
pub fn export_observed(
    renderer: &mut Renderer,
    project: &Project,
    encoder: &mut dyn VideoEncoder,
    settings: ExportSettings,
    observer: ExportObserver<'_>,
) -> Result<u64, RenderError> {
    let rate = settings.frame_rate;
    let frames = export_frame_count(project, settings)?;
    let (start, _) = settings.frame_span(project);
    let native = settings.size == canvas_size(project);

    // Export always applies per-clip transform motion blur when enabled.
    // Interactive preview leaves the flag off for scrub performance.
    let prev_blur = renderer.apply_motion_blur;
    renderer.set_apply_motion_blur(true);
    let result = export_observed_inner(
        renderer, project, encoder, settings, observer, start, frames, native, rate,
    );
    renderer.set_apply_motion_blur(prev_blur);
    result
}

#[allow(clippy::too_many_arguments)]
fn export_observed_inner(
    renderer: &mut Renderer,
    project: &Project,
    encoder: &mut dyn VideoEncoder,
    settings: ExportSettings,
    observer: ExportObserver<'_>,
    // First timeline frame to render, at `rate`. A ranged export renders from
    // here but writes presentation times from zero, so the file plays from its
    // own start rather than carrying a timeline offset no player would honour.
    start: i64,
    frames: u64,
    native: bool,
    rate: Rational,
) -> Result<u64, RenderError> {
    let mixer = if settings.audio {
        ExportAudioMixer::for_project(project)?
    } else {
        None
    };
    let timing = FrameTiming {
        start,
        rate,
        audio_rate: Rational::new(EXPORT_AUDIO_RATE as i32, 1),
    };
    let output_size = (!native).then_some(settings.size);
    std::thread::scope(|scope| {
        let mut audio = match mixer {
            Some(mixer) => Some(AudioFeed::start(scope, mixer, start, frames, rate)?),
            None => None,
        };
        let mut pending: Option<(PendingFrame, u64)> = None;
        let mut frame_sink = ExportFrameSink::new();
        for i in 0..frames {
            if !observer(i, frames) {
                if let Some((ready, ready_index)) = pending.take() {
                    finish_export_frame(
                        renderer,
                        ready,
                        ready_index,
                        timing,
                        encoder,
                        &mut audio,
                        &mut frame_sink,
                    )?;
                }
                return Err(RenderError::Cancelled);
            }
            let source = RationalTime::new(start + i as i64, rate);
            let next = renderer.submit_export_frame(project, source, output_size)?;
            if let Some((ready, ready_index)) = pending.replace((next, i)) {
                finish_export_frame(
                    renderer,
                    ready,
                    ready_index,
                    timing,
                    encoder,
                    &mut audio,
                    &mut frame_sink,
                )?;
            }
        }
        if let Some((ready, ready_index)) = pending {
            finish_export_frame(
                renderer,
                ready,
                ready_index,
                timing,
                encoder,
                &mut audio,
                &mut frame_sink,
            )?;
        }
        encoder.finish()?;
        observer(frames, frames);
        Ok(frames)
    })
}

/// Where an exported frame sits: the first timeline frame rendered, the
/// timeline rate, and the audio sample rate.
///
/// A ranged export renders from `start` but writes presentation times from
/// zero, so the file plays from its own start rather than carrying a timeline
/// offset no player would honour.
#[derive(Clone, Copy)]
struct FrameTiming {
    start: i64,
    rate: Rational,
    audio_rate: Rational,
}

/// Reads back frame `index`, encodes it, and encodes its mixed audio when the
/// export has any.
///
/// An encoder without an audio track reports [`EncodeError::Unsupported`] from
/// its first `push_audio`; the audio feed is then dropped, which ends its mix.
fn finish_export_frame(
    renderer: &mut Renderer,
    pending: PendingFrame,
    index: u64,
    timing: FrameTiming,
    encoder: &mut dyn VideoEncoder,
    audio: &mut Option<AudioFeed>,
    frame_sink: &mut ExportFrameSink,
) -> Result<(), RenderError> {
    let FrameTiming {
        start,
        rate,
        audio_rate,
    } = timing;
    renderer.finish_pending_into(pending, frame_sink)?;
    let out = RationalTime::new(index as i64, rate);
    encoder.push(frame_sink.frame_with_pts(out))?;
    let Some(feed) = audio.as_ref() else {
        return Ok(());
    };
    let block = feed.next()?;
    let from = sample_boundary(start + index as i64, rate.num, rate.den);
    let offset = sample_boundary(start, rate.num, rate.den);
    let pts = RationalTime::new(from - offset, audio_rate);
    match encoder.push_audio(&block, pts) {
        Ok(()) => feed.recycle(block),
        Err(EncodeError::Unsupported(_)) => *audio = None,
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// The [`EncoderConfig`] an exporter should use for `project`: the resolved
/// canvas size, the timeline frame rate, and BT.709 colorimetry. When the
/// project carries audible audio, a stereo 48 kHz AAC track is configured too.
/// Construct a native encoder with this, then drive it via [`export`].
pub fn export_config(project: &Project) -> EncoderConfig {
    export_config_with(project, ExportSettings::for_project(project))
}

/// [`export_config`] for explicit output settings (resolution, fps, keyframe
/// spacing, audio). Pass the same settings to [`export_observed`] so rendered
/// frames match the encoder.
pub fn export_config_with(project: &Project, settings: ExportSettings) -> EncoderConfig {
    let mut config = EncoderConfig::new(settings.size, settings.frame_rate, ColorSpace::BT709);
    // Only whether anything is audible. A project whose audio routing is
    // cyclic has no mix at all, and says so from `export_observed`, which is
    // the call that can report it; the config it would have carried is moot.
    if settings.audio && ExportAudioMixer::for_project(project).is_ok_and(|mix| mix.is_some()) {
        config = config.with_audio(AudioEncoderConfig::new(
            EXPORT_AUDIO_RATE,
            EXPORT_AUDIO_CHANNELS,
        ));
    }
    if let Some(frames) = settings.keyframe_interval {
        config = config.with_keyframe_interval(frames);
    }
    config = config.with_codec(settings.video_codec);
    if let Some(kbps) = settings.bitrate_kbps {
        config = config.with_bitrate_kbps(kbps);
    }
    config
}

/// Wrap a composited [`RgbaImage`] as a single-plane packed-RGBA CPU frame.
fn rgba_to_frame(image: RgbaImage, pts: RationalTime) -> VideoFrame {
    let (width, height) = (image.width, image.height);
    let plane = Plane::new(image.pixels, width as usize * 4, height as usize);
    VideoFrame::new(
        pts,
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (width, height),
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![plane])),
    )
}

/// Reusable [`FrameSink`] that keeps a single packed-RGBA [`VideoFrame`] plane
/// allocation across an entire export instead of allocating and zero-filling a
/// fresh `width * height * 4` buffer on every exported frame.
struct ExportFrameSink {
    frame: VideoFrame,
}

impl ExportFrameSink {
    fn new() -> Self {
        Self {
            frame: rgba_to_frame(
                RgbaImage::new(0, 0, Vec::new()),
                RationalTime::new(0, Rational::new(1, 1)),
            ),
        }
    }

    fn frame_with_pts(&mut self, pts: RationalTime) -> &VideoFrame {
        self.frame.pts = pts;
        &self.frame
    }
}

impl FrameSink for ExportFrameSink {
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        let row_bytes = (width as usize) * 4;
        let rows = height as usize;
        let needed = row_bytes * rows;
        if (self.frame.width(), self.frame.height()) != (width, height) {
            self.frame = rgba_to_frame(
                RgbaImage::new(width, height, vec![0u8; needed]),
                RationalTime::new(0, Rational::new(1, 1)),
            );
        }
        let FrameData::Cpu(cpu) = &mut self.frame.data else {
            unreachable!("ExportFrameSink always holds a CPU RGBA frame");
        };
        let plane = &mut cpu.planes[0];
        if plane.data.len() != needed {
            plane.data.resize(needed, 0);
        }
        plane.stride = row_bytes;
        plane.rows = rows;
        &mut plane.data
    }
}

// The PNG sink and the two PNG boundaries, moved to a sibling to stay under the
// file ceiling. Re-exported so `crate::export::PngSequenceEncoder` and
// `crate::export::decode_png` still name them.
#[path = "export_png.rs"]
mod png_sink;
pub use png_sink::{PngSequenceEncoder, decode_png, encode_png, export_frame_png};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_png_round_trips_rgba_pixels() {
        let image = RgbaImage::new(2, 1, vec![255, 0, 0, 255, 10, 20, 30, 40]);

        let encoded = encode_png(&image).expect("encode");
        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");

        assert_eq!(decode_png(&encoded).expect("decode"), image);
    }

    #[test]
    fn in_memory_png_rejects_malformed_rgba_buffer() {
        let malformed = RgbaImage::new(2, 2, vec![0; 3]);
        let error = encode_png(&malformed).expect_err("bad buffer");
        assert!(
            error.to_string().contains("carries 3 bytes"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn in_memory_png_decode_rejects_invalid_and_oversized_inputs() {
        assert!(decode_png(b"not a png").is_err());

        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 4097, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![0; 4097 * 4]).unwrap();
        }
        let error = decode_png(&encoded).expect_err("edge bound");
        assert!(error.to_string().contains("4097x1"), "{error}");
    }

    /// WHY: `finish_export_frame` previously called `renderer.finish_pending`,
    /// which used `ImageSink::default()` to allocate, zero-fill, and drop a
    /// fresh `width * height * 4` `Vec<u8>` (8.3 MB at 1080p) on every exported
    /// frame. Closes the class of per-frame RGBA readback buffer reallocations
    /// during video export.
    #[test]
    fn export_frame_sink_reuses_plane_buffer_across_frames_and_updates_pts() {
        let mut sink = ExportFrameSink::new();
        let p0 = sink.pixels(4, 2);
        p0.fill(0x24);
        let t0 = RationalTime::new(0, Rational::new(30, 1));
        let ptr0 = {
            let f0 = sink.frame_with_pts(t0);
            assert_eq!(f0.pts, t0);
            assert_eq!((f0.width(), f0.height()), (4, 2));
            let plane = &f0.cpu().expect("cpu frame").planes[0];
            assert_eq!((plane.stride, plane.rows), (16, 2));
            assert!(plane.data.iter().all(|&b| b == 0x24));
            plane.data.as_ptr()
        };

        let p1 = sink.pixels(4, 2);
        p1.fill(0x7B);
        let t1 = RationalTime::new(1, Rational::new(30, 1));
        let f1 = sink.frame_with_pts(t1);
        assert_eq!(f1.pts, t1);
        let plane1 = &f1.cpu().expect("cpu frame").planes[0];
        assert_eq!(
            plane1.data.as_ptr(),
            ptr0,
            "ExportFrameSink reallocated the RGBA plane buffer on a same-size frame"
        );
        assert!(plane1.data.iter().all(|&b| b == 0x7B));
    }
}
#[cfg(test)]
#[path = "export_tests.rs"]
mod span_tests;
