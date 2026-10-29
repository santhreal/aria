//! Interactive preview: the scrubbing session and its C ABI.
//!
//! Declared from `lib.rs` with `#[path]`; `use super::*` reaches the crate
//! root's imports and private helpers, so the move changed no name resolution.
//! The `#[unsafe(no_mangle)]` exports below are unaffected by module privacy.
//! The module exports `#[unsafe(no_mangle)]` C ABI functions that box, borrow, and free raw preview handles and is `unsafe` throughout.

#![allow(unsafe_code)]

use super::*;

// ---------------------------------------------------------------------------
// Interactive preview
//
// Scrubbing is the first real-editor workload, and it's where the per-call
// `GpuContext`/decoder setup above would fall over: each slider tick must reuse
// the device, compositor, and (for video) the decoder cache. `AriaPreview`
// holds that state for the session; the app keeps an opaque handle and calls
// `render` per frame. `Renderer` already caches decoders internally, so a video
// scrub seeks within one open decoder instead of reopening per frame.
// ---------------------------------------------------------------------------

/// A live preview session: a persistent renderer (GPU device + decoder cache)
/// bound to a project, so scrubbing only pays for the frame at `t`.
///
/// Not thread-safe: `render` takes `&mut self`, so the caller must serialize
/// calls on a single handle (a scrubber issues them one at a time).
pub struct AriaPreview {
    renderer: Renderer,
    project: Project,
    fps: Rational,
    duration_frames: i64,
}

impl std::fmt::Debug for AriaPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AriaPreview")
            .field("fps", &self.fps)
            .field("duration_frames", &self.duration_frames)
            .finish_non_exhaustive()
    }
}

impl AriaPreview {
    /// A synthetic, file-free preview: a full-canvas color sweeping through the
    /// hue wheel over ~6s. Needs no assets or decoder, so it proves the scrub
    /// path (slider time -> composited frame) on any device, even offline.
    pub(crate) fn demo() -> Option<Self> {
        let fps = Rational::FPS_30;
        let total = 180i64; // 6s @ 30fps
        let step = 6i64; // a new color every 0.2s
        let mut project = Project::new("preview-demo", fps);
        let track = project.add_track(TrackKind::Sticker, "BG");
        let mut start = 0i64;
        while start < total {
            let len = step.min(total - start);
            let rgba = hue_sweep(start as f32 / total as f32);
            let span = TimeRange::at_rate(start, len, fps);
            if let Err(e) = project.add_generated(track, Generator::SolidColor { rgba }, span) {
                log::error!("preview demo: add_generated failed: {e}");
                return None;
            }
            start += step;
        }
        let renderer = match Renderer::new_headless() {
            Ok(renderer) => renderer,
            Err(e) => {
                log::error!("preview demo: renderer init failed: {e}");
                return None;
            }
        };
        Some(Self {
            renderer,
            project,
            fps,
            duration_frames: total,
        })
    }

    /// A preview that scrubs the video file at `path`. The canvas follows the
    /// footage (`CanvasAspect::Auto`).
    pub(crate) fn video(path: &Path) -> Option<Self> {
        let probe = match aria_decoder::probe(path) {
            Ok(probe) => probe,
            Err(e) => {
                log::error!("preview video: probe failed for {}: {e}", path.display());
                return None;
            }
        };
        let fps = probe.frame_rate;
        let frames = probe.frame_count.max(1);
        let mut project = Project::new("preview-video", fps);
        let media = project.add_media(MediaSource::new(
            path,
            probe.width,
            probe.height,
            fps,
            frames,
            probe.has_audio,
        ));
        let track = project.add_track(TrackKind::Video, "Video");
        if let Err(e) = project.add_clip(
            track,
            media,
            TimeRange::at_rate(0, frames, fps),
            RationalTime::new(0, fps),
        ) {
            log::error!("preview video: add_clip failed: {e}");
            return None;
        }
        let renderer = match Renderer::new_headless() {
            Ok(renderer) => renderer,
            Err(e) => {
                log::error!("preview video: renderer init failed: {e}");
                return None;
            }
        };
        Some(Self {
            renderer,
            project,
            fps,
            duration_frames: frames,
        })
    }

