//! Tests for the RGBA/BGRA → 4:2:0 conversion.
//!
//! Split out of `yuv.rs` as a `#[path]` sibling so both files stay well under
//! the module size ceiling.

use super::*;

/// Build a `width*height` packed image from a per-pixel closure.
fn image(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut buf = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let base = ((y * width + x) * 4) as usize;
            buf[base..base + 4].copy_from_slice(&f(x, y));
        }
    }
    buf
}

/// I420 of a packed RGBA image, into a freshly sized buffer.
fn i420(src: &[u8], width: u32, height: u32, stride: usize, matrix: YuvMatrix) -> Vec<u8> {
    let mut out = vec![0u8; yuv420_len(width, height)];
    rgba_to_i420_into(
        src,
        width,
        height,
        stride,
        matrix,
        ChannelOrder::Rgba,
        &mut out,
    )
    .expect("conversion sized correctly");
    out
}

/// NV12 of a packed RGBA image, into a freshly sized buffer.
fn nv12(src: &[u8], width: u32, height: u32, stride: usize, matrix: YuvMatrix) -> Vec<u8> {
    let mut out = vec![0u8; yuv420_len(width, height)];
    rgba_to_nv12_into(
        src,
        width,
        height,
        stride,
        matrix,
        ChannelOrder::Rgba,
        &mut out,
    )
    .expect("conversion sized correctly");
    out
}

#[test]
fn plane_sizes_are_correct_for_420() {
    // 4*4 luma + 2 * (2*2) chroma = 16 + 8 = 24.
    assert_eq!(yuv420_len(4, 4), 24);
    // Odd dimensions round down to the chroma grid.
    assert_eq!(yuv420_len(5, 5), 24);
    let rgba = image(4, 4, |_, _| [0, 0, 0, 255]);
    assert_eq!(i420(&rgba, 4, 4, 16, YuvMatrix::Bt601).len(), 24);
}

/// Every matrix must have exactly one coefficient row, and the rows must hold
/// the invariants the conversion depends on. Derived from `YuvMatrix::ALL`, so
/// adding a matrix without a row fails to compile (the `index` match is
/// exhaustive) and a mis-ordered table fails here.
#[test]
fn every_matrix_has_one_well_formed_coefficient_row() {
    assert_eq!(COEFFICIENTS.len(), YuvMatrix::ALL.len());
    let mut seen: Vec<usize> = YuvMatrix::ALL.iter().map(|m| m.index()).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), YuvMatrix::ALL.len(), "two matrices share a row");
    assert_eq!(seen.last().copied(), Some(COEFFICIENTS.len() - 1));

    for matrix in YuvMatrix::ALL {
        let c = matrix.coefficients();
        // The rows are limited-range: they scale full-scale white into the
        // 16..235 window, so the Q8 luma row sums to ~220 rather than 256. The
        // assertion is the window itself, run through the conversion's own
        // arithmetic, because 256 is what a full-range row would sum to and this
        // encoder writes no full-range matrix.
        let luma: i32 = c.luma.iter().sum();
        let white = ((luma * 255 + 128) >> 8) + 16;
        assert!(
            (234..=236).contains(&white),
            "{matrix:?} luma row sums to {luma}, which puts full-scale white at \
             {white} instead of 235"
        );
        assert_eq!(
            c.u.iter().sum::<i32>(),
            0,
            "{matrix:?} U row must sum to zero so gray stays neutral"
        );
        assert_eq!(
            c.v.iter().sum::<i32>(),
            0,
            "{matrix:?} V row must sum to zero so gray stays neutral"
        );
    }
}

#[test]
fn solid_black_and_white_map_to_expected_luma() {
    for matrix in YuvMatrix::ALL {
        let black = image(2, 2, |_, _| [0, 0, 0, 255]);
        let white = image(2, 2, |_, _| [255, 255, 255, 255]);
        let yb = i420(&black, 2, 2, 8, matrix);
        let yw = i420(&white, 2, 2, 8, matrix);
        // Limited-range luma: black ≈ 16, white ≈ 235, in either matrix.
        assert_eq!(yb[0], 16, "{matrix:?} black");
        assert!(
            (233..=235).contains(&yw[0]),
            "{matrix:?} white luma was {}",
            yw[0]
        );
    }
}

