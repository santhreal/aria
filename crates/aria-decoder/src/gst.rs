//! Linux backend: GStreamer `uridecodebin` behind [`aria_core::VideoDecoder`].
//!
//! GStreamer is Linux's AVFoundation: system plugins do demux and decode
//! (VAAPI / NVDEC / V4L2 first, software after). Aria never ships a codec.
//! `OutputMode::Cpu` lands NV12 through `appsink`. `OutputMode::Gpu` tries a
//! DMA-BUF appsink first and falls back to CPU NV12 when the host cannot
//! produce a LINEAR NV12 fd.

use std::path::Path;
use std::sync::Once;
use std::time::Duration;

use aria_core::{
    ColorSpace, CpuImage, DecodeError, FrameData, PixelFormat, Rational, RationalTime, Rect,
    Rotation, SourceInfo, VideoDecoder, VideoFrame,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use crate::OutputMode;
use crate::seek::SeekStats;

pub(crate) const NS_PER_SEC: i64 = 1_000_000_000;
const PULL_TIMEOUT: Duration = Duration::from_secs(8);

/// Corrupt frames skipped after a seek before one is returned anyway, when no
/// earlier seek point decodes clean. Past this many the stream is damaged.
const MAX_CORRUPT_AFTER_SEEK: u32 = 300;

fn is_corrupt(sample: &gst::Sample) -> bool {
    sample
        .buffer()
        .is_some_and(|buffer| buffer.flags().contains(gst::BufferFlags::CORRUPTED))
}

/// Whether `sample` is a leading frame the seek's segment clipped: a frame
/// that starts before the sync sample the seek landed on and overlaps it.
/// GStreamer moves such a frame's start up to the segment start and cuts its
/// duration by the same amount, so its time is no longer its own. Only one
/// frame can straddle the segment start; frames wholly before it are dropped
/// upstream.
fn is_clipped_leading(sample: &gst::Sample, period_ns: Option<u64>) -> bool {
    /// A duration shorter than the frame period by more than this was cut.
    const CLIP_SLACK_NS: u64 = 1_000_000;
    let (Some(buffer), Some(period)) = (sample.buffer(), period_ns) else {
        return false;
    };
    let (Some(pts), Some(duration)) = (buffer.pts(), buffer.duration()) else {
        return false;
    };
    let start = sample
        .segment()
        .and_then(|segment| segment.downcast_ref::<gst::ClockTime>())
        .and_then(|segment| segment.start());
    start == Some(pts) && duration.nseconds().saturating_add(CLIP_SLACK_NS) < period
}

/// One frame period of `rate` in nanoseconds, `None` without a valid rate.
fn frame_period_ns(rate: Rational) -> Option<u64> {
    let (num, den) = (u64::try_from(rate.num).ok()?, u64::try_from(rate.den).ok()?);
    (num > 0 && den > 0).then(|| NS_PER_SEC as u64 * den / num)
}

static GST_INIT: Once = Once::new();

pub(crate) fn ensure_init() -> Result<(), DecodeError> {
    let mut err = None;
    GST_INIT.call_once(|| {
        if let Err(e) = gst::init() {
            err = Some(e);
        }
    });
    match err {
        Some(e) => Err(DecodeError::Open(format!(
            "GStreamer failed to initialize: {e}. Install gstreamer1.0 and retry."
        ))),
        None => Ok(()),
    }
}

pub(crate) fn path_uri(path: &Path) -> Result<glib::GString, DecodeError> {
    let abs = path.canonicalize().map_err(|e| {
        DecodeError::Open(format!(
            "couldn't resolve {}: {e}. Check the path.",
            path.display()
        ))
    })?;
    glib::filename_to_uri(abs, None).map_err(|e| {
        DecodeError::Open(format!(
            "couldn't build a file URI for {}: {e}.",
            path.display()
        ))
    })
}

pub(crate) fn gst_err(kind: fn(String) -> DecodeError, e: impl std::fmt::Display) -> DecodeError {
    kind(e.to_string())
}

/// A pull-based decoder over one video file, backed by GStreamer.
pub struct GstDecoder {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    info: SourceInfo,
    /// Whether a frame has been returned since the last seek. Until one has,
    /// corrupt frames are skipped.
    settled: bool,
    /// The first sample after a seek, pulled to check that the seek landed on
    /// a clean start point.
    pending: Option<gst::Sample>,
    last_pts: Option<RationalTime>,
    seek_stats: SeekStats,
    mode: OutputMode,
}

// SAFETY: pipeline and sink are owned by one decode worker. `VideoDecoder: Send`
// without `Sync`.
unsafe impl Send for GstDecoder {}

impl GstDecoder {
    pub fn open(path: &Path, mode: OutputMode) -> Result<Self, DecodeError> {
        ensure_init()?;
        if gst::ElementFactory::find("decodebin").is_none() {
            return Err(DecodeError::unsupported(
                "GStreamer decodebin is missing. Install gstreamer1.0-plugins-base and retry.",
            ));
        }
        let opened = crate::gst_gpu::open_pipeline(path, mode)?;
        Ok(Self {
            pipeline: opened.pipeline,
            sink: opened.sink,
            info: opened.info,
            // The preroll sample is not kept: appsink hands it out again as
            // the first `pull_sample` once the pipeline plays.
            settled: true,
            pending: None,
            last_pts: None,
            seek_stats: SeekStats::default(),
            mode: opened.mode,
        })
    }

    fn pull_sample(&mut self) -> Result<Option<gst::Sample>, DecodeError> {
        match self
            .sink
            .try_pull_sample(gst::ClockTime::from_nseconds(PULL_TIMEOUT.as_nanos() as u64))
        {
            Some(sample) => Ok(Some(sample)),
            None if self.sink.is_eos() => Ok(None),
            None => {
                if let Some(err) = bus_error(&self.pipeline) {
                    return Err(err);
                }
                if self.sink.is_eos() {
                    return Ok(None);
                }
                Err(DecodeError::Decode(
                    "GStreamer timed out waiting for a frame. Try another file or reinstall codec plugins.".into(),
                ))
            }
        }
    }

    /// One flushing key-unit seek to the sync sample at or before `target`.
    fn seek_pipeline(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let ns = rational_to_ns(target);
        self.pending = None;
        self.settled = false;
        self.last_pts = None;
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_BEFORE,
                gst::ClockTime::from_nseconds(ns as u64),
            )
            .map_err(|_| {
                bus_or(
                    &self.pipeline,
                    DecodeError::Seek(format!(
                        "GStreamer could not seek to {:.3}s.",
                        ns as f64 / NS_PER_SEC as f64
                    )),
                )
            })
    }
}

