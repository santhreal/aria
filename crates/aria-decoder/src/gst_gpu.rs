//! Linux GPU output: DMA-BUF samples as [`GpuSurfaceKind::DmaBuf`].
//!
//! `OutputMode::Gpu` tries a DMA-BUF appsink first
//! (`video/x-raw(memory:DMABuf)`). LINEAR NV12 fds become a
//! [`DmaBufSurface`]. Tiled modifiers, missing fds, or a pipeline that
//! never negotiates DMABuf fall back to CPU NV12 so preview still works.
//! That fallback is per-open, not silent per-frame, and a tiled modifier is
//! named once per session ([`report_tiled_fallback`]) so the reason a machine
//! decodes on the CPU is on the record rather than inferred from a frame rate.
//!
//! The GPU path must not force `videoconvert`: that downloads the hardware
//! surface. `uridecodebin` is asked for a DMABuf pad directly.

use std::os::raw::c_int;

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
use crate::gst::{bus_error, bus_or, gst_err, path_uri};

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
    pub preroll: gst::Sample,
    pub mode: OutputMode,
}

/// Try DMABuf first when the caller asked for GPU; otherwise CPU NV12.
pub(crate) fn open_pipeline(
    path: &std::path::Path,
    mode: OutputMode,
) -> Result<OpenedPipeline, DecodeError> {
    let uri = path_uri(path)?;
    if matches!(mode, OutputMode::Gpu) {
        match try_open(&uri, true) {
            Ok(opened) => return Ok(opened),
            Err(err) => {
                tracing::info!(
                    error = %err,
                    "DMABuf decode pipeline failed; falling back to CPU NV12"
                );
            }
        }
    }
    try_open(&uri, false)
}

