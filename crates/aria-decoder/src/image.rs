//! Still-image probe and decode.
//!
//! Photos are half of every mobile media picker, so stills get the same
//! FFmpeg-free treatment as video: sniff the container by magic bytes (never
//! by file extension), read dimensions without decoding for the media pool,
//! and decode the single frame to straight-alpha RGBA when the renderer
//! actually composites it.
//!
//! Backends mirror the video decoder split:
//!
//! - **Apple** (`image_apple`): ImageIO / `CGImageSource`, which
//!   covers HEIC/HEIF (the iPhone camera default), WebP, GIF, TIFF, and BMP
//!   on top of PNG/JPEG, and applies EXIF orientation.
//! - **Portable** ([`portable`]): pure-Rust PNG + JPEG, compiled everywhere.
//!   It is the primary path on platforms with no native image backend yet
//!   and the fallback behind ImageIO on Apple.

use std::io::Read;
use std::path::Path;

use aria_core::{DecodeError, RgbaImage};

/// The container catalog every question below reads: what a header means, what a
/// dialog offers, and which pure-Rust decoder exists for it. A child module so
/// the rows stay beside the enum they describe.
#[path = "image_catalog.rs"]
pub mod catalog;

pub use catalog::{ImageFormatSpec, every_image_format, image_extensions};

/// Image containers recognized by [`sniff_image`]'s magic-byte check.
///
/// Recognition is deliberately broader than what every backend can decode:
/// classifying a file as a still is what routes it away from the video
/// probe, and an unsupported-format error from the image path is more
/// truthful than a demuxer error from the video path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
    /// HEIF family (HEIC/AVIF): ISO-BMFF `ftyp` with an image major brand.
    Heif,
    Gif,
    Bmp,
    Tiff,
}

/// Static metadata for a still image, read without decoding pixels.
///
/// Dimensions are *display* dimensions: the Apple backend swaps
/// width/height for EXIF orientations that rotate by 90°, matching what
/// [`decode_image`] produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

/// Bytes to read for [`sniff_image`]: enough for every magic we check
/// (the longest probe is the 12-byte ISO-BMFF `ftyp` + brand window).
const SNIFF_LEN: u64 = 16;

/// Classify `path` as a still image by magic bytes, or `None` if the header
/// matches no known image container (including unreadable/short files).
pub fn sniff_image(path: &Path) -> Option<ImageFormat> {
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::with_capacity(SNIFF_LEN as usize);
    file.take(SNIFF_LEN).read_to_end(&mut head).ok()?;
    sniff_bytes(&head)
}

/// [`sniff_image`] on an in-memory header (at least `SNIFF_LEN` bytes of
/// the file, shorter is tolerated).
pub fn sniff_bytes(head: &[u8]) -> Option<ImageFormat> {
    catalog::SHIPPED
        .iter()
        .find(|spec| (spec.magic)(head))
        .map(|spec| spec.format)
}

/// Read `path`'s display dimensions without decoding pixels.
///
/// Errors with [`DecodeError::Open`] when the file isn't a recognized image,
/// so callers can fall through to other probes.
pub fn probe_image(path: &Path) -> Result<ImageInfo, DecodeError> {
    let format = require_image(path)?;
    let info = backend_probe(path, format)?;
    if info.width == 0 || info.height == 0 {
        return Err(DecodeError::Decode(format!(
            "{}: image reports zero dimensions",
            path.display()
        )));
    }
    Ok(info)
}

/// Decode `path`'s single frame to straight-alpha RGBA8.
///
/// The Apple backend applies EXIF orientation and caps the longest side at
/// [`MAX_DECODE_DIMENSION`] (aspect preserved) so one 48-megapixel photo
/// cannot blow the mobile memory budget; the composited quad is sized from
/// the probe's full dimensions, so a capped bitmap only changes sampling
/// density, never layout.
pub fn decode_image(path: &Path) -> Result<RgbaImage, DecodeError> {
    let format = require_image(path)?;
    let image = backend_decode(path, format)?;
    if image.width == 0 || image.height == 0 {
        return Err(DecodeError::Decode(format!(
            "{}: image decoded to zero pixels",
            path.display()
        )));
    }
    Ok(image)
}