impl VideoDecoder for GstDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    /// Seek to the sync sample at or before `target`. When the first frame
    /// after it decodes corrupt, the sync sample needs frames before it (an
    /// open GOP without an IDR there), so the seek moves earlier, one second
    /// then doubling, until the first frame decodes clean. A leading frame the
    /// seek's segment clipped is skipped: its time was moved, so returning it
    /// would show one frame under another's time.
    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let mut backoff = crate::seek::Backoff::new(target);
        let mut point = target;
        let period = frame_period_ns(self.info.frame_rate);
        loop {
            self.seek_pipeline(point)?;
            let Some(mut sample) = self.pull_sample()? else {
                return Ok(());
            };
            if is_clipped_leading(&sample, period) {
                let Some(next) = self.pull_sample()? else {
                    return Ok(());
                };
                sample = next;
            }
            if is_corrupt(&sample)
                && let Some((earlier, _)) = backoff.next()
            {
                point = earlier;
                continue;
            }
            self.pending = Some(sample);
            return Ok(());
        }
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let mut skipped = 0u32;
        let sample = loop {
            let sample = match self.pending.take() {
                Some(sample) => sample,
                None => match self.pull_sample()? {
                    Some(sample) => sample,
                    None => return Ok(None),
                },
            };
            if is_corrupt(&sample) && !self.settled && skipped < MAX_CORRUPT_AFTER_SEEK {
                skipped += 1;
                continue;
            }
            break sample;
        };
        self.settled = true;
        let frame = crate::gst_gpu::sample_to_frame(&sample, &self.info, self.mode)?;
        self.last_pts = Some(frame.pts);
        Ok(Some(frame))
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_nearest_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }
}

impl Drop for GstDecoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

pub(crate) fn probe_sink(
    sink: &gst_app::AppSink,
) -> Result<(SourceInfo, gst::Sample), DecodeError> {
    let sample = sink
        .try_pull_preroll(gst::ClockTime::from_seconds(8))
        .ok_or_else(|| {
            DecodeError::Open(
                "GStreamer produced no video preroll. The file may have no video track.".into(),
            )
        })?;
    let info = sample_source_info(&sample)?;
    Ok((info, sample))
}