    pub(crate) fn duration_seconds(&self) -> f64 {
        let fps = self.fps.as_f64();
        if fps <= 0.0 {
            return 0.0;
        }
        self.duration_frames as f64 / fps
    }

    pub(crate) fn render_seconds(&mut self, seconds: f64) -> Option<RgbaImage> {
        let max_frame = (self.duration_frames - 1).max(0);
        let frame = (seconds.max(0.0) * self.fps.as_f64()).round() as i64;
        let frame = frame.clamp(0, max_frame);
        match self
            .renderer
            .render_frame(&self.project, RationalTime::new(frame, self.fps))
        {
            Ok(image) => Some(image),
            Err(e) => {
                log::error!("preview render at frame {frame} failed: {e}");
                None
            }
        }
    }
}

/// An opaque RGBA sweep through the hue wheel as `t` runs 0->1 (always opaque).
fn hue_sweep(t: f32) -> [u8; 4] {
    let h = (t.rem_euclid(1.0) * 6.0).rem_euclid(6.0);
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        255,
    ]
}

/// Open the synthetic demo preview. Returns null if the GPU/renderer can't be
/// brought up. Free the handle with [`aria_preview_close`].
#[unsafe(no_mangle)]
pub extern "C" fn aria_preview_open_demo() -> *mut AriaPreview {
    match AriaPreview::demo() {
        Some(preview) => Box::into_raw(Box::new(preview)),
        None => std::ptr::null_mut(),
    }
}

/// Open a preview that scrubs the video at `path_utf8` (UTF-8, `path_len`
/// bytes). Returns null if the file can't be probed or the GPU is unavailable.
///
/// # Safety
/// `path_utf8` must point to `path_len` initialized bytes. The returned handle
/// is owned by the callee until passed to [`aria_preview_close`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_preview_open_video(
    path_utf8: *const u8,
    path_len: usize,
) -> *mut AriaPreview {
    if path_utf8.is_null() || path_len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `path_len` initialized bytes at `path_utf8`.
    let bytes = unsafe { std::slice::from_raw_parts(path_utf8, path_len) };
    let Ok(path) = std::str::from_utf8(bytes) else {
        return std::ptr::null_mut();
    };
    match AriaPreview::video(Path::new(path)) {
        Some(preview) => Box::into_raw(Box::new(preview)),
        None => std::ptr::null_mut(),
    }
}

/// Total scrub length in seconds. `0.0` for a null handle.
///
/// # Safety
/// `handle` must be null or a live pointer from `aria_preview_open_*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_preview_duration_seconds(handle: *const AriaPreview) -> f64 {
    if handle.is_null() {
        return 0.0;
    }
    // SAFETY: caller guarantees a live, exclusive-enough handle for a read.
    unsafe { &*handle }.duration_seconds()
}

/// Render the preview frame at `seconds` (clamped to the project's range).
///
/// On failure the returned [`AriaImage`] has `data == null`. Every non-null
/// result must be released exactly once with [`aria_image_free`].
///
/// # Safety
/// `handle` must be null or a live pointer from `aria_preview_open_*`, and
/// must not be used concurrently from another thread (render takes `&mut`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_preview_render(handle: *mut AriaPreview, seconds: f64) -> AriaImage {
    if handle.is_null() {
        return AriaImage::empty();
    }
    // SAFETY: caller guarantees a live handle used non-concurrently.
    let preview = unsafe { &mut *handle };
    match preview.render_seconds(seconds) {
        Some(img) => AriaImage::from_rgba(img),
        None => AriaImage::empty(),
    }
}

/// Free a handle from `aria_preview_open_*`. Null is a no-op.
///
/// # Safety
/// `handle` must be null or a value previously returned by an open call and not
/// already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_preview_close(handle: *mut AriaPreview) {
    if handle.is_null() {
        return;
    }
    // SAFETY: `handle` came from `Box::into_raw` and is freed exactly once.
    drop(unsafe { Box::from_raw(handle) });
}
