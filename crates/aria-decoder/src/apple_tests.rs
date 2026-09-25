//! Tests for [`apple`].
//!
//! Declared from `apple.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;

#[test]
fn detect_known_pixel_formats() {
    assert_eq!(
        detect_format(FOURCC_420V),
        Some((PixelFormat::Nv12, ColorRange::Limited))
    );
    assert_eq!(
        detect_format(FOURCC_420F),
        Some((PixelFormat::Nv12, ColorRange::Full))
    );
    assert_eq!(
        detect_format(FOURCC_X420),
        Some((PixelFormat::P010, ColorRange::Limited))
    );
    assert_eq!(
        detect_format(FOURCC_BGRA),
        Some((PixelFormat::Bgra8, ColorRange::Full))
    );
    assert_eq!(detect_format(u32::from_be_bytes(*b"av01")), None);
}

#[test]
fn fourcc_roundtrips_to_text() {
    assert_eq!(fourcc_str(FOURCC_420V), "420v");
    assert_eq!(fourcc_str(FOURCC_BGRA), "BGRA");
}

#[test]
fn maps_color_tags_from_constants() {
    // Primaries.
    assert_eq!(
        map_primaries(unsafe { kCMFormatDescriptionColorPrimaries_ITU_R_709_2 }),
        ColorPrimaries::Bt709
    );
    assert_eq!(
        map_primaries(unsafe { kCMFormatDescriptionColorPrimaries_ITU_R_2020 }),
        ColorPrimaries::Bt2020
    );
    assert_eq!(
        map_primaries(unsafe { kCMFormatDescriptionColorPrimaries_P3_D65 }),
        ColorPrimaries::DisplayP3
    );
    assert_eq!(
        map_primaries(unsafe { kCMFormatDescriptionColorPrimaries_SMPTE_C }),
        ColorPrimaries::Bt601_525
    );

    // Transfer (PQ / HLG are the HDR signals we care about).
    assert_eq!(
        map_transfer(unsafe { kCMFormatDescriptionTransferFunction_ITU_R_709_2 }),
        TransferFunction::Bt709
    );
    assert_eq!(
        map_transfer(unsafe { kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ }),
        TransferFunction::Pq
    );
    assert_eq!(
        map_transfer(unsafe { kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG }),
        TransferFunction::Hlg
    );

    // Matrix.
    assert_eq!(
        map_matrix(unsafe { kCMFormatDescriptionYCbCrMatrix_ITU_R_709_2 }),
        MatrixCoefficients::Bt709
    );
    assert_eq!(
        map_matrix(unsafe { kCMFormatDescriptionYCbCrMatrix_ITU_R_601_4 }),
        MatrixCoefficients::Bt601
    );
    assert_eq!(
        map_matrix(unsafe { kCMFormatDescriptionYCbCrMatrix_ITU_R_2020 }),
        MatrixCoefficients::Bt2020Ncl
    );

    // A BT.2020 + PQ combination must read back as HDR + wide gamut.
    let hdr = ColorSpace {
        primaries: map_primaries(unsafe { kCMFormatDescriptionColorPrimaries_ITU_R_2020 }),
        transfer: map_transfer(unsafe { kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ }),
        matrix: map_matrix(unsafe { kCMFormatDescriptionYCbCrMatrix_ITU_R_2020 }),
        range: ColorRange::Limited,
    };
    assert!(hdr.is_hdr());
    assert!(hdr.is_wide_gamut());
    assert_eq!(hdr, ColorSpace::BT2020_PQ);
}

#[test]
fn cmtime_to_rational_is_exact_and_guards_invalid() {
    let rt = cmtime_to_rational(CMTime {
        value: 1001,
        timescale: 30000,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    })
    .expect("valid");
    assert_eq!(rt.value, 1001);
    assert_eq!(rt.rate, Rational::new(30000, 1));
    // Invalid (timescale 0, no Valid flag) -> None.
    assert!(
        cmtime_to_rational(CMTime {
            value: 0,
            timescale: 0,
            flags: CMTimeFlags::empty(),
            epoch: 0,
        })
        .is_none()
    );
}

