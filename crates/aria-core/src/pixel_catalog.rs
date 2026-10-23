//! Every plane layout that crosses the decode seam, as data.
//!
//! A [`PixelFormat`] used to answer each of its questions with its own `match`,
//! and two of those matches ended in a wildcard: `bit_depth` returned 8 for
//! "everything else" and `bytes_per_sample` returned 1. A 12-bit format added to
//! the enum would have compiled, reported 8 bits, and produced a buffer a third
//! too small — with nothing going red.
//!
//! A row here carries the layout's identity and every measurement a caller used
//! to derive: how many planes it carries, how wide a sample is stored and how
//! many of those bits mean anything, and how its chroma is subsampled. The
//! methods on [`PixelFormat`] read the column.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`PixelFormat`], so a layout added to
//! the enum cannot compile until it names a row, and
//! `a_row_per_pixel_format_variant` proves the arm points at that layout's own
//! row rather than reusing a neighbour's.

use super::{ChromaSubsampling, PixelFormat};

/// One plane layout, with every measurement a caller needs from it.
#[derive(Debug)]
pub struct PixelFormatSpec {
    /// The variant this row describes. Enumerating [`SHIPPED`] over this column
    /// is how a surface lists every layout without hand-copying the enum.
    pub format: PixelFormat,
    /// Stable wire name, for a log line or a report.
    pub id: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// Separate planes the frame carries.
    pub planes: usize,
    /// Bits that actually carry signal in one component sample.
    pub bit_depth: u8,
    /// Storage bytes one component sample occupies.
    pub bytes_per_sample: usize,
    /// Chroma subsampling, or `None` for a packed-RGB layout.
    pub subsampling: Option<ChromaSubsampling>,
    /// Whether the samples are YUV, so a caller knows to apply a matrix.
    ///
    /// Its own column rather than `subsampling.is_some()`: a 4:4:4 YUV layout is
    /// unsubsampled in the colloquial sense and a future packed-YUV layout would
    /// still need the matrix, so the two questions are not the same one.
    pub is_yuv: bool,
}

/// The shipped layouts, in [`PixelFormat`] declaration order.
pub const SHIPPED: &[PixelFormatSpec] = &[
    PixelFormatSpec {
        format: PixelFormat::Nv12,
        id: "nv12",
        label: "NV12 (8-bit biplanar 4:2:0)",
        planes: 2,
        bit_depth: 8,
        bytes_per_sample: 1,
        subsampling: Some(ChromaSubsampling::Yuv420),
        is_yuv: true,
    },
    PixelFormatSpec {
        format: PixelFormat::P010,
        id: "p010",
        label: "P010 (10-bit biplanar 4:2:0)",
        planes: 2,
        bit_depth: 10,
        bytes_per_sample: 2,
        subsampling: Some(ChromaSubsampling::Yuv420),
        is_yuv: true,
    },
    PixelFormatSpec {
        format: PixelFormat::Yuv420p,
        id: "yuv420p",
        label: "I420 (8-bit planar 4:2:0)",
        planes: 3,
        bit_depth: 8,
        bytes_per_sample: 1,
        subsampling: Some(ChromaSubsampling::Yuv420),
        is_yuv: true,
    },
    PixelFormatSpec {
        format: PixelFormat::Rgba8,
        id: "rgba8",
        label: "RGBA8 (packed)",
        planes: 1,
        bit_depth: 8,
        bytes_per_sample: 1,
        subsampling: None,
        is_yuv: false,
    },
    PixelFormatSpec {
        format: PixelFormat::Bgra8,
        id: "bgra8",
        label: "BGRA8 (packed)",
        planes: 1,
        bit_depth: 8,
        bytes_per_sample: 1,
        subsampling: None,
        is_yuv: false,
    },
];

/// The row index for one layout.
///
/// Exhaustive on purpose: this is the arm a new [`PixelFormat`] variant has to
/// add, and until it does the crate does not build.
const fn index_of(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Nv12 => 0,
        PixelFormat::P010 => 1,
        PixelFormat::Yuv420p => 2,
        PixelFormat::Rgba8 => 3,
        PixelFormat::Bgra8 => 4,
    }
}

impl PixelFormat {
    /// This layout's row.
    #[must_use]
    pub const fn spec(self) -> &'static PixelFormatSpec {
        &SHIPPED[index_of(self)]
    }
}

/// Every shipped layout, read from the catalog rather than hand-listed.
pub fn every_pixel_format() -> impl Iterator<Item = PixelFormat> {
    SHIPPED.iter().map(|spec| spec.format)
}

/// One chroma layout.
///
/// Its own axis rather than a column on [`PixelFormatSpec`]: several layouts
/// share a subsampling, and a surface that names the sampling needs the name
/// once, not once per format that uses it.
#[derive(Debug)]
pub struct ChromaSubsamplingSpec {
    /// The variant this row describes.
    pub subsampling: ChromaSubsampling,
    /// Stable wire name.
    pub id: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// Horizontal and vertical chroma divisors, so a caller sizing a chroma
    /// plane reads them instead of matching on the variant.
    pub divisors: (u32, u32),
}

