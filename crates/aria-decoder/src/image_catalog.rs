//! Every still-image container Aria recognizes, as data.
//!
//! Recognition and decode used to be three separate matches over
//! [`ImageFormat`], two of them ending in `other =>`: the portable backend said
//! "no decoder for this" for everything it had not been taught, and the
//! animation path said "one frame" for everything that was not APNG or GIF. A
//! container added to the enum compiled, sniffed correctly, and then silently
//! took the fallback arm — including the case where the fallback was wrong,
//! because an animated container that falls into the still arm loses every frame
//! after the first with nothing going red.
//!
//! A row here carries the container's identity, the extensions a file dialog
//! offers for it, the magic-byte test that classifies it, whether it can hold
//! more than one frame, and a route to each pure-Rust decoder that exists for
//! it. `None` in a decoder column is a recorded decision — "this build has no
//! portable path, use the native backend or refuse" — rather than the absence of
//! one.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`ImageFormat`], so a container
//! added to the enum cannot compile until it names a row, and it cannot name a
//! row without answering every column: its extensions, its magic, whether it
//! animates, and which decoders it has.
//!
//! # Disjoint magic
//!
//! [`super::sniff_bytes`] walks the rows in order and takes the first match, so
//! two rows must never both claim one header. `census` is a minimal header for
//! each row, and `magic_is_disjoint` runs every census past every row.

use std::path::Path;

use aria_core::{DecodeError, RgbaImage};

use super::portable;
use super::{ImageFormat, ImageInfo};
use crate::animation::AnimationFrame;

/// One still-image container.
pub struct ImageFormatSpec {
    /// The variant this row describes. Enumerating [`SHIPPED`] over this column
    /// is how a surface lists every container without hand-copying the enum.
    pub format: ImageFormat,
    /// Stable wire name.
    pub id: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// Filename extensions a file dialog offers for this container, lowercase
    /// and without the dot. Sniffing never reads them — an extension names what
    /// a file claims to be — but a dialog filter has nothing else to go on.
    pub extensions: &'static [&'static str],
    /// Whether the header at the start of a file belongs to this container.
    pub magic: fn(&[u8]) -> bool,
    /// A minimal header this row claims and no other row does, for the
    /// disjointness proof.
    pub census: &'static [u8],
    /// Whether the container can carry more than one frame. A still imported
    /// from an animating container composites as its first frame, which is worth
    /// saying out loud before an export.
    pub animates: bool,
    /// Pure-Rust dimension read, or `None` when only a native backend has one.
    pub portable_probe: Option<fn(&Path) -> Result<ImageInfo, DecodeError>>,
    /// Pure-Rust single-frame decode from a path.
    pub portable_decode: Option<fn(&Path) -> Result<RgbaImage, DecodeError>>,
    /// Pure-Rust single-frame decode from memory.
    pub portable_decode_bytes: Option<fn(&[u8]) -> Result<RgbaImage, DecodeError>>,
    /// Pure-Rust multi-frame decode from memory. `None` for a single-frame
    /// container, and for an animating one with no portable animated decoder —
    /// the caller then falls back to the single-frame column, which is the
    /// honest first-frame answer rather than a silent one.
    pub portable_animate: Option<fn(&[u8]) -> Result<Vec<AnimationFrame>, DecodeError>>,
}

impl std::fmt::Debug for ImageFormatSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageFormatSpec")
            .field("id", &self.id)
            .field("extensions", &self.extensions)
            .field("animates", &self.animates)
            .finish_non_exhaustive()
    }
}