/// [`decode_image`] for an in-memory encoded image (embedded assets such as
/// bundled stickers). Same output contract: straight-alpha RGBA8, oriented
/// upright, longest side capped at [`MAX_DECODE_DIMENSION`].
pub fn decode_image_bytes(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
    let format = sniff_bytes(bytes).ok_or_else(|| {
        DecodeError::Open("embedded image: not a recognized image container".into())
    })?;
    let image = backend_decode_bytes(bytes, format)?;
    if image.width == 0 || image.height == 0 {
        return Err(DecodeError::Decode(
            "embedded image: decoded to zero pixels".into(),
        ));
    }
    Ok(image)
}

/// Upper bound on a decoded still's longest side (Apple backend). 4096 px
/// keeps a worst-case bitmap at ~50 MB RGBA while comfortably out-resolving
/// a 4K canvas.
pub const MAX_DECODE_DIMENSION: u32 = 4096;

fn require_image(path: &Path) -> Result<ImageFormat, DecodeError> {
    sniff_image(path).ok_or_else(|| {
        DecodeError::Open(format!("{}: not a recognized still image", path.display()))
    })
}

#[cfg(target_vendor = "apple")]
fn compose_errors(apple_err: DecodeError, portable_err: DecodeError) -> DecodeError {
    match (apple_err, portable_err) {
        (DecodeError::Open(a), DecodeError::Open(p)) => {
            DecodeError::Open(format!("ImageIO: {a}; portable: {p}"))
        }
        (DecodeError::Unsupported(a), DecodeError::Unsupported(p)) => {
            DecodeError::Unsupported(format!("ImageIO: {a}; portable: {p}"))
        }
        (a, p) => DecodeError::Decode(format!("ImageIO: {a}; portable: {p}")),
    }
}

#[cfg(target_vendor = "apple")]
fn backend_probe(path: &Path, format: ImageFormat) -> Result<ImageInfo, DecodeError> {
    crate::image_apple::probe(path).or_else(|apple_err| {
        portable::probe(path, format).map_err(|p_err| compose_errors(apple_err, p_err))
    })
}

#[cfg(not(target_vendor = "apple"))]
fn backend_probe(path: &Path, format: ImageFormat) -> Result<ImageInfo, DecodeError> {
    portable::probe(path, format)
}

#[cfg(target_vendor = "apple")]
fn backend_decode(path: &Path, format: ImageFormat) -> Result<RgbaImage, DecodeError> {
    crate::image_apple::decode(path).or_else(|apple_err| {
        portable::decode(path, format).map_err(|p_err| compose_errors(apple_err, p_err))
    })
}

#[cfg(not(target_vendor = "apple"))]
fn backend_decode(path: &Path, format: ImageFormat) -> Result<RgbaImage, DecodeError> {
    portable::decode(path, format)
}

#[cfg(target_vendor = "apple")]
fn backend_decode_bytes(bytes: &[u8], format: ImageFormat) -> Result<RgbaImage, DecodeError> {
    crate::image_apple::decode_bytes(bytes).or_else(|apple_err| {
        portable::decode_bytes(bytes, format).map_err(|p_err| compose_errors(apple_err, p_err))
    })
}

#[cfg(not(target_vendor = "apple"))]
fn backend_decode_bytes(bytes: &[u8], format: ImageFormat) -> Result<RgbaImage, DecodeError> {
    portable::decode_bytes(bytes, format)
}

/// Pure-Rust still backend, compiled on every platform.
///
/// Which containers it covers is a column in `catalog`, not a match
/// here: the three entry points look up the row and call what it names, so a
/// container gains a portable path by filling in its own columns.
pub mod portable {
    use std::path::Path;

    use aria_core::{DecodeError, RgbaImage};

    use super::{ImageFormat, ImageInfo};

    pub fn probe(path: &Path, format: ImageFormat) -> Result<ImageInfo, DecodeError> {
        match format.spec().portable_probe {
            Some(probe) => probe(path),
            None => Err(unsupported(&path.display().to_string(), format)),
        }
    }

    pub fn decode(path: &Path, format: ImageFormat) -> Result<RgbaImage, DecodeError> {
        match format.spec().portable_decode {
            Some(decode) => decode(path),
            None => Err(unsupported(&path.display().to_string(), format)),
        }
    }