fn sample_source_info(sample: &gst::Sample) -> Result<SourceInfo, DecodeError> {
    let caps = sample.caps().ok_or_else(|| {
        DecodeError::Open("GStreamer sample has no caps. Try another file.".into())
    })?;
    let vinfo =
        gst_video::VideoInfo::from_caps(&caps).map_err(|e| gst_err(DecodeError::Open, e))?;
    let width = vinfo.width();
    let height = vinfo.height();
    let fps = vinfo.fps();
    let frame_rate = if fps.numer() > 0 && fps.denom() > 0 {
        Rational::new(fps.numer(), fps.denom())
    } else {
        Rational::new(30, 1)
    };
    Ok(SourceInfo {
        coded_size: (width, height),
        display_size: (width, height),
        rotation: Rotation::None,
        pixel_format: PixelFormat::Nv12,
        color: ColorSpace::BT709,
        frame_rate,
        time_base: Rational::new(NS_PER_SEC as i32, 1),
        duration: None,
    })
}

pub(crate) fn sample_to_cpu_frame(
    sample: &gst::Sample,
    info: &SourceInfo,
) -> Result<VideoFrame, DecodeError> {
    let caps = sample.caps().ok_or_else(|| {
        DecodeError::Decode("GStreamer frame has no caps. Try another file.".into())
    })?;
    let vinfo =
        gst_video::VideoInfo::from_caps(&caps).map_err(|e| gst_err(DecodeError::Decode, e))?;
    let buffer = sample.buffer().ok_or_else(|| {
        DecodeError::Decode("GStreamer frame has no buffer. Try another file.".into())
    })?;
    let (width, height) = (vinfo.width(), vinfo.height());
    let planes = match gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &vinfo) {
        Ok(frame) => crate::gst_gpu::nv12_from_video_frame(&frame, height)?,
        Err(_) => {
            let map = buffer.map_readable().map_err(|_| {
                DecodeError::Decode("couldn't map a decoded frame. Try another file.".into())
            })?;
            crate::gst_gpu::nv12_from_bytes(map.as_slice(), width, height)?
        }
    };
    let pts = stream_time_ns(sample, buffer)
        .map(|ns| RationalTime::new(ns, Rational::new(NS_PER_SEC as i32, 1)))
        .unwrap_or_else(|| RationalTime::new(0, info.time_base));
    let duration = buffer
        .duration()
        .map(|t| RationalTime::new(t.nseconds() as i64, Rational::new(NS_PER_SEC as i32, 1)));
    let mut out = VideoFrame::new(
        pts,
        PixelFormat::Nv12,
        info.color,
        (width, height),
        Rect::from_size(width, height),
        info.rotation,
        FrameData::Cpu(CpuImage::new(planes)),
    );
    if let Some(d) = duration {
        out = out.with_duration(d);
    }
    Ok(out)
}

/// A decoded buffer's presentation time in nanoseconds on the file's own
/// timeline: the buffer timestamp mapped through the sample's segment.
///
/// An MP4 edit list starts buffer timestamps after zero (two frames for H.264
/// with B-frames, the encoder delay for AAC), and the segment maps them back,
/// so the first presented frame is at zero and frame times share one timeline
/// with seek targets. Negative for a frame before the segment start.
pub(crate) fn stream_time_ns(sample: &gst::Sample, buffer: &gst::BufferRef) -> Option<i64> {
    let pts = i64::try_from(buffer.pts()?.nseconds()).ok()?;
    let Some(segment) = sample
        .segment()
        .and_then(|segment| segment.downcast_ref::<gst::ClockTime>())
    else {
        return Some(pts);
    };
    let (Some(start), Some(time)) = (segment.start(), segment.time()) else {
        return Some(pts);
    };
    let start = i64::try_from(start.nseconds()).ok()?;
    let time = i64::try_from(time.nseconds()).ok()?;
    pts.checked_sub(start)?.checked_add(time)
}

pub(crate) fn bus_error(pipeline: &gst::Pipeline) -> Option<DecodeError> {
    let bus = pipeline.bus()?;
    while let Some(msg) = bus.pop() {
        if let gst::MessageView::Error(err) = msg.view() {
            return Some(DecodeError::Decode(format!(
                "GStreamer: {}. Install missing plugins or pick another file.",
                err.error()
            )));
        }
    }
    None
}

