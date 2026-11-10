//! The [`ColorSpace`] of the samples GStreamer decodes, read from the
//! colorimetry their caps state. Caps of a stream that signals none state
//! GStreamer's default for the frame size: BT.601 up to 576 lines, BT.709
//! above.

use aria_core::{ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, TransferFunction};
use gstreamer as gst;
use gstreamer_video as gst_video;
use gstreamer_video::{
    VideoColorMatrix, VideoColorPrimaries, VideoColorRange, VideoTransferFunction,
};

/// The color of the samples `caps` describe, system-memory or DMA-BUF.
/// `None` when the caps describe no raw video.
pub(crate) fn caps_color(caps: &gst::CapsRef) -> Option<ColorSpace> {
    let colorimetry = match gst_video::VideoInfo::from_caps(caps) {
        Ok(info) => info.colorimetry(),
        Err(_) => gst_video::VideoInfoDmaDrm::from_caps(caps)
            .ok()?
            .colorimetry(),
    };
    Some(color_space(&colorimetry))
}

/// The [`ColorSpace`] `colorimetry` states. A value with no counterpart in
/// [`ColorSpace`] reads as unspecified, which the compositor reads as
/// BT.709.
pub(crate) fn color_space(colorimetry: &gst_video::VideoColorimetry) -> ColorSpace {
    ColorSpace {
        primaries: primaries(colorimetry.primaries()),
        transfer: transfer(colorimetry.transfer()),
        matrix: matrix(colorimetry.matrix()),
        range: match colorimetry.range() {
            VideoColorRange::Range0_255 => ColorRange::Full,
            _ => ColorRange::Limited,
        },
    }
}

/// SMPTE 240M primaries are the SMPTE 170M ones.
fn primaries(primaries: VideoColorPrimaries) -> ColorPrimaries {
    match primaries {
        VideoColorPrimaries::Bt709 => ColorPrimaries::Bt709,
        VideoColorPrimaries::Bt470bg => ColorPrimaries::Bt601_625,
        VideoColorPrimaries::Smpte170m | VideoColorPrimaries::Smpte240m => {
            ColorPrimaries::Bt601_525
        }
        VideoColorPrimaries::Bt2020 => ColorPrimaries::Bt2020,
        VideoColorPrimaries::Smpteeg432 => ColorPrimaries::DisplayP3,
        _ => ColorPrimaries::Unspecified,
    }
}

/// BT.601 and the 10- and 12-bit BT.2020 transfers are the BT.709 curve.
fn transfer(transfer: VideoTransferFunction) -> TransferFunction {
    match transfer {
        VideoTransferFunction::Bt709
        | VideoTransferFunction::Bt601
        | VideoTransferFunction::Bt202010
        | VideoTransferFunction::Bt202012 => TransferFunction::Bt709,
        VideoTransferFunction::Srgb => TransferFunction::Srgb,
        VideoTransferFunction::Smpte2084 => TransferFunction::Pq,
        VideoTransferFunction::AribStdB67 => TransferFunction::Hlg,
        _ => TransferFunction::Unspecified,
    }
}

/// The FCC matrix differs from BT.601 in the third decimal place.
fn matrix(matrix: VideoColorMatrix) -> MatrixCoefficients {
    match matrix {
        VideoColorMatrix::Bt709 => MatrixCoefficients::Bt709,
        VideoColorMatrix::Bt601 | VideoColorMatrix::Fcc => MatrixCoefficients::Bt601,
        VideoColorMatrix::Bt2020 => MatrixCoefficients::Bt2020Ncl,
        VideoColorMatrix::Rgb => MatrixCoefficients::Identity,
        _ => MatrixCoefficients::Unspecified,
    }
}

#[cfg(test)]
#[path = "gst_color_tests.rs"]
mod tests;