    /// [`decode`] for in-memory encoded bytes (embedded assets).
    pub fn decode_bytes(bytes: &[u8], format: ImageFormat) -> Result<RgbaImage, DecodeError> {
        match format.spec().portable_decode_bytes {
            Some(decode) => decode(bytes),
            None => Err(unsupported(BYTES_CTX, format)),
        }
    }

    /// What an error names when the input is a byte slice with no path.
    pub(super) const BYTES_CTX: &str = "embedded image";

    /// The refusal a `None` decoder column produces, naming the container so an
    /// operator learns which format this build cannot read rather than that
    /// "the image failed".
    fn unsupported(ctx: &str, format: ImageFormat) -> DecodeError {
        DecodeError::unsupported(format!(
            "{ctx}: no portable decoder for {} stills on this platform",
            format.label()
        ))
    }

    fn open(path: &Path) -> Result<std::io::BufReader<std::fs::File>, DecodeError> {
        let file = std::fs::File::open(path)
            .map_err(|e| DecodeError::Io(format!("{}: {e}", path.display())))?;
        Ok(std::io::BufReader::new(file))
    }

    pub(super) fn png_probe(path: &Path) -> Result<ImageInfo, DecodeError> {
        let reader = png::Decoder::new(open(path)?)
            .read_info()
            .map_err(|e| DecodeError::Open(format!("{}: {e}", path.display())))?;
        let info = reader.info();
        Ok(ImageInfo {
            width: info.width,
            height: info.height,
        })
    }

    pub(super) fn png_decode(path: &Path) -> Result<RgbaImage, DecodeError> {
        png_decode_reader(open(path)?, &path.display().to_string())
    }

    /// [`png_decode`] from memory, the shape the catalog's column holds.
    pub(super) fn png_decode_bytes(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        png_decode_reader(std::io::Cursor::new(bytes), BYTES_CTX)
    }