/// The shipped containers, in [`ImageFormat`] declaration order.
pub const SHIPPED: &[ImageFormatSpec] = &[
    ImageFormatSpec {
        format: ImageFormat::Png,
        id: "png",
        label: "PNG",
        extensions: &["png", "apng"],
        magic: |head| head.starts_with(b"\x89PNG\r\n\x1a\n"),
        census: b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
        // APNG rides in the same container, so a `.png` may be an animation.
        animates: true,
        portable_probe: Some(portable::png_probe),
        portable_decode: Some(portable::png_decode),
        portable_decode_bytes: Some(portable::png_decode_bytes),
        portable_animate: Some(crate::animation::apng_decode),
    },
    ImageFormatSpec {
        format: ImageFormat::Jpeg,
        id: "jpeg",
        label: "JPEG",
        extensions: &["jpg", "jpeg", "jpe"],
        magic: |head| head.starts_with(&[0xFF, 0xD8, 0xFF]),
        census: &[0xFF, 0xD8, 0xFF, 0xE0],
        animates: false,
        portable_probe: Some(portable::jpeg_probe),
        portable_decode: Some(portable::jpeg_decode),
        portable_decode_bytes: Some(portable::jpeg_decode_bytes),
        portable_animate: None,
    },
    ImageFormatSpec {
        format: ImageFormat::WebP,
        id: "webp",
        label: "WebP",
        extensions: &["webp"],
        magic: |head| head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP",
        census: b"RIFF\x24\x00\x00\x00WEBPVP8 ",
        animates: true,
        // No pure-Rust WebP path in this build; ImageIO covers it on Apple and
        // every other platform refuses by name.
        portable_probe: None,
        portable_decode: None,
        portable_decode_bytes: None,
        portable_animate: None,
    },
    ImageFormatSpec {
        format: ImageFormat::Heif,
        id: "heif",
        label: "HEIF / HEIC / AVIF",
        extensions: &["heic", "heif", "avif"],
        magic: is_image_ftyp,
        census: b"\0\0\0\x18ftypheic\0\0\0\0",
        animates: false,
        portable_probe: None,
        portable_decode: None,
        portable_decode_bytes: None,
        portable_animate: None,
    },
    ImageFormatSpec {
        format: ImageFormat::Gif,
        id: "gif",
        label: "GIF",
        extensions: &["gif"],
        magic: |head| head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a"),
        census: b"GIF89a\x01\x00",
        animates: true,
        portable_probe: None,
        portable_decode: None,
        portable_decode_bytes: None,
        portable_animate: Some(crate::animation::gif_decode),
    },
    ImageFormatSpec {
        format: ImageFormat::Bmp,
        id: "bmp",
        label: "BMP",
        extensions: &["bmp"],
        magic: |head| head.starts_with(b"BM"),
        census: b"BM\x36\x00\x00\x00",
        animates: false,
        portable_probe: None,
        portable_decode: None,
        portable_decode_bytes: None,
        portable_animate: None,
    },
    ImageFormatSpec {
        format: ImageFormat::Tiff,
        id: "tiff",
        label: "TIFF",
        extensions: &["tif", "tiff"],
        magic: |head| head.starts_with(b"II*\0") || head.starts_with(b"MM\0*"),
        census: b"II*\0\x08\0\0\0",
        animates: false,
        portable_probe: None,
        portable_decode: None,
        portable_decode_bytes: None,
        portable_animate: None,
    },
];

/// ISO-BMFF `ftyp` with an image major brand.
///
/// Video containers (mp4/mov) share the box, so only an image brand classifies
/// as a still. The leading size guard keeps this row disjoint from the rows whose
/// magic sits at offset 0: every real `ftyp` box is far under 64 KiB, so its
/// first two length bytes are zero, which no TIFF, GIF or BMP header is.
fn is_image_ftyp(head: &[u8]) -> bool {
    const IMAGE_BRANDS: &[&[u8; 4]] = &[
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1", b"avif", b"avis",
    ];
    if head.len() < 12 || &head[..2] != b"\0\0" || &head[4..8] != b"ftyp" {
        return false;
    }
    let brand: &[u8] = &head[8..12];
    IMAGE_BRANDS.iter().any(|known| &known[..] == brand)
}

/// The row index for one container.
///
/// Exhaustive on purpose, and the whole fail-by-default mechanism for this axis:
/// a container added to `image.rs` and nowhere else does not compile.
const fn index_of(format: ImageFormat) -> usize {
    match format {
        ImageFormat::Png => 0,
        ImageFormat::Jpeg => 1,
        ImageFormat::WebP => 2,
        ImageFormat::Heif => 3,
        ImageFormat::Gif => 4,
        ImageFormat::Bmp => 5,
        ImageFormat::Tiff => 6,
    }
}

