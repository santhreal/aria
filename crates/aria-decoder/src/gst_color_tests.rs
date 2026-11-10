//! WHY: the GStreamer backend stated BT.709 limited range for every frame it
//! decoded, whatever the stream signalled. A BT.601 file, which is every SD
//! file and most phone and camera files under 720 lines, drew with the BT.709
//! matrix and shifted its colors: a 0x3080C0 fill drew as 0x287DC4. A
//! full-range stream, such as Motion JPEG, drew with crushed blacks and
//! clipped whites, and an HDR stream stated no HDR transfer. These tests close
//! that class at the one reader of a sample's colorimetry: every value of
//! every GStreamer colorimetry enum has a recorded counterpart, and a decoded
//! stream of each signalled colorimetry states it in its source info and on
//! every frame. They do not cover the compositor's conversion, which the
//! compositor's YUV tests cover, or HDR tone-mapping.

use std::path::{Path, PathBuf};

use aria_core::VideoDecoder;
use gstreamer::glib;
use gstreamer::prelude::*;

use super::*;
use crate::gst_test_media::run_to_eos;
use crate::{GstDecoder, OutputMode};

/// Every value of the GStreamer enum `T`, so a value a later GStreamer adds
/// fails the census until it has a counterpart.
fn every<T>() -> Vec<T>
where
    T: StaticType + for<'a> glib::value::FromValue<'a>,
{
    let class = glib::EnumClass::with_type(T::static_type()).expect("an enum type");
    class
        .values()
        .iter()
        .map(|value| {
            class
                .to_value(value.value())
                .expect("a value of its own class")
                .get::<T>()
                .expect("the enum's own type")
        })
        .collect()
}

/// Assert `map` gives each value of `T` the counterpart `expected` records,
/// and that `expected` records every value.
fn census<T, U>(expected: &[(T, U)], map: fn(T) -> U)
where
    T: StaticType + for<'a> glib::value::FromValue<'a> + PartialEq + Copy + std::fmt::Debug,
    U: PartialEq + Copy + std::fmt::Debug,
{
    let every = every::<T>();
    assert_eq!(every.len(), expected.len(), "one counterpart per value");
    for value in every {
        let (_, want) = expected
            .iter()
            .find(|(gst, _)| *gst == value)
            .unwrap_or_else(|| panic!("{value:?} has no recorded counterpart"));
        assert_eq!(map(value), *want, "{value:?}");
    }
}

#[test]
fn every_matrix_has_its_counterpart() {
    use MatrixCoefficients as M;
    use VideoColorMatrix as G;
    census(
        &[
            (G::Unknown, M::Unspecified),
            (G::Rgb, M::Identity),
            (G::Fcc, M::Bt601),
            (G::Bt709, M::Bt709),
            (G::Bt601, M::Bt601),
            (G::Smpte240m, M::Unspecified),
            (G::Bt2020, M::Bt2020Ncl),
        ],
        matrix,
    );
}

#[test]
fn every_transfer_has_its_counterpart() {
    use TransferFunction as T;
    use VideoTransferFunction as G;
    census(
        &[
            (G::Unknown, T::Unspecified),
            (G::Gamma10, T::Unspecified),
            (G::Gamma18, T::Unspecified),
            (G::Gamma20, T::Unspecified),
            (G::Gamma22, T::Unspecified),
            (G::Bt709, T::Bt709),
            (G::Smpte240m, T::Unspecified),
            (G::Srgb, T::Srgb),
            (G::Gamma28, T::Unspecified),
            (G::Log100, T::Unspecified),
            (G::Log316, T::Unspecified),
            (G::Bt202012, T::Bt709),
            (G::Adobergb, T::Unspecified),
            (G::Bt202010, T::Bt709),
            (G::Smpte2084, T::Pq),
            (G::AribStdB67, T::Hlg),
            (G::Bt601, T::Bt709),
        ],
        transfer,
    );
}

#[test]
fn every_primaries_value_has_its_counterpart() {
    use ColorPrimaries as P;
    use VideoColorPrimaries as G;
    census(
        &[
            (G::Unknown, P::Unspecified),
            (G::Bt709, P::Bt709),
            (G::Bt470m, P::Unspecified),
            (G::Bt470bg, P::Bt601_625),
            (G::Smpte170m, P::Bt601_525),
            (G::Smpte240m, P::Bt601_525),
            (G::Film, P::Unspecified),
            (G::Bt2020, P::Bt2020),
            (G::Adobergb, P::Unspecified),
            (G::Smptest428, P::Unspecified),
            (G::Smpterp431, P::Unspecified),
            (G::Smpteeg432, P::DisplayP3),
            (G::Ebu3213, P::Unspecified),
        ],
        primaries,
    );
}

