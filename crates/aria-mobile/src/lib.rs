//! The C ABI mobile hosts call.
//!
//! One crate per boundary: everything an iOS or Android host needs is a
//! `extern "C"` function here, and nothing here decides editing policy. A
//! session (`session`) holds a project and applies intents (`intents`); the
//! rest are the read paths a host draws with — interactive preview
//! (`mobile_preview`), thumbnails (`thumbs`), audio (`audio`), export jobs
//! (`export_job`), the projected view model (`ui_state`), the wire vocabulary
//! (`wire`), and the shipped catalogs (`catalogs`).
//!
//! Every handle is opened by an `aria_*_open*` call and freed by its matching
//! `aria_*_close`/`aria_*_free`; a null handle is a no-op rather than a panic,
//! because an unwind across the ABI is undefined. Rendering needs a GPU, so an
//! open call returns null on a device that cannot bring one up.
#[cfg(target_os = "android")]
pub mod android;
pub mod audio;
pub mod catalogs;
pub mod export_job;
pub mod intents;
pub mod session;
pub mod thumbs;
pub mod ui_state;
pub mod wire;

#[path = "mobile_preview.rs"]
mod mobile_preview;

#[cfg(target_os = "android")]
#[path = "android_jni.rs"]
mod android_jni;

pub use audio::AriaAudioReader;
pub use export_job::AriaExportJob;
pub use session::AriaSession;
pub use thumbs::AriaThumbnailer;
// The C ABI entry points are re-exported at the crate root, which is the path
// the header generator and the Rust callers both use; the module itself stays
// private so there is one path per symbol.
pub use mobile_preview::{
    AriaPreview, aria_preview_close, aria_preview_duration_seconds, aria_preview_open_demo,
    aria_preview_open_video, aria_preview_render,
};

use std::path::Path;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, RgbaImage,
};
use aria_decoder::OutputMode;
use aria_models::{Generator, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::Renderer;

/// Render the built-in demo scene at `width`×`height`. `None` if the dimensions
/// are degenerate or the GPU device/compositor can't be brought up.
///
/// The scene is solid layers only (no decode, no fonts) so it renders
/// identically on every device and isolates "does this device's GPU pipeline
/// work" from codec/asset concerns.
fn render_demo_rgba(width: u32, height: u32) -> Option<RgbaImage> {
    if width == 0 || height == 0 {
        return None;
    }

    let gpu = match GpuContext::new_headless_blocking() {
        Ok(gpu) => gpu,
        Err(e) => {
            log::error!("demo: GPU init failed: {e}");
            return None;
        }
    };
    let mut compositor = Compositor::new(&gpu);

    let config = CompositorConfig::new(width, height).with_background([18, 22, 30, 255]);
    let (w, h) = (width as f32, height as f32);
    let layers = [
        // A large centered block — fills most of the canvas.
        CompositeLayer::solid(
            [235, 78, 58, 255],
            LayerPlacement {
                center: [w * 0.5, h * 0.5],
                size: [w * 0.62, h * 0.62],
                rotation: 0.0,
                opacity: 1.0,
            },
        ),
        // A teal square in the upper-left quadrant (proves placement + y-down).
        CompositeLayer::solid(
            [46, 196, 182, 255],
            LayerPlacement {
                center: [w * 0.3, h * 0.32],
                size: [w * 0.2, w * 0.2],
                rotation: 0.0,
                opacity: 1.0,
            },
        ),
        // A rotated, slightly translucent amber square lower-right (proves
        // rotation + alpha blending).
        CompositeLayer::solid(
            [255, 196, 61, 235],
            LayerPlacement {
                center: [w * 0.7, h * 0.7],
                size: [w * 0.2, w * 0.2],
                rotation: 0.6,
                opacity: 1.0,
            },
        ),
    ];

    compositor.render(&gpu, &config, &layers).ok()
}

/// Fit `src` inside `max` preserving aspect ratio, never upscaling. Always
/// returns at least 1×1.
fn fit_within(src: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let (sw, sh) = src;
    let (mw, mh) = (max.0.max(1), max.1.max(1));
    if sw == 0 || sh == 0 {
        return (mw, mh);
    }
    let scale = (f64::from(mw) / f64::from(sw))
        .min(f64::from(mh) / f64::from(sh))
        .min(1.0);
    let w = ((f64::from(sw) * scale).round() as u32).max(1);
    let h = ((f64::from(sh) * scale).round() as u32).max(1);
    (w, h)
}

/// Decode the first frame of `path` in `mode` and composite it full-canvas onto
/// a canvas that fits within `max`. `None` on any decode/composite failure.
fn compose_first_frame(
    gpu: &GpuContext,
    compositor: &mut Compositor,
    path: &Path,
    mode: OutputMode,
    max: (u32, u32),
) -> Option<RgbaImage> {
    let mut decoder = aria_decoder::open_video_decoder(path, mode).ok()?;
    let display = decoder.info().display_size;
    let frame = decoder.next_frame().ok()??;

    let (cw, ch) = fit_within(display, max);
    let config = CompositorConfig::new(cw, ch);
    let layer = CompositeLayer::frame(&frame, LayerPlacement::full_canvas(&config));
    compositor.render(gpu, &config, &[layer]).ok()
}

/// Decode + composite the first frame of the video at `path`, scaled to fit
/// `max_width`×`max_height`. `None` if the file can't be opened/decoded or the
/// GPU is unavailable.
///
/// Tries the zero-copy GPU decode path first (the fast path on Apple) and falls
/// back to the portable CPU path — so this automatically picks up zero-copy on
/// platforms once their import lands, with no caller change.
fn render_file_frame_rgba(path: &Path, max_width: u32, max_height: u32) -> Option<RgbaImage> {
    if max_width == 0 || max_height == 0 {
        return None;
    }
    let gpu = GpuContext::new_headless_blocking().ok()?;
    let mut compositor = Compositor::new(&gpu);
    let max = (max_width, max_height);

    compose_first_frame(&gpu, &mut compositor, path, OutputMode::Gpu, max)
        .or_else(|| compose_first_frame(&gpu, &mut compositor, path, OutputMode::Cpu, max))
}

/// An RGBA8 image handed across the C ABI. `data` points at `len`
/// (`= width * height * 4`) bytes; `data` is null when rendering failed.
#[derive(Debug)]
#[repr(C)]
pub struct AriaImage {
    pub data: *mut u8,
    pub len: usize,
    pub width: u32,
    pub height: u32,
}

impl AriaImage {
    fn empty() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
            width: 0,
            height: 0,
        }
    }

    fn from_rgba(img: RgbaImage) -> Self {
        let width = img.width;
        let height = img.height;
        let boxed: Box<[u8]> = img.pixels.into_boxed_slice();
        let len = boxed.len();
        let data = Box::into_raw(boxed) as *mut u8;
        Self {
            data,
            len,
            width,
            height,
        }
    }
}

