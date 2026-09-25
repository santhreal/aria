//! Animated-image decode from in-memory bytes (GIF / APNG / animated WebP).
//!
//! Stickers are the consumer: bundled assets are embedded byte slices, so the
//! entry point takes bytes rather than a path, and a whole animation decodes
//! up front into straight-alpha RGBA frames (sticker assets are small; the
//! renderer wants O(1) frame lookup on the hot path, not per-frame decode).
//!
//! Backends mirror the still-image split ([`crate::image`]):
//!
//! - **Apple**: ImageIO enumerates frames and per-frame delays for GIF, APNG,
//!   and animated WebP in one API.
//! - **Portable**: APNG via the `png` crate, GIF via the `gif` crate. Both
//!   formats deliver *sub-frame patches* that must be composited onto a
//!   canvas with per-frame blend/dispose rules; the shared [`Compositor`]
//!   below implements that state machine. Animated WebP has no portable
//!   decoder yet and errors as unsupported off-Apple.

use aria_core::{DecodeError, RgbaImage};

use crate::image::{ImageFormat, sniff_bytes};

/// One decoded animation frame: straight-alpha RGBA pixels plus how long the
/// frame displays. A static image decodes to a single frame.
#[derive(Debug, Clone)]
pub struct AnimationFrame {
    pub image: RgbaImage,
    /// Display duration in milliseconds (normalized by `normalize_delay_ms`).
    pub delay_ms: u32,
}

/// Most frames decoded from one animation; extras are ignored. Bounds worst-
/// case memory together with [`MAX_ANIMATION_DIMENSION`] (256 × 1024² RGBA ≈
/// 1 GB is still too big, but real stickers are two orders of magnitude
/// smaller on both axes; the caps only stop pathological files).
pub const MAX_ANIMATION_FRAMES: usize = 256;

/// Longest canvas side an animation may have. Unlike the still-image path
/// (which downscales), oversized animations error: there is no legitimate
/// sticker anywhere near this large.
pub const MAX_ANIMATION_DIMENSION: u32 = 1024;

/// Delay assigned when a file omits one or stores zero ("as fast as
/// possible"), matching the long-standing browser convention.
const DEFAULT_DELAY_MS: u32 = 100;

/// Map a stored delay to a usable one: missing/zero delays become
/// [`DEFAULT_DELAY_MS`].
pub(crate) fn normalize_delay_ms(ms: u32) -> u32 {
    if ms == 0 { DEFAULT_DELAY_MS } else { ms }
}

/// Decode an animation (or a static image, yielding one frame) from
/// in-memory bytes to straight-alpha RGBA frames with per-frame delays.
pub fn decode_animation(bytes: &[u8]) -> Result<Vec<AnimationFrame>, DecodeError> {
    let format = sniff_bytes(bytes).ok_or_else(|| {
        DecodeError::Open("embedded image: not a recognized image container".into())
    })?;
    let frames = backend_animation(bytes, format)?;
    if frames.is_empty() {
        return Err(DecodeError::Decode(
            "embedded image: animation decoded to zero frames".into(),
        ));
    }
    Ok(frames)
}

/// Largest animation file [`decode_animation_file`] will read into memory.
///
/// The frame and dimension caps bound what a decode produces; this bounds what
/// reading the file costs before any of them can apply. 64 MiB is an order of
/// magnitude above any real GIF or APNG and still nothing next to a video.
pub const MAX_ANIMATION_FILE_BYTES: u64 = 64 << 20;

/// [`decode_animation`] for a file the operator chose.
///
/// Every in-tree caller holds bytes already (stickers are embedded assets), so
/// the byte entry point came first; a file picked in the editor is a path, and
/// reading it needs the size check that a compiled-in asset does not.
pub fn decode_animation_file(path: &std::path::Path) -> Result<Vec<AnimationFrame>, DecodeError> {
    let length = std::fs::metadata(path)
        .map_err(|e| DecodeError::Io(format!("{}: {e}", path.display())))?
        .len();
    if length > MAX_ANIMATION_FILE_BYTES {
        return Err(DecodeError::unsupported(format!(
            "{}: {length} bytes is larger than the {MAX_ANIMATION_FILE_BYTES} byte animation cap",
            path.display()
        )));
    }
    let bytes =
        std::fs::read(path).map_err(|e| DecodeError::Io(format!("{}: {e}", path.display())))?;
    decode_animation(&bytes)
}