#[test]
fn gray_is_chroma_neutral() {
    for matrix in YuvMatrix::ALL {
        let gray = image(2, 2, |_, _| [128, 128, 128, 255]);
        let out = i420(&gray, 2, 2, 8, matrix);
        // One U and one V sample, both ≈ 128 (neutral chroma) for gray.
        let u = out[4];
        let v = out[5];
        assert!((127..=129).contains(&u), "{matrix:?} U was {u}");
        assert!((127..=129).contains(&v), "{matrix:?} V was {v}");
    }
}

#[test]
fn matrices_produce_distinct_chroma_for_saturated_color() {
    // Pure blue: BT.601 puts more of it in luma than BT.709 does, so the
    // planes must differ — guards against the matrix silently not being
    // applied.
    let blue = image(2, 2, |_, _| [0, 0, 255, 255]);
    let out_601 = i420(&blue, 2, 2, 8, YuvMatrix::Bt601);
    let out_709 = i420(&blue, 2, 2, 8, YuvMatrix::Bt709);
    assert!(out_601[0] > out_709[0], "601 blue luma should exceed 709");
    assert_ne!(out_601, out_709);
}

#[test]
fn bt709_roundtrips_the_mobile_fixture_blue() {
    // The workspace's decode paths convert untagged HD H.264 back with
    // BT.709; encoding [40, 80, 160] with BT.709 must land within the
    // decoder tests' per-channel tolerance after the inverse transform.
    let rgba = image(2, 2, |_, _| [40, 80, 160, 255]);
    let out = i420(&rgba, 2, 2, 8, YuvMatrix::Bt709);
    let (y, u, v) = (
        f64::from(out[0]) - 16.0,
        f64::from(out[4]) - 128.0,
        f64::from(out[5]) - 128.0,
    );
    // Inverse BT.709 limited-range.
    let r = 1.164 * y + 1.793 * v;
    let g = 1.164 * y - 0.213 * u - 0.533 * v;
    let b = 1.164 * y + 2.112 * u;
    assert!((r - 40.0).abs() < 3.0, "R came back as {r}");
    assert!((g - 80.0).abs() < 3.0, "G came back as {g}");
    assert!((b - 160.0).abs() < 3.0, "B came back as {b}");
}

#[test]
fn matrix_for_maps_every_declared_color_matrix() {
    use aria_core::{ColorSpace, MatrixCoefficients};

    assert_eq!(matrix_for(&ColorSpace::BT601), YuvMatrix::Bt601);
    assert_eq!(matrix_for(&ColorSpace::BT709), YuvMatrix::Bt709);
    assert_eq!(matrix_for(&ColorSpace::BT2020_PQ), YuvMatrix::Bt2020);
    assert_eq!(matrix_for(&ColorSpace::SRGB), YuvMatrix::Bt709);
    assert_eq!(
        matrix_for(&ColorSpace {
            matrix: MatrixCoefficients::Unspecified,
            ..ColorSpace::BT709
        }),
        YuvMatrix::Bt709
    );
}

