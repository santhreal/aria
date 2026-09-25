//! The codec-free sink and the two PNG boundaries around it.
//!
//! Moved out of `export.rs` to stay under the file ceiling; nothing here
//! changed. Re-exported from the parent, so `crate::export::PngSequenceEncoder`
//! and `crate::export::decode_png` still name these.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use aria_core::{EncodeError, PixelFormat, Plane, RgbaImage, VideoEncoder, VideoFrame};

use crate::error::RenderError;

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
        std::fs::create_dir_all(&dir).map_err(|e| EncodeError::Start(e.to_string()))?;
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
        write_png(&path, frame.width(), frame.height(), plane)?;
        self.index += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        Ok(())
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
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| RenderError::Encode(EncodeError::Encode(e.to_string())))?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|e| RenderError::Encode(EncodeError::Encode(e.to_string())))?;
    }
    Ok(bytes)
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
            for pixel in source.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &value in source {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in source.chunks_exact(2) {
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
fn write_png(path: &Path, width: u32, height: u32, plane: &Plane) -> Result<(), EncodeError> {
    let row_bytes = width as usize * 4;
    let file = File::create(path).map_err(|e| EncodeError::Encode(e.to_string()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| EncodeError::Encode(e.to_string()))?;

    if plane.stride == row_bytes {
        writer
            .write_image_data(&plane.data[..row_bytes * height as usize])
            .map_err(|e| EncodeError::Encode(e.to_string()))?;
    } else {
        // Decoder/compositor readback may pad rows; PNG wants tight rows.
        let mut tight = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * plane.stride;
            tight.extend_from_slice(&plane.data[start..start + row_bytes]);
        }
        writer
            .write_image_data(&tight)
            .map_err(|e| EncodeError::Encode(e.to_string()))?;
    }
    Ok(())
}