#[cfg(target_vendor = "apple")]
fn backend_animation(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<Vec<AnimationFrame>, DecodeError> {
    // ImageIO first, pure-Rust rescue — same policy as the still-image path.
    crate::image_apple::decode_animation_bytes(bytes)
        .or_else(|apple_err| portable_animation(bytes, format).map_err(|_| apple_err))
}

#[cfg(not(target_vendor = "apple"))]
fn backend_animation(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<Vec<AnimationFrame>, DecodeError> {
    portable_animation(bytes, format)
}

/// Decode every frame the portable backend can reach.
///
/// Which containers have a portable animated decoder is a column in the image
/// catalog, not a match here. A row with no animated decoder falls back to its
/// single-frame column, which is the right answer for a still container and the
/// honest first-frame answer for an animating one the portable backend cannot
/// enumerate — and a row with neither refuses by name.
fn portable_animation(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<Vec<AnimationFrame>, DecodeError> {
    match format.spec().portable_animate {
        Some(animate) => animate(bytes),
        None => Ok(vec![AnimationFrame {
            image: crate::image::portable::decode_bytes(bytes, format)?,
            delay_ms: DEFAULT_DELAY_MS,
        }]),
    }
}

/// How a sub-frame patch lands on the canvas.
enum Blend {
    /// Replace the region's pixels, alpha included (APNG `Source`).
    Source,
    /// Alpha-composite over the region (APNG `Over`; GIF's keep-transparent
    /// semantics are `Over` with binary alpha).
    Over,
}

/// What happens to the patched region after the frame displays.
enum Dispose {
    Keep,
    /// Clear the region to transparent black.
    Background,
    /// Restore the region to its pre-frame pixels.
    Previous,
}

/// Canvas state machine shared by the APNG and GIF paths: blit a patch,
/// snapshot the full canvas as the output frame, then apply disposal.
struct Compositor {
    width: u32,
    height: u32,
    canvas: Vec<u8>,
    /// The region a `Dispose::Previous` frame restores. Retained across frames:
    /// an animation that disposes to previous does so on every frame, and the
    /// region is the same size each time.
    saved: Vec<u8>,
}

impl Compositor {
    fn new(width: u32, height: u32) -> Result<Self, DecodeError> {
        if width == 0 || height == 0 {
            return Err(DecodeError::Decode(
                "embedded image: animation has zero dimensions".into(),
            ));
        }
        if width.max(height) > MAX_ANIMATION_DIMENSION {
            return Err(DecodeError::Decode(format!(
                "embedded image: animation is {width}x{height}, larger than the \
                 {MAX_ANIMATION_DIMENSION} px cap"
            )));
        }
        Ok(Self {
            width,
            height,
            canvas: vec![0; width as usize * height as usize * 4],
            saved: Vec::new(),
        })
    }

    /// Apply one sub-frame patch (`pixels` is `w`×`h` straight-alpha RGBA at
    /// offset `(x, y)`) and return the resulting full frame.
    fn frame(
        &mut self,
        (x, y, w, h): (u32, u32, u32, u32),
        pixels: &[u8],
        blend: Blend,
        dispose: Dispose,
    ) -> RgbaImage {
        if matches!(dispose, Dispose::Previous) {
            self.save_region(x, y, w, h);
        }
        self.blit(x, y, w, h, pixels, blend);
        let snapshot = RgbaImage::new(self.width, self.height, self.canvas.clone());
        match dispose {
            Dispose::Keep => {}
            Dispose::Background => self.clear_region(x, y, w, h),
            Dispose::Previous => self.paste_saved_region(x, y, w, h),
        }
        snapshot
    }

    /// Rows of the region that actually intersect the canvas, as
    /// `(canvas_range, patch_range)` byte ranges per row.
    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, pixels: &[u8], blend: Blend) {
        let (cw, ch) = (self.width as usize, self.height as usize);
        for (canvas_range, patch_range) in region_row_ranges(cw, ch, x, y, w, h) {
            let dst = &mut self.canvas[canvas_range];
            let src = &pixels[patch_range];
            match blend {
                Blend::Source => dst.copy_from_slice(src),
                Blend::Over => {
                    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                        over(d, s);
                    }
                }
            }
        }
    }

    /// Copy the region into the retained scratch, replacing whatever the last
    /// disposal saved.
    fn save_region(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let (cw, ch) = (self.width as usize, self.height as usize);
        let Self { canvas, saved, .. } = self;
        saved.clear();
        for (canvas_range, _) in region_row_ranges(cw, ch, x, y, w, h) {
            saved.extend_from_slice(&canvas[canvas_range]);
        }
    }

    fn paste_saved_region(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let (cw, ch) = (self.width as usize, self.height as usize);
        let Self { canvas, saved, .. } = self;
        let mut offset = 0;
        for (canvas_range, _) in region_row_ranges(cw, ch, x, y, w, h) {
            let len = canvas_range.len();
            let Some(row) = saved.get(offset..offset + len) else {
                break;
            };
            canvas[canvas_range].copy_from_slice(row);
            offset += len;
        }
    }

    fn clear_region(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let (cw, ch) = (self.width as usize, self.height as usize);
        for (canvas_range, _) in region_row_ranges(cw, ch, x, y, w, h) {
            self.canvas[canvas_range].fill(0);
        }
    }
}

/// `(canvas_range, patch_range)` byte ranges per row without borrowing Compositor.
fn region_row_ranges(
    canvas_w: usize,
    canvas_h: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> impl Iterator<Item = (std::ops::Range<usize>, std::ops::Range<usize>)> {
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    let visible_w = w.min(canvas_w.saturating_sub(x));
    let visible_h = h.min(canvas_h.saturating_sub(y));
    (0..visible_h).map(move |row| {
        let canvas_start = ((y + row) * canvas_w + x) * 4;
        let patch_start = row * w * 4;
        (
            canvas_start..canvas_start + visible_w * 4,
            patch_start..patch_start + visible_w * 4,
        )
    })
}

/// Straight-alpha source-over: `out = src OVER dst`, in place on `dst`.
fn over(dst: &mut [u8], src: &[u8]) {
    let sa = src[3] as u32;
    if sa == 255 {
        dst.copy_from_slice(src);
        return;
    }
    if sa == 0 {
        return;
    }
    let da = dst[3] as u32;
    // out_a = sa + da*(1-sa), in 0..=255*255 fixed point.
    let out_a = sa * 255 + da * (255 - sa);
    for c in 0..3 {
        let sc = src[c] as u32;
        let dc = dst[c] as u32;
        // out_c = (sc*sa + dc*da*(1-sa)/255) / out_a, rounded.
        let num = sc * sa * 255 + dc * da * (255 - sa);
        dst[c] = ((num + out_a / 2) / out_a) as u8;
    }
    dst[3] = ((out_a + 127) / 255) as u8;
}

/// APNG (and plain PNG, as a single frame) via the `png` crate.
pub(crate) fn apng_decode(bytes: &[u8]) -> Result<Vec<AnimationFrame>, DecodeError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| DecodeError::Open(format!("embedded png: {e}")))?;

    let Some(actl) = reader.info().animation_control().copied() else {
        // Plain PNG: one frame.
        return Ok(vec![AnimationFrame {
            image: crate::image::portable::decode_bytes(bytes, ImageFormat::Png)?,
            delay_ms: DEFAULT_DELAY_MS,
        }]);
    };
    let mut compositor = Compositor::new(reader.info().width, reader.info().height)?;
    let mut buf = vec![0u8; reader.output_buffer_size()];

    // When no fcTL precedes IDAT, the default image is not part of the
    // animation: decode and discard it so the loop below sees frames only.
    if reader.info().frame_control().is_none() {
        reader
            .next_frame(&mut buf)
            .map_err(|e| DecodeError::Decode(format!("embedded png: {e}")))?;
    }

    let mut frames = Vec::new();
    for index in 0..(actl.num_frames as usize).min(MAX_ANIMATION_FRAMES) {
        let out = reader
            .next_frame(&mut buf)
            .map_err(|e| DecodeError::Decode(format!("embedded png frame {index}: {e}")))?;
        let fc = *reader.info().frame_control().ok_or_else(|| {
            DecodeError::Decode(format!("embedded png frame {index}: missing frame control"))
        })?;
        let pixels = rgba_from_png(&buf[..out.buffer_size()], out.color_type)
            .map_err(|e| DecodeError::Decode(format!("embedded png frame {index}: {e}")))?;
        let dispose = match fc.dispose_op {
            // Spec: PREVIOUS on the first frame is treated as BACKGROUND.
            png::DisposeOp::Previous if index == 0 => Dispose::Background,
            png::DisposeOp::Previous => Dispose::Previous,
            png::DisposeOp::Background => Dispose::Background,
            png::DisposeOp::None => Dispose::Keep,
        };
        let blend = match fc.blend_op {
            png::BlendOp::Source => Blend::Source,
            png::BlendOp::Over => Blend::Over,
        };
        let image = compositor.frame(
            (fc.x_offset, fc.y_offset, fc.width, fc.height),
            &pixels,
            blend,
            dispose,
        );
        let den = if fc.delay_den == 0 { 100 } else { fc.delay_den };
        let delay_ms = (u32::from(fc.delay_num) * 1000 + u32::from(den) / 2) / u32::from(den);
        frames.push(AnimationFrame {
            image,
            delay_ms: normalize_delay_ms(delay_ms),
        });
    }
    Ok(frames)
}