#[test]
fn nv12_matches_i420_samples_with_interleaved_chroma() {
    // A red/blue split so U and V differ per sample.
    let rgba = image(4, 4, |x, _| {
        if x < 2 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let planar = i420(&rgba, 4, 4, 16, YuvMatrix::Bt601);
    let packed = nv12(&rgba, 4, 4, 16, YuvMatrix::Bt601);
    assert_eq!(packed.len(), planar.len());
    // Identical luma plane.
    assert_eq!(packed[..16], planar[..16]);
    // Chroma: NV12 interleaves the I420 U and V planes pairwise.
    let (u, v) = planar[16..].split_at(4);
    for i in 0..4 {
        assert_eq!(packed[16 + i * 2], u[i], "U sample {i}");
        assert_eq!(packed[16 + i * 2 + 1], v[i], "V sample {i}");
    }
}

#[test]
fn honors_source_stride() {
    // 2×2 image in a buffer padded to a 12-byte stride; padding must be
    // ignored.
    let width = 2;
    let height = 2;
    let stride = 12;
    let mut rgba = vec![7u8; stride * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let base = y * stride + x * 4;
            rgba[base..base + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let out = i420(&rgba, width, height, stride, YuvMatrix::Bt601);
    assert!((233..=235).contains(&out[0]));
}

/// Converting BGRA in place must match converting a pre-swapped RGBA copy —
/// the whole point of folding the channel order into the pass instead of
/// copying the frame into a re-ordered buffer first.
#[test]
fn bgra_order_matches_a_pre_swapped_rgba_frame() {
    let rgba = image(4, 4, |x, y| [(x * 60) as u8, (y * 50) as u8, 200, 255]);
    let mut bgra = rgba.clone();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    for matrix in YuvMatrix::ALL {
        let expected = i420(&rgba, 4, 4, 16, matrix);
        let mut got = vec![0u8; yuv420_len(4, 4)];
        rgba_to_i420_into(&bgra, 4, 4, 16, matrix, ChannelOrder::Bgra, &mut got)
            .expect("conversion sized correctly");
        assert_eq!(got, expected, "{matrix:?} BGRA");

        let expected_nv12 = nv12(&rgba, 4, 4, 16, matrix);
        let mut got_nv12 = vec![0u8; yuv420_len(4, 4)];
        rgba_to_nv12_into(&bgra, 4, 4, 16, matrix, ChannelOrder::Bgra, &mut got_nv12)
            .expect("conversion sized correctly");
        assert_eq!(got_nv12, expected_nv12, "{matrix:?} BGRA NV12");
    }
}

/// A reused scratch buffer is only correct if the writers cover every output
/// byte: leftovers from the previous frame must not survive.
#[test]
fn conversion_overwrites_every_byte_of_a_dirty_buffer() {
    let black = image(4, 4, |_, _| [0, 0, 0, 255]);
    let mut dirty = vec![0xABu8; yuv420_len(4, 4)];
    rgba_to_nv12_into(
        &black,
        4,
        4,
        16,
        YuvMatrix::Bt709,
        ChannelOrder::Rgba,
        &mut dirty,
    )
    .expect("conversion sized correctly");
    assert!(
        dirty.iter().all(|&b| b != 0xAB),
        "stale bytes survived: {dirty:?}"
    );
}

#[test]
fn short_buffers_are_refused_rather_than_panicking() {
    let rgba = image(4, 4, |_, _| [0, 0, 0, 255]);
    let mut small = vec![0u8; yuv420_len(4, 4) - 1];
    assert!(
        rgba_to_i420_into(
            &rgba,
            4,
            4,
            16,
            YuvMatrix::Bt709,
            ChannelOrder::Rgba,
            &mut small
        )
        .is_err(),
        "a destination one byte short must be an error"
    );

    let mut out = vec![0u8; yuv420_len(4, 4)];
    assert!(
        rgba_to_i420_into(
            &rgba[..16],
            4,
            4,
            16,
            YuvMatrix::Bt709,
            ChannelOrder::Rgba,
            &mut out
        )
        .is_err(),
        "a source shorter than stride × rows must be an error"
    );
    assert!(
        rgba_to_i420_into(
            &rgba,
            4,
            4,
            8,
            YuvMatrix::Bt709,
            ChannelOrder::Rgba,
            &mut out
        )
        .is_err(),
        "a stride narrower than the picture must be an error"
    );
    assert!(
        rgba_to_i420_into(
            &rgba,
            1,
            1,
            16,
            YuvMatrix::Bt709,
            ChannelOrder::Rgba,
            &mut out
        )
        .is_err(),
        "a sub-2×2 picture has no chroma sample"
    );
}