fn try_open(uri: &glib::GString, dmabuf: bool) -> Result<OpenedPipeline, DecodeError> {
    let desc = if dmabuf {
        format!(
            "uridecodebin uri=\"{uri}\" ! video/x-raw(memory:DMABuf) ! appsink name=out sync=false max-buffers=4 drop=false"
        )
    } else {
        format!(
            "uridecodebin uri=\"{uri}\" ! videoconvert ! videoscale ! video/x-raw,format=NV12 ! appsink name=out sync=false max-buffers=4 drop=false"
        )
    };
    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| {
            DecodeError::Open(format!(
                "couldn't build the GStreamer decode pipeline: {e}. Install gstreamer1.0-plugins-base."
            ))
        })?
        .downcast::<gst::Pipeline>()
        .map_err(|_| {
            DecodeError::Open("GStreamer did not return a pipeline. Retry the install.".into())
        })?;
    let sink = pipeline
        .by_name("out")
        .ok_or_else(|| DecodeError::Open("decode pipeline is missing appsink.".into()))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| DecodeError::Open("decode sink is not an appsink.".into()))?;

    pipeline
        .set_state(gst::State::Paused)
        .map_err(|e| gst_err(DecodeError::Open, e))?;
    // One clean retry: `Async` after the wait means the pipeline was still
    // prerolling when it expired — on a host busy with other work (a full test
    // run, a cold codec plugin), not necessarily a broken file. A real defect
    // posts a bus error, and `bus_or` names it instead of this path; a retry
    // that still has not prerolled fails like the single wait did.
    let (state_res, ..) = pipeline.state(gst::ClockTime::from_seconds(8));
    let state_res = if state_res == Ok(gst::StateChangeSuccess::Async)
        && bus_error(&pipeline).is_none()
    {
        let _ = pipeline.set_state(gst::State::Paused);
        let (retry, ..) = pipeline.state(gst::ClockTime::from_seconds(8));
        retry
    } else {
        state_res
    };
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
        let _ = pipeline.set_state(gst::State::Null);
        return Err(err);
    }

    let (mut info, preroll) = match crate::gst::probe_sink(&sink) {
        Ok(opened) => opened,
        Err(err) => {
            let err = bus_or(&pipeline, err);
            let _ = pipeline.set_state(gst::State::Null);
            return Err(err);
        }
    };
    if dmabuf && sample_to_dmabuf(&preroll).is_none() {
        let _ = pipeline.set_state(gst::State::Null);
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
        pipeline,
        sink,
        info,
        preroll,
        mode: if dmabuf {
            OutputMode::Gpu
        } else {
            OutputMode::Cpu
        },
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
    let pts = buffer
        .pts()
        .map(|t| {
            RationalTime::new(
                t.nseconds() as i64,
                Rational::new(crate::gst::NS_PER_SEC as i32, 1),
            )
        })
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
    let vinfo = gst_video::VideoInfo::from_caps(&caps).ok()?;
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
        .and_then(|caps| gst_video::VideoInfoDmaDrm::from_caps(&caps).ok())
        .map(|drm| (drm.fourcc(), drm.modifier()))
        .unwrap_or((DmaBufSurface::NV12, DmaBufSurface::LINEAR));

    if let Some(meta) = buffer.meta::<gst_video::VideoMeta>() {
        for (i, stride) in meta.stride().iter().enumerate().take(4) {
            strides[i] = (*stride).max(0) as u32;
        }
        for (i, offset) in meta.offset().iter().enumerate().take(4) {
            offsets[i] = u32::try_from(*offset).unwrap_or(0);
        }
    } else if let Some(caps) = sample.caps() {
        if let Ok(vinfo) = gst_video::VideoInfo::from_caps(&caps) {
            for (i, stride) in vinfo.stride().iter().enumerate().take(4) {
                strides[i] = (*stride).max(0) as u32;
            }
            for (i, offset) in vinfo.offset().iter().enumerate().take(4) {
                offsets[i] = u32::try_from(*offset).unwrap_or(0);
            }
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
mod tests {
    use super::*;

    #[test]
    fn packed_nv12_bytes_split() {
        let w = 4u32;
        let h = 4u32;
        let mut bytes = vec![16u8; (w * h) as usize];
        bytes.extend(std::iter::repeat(128u8).take((w * h / 2) as usize));
        let planes = nv12_from_bytes(&bytes, w, h).expect("split");
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0].rows, 4);
        assert_eq!(planes[1].rows, 2);
    }

    #[test]
    fn linear_nv12_surface_is_importable() {
        let s = DmaBufSurface {
            fourcc: DmaBufSurface::NV12,
            modifier: DmaBufSurface::LINEAR,
            plane_count: 1,
            fds: [5, 0, 0, 0],
            strides: [64, 64, 0, 0],
            offsets: [0, 256, 0, 0],
        };
        assert!(s.is_linear_nv12());
    }

    /// A tiled modifier must be named, not called unknown, and a vendor byte
    /// Aria has no name for must still print its number: a new GPU family is
    /// the case where this report matters most.
    #[test]
    fn every_drm_vendor_byte_gets_a_name() {
        // Vendor byte is the top byte; the low bits are the layout.
        assert_eq!(modifier_vendor(0), "none");
        assert_eq!(modifier_vendor(0x01 << 56 | 0x9), "Intel");
        assert_eq!(modifier_vendor(0x02 << 56 | 0x1a), "AMD");
        assert_eq!(modifier_vendor(0x03 << 56 | 0x10), "NVIDIA");
        assert_eq!(modifier_vendor(0x08 << 56 | 0x1), "Arm");
        assert_eq!(modifier_vendor(0xfe << 56), "DRM vendor 254");
        // Every byte resolves to something non-empty, so no modifier can be
        // reported as a blank vendor.
        for byte in 0u8..=255 {
            let name = modifier_vendor(u64::from(byte) << 56);
            assert!(!name.is_empty(), "vendor byte {byte} has no name");
        }
    }

    /// The report is once per session because the same pipeline negotiates the
    /// same modifier for every sample: per-frame logging would bury it.
    #[test]
    fn the_tiled_fallback_is_reported_once() {
        let tiled = DmaBufSurface {
            fourcc: DmaBufSurface::NV12,
            modifier: 0x03 << 56 | 0x10,
            plane_count: 2,
            fds: [7, 8, 0, 0],
            strides: [64, 64, 0, 0],
            offsets: [0, 256, 0, 0],
        };
        assert!(!tiled.is_linear_nv12(), "fixture must be unimportable");

        assert!(
            report_tiled_fallback_for_source(&tiled, "test_session_1"),
            "the first tiled surface must be reported"
        );
        assert!(
            !report_tiled_fallback_for_source(&tiled, "test_session_1"),
            "the same fallback was reported twice"
        );
    }

    #[test]
    fn two_tiled_sources_both_log() {
        let tiled = DmaBufSurface {
            fourcc: DmaBufSurface::NV12,
            modifier: 0x03 << 56 | 0x10,
            plane_count: 2,
            fds: [7, 8, 0, 0],
            strides: [64, 64, 0, 0],
            offsets: [0, 256, 0, 0],
        };
        assert!(
            report_tiled_fallback_for_source(&tiled, "stream_alpha"),
            "first source must log"
        );
        assert!(
            !report_tiled_fallback_for_source(&tiled, "stream_alpha"),
            "first source must not duplicate log"
        );
        assert!(
            report_tiled_fallback_for_source(&tiled, "stream_beta"),
            "second source must also log"
        );
        assert!(
            !report_tiled_fallback_for_source(&tiled, "stream_beta"),
            "second source must not duplicate log"
        );
    }
}