/// Four 64x48 frames coded by `encoder` from I420 samples of `colorimetry`
/// into an MP4 or MOV.
fn signalled(dir: &Path, encoder: &str, colorimetry: &str) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("{encoder}-{colorimetry}.mov"));
    run_to_eos(&format!(
        "videotestsrc num-buffers=4 ! \
         video/x-raw,format=I420,width=64,height=48,framerate=24/1,colorimetry={colorimetry} ! \
         {encoder} ! qtmux ! filesink location=\"{}\"",
        path.display()
    ));
    path
}

#[test]
fn a_decoded_stream_states_the_colorimetry_it_signals() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let bt601 = ColorSpace {
        primaries: ColorPrimaries::Bt601_525,
        ..ColorSpace::BT601
    };
    let hlg = ColorSpace {
        transfer: TransferFunction::Hlg,
        ..ColorSpace::BT2020_PQ
    };
    // JPEG samples are full-range BT.601 and state no primaries or transfer.
    let jpeg = ColorSpace {
        primaries: ColorPrimaries::Unspecified,
        transfer: TransferFunction::Unspecified,
        matrix: MatrixCoefficients::Bt601,
        range: ColorRange::Full,
    };
    for (encoder, colorimetry, want) in [
        ("x264enc", "bt601", bt601),
        ("x264enc", "bt709", ColorSpace::BT709),
        ("x264enc", "bt2100-pq", ColorSpace::BT2020_PQ),
        ("x264enc", "bt2100-hlg", hlg),
        ("jpegenc", "1:4:0:0", jpeg),
    ] {
        let file = signalled(dir.path(), encoder, colorimetry);
        let case = format!("{encoder} {colorimetry}");
        let mut decoder = GstDecoder::open(&file, OutputMode::Cpu)
            .unwrap_or_else(|e| panic!("{case}: open: {e}"));
        assert_eq!(decoder.info().color, want, "{case}: source info");
        let mut frames = 0;
        while let Some(frame) = decoder.next_frame().expect("the fixture decodes") {
            assert_eq!(frame.color, want, "{case}: frame {frames}");
            frames += 1;
        }
        assert_eq!(frames, 4, "{case}: every frame decodes");
    }
}

/// The colorimetry the caps of `height`-line NV12 samples with no
/// `colorimetry` field read as.
fn unstated(height: i32) -> Option<ColorSpace> {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "NV12")
        .field("width", 640)
        .field("height", height)
        .field("framerate", gst::Fraction::new(24, 1))
        .build();
    caps_color(&caps)
}

#[test]
fn caps_that_state_no_colorimetry_read_by_frame_height() {
    let bt601 = ColorSpace {
        primaries: ColorPrimaries::Bt601_525,
        ..ColorSpace::BT601
    };
    for (height, want) in [
        (360, bt601),
        (576, bt601),
        (578, ColorSpace::BT709),
        (1080, ColorSpace::BT709),
        (2160, ColorSpace::BT709),
    ] {
        assert_eq!(unstated(height), Some(want), "{height} lines");
    }
    let audio = gst::Caps::builder("audio/x-raw").build();
    assert_eq!(caps_color(&audio), None, "caps of no video");
}

/// The repository's untagged 360-line fixture decodes as BT.601, and its copy
/// tagged BT.709 as BT.709.
#[test]
fn the_fixtures_decode_at_the_colorimetry_they_signal() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for (name, want) in [
        (
            "fixture-640x360-30fps-3s.mp4",
            ColorSpace {
                primaries: ColorPrimaries::Bt601_525,
                ..ColorSpace::BT601
            },
        ),
        ("fixture-640x360-30fps-3s-bt709.mp4", ColorSpace::BT709),
    ] {
        let mut decoder = GstDecoder::open(&fixtures.join(name), OutputMode::Cpu)
            .unwrap_or_else(|e| panic!("{name}: open: {e}"));
        assert_eq!(decoder.info().color, want, "{name}: source info");
        let frame = decoder
            .next_frame()
            .expect("the fixture decodes")
            .expect("the fixture holds a frame");
        assert_eq!(frame.color, want, "{name}: first frame");
    }
}