impl ImageFormat {
    /// This container's row.
    #[must_use]
    pub const fn spec(self) -> &'static ImageFormatSpec {
        &SHIPPED[index_of(self)]
    }

    /// Operator-facing name, from the row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Whether the container can carry more than one frame.
    #[must_use]
    pub const fn animates(self) -> bool {
        self.spec().animates
    }
}

/// Every recognized container, read from the catalog rather than hand-listed.
pub fn every_image_format() -> impl Iterator<Item = ImageFormat> {
    SHIPPED.iter().map(|spec| spec.format)
}

/// Every extension a file dialog should offer for stills, deduplicated and in
/// row order.
///
/// The one list an import filter reads, so a container added to the catalog is
/// pickable without editing a dialog.
#[must_use]
pub fn image_extensions() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for spec in SHIPPED {
        for extension in spec.extensions {
            if !out.contains(extension) {
                out.push(extension);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm-points-at-its-own-row proof. A copy-pasted arm that returns a
    /// neighbour's index goes red here, and a variant with no arm never compiles.
    #[test]
    fn a_row_per_image_format_variant() {
        for (index, spec) in SHIPPED.iter().enumerate() {
            assert_eq!(
                index_of(spec.format),
                index,
                "{} points at the wrong row",
                spec.id
            );
            assert_eq!(spec.format.spec().id, spec.id);
        }
    }

    /// `sniff_bytes` takes the first matching row, so two rows claiming one
    /// header would make classification depend on declaration order. Each row's
    /// own census must match it and nothing else.
    #[test]
    fn magic_is_disjoint() {
        for spec in SHIPPED {
            let claimed: Vec<&str> = SHIPPED
                .iter()
                .filter(|other| (other.magic)(spec.census))
                .map(|other| other.id)
                .collect();
            assert_eq!(
                claimed,
                vec![spec.id],
                "{}'s census header is claimed by {claimed:?}",
                spec.id
            );
        }
    }

    /// An extension is how a dialog offers a container, so two containers may
    /// not share one: the filter would be ambiguous and the ids would not be.
    #[test]
    fn ids_and_extensions_are_unique() {
        let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
        let rows = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), rows, "duplicate image-format id");

        let mut extensions: Vec<&str> = SHIPPED
            .iter()
            .flat_map(|spec| spec.extensions.iter().copied())
            .collect();
        let listed = extensions.len();
        extensions.sort_unstable();
        extensions.dedup();
        assert_eq!(
            extensions.len(),
            listed,
            "two containers claim one extension"
        );
        assert_eq!(image_extensions().len(), listed);
    }

    /// Every row must be offerable: a container with no extension cannot be
    /// picked in a dialog, and one with no census cannot be proven disjoint.
    #[test]
    fn every_row_is_offerable() {
        for spec in SHIPPED {
            assert!(
                !spec.extensions.is_empty(),
                "{} has no extension a dialog can offer",
                spec.id
            );
            assert!(
                spec.extensions
                    .iter()
                    .all(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_lowercase())),
                "{} has a non-lowercase or empty extension",
                spec.id
            );
            assert!(
                (spec.magic)(spec.census),
                "{} does not claim its own census header",
                spec.id
            );
        }
    }

    /// A multi-frame decoder on a container that cannot hold multiple frames
    /// would be dead code; a portable path that can decode from a file but not
    /// from memory (or the reverse) would make one entry point refuse what the
    /// other accepts.
    #[test]
    fn decoder_columns_agree_with_the_container() {
        for spec in SHIPPED {
            assert!(
                !(spec.portable_animate.is_some() && !spec.animates),
                "{} has an animated decoder but claims a single frame",
                spec.id
            );
            assert_eq!(
                spec.portable_decode.is_some(),
                spec.portable_decode_bytes.is_some(),
                "{}'s path and memory decoders disagree",
                spec.id
            );
            assert!(
                !(spec.portable_decode.is_some() && spec.portable_probe.is_none()),
                "{} can be decoded portably but not probed",
                spec.id
            );
        }
    }
}