pub(crate) fn bus_or(pipeline: &gst::Pipeline, fallback: DecodeError) -> DecodeError {
    bus_error(pipeline).unwrap_or(fallback)
}

pub(crate) fn rational_to_ns(t: RationalTime) -> i64 {
    if t.rate.num <= 0 {
        return 0;
    }
    let den = i128::from(t.rate.den.max(1));
    let num = i128::from(t.rate.num);
    let ns = (i128::from(t.value) * den * i128::from(NS_PER_SEC)) / num;
    ns.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Duration of the first audio track, for the audio-only fallback probe.
pub(crate) fn audio_track_duration(path: &Path) -> Option<RationalTime> {
    ensure_init().ok()?;
    let uri = path_uri(path).ok()?;
    let discoverer = gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(8)).ok()?;
    let info = discoverer.discover_uri(uri.as_str()).ok()?;
    let audio = info.audio_streams().into_iter().next()?;
    let dur = info.duration()?;
    let _ = audio;
    Some(RationalTime::new(
        dur.nseconds() as i64,
        Rational::new(NS_PER_SEC as i32, 1),
    ))
}

/// What one discovery pass can say about a file's tracks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StreamSummary {
    pub has_audio: bool,
    /// Codec names as the demuxer describes them ("H.264", "MPEG-4 AAC"), or
    /// `None` when a track is absent or the caps carry no description.
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

/// Everything the probe needs from the demuxer, in one discovery pass.
///
/// One pass rather than one per question: a discovery walks the file and can
/// wait up to its timeout, so asking separately for the audio track and then
/// for each codec name would multiply the cost of every import.
pub(crate) fn stream_summary(path: &Path) -> StreamSummary {
    let mut summary = StreamSummary::default();
    if ensure_init().is_err() {
        return summary;
    }
    let Ok(uri) = path_uri(path) else {
        return summary;
    };
    let Ok(discoverer) = gstreamer_pbutils::Discoverer::new(gst::ClockTime::from_seconds(8)) else {
        return summary;
    };
    let Ok(info) = discoverer.discover_uri(uri.as_str()) else {
        return summary;
    };
    let audio = info.audio_streams();
    summary.has_audio = !audio.is_empty();
    summary.audio_codec = audio.first().and_then(codec_name);
    summary.video_codec = info.video_streams().first().and_then(codec_name);
    summary
}

/// The description GStreamer gives a stream's caps, e.g. `H.264`. `None` when
/// the stream reports no caps, so an unknown codec is stated rather than named
/// wrongly.
fn codec_name(stream: &impl gstreamer_pbutils::prelude::DiscovererStreamInfoExt) -> Option<String> {
    let caps = stream.caps()?;
    let described = gstreamer_pbutils::functions::pb_utils_get_codec_description(&caps).to_string();
    if described.is_empty() {
        None
    } else {
        Some(described)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_zero_is_zero_ns() {
        assert_eq!(
            rational_to_ns(RationalTime::new(0, Rational::new(24, 1))),
            0
        );
    }

    #[test]
    fn one_second_at_24fps_is_onee9_ns() {
        assert_eq!(
            rational_to_ns(RationalTime::new(24, Rational::new(24, 1))),
            NS_PER_SEC
        );
    }

    #[test]
    fn one_hour_timestamp_converts_exactly() {
        // 1 hour at 1GHz (nanosecond clock) = 3,600,000,000,000 ns.
        // In i64 saturating arithmetic: 3.6e12 * 1e9 saturates at i64::MAX (~9.22e18),
        // dividing by 1e9 erroneously clamped to ~9.22e9 ns (~9.22 seconds).
        // i128 arithmetic converts this without clamping.
        let one_hour_ns = 3_600 * NS_PER_SEC;
        let t = RationalTime::new(one_hour_ns, Rational::new(1_000_000_000, 1));
        assert_eq!(rational_to_ns(t), one_hour_ns);

        // Also verify 1 hour at standard 48kHz audio rate.
        let t_48k = RationalTime::new(3_600 * 48_000, Rational::new(48_000, 1));
        assert_eq!(rational_to_ns(t_48k), one_hour_ns);
    }
}
