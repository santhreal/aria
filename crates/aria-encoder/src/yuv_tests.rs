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
    for px in bgra.as_chunks_mut::<4>().0 {
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

/// NV12 of `src` written in one pass over every row, the way a frame too
/// small to split is converted.
fn reference_write_luma<const SWAP: bool>(
    src: &[u8],
    y_plane: &mut [u8],
    plan: &Plan,
    coeffs: &YuvCoefficients,
) {
    let [cr, cg, cb] = coeffs.luma;
    for (dst_row, src_row) in y_plane
        .chunks_exact_mut(plan.w)
        .zip(src.chunks_exact(plan.stride))
    {
        let (pixels, _) = src_row.as_chunks::<4>();
        for (dst, px) in dst_row.iter_mut().zip(pixels) {
            let (r, g, b) = rgb::<SWAP>(px);
            let luma = (cr * r + cg * g + cb * b + 128) >> 8;
            *dst = (luma + 16).clamp(0, 255) as u8;
        }
    }
}

fn reference_write_chroma_interleaved<const SWAP: bool>(
    src: &[u8],
    uv_plane: &mut [u8],
    plan: &Plan,
    coeffs: &YuvCoefficients,
) {
    for (uv_row, rows) in uv_plane
        .chunks_exact_mut(plan.cw * 2)
        .zip(src.chunks_exact(plan.stride * 2))
    {
        let (top, bottom) = rows.split_at(plan.stride);
        let (samples, _) = uv_row.as_chunks_mut::<2>();
        for ((sample, t), b) in samples
            .iter_mut()
            .zip(pixel_pairs(top))
            .zip(pixel_pairs(bottom))
        {
            let (mut rs, mut gs, mut bs) = (0i32, 0i32, 0i32);
            for px in t.iter().chain(b.iter()) {
                let (r, g, b) = rgb::<SWAP>(px);
                rs += r;
                gs += g;
                bs += b;
            }
            let (r, g, b) = (rs / 4, gs / 4, bs / 4);
            let [ur, ug, ub] = coeffs.u;
            let [vr, vg, vb] = coeffs.v;
            let u = (((ur * r + ug * g + ub * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            let v = (((vr * r + vg * g + vb * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            *sample = [u, v];
        }
    }
}

fn one_pass<const SWAP: bool>(src: &[u8], width: u32, height: u32, stride: usize) -> Vec<u8> {
    let mut out = vec![0u8; yuv420_len(width, height)];
    let plan = Plan::new(width, height, stride, src.len(), out.len()).expect("valid geometry");
    let coeffs = YuvMatrix::Bt709.coefficients();
    let (y, uv) = out[..plan.frame_len()].split_at_mut(plan.y_len());
    reference_write_luma::<SWAP>(&src[..plan.src_len()], y, &plan, &coeffs);
    reference_write_chroma_interleaved::<SWAP>(&src[..plan.src_len()], uv, &plan, &coeffs);
    out
}

/// A frame large enough to be converted in bands gives the bytes one pass
/// over its rows gives, in both channel orders, with a padded stride and an
/// odd size. A band that started on the wrong row, or a chroma row written
/// into another band's place, changes bytes here.
#[test]
fn banded_conversion_matches_one_pass() {
    let (width, height) = (1923u32, 1083u32);
    let stride = width as usize * 4 + 64;
    let src: Vec<u8> = (0..stride * height as usize)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect();
    let plan = Plan::new(width, height, stride, src.len(), yuv420_len(width, height))
        .expect("valid geometry");
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    assert!(
        cores == 1 || bands(&plan) > 1,
        "a 1080p frame on {cores} cores is converted in one band"
    );
    for (order, expected) in [
        (
            ChannelOrder::Rgba,
            one_pass::<false>(&src, width, height, stride),
        ),
        (
            ChannelOrder::Bgra,
            one_pass::<true>(&src, width, height, stride),
        ),
    ] {
        let mut got = vec![0u8; yuv420_len(width, height)];
        rgba_to_nv12_into(
            &src,
            width,
            height,
            stride,
            YuvMatrix::Bt709,
            order,
            &mut got,
        )
        .expect("conversion sized correctly");
        let first = got.iter().zip(&expected).position(|(a, b)| a != b);
        assert_eq!(first, None, "{order:?}: the first differing byte");
    }
}

/// **Every `YuvMatrix` variant, its fixed-point `[cr, cg, cb]` / `U` / `V` coefficients, and `matrix_for` mapping match `docs/reference/color.md`.**
///
/// Closes: undocumented `YuvMatrix` variant, drifted fixed-point coefficient
/// row in `COEFFICIENTS`, or changed `matrix_for` mapping.
/// Not caught: prose commentary around the YUV equations.
#[test]
fn color_doc_yuv_matrices_match_coefficients() {
    let color_doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reference/color.md"
    ))
    .expect("read docs/reference/color.md");
    let start = color_doc
        .find("## YUV 4:2:0 conversion matrices (`YuvMatrix`)")
        .expect("missing YuvMatrix section in color.md");
    let rest = &color_doc[start..];
    let end = rest[1..].find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
    let sec = &rest[..end];

    let mut rows = Vec::new();
    let mut in_table = false;
    let mut past_sep = false;
    for line in sec.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            if in_table {
                in_table = false;
                past_sep = false;
            }
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if !in_table {
            in_table = true;
            continue;
        }
        if !past_sep {
            past_sep = true;
            continue;
        }
        rows.push(cells);
    }

    fn unquote(s: &str) -> &str {
        s.trim().trim_start_matches('`').trim_end_matches('`')
    }
    let doc_matrices: Vec<(String, String, String, String, Vec<String>)> = rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed YuvMatrix row: {r:?}");
            let mapped: Vec<String> = r[4].split(',').map(|s| unquote(s).to_string()).collect();
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                mapped,
            )
        })
        .collect();

    let all_mc = [
        aria_core::MatrixCoefficients::Bt709,
        aria_core::MatrixCoefficients::Bt601,
        aria_core::MatrixCoefficients::Bt2020Ncl,
        aria_core::MatrixCoefficients::Identity,
        aria_core::MatrixCoefficients::Unspecified,
    ];
    let expected_matrices: Vec<(String, String, String, String, Vec<String>)> = YuvMatrix::ALL
        .iter()
        .map(|&m| {
            let c = m.coefficients();
            let mapped: Vec<String> = all_mc
                .iter()
                .copied()
                .filter(|&mc| {
                    let cs = aria_core::ColorSpace {
                        matrix: mc,
                        ..aria_core::ColorSpace::BT709
                    };
                    matrix_for(&cs) == m
                })
                .map(|mc| format!("{mc:?}"))
                .collect();
            (
                format!("{m:?}"),
                format!("{:?}", c.luma),
                format!("{:?}", c.u),
                format!("{:?}", c.v),
                mapped,
            )
        })
        .collect();
    assert_eq!(
        doc_matrices, expected_matrices,
        "docs/reference/color.md YuvMatrix table drifted from YuvMatrix::ALL / COEFFICIENTS / matrix_for"
    );
}
