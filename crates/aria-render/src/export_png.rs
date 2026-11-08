//! The codec-free sink, the two PNG boundaries around it, and the
//! single-frame PNG export.
//!
//! Re-exported from the parent, so `crate::export::PngSequenceEncoder` and
//! `crate::export::decode_png` still name these.

use std::path::{Path, PathBuf};

use aria_core::{
    EncodeError, EncoderChoice, PixelFormat, Plane, RationalTime, RgbaImage, VideoEncoder,
    VideoFrame,
};
use aria_models::Project;

use crate::error::RenderError;
use crate::render::Renderer;
use crate::transactional_file::TransactionalFile;

/// A portable [`VideoEncoder`] that writes each pushed frame as a numbered PNG
/// (`frame_00000.png`, `frame_00001.png`, …) into a directory.
///
/// Pure-Rust and platform-independent: it proves the whole render→export loop
/// without a native codec, and gives an eyeballable result. It accepts only the
/// packed-RGBA frames [`super::export`] produces.
#[derive(Debug)]
pub struct PngSequenceEncoder {
    dir: PathBuf,
    index: u64,
}

impl PngSequenceEncoder {
    /// Create the output directory (if needed) and start at frame 0.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, EncodeError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| {
            EncodeError::Start(format!("{}: {}", dir.display(), aria_models::io_reason(&e)))
        })?;
        Ok(Self { dir, index: 0 })
    }

    /// Number of frames written so far.
    pub fn frames_written(&self) -> u64 {
        self.index
    }
}

impl VideoEncoder for PngSequenceEncoder {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(EncodeError::unsupported(format!(
                "PNG sink expects Rgba8 frames, got {:?}",
                frame.format
            )));
        }
        let cpu = frame
            .cpu()
            .ok_or_else(|| EncodeError::unsupported("PNG sink needs CPU frames"))?;
        let plane = cpu
            .planes
            .first()
            .ok_or_else(|| EncodeError::Encode("frame has no plane".into()))?;

        let path = self.dir.join(format!("frame_{:05}.png", self.index));
        write_plane_png(&path, frame.width(), frame.height(), plane)?;
        self.index += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }

    fn choice(&self) -> EncoderChoice {
        EncoderChoice::only("png", Some(false))
    }
}

/// Encode one tightly packed RGBA8 image as an in-memory PNG.
///
/// Agent vision tools use this boundary rather than depending on `png`
/// themselves: rendered frames stay [`RgbaImage`] until the provider needs
/// encoded bytes, and every caller gets the same well-formedness check.
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, RenderError> {
    if !image.is_well_formed() {
        return Err(RenderError::unsupported(format!(
            "RGBA image is {}x{} but carries {} bytes",
            image.width,
            image.height,
            image.pixels.len()
        )));
    }
    encode_rgba8(image.width, image.height, &image.pixels)
        .map_err(|e| RenderError::Encode(EncodeError::Encode(e.to_string())))
}

/// PNG bytes for `width` x `height` tightly packed RGBA8 `pixels`.
///
/// `finish` writes the closing chunk and reports its error; a dropped
/// `png::Writer` writes it in `Drop` and discards the error.
fn encode_rgba8(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()?;
    Ok(bytes)
}

/// Encode `image` as an 8-bit RGBA PNG and publish it to `path` atomically.
///
/// Encoding and writing run in a sibling staging directory and replace `path`
/// only after the PNG is complete, so a failed encode or write leaves whatever
/// was at `path` as it was.
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), RenderError> {
    let bytes = encode_png(image)?;
    let staged = TransactionalFile::new(path)?;
    std::fs::write(staged.staging_path(), &bytes)
        .map_err(|error| aria_models::path_error(path, &error))?;
    staged.publish()?;
    Ok(())
}

/// Render the frame `project` shows at `at`, at the canvas size, and write it
/// to `path` as a PNG. Returns the image's width and height.
///
/// The frame matches what an export renders at that time, motion blur
/// included. The file is published atomically: a failed render, encode, or
/// write leaves whatever was at `path` as it was.
pub fn export_frame_png(
    renderer: &mut Renderer,
    project: &Project,
    at: RationalTime,
    path: &Path,
) -> Result<(u32, u32), RenderError> {
    crate::export::check_export_destination(project, path, None)?;
    let image = renderer.exporting(|renderer| {
        renderer
            .resolve_frame(project, at)
            .and_then(|scene| renderer.render_scene(project, &scene))
    })?;
    write_png(&image, path)?;
    Ok((image.width, image.height))
}

/// Decode one PNG into tightly packed straight-alpha RGBA8 pixels.
///
/// This is the inverse boundary used by desktop transcript previews. Input
/// is bounded before allocation so an image returned by an extensible host
/// tool cannot turn into an unbounded UI-thread allocation.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, RenderError> {
    const MAX_EDGE: u32 = 2048;
    const MAX_PIXELS: u64 = 4 * 1024 * 1024;
    const DECODER_BUDGET: usize = 24 * 1024 * 1024;

    let limits = png::Limits {
        bytes: DECODER_BUDGET,
    };
    let mut decoder = png::Decoder::new_with_limits(bytes, limits);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| RenderError::unsupported(format!("invalid PNG header: {error}")))?;
    let (width, height) = (reader.info().width, reader.info().height);
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| RenderError::unsupported("PNG dimensions overflow"))?;
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE || pixels > MAX_PIXELS {
        return Err(RenderError::unsupported(format!(
            "PNG dimensions {width}x{height} exceed the preview bound"
        )));
    }

    let mut decoded = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|error| RenderError::unsupported(format!("invalid PNG frame: {error}")))?;
    let source = &decoded[..info.buffer_size()];
    let capacity = usize::try_from(pixels)
        .ok()
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| RenderError::unsupported("PNG RGBA allocation size overflow"))?;
    let mut rgba = Vec::with_capacity(capacity);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(source),
        png::ColorType::Rgb => {
            for pixel in source.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &value in source {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in source.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => {
            return Err(RenderError::unsupported(
                "indexed PNG was not expanded by the decoder",
            ));
        }
    }
    if rgba.len() != capacity {
        return Err(RenderError::unsupported(format!(
            "decoded PNG is {width}x{height} but carries {} RGBA bytes",
            rgba.len()
        )));
    }
    Ok(RgbaImage::new(width, height, rgba))
}

/// Write one RGBA8 plane to an 8-bit PNG, compacting away any row padding.
///
/// The PNG is encoded in memory and written with one call, so a write that
/// fails at any byte, the last included, fails the frame.
fn write_plane_png(path: &Path, width: u32, height: u32, plane: &Plane) -> Result<(), EncodeError> {
    let row_bytes = width as usize * 4;
    let failed = |reason: String| EncodeError::Encode(format!("{}: {reason}", path.display()));
    let tight;
    let pixels = if plane.stride == row_bytes {
        &plane.data[..row_bytes * height as usize]
    } else {
        // Decoder/compositor readback may pad rows; PNG wants tight rows.
        let mut rows = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * plane.stride;
            rows.extend_from_slice(&plane.data[start..start + row_bytes]);
        }
        tight = rows;
        &tight[..]
    };
    let bytes = encode_rgba8(width, height, pixels).map_err(|e| failed(e.to_string()))?;
    std::fs::write(path, bytes).map_err(|e| failed(aria_models::io_reason(&e)))
}
