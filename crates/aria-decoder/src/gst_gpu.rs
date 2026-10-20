//! Linux GPU output: DMA-BUF samples as [`GpuSurfaceKind::DmaBuf`].
//!
//! `OutputMode::Gpu` tries a DMA-BUF appsink first
//! (`video/x-raw(memory:DMABuf)`). LINEAR NV12 fds become a
//! [`DmaBufSurface`]. Tiled modifiers, missing fds, or a pipeline that
//! never negotiates DMABuf fall back to CPU NV12 so preview still works.
//! The first DMA-BUF open that fails on a file the CPU path then opens is
//! logged with its cause ([`dmabuf_cause`]: each video decoder plugged and the
//! memory it can output, such as `nvh264dec` with CUDA, GL, and system memory)
//! and later opens in the process go straight to CPU NV12. A file neither path
//! opens leaves DMA-BUF on. A decoder with no DMABuf output stops the
//! attempt before it opens ([`crate::gst_dmabuf_watch`]). A
//! tiled modifier is named once per session ([`report_tiled_fallback`]) so the
//! reason a machine decodes on the CPU is on the record rather than inferred
//! from a frame rate.
//!
//! The GPU path must not force `videoconvert`: that downloads the hardware
//! surface. `uridecodebin` is asked for a DMABuf pad directly.

use std::os::raw::c_int;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use aria_core::{
    DecodeError, DmaBufSurface, FrameData, GpuSurface, GpuSurfaceKind, PixelFormat, Plane,
    Rational, RationalTime, Rect, SourceInfo, VideoFrame,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::VideoFrameExt;

use crate::OutputMode;
use crate::gst::{PipelineGuard, bus_error, bus_or, gst_err, path_uri};
use crate::gst_dmabuf_watch::{DmabufWatch, dmabuf_cause, watch_video_decoders};
use crate::gst_key_snap::{KeySnap, snap_video_to_keyframes};

#[link(name = "gstallocators-1.0")]
unsafe extern "C" {
    fn gst_is_dmabuf_memory(mem: *mut gst::ffi::GstMemory) -> glib::ffi::gboolean;
    fn gst_dmabuf_memory_get_fd(mem: *mut gst::ffi::GstMemory) -> c_int;
}

/// Result of opening a decode pipeline.
pub(crate) struct OpenedPipeline {
    pub pipeline: gst::Pipeline,
    pub sink: gst_app::AppSink,
    pub info: SourceInfo,
    pub mode: OutputMode,
    /// Corrects where the video decoder starts after a key-unit seek.
    pub snap: Arc<KeySnap>,
    /// Counts this pipeline while it is open so concurrent software video
    /// decoders share the host's CPU thread budget.
    pub slot: VideoPipelineSlot,
}

/// Why the DMA-BUF pipeline failed the first time it failed on a file the CPU
/// path opens in this process. Set once; every later GPU open skips the
/// attempt.
static DMABUF_FAILED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Try DMABuf first when the caller asked for GPU and no DMABuf open has
/// failed in this process; otherwise CPU NV12.
pub(crate) fn open_pipeline(
    path: &std::path::Path,
    mode: OutputMode,
) -> Result<OpenedPipeline, DecodeError> {
    let uri = path_uri(path)?;
    crate::gst_context::await_warm_up();
    let has_dmabuf = crate::gst_dmabuf_watch::registry_has_dmabuf_video_decoder();
    open_latched_for(
        &DMABUF_FAILED,
        matches!(mode, OutputMode::Gpu),
        has_dmabuf,
        |dmabuf, watch| try_open(&uri, dmabuf, watch),
    )
}

/// Open through `open` with a DMABuf watch when `gpu` holds and `latch` is
/// empty, and with none otherwise or after that attempt fails.
///
/// A failed DMABuf attempt sets `latch` only when the CPU attempt then opens
/// the same file. An unreadable, empty, or audio-only source fails both, says
/// nothing about DMABuf, and leaves every later GPU open its DMABuf attempt.
#[cfg(test)]
fn open_latched<T>(
    latch: &std::sync::OnceLock<String>,
    gpu: bool,
    open: impl Fn(Option<&Arc<DmabufWatch>>) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
    open_latched_for(latch, gpu, true, |dmabuf, watch| {
        open(if dmabuf { watch } else { None })
    })
}

fn open_latched_for<T>(
    latch: &std::sync::OnceLock<String>,
    gpu: bool,
    has_dmabuf_decoder: bool,
    open: impl Fn(bool, Option<&Arc<DmabufWatch>>) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
    if !gpu || latch.get().is_some() {
        return open(false, None);
    }
    let watch = Arc::new(DmabufWatch::default());
    if !has_dmabuf_decoder {
        let opened = open(false, Some(&watch))?;
        let reason = format!(
            "{} (no ranked video decoder offers memory:DMABuf)",
            dmabuf_cause(&watch.decoders())
        );
        tracing::info!(
            cause = %reason,
            "DMABuf decode is unavailable; this process decodes to CPU NV12 from now on"
        );
        let _ = latch.set(reason);
        return Ok(opened);
    }
    let failed = match open(true, Some(&watch)) {
        Ok(opened) => return Ok(opened),
        Err(err) => err,
    };
    let opened = open(false, None)?;
    let reason = format!("{} ({failed})", dmabuf_cause(&watch.decoders()));
    tracing::warn!(
        cause = %reason,
        "DMABuf decode is unavailable; this process decodes to CPU NV12 from now on"
    );
    let _ = latch.set(reason);
    Ok(opened)
}

/// The error a starved DMABuf attempt returns.
pub(crate) const STOPPED: &str = "the DMABuf attempt stopped before its decoder opened, because the decoder has no DMABuf output";

/// How long one preroll wait lasts before the retry.
const PREROLL_WAIT: Duration = Duration::from_secs(8);

/// How often a preroll wait checks `stopped`.
const STOP_POLL: gst::ClockTime = gst::ClockTime::from_mseconds(5);

/// Wait up to [`PREROLL_WAIT`] for `pipeline` to finish its state change,
/// returning early with `Async` once `stopped` holds.
fn wait_preroll(
    pipeline: &gst::Pipeline,
    stopped: &dyn Fn() -> bool,
) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
    let deadline = Instant::now() + PREROLL_WAIT;
    loop {
        let (res, ..) = pipeline.state(STOP_POLL);
        if res != Ok(gst::StateChangeSuccess::Async) || stopped() || Instant::now() >= deadline {
            return res;
        }
    }
}