/// Render the demo scene to a freshly allocated RGBA8 buffer.
///
/// On failure the returned [`AriaImage`] has `data == null`. Every non-null
/// result must be released exactly once with [`aria_image_free`].
///
/// # Safety
/// The returned buffer is owned by the callee until passed back to
/// [`aria_image_free`]; do not free it any other way.
// SAFETY: `aria_render_demo` is a unique C ABI export symbol of `aria-mobile`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn aria_render_demo(width: u32, height: u32) -> AriaImage {
    match render_demo_rgba(width, height) {
        Some(img) => AriaImage::from_rgba(img),
        None => AriaImage::empty(),
    }
}

/// Decode + composite the first frame of the video file at `path_utf8` (a UTF-8
/// path of `path_len` bytes), scaled to fit `max_width`×`max_height`.
///
/// On failure the returned [`AriaImage`] has `data == null`. Every non-null
/// result must be released exactly once with [`aria_image_free`].
///
/// # Safety
/// `path_utf8` must point to `path_len` initialized bytes (no NUL terminator
/// required). The returned buffer is owned by the callee until freed.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_render_file_frame(
    path_utf8: *const u8,
    path_len: usize,
    max_width: u32,
    max_height: u32,
) -> AriaImage {
    if path_utf8.is_null() || path_len == 0 {
        return AriaImage::empty();
    }
    // SAFETY: caller guarantees `path_len` initialized bytes at `path_utf8`.
    let bytes = unsafe { std::slice::from_raw_parts(path_utf8, path_len) };
    let Ok(path) = std::str::from_utf8(bytes) else {
        return AriaImage::empty();
    };
    match render_file_frame_rgba(Path::new(path), max_width, max_height) {
        Some(img) => AriaImage::from_rgba(img),
        None => AriaImage::empty(),
    }
}

/// Free a buffer returned by [`aria_render_demo`]. A null/empty image is a
/// no-op, so double-free of an already-empty handle is safe.
///
/// # Safety
/// `img` must be a value previously returned by [`aria_render_demo`] and not
/// already freed; `data`/`len` must not have been modified.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aria_image_free(img: AriaImage) {
    if img.data.is_null() || img.len == 0 {
        return;
    }
    // SAFETY: `data`/`len` came from `Box::<[u8]>::into_raw` in `from_rgba`.
    unsafe {
        let slice = std::slice::from_raw_parts_mut(img.data, img.len);
        drop(Box::from_raw(slice as *mut [u8]));
    }
}