    fn png_decode_reader(reader: impl std::io::Read, ctx: &str) -> Result<RgbaImage, DecodeError> {
        let mut decoder = png::Decoder::new(reader);
        // Palette/gray expansion + 16→8 bit strip: everything below is 8-bit.
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder
            .read_info()
            .map_err(|e| DecodeError::Open(format!("{ctx}: {e}")))?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let frame = reader
            .next_frame(&mut buf)
            .map_err(|e| DecodeError::Decode(format!("{ctx}: {e}")))?;
        buf.truncate(frame.buffer_size());
        let (width, height) = (frame.width, frame.height);
        let pixels = match frame.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => expand(&buf, 3, |px| [px[0], px[1], px[2], 255]),
            png::ColorType::Grayscale => expand(&buf, 1, |px| [px[0], px[0], px[0], 255]),
            png::ColorType::GrayscaleAlpha => expand(&buf, 2, |px| [px[0], px[0], px[0], px[1]]),
            png::ColorType::Indexed => {
                // normalize_to_color8 expands palettes; reaching here is a bug.
                return Err(DecodeError::Decode(format!("{ctx}: palette not expanded")));
            }
        };
        Ok(RgbaImage::new(width, height, pixels))
    }

    pub(super) fn jpeg_probe(path: &Path) -> Result<ImageInfo, DecodeError> {
        let bytes =
            std::fs::read(path).map_err(|e| DecodeError::Io(format!("{}: {e}", path.display())))?;
        let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(bytes));
        decoder
            .decode_headers()
            .map_err(|e| DecodeError::Open(format!("{}: {e:?}", path.display())))?;
        let (width, height) = decoder
            .dimensions()
            .ok_or_else(|| DecodeError::Open(format!("{}: no dimensions", path.display())))?;
        Ok(ImageInfo {
            width: width as u32,
            height: height as u32,
        })
    }

    pub(super) fn jpeg_decode(path: &Path) -> Result<RgbaImage, DecodeError> {
        let bytes =
            std::fs::read(path).map_err(|e| DecodeError::Io(format!("{}: {e}", path.display())))?;
        jpeg_decode_owned(bytes, &path.display().to_string())
    }

    /// [`jpeg_decode`] from memory, the shape the catalog's column holds.
    pub(super) fn jpeg_decode_bytes(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
        jpeg_decode_owned(bytes.to_vec(), BYTES_CTX)
    }

    fn jpeg_decode_owned(bytes: Vec<u8>, ctx: &str) -> Result<RgbaImage, DecodeError> {
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder =
            zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
        let data = decoder
            .decode()
            .map_err(|e| DecodeError::Decode(format!("{ctx}: {e:?}")))?;
        let (width, height) = decoder
            .dimensions()
            .ok_or_else(|| DecodeError::Decode(format!("{ctx}: no dimensions")))?;
        // The decoder may override the requested colorspace (e.g. CMYK
        // sources); normalize whatever it actually produced.
        let pixels = match decoder.output_colorspace() {
            Some(ColorSpace::RGBA) => data,
            Some(ColorSpace::RGB) => expand(&data, 3, |px| [px[0], px[1], px[2], 255]),
            Some(ColorSpace::Luma) => expand(&data, 1, |px| [px[0], px[0], px[0], 255]),
            Some(ColorSpace::LumaA) => expand(&data, 2, |px| [px[0], px[0], px[0], px[1]]),
            other => {
                return Err(DecodeError::unsupported(format!(
                    "{ctx}: jpeg decoded to unsupported colorspace {other:?}"
                )));
            }
        };
        let expected = width * height * 4;
        if pixels.len() != expected {
            return Err(DecodeError::Decode(format!(
                "{ctx}: decoded {} bytes, expected {expected}",
                pixels.len()
            )));
        }
        Ok(RgbaImage::new(width as u32, height as u32, pixels))
    }

    /// Expand `channels`-per-pixel rows to RGBA with `map`.
    fn expand(data: &[u8], channels: usize, map: impl Fn(&[u8]) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() / channels * 4);
        for px in data.chunks_exact(channels) {
            out.extend_from_slice(&map(px));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png_jpeg_webp_magic() {
        assert_eq!(
            sniff_bytes(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            sniff_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F']),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff_bytes(b"RIFF\x24\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
        assert_eq!(sniff_bytes(b"GIF89a\x01\x00"), Some(ImageFormat::Gif));
        assert_eq!(sniff_bytes(b"II*\0\x08\0\0\0"), Some(ImageFormat::Tiff));
        assert_eq!(sniff_bytes(b"BM\x36\x00\x00\x00"), Some(ImageFormat::Bmp));
    }

    #[test]
    fn sniffs_heif_brands_but_not_video_ftyp() {
        assert_eq!(
            sniff_bytes(b"\0\0\0\x18ftypheic\0\0\0\0"),
            Some(ImageFormat::Heif)
        );
        assert_eq!(
            sniff_bytes(b"\0\0\0\x18ftypmif1\0\0\0\0"),
            Some(ImageFormat::Heif)
        );
        assert_eq!(
            sniff_bytes(b"\0\0\0\x18ftypavif\0\0\0\0"),
            Some(ImageFormat::Heif)
        );
        // mp4/mov major brands stay on the video path.
        assert_eq!(sniff_bytes(b"\0\0\0\x18ftypisom\0\0\0\0"), None);
        assert_eq!(sniff_bytes(b"\0\0\0\x18ftypmp42\0\0\0\0"), None);
        assert_eq!(sniff_bytes(b"\0\0\0\x14ftypqt  \0\0\0\0"), None);
    }

    #[test]
    fn short_or_unknown_headers_are_not_images() {
        assert_eq!(sniff_bytes(b""), None);
        assert_eq!(sniff_bytes(b"\x89PN"), None);
        assert_eq!(sniff_bytes(b"RIFF\x24\x00\x00\x00WAVE"), None);
        assert_eq!(sniff_bytes(b"plain text file"), None);
    }

    #[test]
    fn compose_errors_formats_both_backends() {
        #[cfg(target_vendor = "apple")]
        {
            let err1 = DecodeError::Open("file corrupt".into());
            let err2 = DecodeError::Open("invalid header".into());
            let composed = compose_errors(err1, err2);
            let msg = composed.to_string();
            assert!(msg.contains("ImageIO: file corrupt"));
            assert!(msg.contains("portable: invalid header"));
        }
    }
}