/// `n-threads` for the CPU path's `videoconvert` and `videoscale`: `0` is one
/// thread per logical CPU. The default of one thread halves 4K HEVC Main10
/// throughput, because the 10-bit to NV12 conversion then takes as long as
/// the decode (600 frames on an i7-11700K: 10.3 s with one thread, 4.7 s with
/// `0`).
const CONVERT_THREADS: u32 = 0;

/// Maximum `max-threads` assigned to a single software video decoder element
/// (matching libavcodec's `FF_MAX_AUTO_THREADS = 16` ceiling when one pipeline
/// is open).
pub(crate) const MAX_DECODER_THREADS: usize = 16;

/// Minimum `max-threads` floor when multiple pipelines are open on a host with
/// enough CPUs: with `appsink` `max-buffers = 2` and `avdec` frame-thread delay
/// `max_threads - 1`, `8` frame threads preroll all [`crate::ahead::AHEAD_FRAMES`]
/// (`8`) frames during pipeline open instead of spilling background decode into
/// subsequent UI frames.
pub(crate) const MIN_DECODER_THREADS: usize = 8;

/// Live video decode pipelines in this process, used to bound the host CPU
/// thread budget across concurrent software decoders.
static ACTIVE_VIDEO_PIPELINES: AtomicUsize = AtomicUsize::new(0);

/// RAII registration of one open (or opening) video decode pipeline.
pub(crate) struct VideoPipelineSlot {
    active_at_open: usize,
}

impl VideoPipelineSlot {
    fn acquire() -> Self {
        let active_at_open = ACTIVE_VIDEO_PIPELINES.fetch_add(1, Ordering::Relaxed) + 1;
        Self { active_at_open }
    }