#[test]
fn resolve_frame_rate_prefers_exact_but_guards_vfr() {
    // CFR: min frame duration agrees with nominal — keep the exact rate.
    let ntsc = Rational::new(24_000, 1_001);
    assert_eq!(resolve_frame_rate(Some(ntsc), 23.976), ntsc);
    // Missing nominal still keeps the exact rate.
    assert_eq!(resolve_frame_rate(Some(ntsc), 0.0), ntsc);
    // VFR: the smallest gap (120 fps) is far above the 30 fps average —
    // report the average, not the artifact.
    assert_eq!(
        resolve_frame_rate(Some(Rational::new(120, 1)), 30.0),
        Rational::new(30_000, 1000)
    );
    // No min duration: nominal, then the 30 fps fallback.
    assert_eq!(resolve_frame_rate(None, 25.0), Rational::new(25_000, 1000));
    assert_eq!(resolve_frame_rate(None, 0.0), Rational::FPS_30);
}

#[test]
fn one_second_ticks_rounds_up_and_never_zero() {
    assert_eq!(one_second_ticks(Rational::FPS_30), 30);
    // 24000/1001 fps: one second is 23.976… ticks -> 24.
    assert_eq!(one_second_ticks(Rational::new(24_000, 1_001)), 24);
    assert_eq!(one_second_ticks(Rational::new(90_000, 1)), 90_000);
    // Degenerate rates still make progress.
    assert_eq!(one_second_ticks(Rational::new(0, 0)), 1);
}

#[test]
fn describe_ns_error_carries_domain_code_and_underlying() {
    use objc2_foundation::NSInteger;
    let underlying = unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("NSOSStatusErrorDomain"),
            -12909 as NSInteger, // kVTVideoDecoderBadDataErr
            None,
        )
    };
    let key: &NSString = unsafe { NSUnderlyingErrorKey };
    let value: &AnyObject = underlying.as_ref();
    let user_info = NSDictionary::from_slices(&[key], &[value]);
    let err = unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("AVFoundationErrorDomain"),
            -11821 as NSInteger, // AVErrorDecodeFailed
            Some(&user_info),
        )
    };
    let text = describe_ns_error(&err);
    assert!(text.contains("AVFoundationErrorDomain"), "{text}");
    assert!(text.contains("-11821"), "{text}");
    assert!(text.contains("NSOSStatusErrorDomain"), "{text}");
    assert!(text.contains("-12909"), "{text}");
}

#[test]
fn rational_to_cmtime_preserves_seconds() {
    // 5 frames at 30fps -> 5/30 s. Represent as value*den / num.
    // CMTime is a packed struct, so copy fields out before comparing.
    let t = rational_to_cmtime(RationalTime::new(5, Rational::FPS_30));
    let (t_val, t_scale) = (t.value, t.timescale);
    assert_eq!(t_scale, 30);
    assert_eq!(t_val, 5);
    assert_eq!(t_val as f64 / t_scale as f64, 5.0 / 30.0);
    // NTSC: 1 frame at 30000/1001 -> value 1001, timescale 30000.
    let n = rational_to_cmtime(RationalTime::new(1, Rational::FPS_29_97));
    let (n_val, n_scale) = (n.value, n.timescale);
    assert_eq!(n_val, 1001);
    assert_eq!(n_scale, 30000);
}

#[test]
fn rational_to_cmtime_survives_value_den_overflow() {
    // value·den overflows i64: falls back to microsecond resolution
    // instead of wrapping negative.
    let t = rational_to_cmtime(RationalTime::new(i64::MAX / 2, Rational::new(1, 4)));
    let (value, timescale) = (t.value, t.timescale);
    assert_eq!(timescale, 1_000_000);
    assert!(value > 0, "must not wrap negative");
}

#[test]
fn rotation_from_transform_maps_quadrants() {
    let id = CGAffineTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };
    assert_eq!(rotation_from_transform(id), Rotation::None);
    // 90° CW: a=0, b=1.
    let r90 = CGAffineTransform {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
        tx: 0.0,
        ty: 0.0,
    };
    assert_eq!(rotation_from_transform(r90), Rotation::Cw90);
    // 180°: a=-1, b=0.
    let r180 = CGAffineTransform {
        a: -1.0,
        b: 0.0,
        c: 0.0,
        d: -1.0,
        tx: 0.0,
        ty: 0.0,
    };
    assert_eq!(rotation_from_transform(r180), Rotation::Cw180);
    // 270° CW (a=0, b=-1) normalizes from -90°.
    let r270 = CGAffineTransform {
        a: 0.0,
        b: -1.0,
        c: 1.0,
        d: 0.0,
        tx: 0.0,
        ty: 0.0,
    };
    assert_eq!(rotation_from_transform(r270), Rotation::Cw270);
}
