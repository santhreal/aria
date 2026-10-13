//! Tests for [`wmf`].
//!
//! Declared from `wmf.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;

#[test]
fn maps_primaries_from_mf_codes() {
    assert_eq!(
        map_primaries(MFVideoPrimaries_BT709.0 as u32),
        ColorPrimaries::Bt709
    );
    assert_eq!(
        map_primaries(MFVideoPrimaries_SMPTE170M.0 as u32),
        ColorPrimaries::Bt601_525
    );
    assert_eq!(
        map_primaries(MFVideoPrimaries_BT470_2_SysBG.0 as u32),
        ColorPrimaries::Bt601_625
    );
    assert_eq!(
        map_primaries(MFVideoPrimaries_BT2020.0 as u32),
        ColorPrimaries::Bt2020
    );
    assert_eq!(
        map_primaries(MFVideoPrimaries_DCI_P3.0 as u32),
        ColorPrimaries::DisplayP3
    );
    assert_eq!(map_primaries(0), ColorPrimaries::Unspecified);
}

#[test]
fn maps_transfer_from_mf_codes() {
    assert_eq!(
        map_transfer(MFVideoTransFunc_709.0 as u32),
        TransferFunction::Bt709
    );
    assert_eq!(
        map_transfer(MFVideoTransFunc_sRGB.0 as u32),
        TransferFunction::Srgb
    );
    assert_eq!(
        map_transfer(MFVideoTransFunc_2084.0 as u32),
        TransferFunction::Pq
    );
    assert_eq!(
        map_transfer(MFVideoTransFunc_HLG.0 as u32),
        TransferFunction::Hlg
    );
    assert_eq!(map_transfer(0), TransferFunction::Unspecified);
}

#[test]
fn maps_matrix_from_mf_codes() {
    assert_eq!(
        map_matrix(MFVideoTransferMatrix_BT709.0 as u32),
        MatrixCoefficients::Bt709
    );
    assert_eq!(
        map_matrix(MFVideoTransferMatrix_BT601.0 as u32),
        MatrixCoefficients::Bt601
    );
    assert_eq!(
        map_matrix(MFVideoTransferMatrix_BT2020_10.0 as u32),
        MatrixCoefficients::Bt2020Ncl
    );
    assert_eq!(
        map_matrix(MFVideoTransferMatrix_BT2020_12.0 as u32),
        MatrixCoefficients::Bt2020Ncl
    );
    assert_eq!(map_matrix(0), MatrixCoefficients::Unspecified);
}

#[test]
fn maps_range_from_mf_codes() {
    assert_eq!(map_range(MFNominalRange_0_255.0 as u32), ColorRange::Full);
    assert_eq!(map_range(2), ColorRange::Limited);
    assert_eq!(map_range(0), ColorRange::Limited);
}

#[test]
fn subtype_maps_to_pixel_format() {
    assert_eq!(
        subtype_to_format(Some(MFVideoFormat_NV12)),
        Some(PixelFormat::Nv12)
    );
    assert_eq!(
        subtype_to_format(Some(MFVideoFormat_P010)),
        Some(PixelFormat::P010)
    );
    assert_eq!(subtype_to_format(None), None);
}

#[test]
fn hns_conversion_matches_time_base() {
    // 3 ticks already on the 100-ns base is exactly 3 hns.
    let exact = RationalTime::new(3, Rational::new(HNS_PER_SEC as i32, 1));
    assert_eq!(to_hns(exact), 3);
    // Frame 30 at 30 fps is one second = 1e7 hns.
    let one_second = RationalTime::new(30, Rational::FPS_30);
    assert_eq!(to_hns(one_second), HNS_PER_SEC);
    // Frame 5 at 30 fps: 5/30 s -> 1_666_666 hns (truncated toward zero).
    let frame_5 = RationalTime::new(5, Rational::FPS_30);
    assert_eq!(to_hns(frame_5), 1_666_666);
}

#[test]
fn verbatim_prefixes_fold_to_paths_media_foundation_opens() {
    // `canonicalize` output: the DOS-device bypass must go.
    assert_eq!(mf_path(r"\\?\C:\media\clip.mp4"), r"C:\media\clip.mp4");
    // Verbatim UNC server form folds back to an ordinary UNC path.
    assert_eq!(
        mf_path(r"\\?\UNC\host\share\clip.mp4"),
        r"\\host\share\clip.mp4"
    );
    // Ordinary and relative paths pass through untouched.
    assert_eq!(mf_path(r"C:\media\clip.mp4"), r"C:\media\clip.mp4");
    assert_eq!(mf_path(r"relative\clip.mp4"), r"relative\clip.mp4");
}
