//! Pixel formats: the plane layouts that cross the decode→compositor seam.
//!
//! The set is intentionally small — exactly what platform hardware decoders
//! emit plus packed RGB for stills and the CPU fallback. A `VideoToolbox` /
//! `MediaCodec` / `Media Foundation` decoder hands back biplanar YUV (NV12 for
//! 8-bit, P010 for 10-bit); software/Linux fallbacks and image decoders may give
//! planar YUV or packed RGBA/BGRA. The compositor samples whichever layout it
//! receives (planes bound as textures) and converts on the GPU.

/// Chroma subsampling of a YUV format.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChromaSubsampling {
    /// 4:2:0 — chroma at half width and half height (the video norm).
    Yuv420,
    /// 4:4:4 — chroma at full resolution (no subsampling; RGB-like).
    Yuv444,
}

/// A pixel format crossing the frame handoff.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 8-bit biplanar 4:2:0: full-res Y plane, then interleaved U/V at quarter
    /// resolution. The standard hardware-decode output (VideoToolbox / NVDEC /
    /// MediaCodec / Media Foundation).
    Nv12,
    /// 10-bit biplanar 4:2:0: like NV12 but each sample is 16-bit little-endian
    /// with the 10 significant bits in the high end. The 10-bit HEVC / HDR
    /// hardware-decode output.
    P010,
    /// 8-bit planar 4:2:0: separate Y, U, V planes. Software-decode and image
    /// fallback layout.
    Yuv420p,
    /// 8-bit packed RGBA, one plane (decoded stills, generated frames).
    Rgba8,
    /// 8-bit packed BGRA, one plane. CoreVideo's common RGB output order.
    Bgra8,
}

/// The catalog every method below reads. A child module so the rows stay
/// readable next to the enum they describe.
#[path = "pixel_catalog.rs"]
pub mod catalog;

pub use catalog::{
    ChromaSubsamplingSpec, PixelFormatSpec, every_chroma_subsampling, every_pixel_format,
};

impl PixelFormat {
    /// Number of separate planes the frame carries.
    pub const fn plane_count(self) -> usize {
        self.spec().planes
    }

    /// Bits actually used per component sample (8 for SDR layouts, 10 for P010).
    pub const fn bit_depth(self) -> u8 {
        self.spec().bit_depth
    }

    /// Storage bytes per component sample (P010 stores 10-bit values in 16 bits).
    pub const fn bytes_per_sample(self) -> usize {
        self.spec().bytes_per_sample
    }

    /// True for the YUV formats (need a YUV→RGB matrix); false for packed RGB.
    pub const fn is_yuv(self) -> bool {
        self.spec().is_yuv
    }

    /// Chroma subsampling, or `None` for packed-RGB formats.
    pub const fn chroma_subsampling(self) -> Option<ChromaSubsampling> {
        self.spec().subsampling
    }

    /// True if width/height must be even, because a chroma plane divides them.
    ///
    /// Read off the sampling's divisors rather than naming 4:2:0: a 4:2:2 row
    /// added to the chroma catalog needs an even width and any odd height, and
    /// answering that from the column gets it right with no edit here.
    pub const fn requires_even_dimensions(self) -> bool {
        match self.spec().subsampling {
            Some(subsampling) => {
                let (horizontal, vertical) = subsampling.divisors();
                horizontal > 1 || vertical > 1
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_counts() {
        assert_eq!(PixelFormat::Nv12.plane_count(), 2);
        assert_eq!(PixelFormat::P010.plane_count(), 2);
        assert_eq!(PixelFormat::Yuv420p.plane_count(), 3);
        assert_eq!(PixelFormat::Rgba8.plane_count(), 1);
        assert_eq!(PixelFormat::Bgra8.plane_count(), 1);
    }

    #[test]
    fn p010_is_ten_bit_two_bytes() {
        assert_eq!(PixelFormat::P010.bit_depth(), 10);
        assert_eq!(PixelFormat::P010.bytes_per_sample(), 2);
        assert_eq!(PixelFormat::Nv12.bit_depth(), 8);
        assert_eq!(PixelFormat::Nv12.bytes_per_sample(), 1);
    }

    #[test]
    fn yuv_classification() {
        assert!(PixelFormat::Nv12.is_yuv());
        assert!(PixelFormat::P010.is_yuv());
        assert!(PixelFormat::Yuv420p.is_yuv());
        assert!(!PixelFormat::Rgba8.is_yuv());
        assert!(!PixelFormat::Bgra8.is_yuv());
    }

    #[test]
    fn subsampling_and_even_requirement() {
        assert_eq!(
            PixelFormat::Nv12.chroma_subsampling(),
            Some(ChromaSubsampling::Yuv420)
        );
        assert_eq!(PixelFormat::Bgra8.chroma_subsampling(), None);
        assert!(PixelFormat::Nv12.requires_even_dimensions());
        assert!(!PixelFormat::Rgba8.requires_even_dimensions());
    }
}