    fn thread_budget(&self) -> i32 {
        let cpus = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(4);
        let active = ACTIVE_VIDEO_PIPELINES
            .load(Ordering::Relaxed)
            .max(self.active_at_open)
            .max(1);
        let floor = cpus.clamp(1, MIN_DECODER_THREADS);
        let ceiling = cpus.clamp(1, MAX_DECODER_THREADS);
        (cpus / active).clamp(floor, ceiling) as i32
    }
}

impl Drop for VideoPipelineSlot {
    fn drop(&mut self) {
        ACTIVE_VIDEO_PIPELINES.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Restrict `uridecodebin` to raw video output so non-video tracks (audio,
/// subtitles) are not demuxed or queued, and bound software video decoder
/// thread pools (`max-threads`) as decoders are plugged.
fn configure_video_uridecodebin(bin: &gst::Bin, thread_budget: i32) {
    if bin.find_property("expose-all-streams").is_some() {
        bin.set_property("expose-all-streams", false);
    }
    if bin.find_property("caps").is_some() {
        let video_caps = gst::Caps::builder_full_with_any_features()
            .structure(gst::Structure::builder("video/x-raw").build())
            .build();
        bin.set_property("caps", &video_caps);
    }
    bin.connect("deep-element-added", false, move |values| {
        let Ok(element) = values[2].get::<gst::Element>() else {
            return None;
        };
        let factory = element.factory()?;
        let klass = factory
            .metadata(gst::ELEMENT_METADATA_KLASS)
            .unwrap_or_default();
        if klass.contains("Decoder") && klass.contains("Video") {
            if element.find_property("max-threads").is_some() {
                element.set_property("max-threads", thread_budget);
            }
            if element.find_property("thread-type").is_some() {
                element.set_property_from_str("thread-type", "slice");
            }
        }
        None
    });
}

/// Build the decode pipeline directly without `gst::parse::launch` grammar
/// parsing or static pad-template caps intersection across converter elements.
fn build_decode_pipeline(
    uri: &str,
    dmabuf: bool,
) -> Result<(PipelineGuard, gst::Bin, gst_app::AppSink), DecodeError> {
    let make = |factory: &str, name: Option<&str>| -> Result<gst::Element, DecodeError> {
        let mut builder = gst::ElementFactory::make(factory);
        if let Some(name) = name {
            builder = builder.name(name);
        }
        builder.build().map_err(|e| {
            DecodeError::Open(format!(
                "couldn't build the GStreamer decode pipeline ({factory}: {e}). Install gstreamer1.0-plugins-base."
            ))
        })
    };
    let pipeline = PipelineGuard::new(gst::Pipeline::new());
    let src = make("uridecodebin", Some("src"))?;
    src.set_property("uri", uri);
    let filter = make("capsfilter", None)?;
    let sink = gst_app::AppSink::builder()
        .name("out")
        .sync(false)
        .max_buffers(4)
        .drop(false)
        .build();
    let sink_el = sink.upcast_ref::<gst::Element>();

    let first_downstream = if dmabuf {
        let caps = gst::Caps::builder("video/x-raw")
            .features([crate::gst_dmabuf_watch::DMABUF_MEMORY])
            .build();
        filter.set_property("caps", &caps);
        pipeline
            .add_many([&src, &filter, sink_el])
            .map_err(|e| DecodeError::Open(format!("couldn't build decode pipeline: {e}")))?;
        filter
            .link_pads_full(
                Some("src"),
                sink_el,
                Some("sink"),
                gst::PadLinkCheck::empty(),
            )
            .map_err(|e| DecodeError::Open(format!("couldn't link decode sink: {e}")))?;
        filter
    } else {
        let convert = make("videoconvert", None)?;
        convert.set_property("n-threads", CONVERT_THREADS);
        let scale = make("videoscale", None)?;
        scale.set_property("n-threads", CONVERT_THREADS);
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "NV12")
            .build();
        filter.set_property("caps", &caps);
        pipeline
            .add_many([&src, &convert, &scale, &filter, sink_el])
            .map_err(|e| DecodeError::Open(format!("couldn't build decode pipeline: {e}")))?;
        for (up, down) in [(&convert, &scale), (&scale, &filter), (&filter, sink_el)] {
            up.link_pads_full(Some("src"), down, Some("sink"), gst::PadLinkCheck::empty())
                .map_err(|e| DecodeError::Open(format!("couldn't link decode pipeline: {e}")))?;
        }
        convert
    };
    src.connect_pad_added(move |_, pad| {
        if let Some(sink_pad) = first_downstream.static_pad("sink")
            && !sink_pad.is_linked()
        {
            let _ = pad.link_full(&sink_pad, gst::PadLinkCheck::empty());
        }
    });
    let bin = src
        .downcast::<gst::Bin>()
        .map_err(|_| DecodeError::Open("uridecodebin is not a GstBin.".into()))?;
    Ok((pipeline, bin, sink))
}

/// Open `uri` for DMABuf output when `dmabuf` holds, or for CPU NV12
/// otherwise, recording plugged video decoders into `watch` when provided.
fn try_open(
    uri: &glib::GString,
    dmabuf: bool,
    watch: Option<&Arc<DmabufWatch>>,
) -> Result<OpenedPipeline, DecodeError> {
    let slot = VideoPipelineSlot::acquire();
    let (pipeline, bin, sink) = build_decode_pipeline(uri.as_str(), dmabuf)?;
    crate::gst_context::share(&pipeline);
    let snap = Arc::new(KeySnap::default());
    configure_video_uridecodebin(&bin, slot.thread_budget());
    snap_video_to_keyframes(&bin, &snap);
    if let Some(watch) = watch {
        watch_video_decoders(&bin, watch, dmabuf);
    }
    pipeline
        .set_state(gst::State::Paused)
        .map_err(|e| gst_err(DecodeError::Open, e))?;
    let stopped = || dmabuf && watch.is_some_and(|watch| watch.stopped());
    // One clean retry: `Async` after the wait means the pipeline was still
    // prerolling when it expired — on a host busy with other work (a full test
    // run, a cold codec plugin), not necessarily a broken file. A real defect
    // posts a bus error, and `bus_or` names it instead of this path; a retry
    // that still has not prerolled fails like the single wait did.
    let state_res = wait_preroll(&pipeline, &stopped);
    let state_res = if state_res == Ok(gst::StateChangeSuccess::Async)
        && !stopped()
        && bus_error(&pipeline).is_none()
    {
        let _ = pipeline.set_state(gst::State::Paused);
        wait_preroll(&pipeline, &stopped)
    } else {
        state_res
    };
    if stopped() {
        return Err(DecodeError::Open(STOPPED.into()));
    }
    if !matches!(
        state_res,
        Ok(gst::StateChangeSuccess::Success) | Ok(gst::StateChangeSuccess::NoPreroll)
    ) {
        let err = bus_or(
            &pipeline,
            DecodeError::Open(
                "GStreamer could not open this file. Install codec plugins (gstreamer1.0-plugins-good, -bad, -libav) or pick another file.".into(),
            ),
        );
        return Err(err);
    }

    let (mut info, preroll) = match crate::gst::probe_sink(&sink) {
        Ok(opened) => opened,
        Err(err) => {
            let err = bus_or(&pipeline, err);
            return Err(err);
        }
    };
    if dmabuf && sample_to_dmabuf(&preroll).is_none() {
        return Err(DecodeError::Open(
            "DMABuf preroll is not a LINEAR NV12 fd. Falling back to CPU.".into(),
        ));
    }
    if let Some(dur) = pipeline.query_duration::<gst::ClockTime>() {
        info.duration = Some(RationalTime::new(
            dur.nseconds() as i64,
            Rational::new(crate::gst::NS_PER_SEC as i32, 1),
        ));
    }
    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| gst_err(DecodeError::Open, e))?;

    Ok(OpenedPipeline {
        pipeline: pipeline.take(),
        sink,
        info,
        mode: if dmabuf {
            OutputMode::Gpu
        } else {
            OutputMode::Cpu
        },
        snap,
        slot,
    })
}

pub(crate) fn sample_to_frame(
    sample: &gst::Sample,
    info: &SourceInfo,
    mode: OutputMode,
) -> Result<VideoFrame, DecodeError> {
    if matches!(mode, OutputMode::Gpu) {
        if let Some(frame) = sample_to_gpu_frame(sample, info) {
            return Ok(frame);
        }
        tracing::debug!("DMABuf sample was not importable; delivering CPU NV12");
    }
    crate::gst::sample_to_cpu_frame(sample, info)
}

fn sample_to_gpu_frame(sample: &gst::Sample, info: &SourceInfo) -> Option<VideoFrame> {
    let (width, height) = sample_size(sample)?;
    let payload = sample_to_dmabuf(sample)?;
    let buffer = sample.buffer()?;
    let pts = crate::gst::stream_time_ns(sample, buffer)
        .map(|ns| RationalTime::new(ns, Rational::new(crate::gst::NS_PER_SEC as i32, 1)))
        .unwrap_or_else(|| RationalTime::new(0, info.time_base));
    let duration = buffer.duration().map(|t| {
        RationalTime::new(
            t.nseconds() as i64,
            Rational::new(crate::gst::NS_PER_SEC as i32, 1),
        )
    });

    let boxed = Box::new(DmaKeepAlive {
        payload,
        _sample: sample.clone(),
    });
    let handle = &boxed.payload as *const DmaBufSurface as u64;
    let surface = GpuSurface::new(GpuSurfaceKind::DmaBuf, handle, Some(boxed));
    let mut out = VideoFrame::new(
        pts,
        PixelFormat::Nv12,
        info.color,
        (width, height),
        Rect::from_size(width, height),
        info.rotation,
        FrameData::Gpu(surface),
    );
    if let Some(d) = duration {
        out = out.with_duration(d);
    }
    Some(out)
}

fn sample_size(sample: &gst::Sample) -> Option<(u32, u32)> {
    let caps = sample.caps()?;
    let vinfo = gst_video::VideoInfo::from_caps(caps).ok()?;
    Some((vinfo.width(), vinfo.height()))
}
fn sample_to_dmabuf(sample: &gst::Sample) -> Option<DmaBufSurface> {
    let buffer = sample.buffer()?;
    let n = buffer.n_memory();
    if n == 0 {
        return None;
    }
    let mut fds = [0i32; 4];
    let mut strides = [0u32; 4];
    let mut offsets = [0u32; 4];
    let mut plane_count = 0u32;

    for i in 0..n.min(4) {
        let mem = buffer.peek_memory(i);
        let fd = dmabuf_fd(mem.as_ptr() as *mut _)?;
        if fd < 0 {
            return None;
        }
        fds[i] = fd;
        plane_count += 1;
    }

    let (fourcc, modifier) = sample
        .caps()
        .and_then(|caps| gst_video::VideoInfoDmaDrm::from_caps(caps).ok())
        .map(|drm| (drm.fourcc(), drm.modifier()))
        .unwrap_or((DmaBufSurface::NV12, DmaBufSurface::LINEAR));

    if let Some(meta) = buffer.meta::<gst_video::VideoMeta>() {
        for (i, stride) in meta.stride().iter().enumerate().take(4) {
            strides[i] = (*stride).max(0) as u32;
        }
        for (i, offset) in meta.offset().iter().enumerate().take(4) {
            offsets[i] = u32::try_from(*offset).unwrap_or(0);
        }
    } else if let Some(caps) = sample.caps()
        && let Ok(vinfo) = gst_video::VideoInfo::from_caps(caps)
    {
        for (i, stride) in vinfo.stride().iter().enumerate().take(4) {
            strides[i] = (*stride).max(0) as u32;
        }
        for (i, offset) in vinfo.offset().iter().enumerate().take(4) {
            offsets[i] = u32::try_from(*offset).unwrap_or(0);
        }
    }

    let surface = DmaBufSurface {
        fourcc,
        modifier,
        plane_count: plane_count.max(1),
        fds,
        strides,
        offsets,
    };
    if !surface.is_linear_nv12() {
        let _ = report_tiled_fallback(&surface);
        return None;
    }
    Some(surface)
}

static REPORTED_MODIFIERS: std::sync::Mutex<Option<std::collections::HashSet<String>>> =
    std::sync::Mutex::new(None);

/// Vendor of a DRM format modifier, from its top byte.
///
/// Names come from `drm_fourcc.h`. A vendor Aria has no name for is reported by
/// its number rather than as "unknown", so a new GPU family is still actionable.
fn modifier_vendor(modifier: u64) -> String {
    match (modifier >> 56) as u8 {
        0 => "none".to_string(),
        1 => "Intel".to_string(),
        2 => "AMD".to_string(),
        3 => "NVIDIA".to_string(),
        4 => "Samsung".to_string(),
        5 => "Qualcomm".to_string(),
        6 => "Vivante".to_string(),
        7 => "Broadcom".to_string(),
        8 => "Arm".to_string(),
        9 => "Allwinner".to_string(),
        10 => "Amlogic".to_string(),
        other => format!("DRM vendor {other}"),
    }
}

/// Say once per source why this pipeline decodes on the CPU, naming the
/// modifier the pipeline negotiated. Returns whether this call was the one that
/// reported.
pub(crate) fn report_tiled_fallback_for_source(surface: &DmaBufSurface, source_id: &str) -> bool {
    let mut guard = REPORTED_MODIFIERS.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(std::collections::HashSet::new);
    let key = format!("{}:{}", source_id, surface.modifier);
    if !set.insert(key) {
        return false;
    }
    tracing::warn!(
        source = %source_id,
        modifier = format!("{:#x}", surface.modifier),
        vendor = %modifier_vendor(surface.modifier),
        fourcc = format!("{:#x}", surface.fourcc),
        planes = surface.plane_count,
        "GPU import needs a LINEAR NV12 DMA-BUF; this pipeline negotiated a \
         modifier Aria cannot import, so video decodes on the CPU this session"
    );
    true
}

fn report_tiled_fallback(surface: &DmaBufSurface) -> bool {
    report_tiled_fallback_for_source(surface, "default")
}

fn dmabuf_fd(mem: *mut gst::ffi::GstMemory) -> Option<i32> {
    if mem.is_null() {
        return None;
    }
    unsafe {
        if gst_is_dmabuf_memory(mem) != glib::ffi::GTRUE {
            return None;
        }
        Some(gst_dmabuf_memory_get_fd(mem))
    }
}

/// Parks the GStreamer sample (and therefore its DMA-BUF fds) for the frame
/// lifetime. The compositor dups fd 0 on import; the original stays here.
struct DmaKeepAlive {
    payload: DmaBufSurface,
    _sample: gst::Sample,
}

// SAFETY: GstSample is refcounted and the fd numbers are integers; the
// decoder worker is the only mutator of the pipeline.
unsafe impl Send for DmaKeepAlive {}
unsafe impl Sync for DmaKeepAlive {}

/// CPU NV12 extraction used by the fallback path and by unit tests.
pub(crate) fn nv12_from_video_frame(
    frame: &gst_video::VideoFrameRef<&gst::BufferRef>,
    height: u32,
) -> Result<Vec<Plane>, DecodeError> {
    let y = frame.plane_data(0).map_err(|_| {
        DecodeError::Decode("decoded frame is missing the Y plane. Try another file.".into())
    })?;
    let uv = frame.plane_data(1).map_err(|_| {
        DecodeError::Decode("decoded frame is missing the UV plane. Try another file.".into())
    })?;
    let y_stride = frame.plane_stride()[0] as usize;
    let uv_stride = frame.plane_stride()[1] as usize;
    Ok(vec![
        Plane::new(y.to_vec(), y_stride, height as usize),
        Plane::new(uv.to_vec(), uv_stride, (height as usize).div_ceil(2)),
    ])
}

pub(crate) fn nv12_from_bytes(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<Plane>, DecodeError> {
    let y_size = width as usize * height as usize;
    let uv_size = width as usize * (height as usize).div_ceil(2);
    if bytes.len() < y_size + uv_size {
        return Err(DecodeError::Decode(format!(
            "decoded frame is too small ({} bytes for {width}x{height}). Try another file.",
            bytes.len()
        )));
    }
    Ok(vec![
        Plane::new(bytes[..y_size].to_vec(), width as usize, height as usize),
        Plane::new(
            bytes[y_size..y_size + uv_size].to_vec(),
            width as usize,
            (height as usize).div_ceil(2),
        ),
    ])
}

#[cfg(test)]
#[path = "gst_gpu_tests.rs"]
mod tests;
