//! Filmstrip thumbnails over the C ABI.
//!
//! Clip filmstrips and picker cells want many small frames from one media
//! file, and they must not contend with the editor session for its renderer.
//! An `AriaThumbnailer` is bound to a single media file and renders through a
//! process-wide pool of private renderers (each with its own GPU queue and
//! decoder cache), apart from the session's, so the shell can run it from any
//! worker queue: thumbnails decode while the session keeps scrubbing.
//!
//! Same non-concurrency rule as the session: calls on one handle must be
//! serialized, but different handles are independent.
//! The module exports `#[unsafe(no_mangle)]` C ABI functions that box, borrow, and free raw thumbnailer handles and is `unsafe` throughout.

#![allow(unsafe_code)]

use parking_lot::Mutex;

use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::Renderer;

use crate::AriaImage;
use crate::wire::str_arg;

static RENDERER_POOL: Mutex<Vec<Renderer>> = Mutex::new(Vec::new());
const MAX_POOLED_RENDERERS: usize = 4;

/// Run `f` on a pooled renderer, then return the renderer to the pool. `None`
/// when the pool is empty and no headless GPU is available.
///
/// A panic in `f` drops the renderer instead of pooling it; a panic aborts at
/// the C ABI boundary regardless.
fn with_pooled_renderer<R>(f: impl FnOnce(&mut Renderer) -> R) -> Option<R> {
    let pooled = RENDERER_POOL.lock().pop();
    let mut renderer = match pooled {
        Some(renderer) => renderer,
        None => Renderer::with_gpu(aria_compositor::GpuContext::new_headless_blocking().ok()?),
    };
    let out = f(&mut renderer);
    let mut pool = RENDERER_POOL.lock();
    if pool.len() < MAX_POOLED_RENDERERS {
        pool.push(renderer);
    }
    Some(out)
}

/// A thumbnail source for one media file. Free with
/// [`aria_thumbnailer_close`].
#[derive(Debug)]
pub struct AriaThumbnailer {
    /// A single-clip project spanning the whole file — thumbnails are just
    /// fit-rendered frames of it, riding the same resolve/render path as the
    /// editor so orientation/aspect handling can never drift.
    project: Project,
    frame_rate: Rational,
    duration_frames: i64,
}
impl AriaThumbnailer {
    fn open(path: &str) -> Option<Self> {
        let path = std::path::Path::new(path);
        let probe = match aria_decoder::probe(path) {
            Ok(probe) => probe,
            Err(e) => {
                log::error!("thumbnailer: probe failed for {}: {e}", path.display());
                return None;
            }
        };
        let source = if probe.is_image {
            // One cached frame for any slot: the still convention gives the
            // filmstrip a nominal 5s window so slot spacing math stays sane.
            MediaSource::image(path, probe.width, probe.height)
        } else {
            MediaSource::new(
                path,
                probe.width,
                probe.height,
                probe.frame_rate,
                probe.frame_count.max(1),
                probe.has_audio,
            )
        };
        let frame_rate = source.frame_rate;
        let frames = source.duration.value.max(1);
        let mut project = Project::new("thumbnailer", frame_rate);
        let media = project.add_media(source);
        let track = project.add_track(TrackKind::Video, "Media");
        if let Err(e) = project.add_clip(
            track,
            media,
            TimeRange::at_rate(0, frames, frame_rate),
            RationalTime::new(0, frame_rate),
        ) {
            log::error!("thumbnailer: add_clip failed: {e}");
            return None;
        }
        // The probe leaves its renderer pooled for the first thumbnail.
        if with_pooled_renderer(|_| ()).is_none() {
            log::error!("thumbnailer: GPU context unavailable");
            return None;
        }
        Some(Self {
            project,
            frame_rate,
            duration_frames: frames,
        })
    }

    fn thumb(&mut self, seconds: f64, max_width: u32, max_height: u32) -> Option<AriaImage> {
        let max_frame = (self.duration_frames - 1).max(0);
        let frame = crate::intents::frame_tick(seconds, self.frame_rate).clamp(0, max_frame);
        let rendered = with_pooled_renderer(|renderer| {
            renderer.render_frame_fit(
                &self.project,
                RationalTime::new(frame, self.frame_rate),
                max_width,
                max_height,
            )
        })?;
        match rendered {
            Ok(image) => Some(AriaImage::from_rgba(image)),
            Err(e) => {
                log::error!("thumbnailer: render at frame {frame} failed: {e}");
                None
            }
        }
    }
}

/// Open a thumbnailer for the media file at `path_utf8` (UTF-8, `path_len`
/// bytes). Null if the file can't be probed or the GPU is unavailable.
///
/// # Safety
/// `path_utf8` must point to `path_len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_thumbnailer_open(
    path_utf8: *const u8,
    path_len: usize,
) -> *mut AriaThumbnailer {
    // SAFETY: caller guarantees the byte range.
    let Some(path) = (unsafe { str_arg(path_utf8, path_len) }) else {
        return std::ptr::null_mut();
    };
    match AriaThumbnailer::open(path) {
        Some(thumbnailer) => Box::into_raw(Box::new(thumbnailer)),
        None => std::ptr::null_mut(),
    }
}

/// Media length in seconds (0 for a null handle) — lets the shell space
/// filmstrip slots without a second probe.
///
/// # Safety
/// `handle` must be null or a live thumbnailer pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_thumbnailer_duration_seconds(handle: *const AriaThumbnailer) -> f64 {
    // SAFETY: caller contract.
    let Some(thumbnailer) = (unsafe { handle.as_ref() }) else {
        return 0.0;
    };
    RationalTime::new(thumbnailer.duration_frames, thumbnailer.frame_rate).seconds()
}

/// Render the frame nearest `seconds`, scaled to fit `max_width`×`max_height`
/// (never upscaled). On failure the returned [`AriaImage`] has
/// `data == null`; free non-null results with [`crate::aria_image_free`].
///
/// # Safety
/// `handle` must be null or a live thumbnailer pointer used non-concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_thumbnailer_thumb(
    handle: *mut AriaThumbnailer,
    seconds: f64,
    max_width: u32,
    max_height: u32,
) -> AriaImage {
    // SAFETY: caller contract.
    let Some(thumbnailer) = (unsafe { handle.as_mut() }) else {
        return AriaImage::empty();
    };
    if max_width == 0 || max_height == 0 {
        return AriaImage::empty();
    }
    thumbnailer
        .thumb(seconds, max_width, max_height)
        .unwrap_or_else(AriaImage::empty)
}

/// Release a thumbnailer handle. Null is a no-op.
///
/// # Safety
/// `handle` must be null or a live thumbnailer pointer, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_thumbnailer_close(handle: *mut AriaThumbnailer) {
    if !handle.is_null() {
        // SAFETY: came from Box::into_raw, freed exactly once.
        drop(unsafe { Box::from_raw(handle) });
    }
}