/// The shipped chroma layouts, in [`ChromaSubsampling`] declaration order.
pub const CHROMA: &[ChromaSubsamplingSpec] = &[
    ChromaSubsamplingSpec {
        subsampling: ChromaSubsampling::Yuv420,
        id: "yuv420",
        label: "4:2:0",
        divisors: (2, 2),
    },
    ChromaSubsamplingSpec {
        subsampling: ChromaSubsampling::Yuv444,
        id: "yuv444",
        label: "4:4:4",
        divisors: (1, 1),
    },
];

/// The row index for one chroma layout.
///
/// Exhaustive for the same reason `index_of` is: a sampling added to the enum
/// does not build until it names a row.
const fn chroma_index_of(subsampling: ChromaSubsampling) -> usize {
    match subsampling {
        ChromaSubsampling::Yuv420 => 0,
        ChromaSubsampling::Yuv444 => 1,
    }
}

impl ChromaSubsampling {
    /// This sampling's row.
    #[must_use]
    pub const fn spec(self) -> &'static ChromaSubsamplingSpec {
        &CHROMA[chroma_index_of(self)]
    }

    /// Operator-facing name, from the row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Horizontal and vertical chroma divisors, from the row.
    #[must_use]
    pub const fn divisors(self) -> (u32, u32) {
        self.spec().divisors
    }
}

/// Every shipped chroma layout, read from the catalog rather than hand-listed.
pub fn every_chroma_subsampling() -> impl Iterator<Item = ChromaSubsampling> {
    CHROMA.iter().map(|spec| spec.subsampling)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm-points-at-its-own-row proof. A copy-pasted arm that returns a
    /// neighbour's index goes red here, and a variant with no arm never compiles.
    #[test]
    fn a_row_per_pixel_format_variant() {
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

    /// Ids and labels identify a row, so two rows may not share either.
    #[test]
    fn ids_and_labels_are_unique() {
        let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate pixel-format id");

        let mut labels: Vec<&str> = SHIPPED.iter().map(|spec| spec.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count, "duplicate pixel-format label");
    }

    /// Every row's measurements have to be usable as a buffer calculation: no
    /// zero-plane layout, no zero-byte sample, and no depth claiming more bits
    /// than the storage holds.
    #[test]
    fn measurements_are_self_consistent() {
        for spec in SHIPPED {
            assert!(spec.planes >= 1, "{} claims no planes", spec.id);
            assert!(
                spec.bytes_per_sample >= 1,
                "{} stores a sample in no bytes",
                spec.id
            );
            assert!(
                usize::from(spec.bit_depth) <= spec.bytes_per_sample * 8,
                "{} claims {} bits with bytes_per_sample {}",
                spec.id,
                spec.bit_depth,
                spec.bytes_per_sample
            );
            assert!(
                !(spec.subsampling.is_some() && !spec.is_yuv),
                "{} subsamples chroma but claims not to be YUV",
                spec.id
            );
        }
    }

    /// The enumeration a surface draws from is the catalog itself, in row order.
    #[test]
    fn enumeration_covers_every_row() {
        let listed: Vec<PixelFormat> = every_pixel_format().collect();
        let rows: Vec<PixelFormat> = SHIPPED.iter().map(|spec| spec.format).collect();
        assert_eq!(listed, rows);
    }

    /// The chroma axis carries the same fail-by-default proof: every arm points
    /// at its own row, and the divisors have to be usable as a plane size.
    #[test]
    fn a_row_per_chroma_subsampling_variant() {
        for (index, spec) in CHROMA.iter().enumerate() {
            assert_eq!(
                chroma_index_of(spec.subsampling),
                index,
                "{} points at the wrong row",
                spec.id
            );
            assert_eq!(spec.subsampling.label(), spec.label);
            assert!(
                spec.divisors.0 >= 1 && spec.divisors.1 >= 1,
                "{} divides a plane by zero",
                spec.id
            );
        }
    }

    /// Every subsampling a pixel-format row names has a chroma row of its own,
    /// compared as sets so neither side can be hand-listed.
    #[test]
    fn pixel_rows_only_name_chroma_rows() {
        let mut named: Vec<&str> = SHIPPED
            .iter()
            .filter_map(|spec| spec.subsampling)
            .map(|subsampling| subsampling.spec().id)
            .collect();
        named.sort_unstable();
        named.dedup();
        let mut offered: Vec<&str> = every_chroma_subsampling()
            .map(|subsampling| subsampling.spec().id)
            .collect();
        offered.sort_unstable();
        for id in &named {
            assert!(offered.contains(id), "{id} has no chroma row");
        }
    }
}