/// Expand a `normalize_to_color8` PNG frame buffer to RGBA.
fn rgba_from_png(buf: &[u8], color_type: png::ColorType) -> Result<Vec<u8>, String> {
    fn expand(data: &[u8], channels: usize, map: impl Fn(&[u8]) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() / channels * 4);
        for px in data.chunks_exact(channels) {
            out.extend_from_slice(&map(px));
        }
        out
    }
    match color_type {
        png::ColorType::Rgba => Ok(buf.to_vec()),
        png::ColorType::Rgb => Ok(expand(buf, 3, |px| [px[0], px[1], px[2], 255])),
        png::ColorType::Grayscale => Ok(expand(buf, 1, |px| [px[0], px[0], px[0], 255])),
        png::ColorType::GrayscaleAlpha => Ok(expand(buf, 2, |px| [px[0], px[0], px[0], px[1]])),
        png::ColorType::Indexed => Err("palette not expanded".into()),
    }
}

/// GIF via the `gif` crate.
pub(crate) fn gif_decode(bytes: &[u8]) -> Result<Vec<AnimationFrame>, DecodeError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(std::io::Cursor::new(bytes))
        .map_err(|e| DecodeError::Open(format!("embedded gif: {e}")))?;
    let mut compositor = Compositor::new(decoder.width().into(), decoder.height().into())?;

    let mut frames: Vec<AnimationFrame> = Vec::new();
    while frames.len() < MAX_ANIMATION_FRAMES {
        let frame = match decoder.read_next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) => {
                return Err(DecodeError::Decode(format!(
                    "embedded gif frame {}: {e}",
                    frames.len()
                )));
            }
        };
        let dispose = match frame.dispose {
            gif::DisposalMethod::Any | gif::DisposalMethod::Keep => Dispose::Keep,
            gif::DisposalMethod::Background => Dispose::Background,
            gif::DisposalMethod::Previous => Dispose::Previous,
        };
        let image = compositor.frame(
            (
                frame.left.into(),
                frame.top.into(),
                frame.width.into(),
                frame.height.into(),
            ),
            &frame.buffer,
            // GIF transparency means "keep the underlying pixel": exactly
            // source-over with the decoder's binary alpha.
            Blend::Over,
            dispose,
        );
        frames.push(AnimationFrame {
            image,
            delay_ms: normalize_delay_ms(u32::from(frame.delay) * 10),
        });
    }
    Ok(frames)
}

#[cfg(test)]
#[path = "animation_tests.rs"]
mod tests;
