//! CPU packing of decoded YUV layouts into biplanar 8-bit `NV12`.
//!
//! Software decoders emit planar `I420` and `I420_10LE`, and hardware
//! decoders copied to system memory emit semi-planar `P010_10LE`. Each pack
//! writes `NV12` planes from the pool in [`crate::plane_pool`] in one pass,
//! with an AVX2 path where the CPU has it and a scalar path otherwise. 10-bit
//! samples keep their 8 high bits.
//! The module uses `x86`/`x86_64` AVX2 SIMD intrinsics and unaligned vector loads/stores and is `unsafe` in its SIMD paths.

#![allow(unsafe_code)]

use aria_core::{DecodeError, Plane};

use crate::plane_pool::{copy_plane_bytes, take_plane_vec};

fn check_plane_len(
    plane: &str,
    bytes: &[u8],
    row_bytes: usize,
    rows: usize,
    stride: usize,
) -> Result<(), DecodeError> {
    if rows == 0 || row_bytes == 0 {
        return Ok(());
    }
    if stride < row_bytes {
        return Err(DecodeError::Decode(format!(
            "decoded {plane} plane stride ({stride} bytes) is smaller than row width ({row_bytes} bytes). Try another file."
        )));
    }
    let needed = (rows - 1)
        .checked_mul(stride)
        .and_then(|prefix| prefix.checked_add(row_bytes))
        .ok_or_else(|| {
            DecodeError::Decode(format!(
                "decoded {plane} plane dimensions overflow ({rows} rows at stride {stride}). Try another file."
            ))
        })?;
    if bytes.len() < needed {
        return Err(DecodeError::Decode(format!(
            "decoded {plane} plane is too small ({} bytes, need {needed} for {rows} rows at stride {stride}). Try another file.",
            bytes.len()
        )));
    }
    Ok(())
}

/// Pack 8-bit planar `I420` (`Y`, `U`, `V`) into biplanar `NV12` (`Y`, `UV`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pack_i420_to_nv12_planes(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
) -> Result<Vec<Plane>, DecodeError> {
    let cw = width.div_ceil(2);
    let ch = rows.div_ceil(2);
    check_plane_len("Y", y, width, rows, y_stride)?;
    check_plane_len("U", u, cw, ch, u_stride)?;
    check_plane_len("V", v, cw, ch, v_stride)?;
    let uv_stride = cw * 2;
    let y_out = if y_stride == width && y.len() >= width * rows {
        copy_plane_bytes(&y[..width * rows])
    } else {
        let mut out = take_plane_vec(width * rows);
        for row in 0..rows {
            let src_row = &y[row * y_stride..row * y_stride + width];
            out[row * width..(row + 1) * width].copy_from_slice(src_row);
        }
        out
    };
    let mut uv = take_plane_vec(uv_stride * ch);
    interleave_i420_uv(u, v, cw, ch, u_stride, v_stride, &mut uv);
    Ok(vec![
        Plane::new(y_out, width, rows),
        Plane::new(uv, uv_stride, ch),
    ])
}

fn interleave_i420_uv(
    u: &[u8],
    v: &[u8],
    cw: usize,
    ch: usize,
    u_stride: usize,
    v_stride: usize,
    uv: &mut [u8],
) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: guarded by runtime AVX2 feature detection above.
        unsafe {
            interleave_i420_uv_avx2(u, v, cw, ch, u_stride, v_stride, uv);
        }
        return;
    }
    interleave_i420_uv_scalar(u, v, cw, ch, u_stride, v_stride, uv);
}

fn interleave_i420_uv_scalar(
    u: &[u8],
    v: &[u8],
    cw: usize,
    ch: usize,
    u_stride: usize,
    v_stride: usize,
    uv: &mut [u8],
) {
    let uv_stride = cw * 2;
    for row in 0..ch {
        let u_row = &u[row * u_stride..row * u_stride + cw];
        let v_row = &v[row * v_stride..row * v_stride + cw];
        let dst_row = &mut uv[row * uv_stride..(row + 1) * uv_stride];
        for ((dst, &u_val), &v_val) in dst_row.chunks_exact_mut(2).zip(u_row).zip(v_row) {
            dst[0] = u_val;
            dst[1] = v_val;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn interleave_i420_uv_avx2(
    u: &[u8],
    v: &[u8],
    cw: usize,
    ch: usize,
    u_stride: usize,
    v_stride: usize,
    uv: &mut [u8],
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{_mm_loadu_si128, _mm_storeu_si128, _mm_unpackhi_epi8, _mm_unpacklo_epi8};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        _mm_loadu_si128, _mm_storeu_si128, _mm_unpackhi_epi8, _mm_unpacklo_epi8,
    };

    let uv_stride = cw * 2;
    for row in 0..ch {
        let u_row = &u[row * u_stride..row * u_stride + cw];
        let v_row = &v[row * v_stride..row * v_stride + cw];
        let dst_row = &mut uv[row * uv_stride..(row + 1) * uv_stride];
        let mut u_chunks = u_row.chunks_exact(16);
        let mut v_chunks = v_row.chunks_exact(16);
        let mut dst_chunks = dst_row.chunks_exact_mut(32);
        for ((dst, u_src), v_src) in (&mut dst_chunks).zip(&mut u_chunks).zip(&mut v_chunks) {
            // SAFETY: `u_src` and `v_src` are 16-byte slices and `dst` is a 32-byte slice.
            unsafe {
                let u_vec = _mm_loadu_si128(u_src.as_ptr().cast());
                let v_vec = _mm_loadu_si128(v_src.as_ptr().cast());
                let lo = _mm_unpacklo_epi8(u_vec, v_vec);
                let hi = _mm_unpackhi_epi8(u_vec, v_vec);
                _mm_storeu_si128(dst.as_mut_ptr().cast(), lo);
                _mm_storeu_si128(dst[16..].as_mut_ptr().cast(), hi);
            }
        }
        for ((dst, &u_val), &v_val) in dst_chunks
            .into_remainder()
            .chunks_exact_mut(2)
            .zip(u_chunks.remainder())
            .zip(v_chunks.remainder())
        {
            dst[0] = u_val;
            dst[1] = v_val;
        }
    }
}

/// Pack 10-bit little-endian planar `I420_10LE` (`Y`, `U`, `V`) into 8-bit
/// biplanar `NV12` (`Y`, `UV`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pack_i420_10le_to_nv12_planes(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
) -> Result<Vec<Plane>, DecodeError> {
    let cw = width.div_ceil(2);
    let ch = rows.div_ceil(2);
    check_plane_len("Y", y, width * 2, rows, y_stride)?;
    check_plane_len("U", u, cw * 2, ch, u_stride)?;
    check_plane_len("V", v, cw * 2, ch, v_stride)?;
    let uv_stride = cw * 2;
    let mut y_out = take_plane_vec(width * rows);
    let mut uv_out = take_plane_vec(uv_stride * ch);
    convert_i420_10le_rows(
        y,
        u,
        v,
        width,
        rows,
        y_stride,
        u_stride,
        v_stride,
        &mut y_out,
        &mut uv_out,
    );
    Ok(vec![
        Plane::new(y_out, width, rows),
        Plane::new(uv_out, uv_stride, ch),
    ])
}

#[allow(clippy::too_many_arguments)]
fn convert_i420_10le_rows(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
    y_out: &mut [u8],
    uv_out: &mut [u8],
) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: guarded by runtime AVX2 feature detection above.
        unsafe {
            convert_i420_10le_rows_avx2(
                y, u, v, width, rows, y_stride, u_stride, v_stride, y_out, uv_out,
            );
        }
        return;
    }
    convert_i420_10le_rows_scalar(
        y, u, v, width, rows, y_stride, u_stride, v_stride, y_out, uv_out,
    );
}

#[allow(clippy::too_many_arguments)]
fn convert_i420_10le_rows_scalar(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
    y_out: &mut [u8],
    uv_out: &mut [u8],
) {
    for row in 0..rows {
        let src_row = &y[row * y_stride..row * y_stride + width * 2];
        let dst_row = &mut y_out[row * width..(row + 1) * width];
        for (dst, src) in dst_row.iter_mut().zip(src_row.chunks_exact(2)) {
            *dst = (u16::from_le_bytes([src[0], src[1]]) >> 2) as u8;
        }
    }
    let cw = width.div_ceil(2);
    let ch = rows.div_ceil(2);
    let uv_stride = cw * 2;
    for row in 0..ch {
        let u_row = &u[row * u_stride..row * u_stride + cw * 2];
        let v_row = &v[row * v_stride..row * v_stride + cw * 2];
        let dst_row = &mut uv_out[row * uv_stride..(row + 1) * uv_stride];
        for ((dst, u_src), v_src) in dst_row
            .chunks_exact_mut(2)
            .zip(u_row.chunks_exact(2))
            .zip(v_row.chunks_exact(2))
        {
            dst[0] = (u16::from_le_bytes([u_src[0], u_src[1]]) >> 2) as u8;
            dst[1] = (u16::from_le_bytes([v_src[0], v_src[1]]) >> 2) as u8;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
unsafe fn convert_i420_10le_rows_avx2(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
    y_out: &mut [u8],
    uv_out: &mut [u8],
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        _mm256_and_si256, _mm256_loadu_si256, _mm256_or_si256, _mm256_packus_epi16,
        _mm256_permute4x64_epi64, _mm256_set1_epi16, _mm256_slli_epi16, _mm256_srli_epi16,
        _mm256_storeu_si256,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        _mm256_and_si256, _mm256_loadu_si256, _mm256_or_si256, _mm256_packus_epi16,
        _mm256_permute4x64_epi64, _mm256_set1_epi16, _mm256_slli_epi16, _mm256_srli_epi16,
        _mm256_storeu_si256,
    };

    let low_byte = _mm256_set1_epi16(0x00ff);
    for row in 0..rows {
        let src_row = &y[row * y_stride..row * y_stride + width * 2];
        let dst_row = &mut y_out[row * width..(row + 1) * width];
        let mut src_chunks = src_row.chunks_exact(64);
        let mut dst_chunks = dst_row.chunks_exact_mut(32);
        for (dst, src) in (&mut dst_chunks).zip(&mut src_chunks) {
            // SAFETY: `src` is a 64-byte slice and `dst` is a 32-byte slice.
            unsafe {
                let lo = _mm256_and_si256(
                    _mm256_srli_epi16(_mm256_loadu_si256(src.as_ptr().cast()), 2),
                    low_byte,
                );
                let hi = _mm256_and_si256(
                    _mm256_srli_epi16(_mm256_loadu_si256(src[32..].as_ptr().cast()), 2),
                    low_byte,
                );
                let packed = _mm256_packus_epi16(lo, hi);
                let ordered = _mm256_permute4x64_epi64(packed, 0b11_01_10_00);
                _mm256_storeu_si256(dst.as_mut_ptr().cast(), ordered);
            }
        }
        for (dst, src) in dst_chunks
            .into_remainder()
            .iter_mut()
            .zip(src_chunks.remainder().chunks_exact(2))
        {
            *dst = (u16::from_le_bytes([src[0], src[1]]) >> 2) as u8;
        }
    }
    let cw = width.div_ceil(2);
    let ch = rows.div_ceil(2);
    let uv_stride = cw * 2;
    for row in 0..ch {
        let u_row = &u[row * u_stride..row * u_stride + cw * 2];
        let v_row = &v[row * v_stride..row * v_stride + cw * 2];
        let dst_row = &mut uv_out[row * uv_stride..(row + 1) * uv_stride];
        let mut u_chunks = u_row.chunks_exact(32);
        let mut v_chunks = v_row.chunks_exact(32);
        let mut dst_chunks = dst_row.chunks_exact_mut(32);
        for ((dst, u_src), v_src) in (&mut dst_chunks).zip(&mut u_chunks).zip(&mut v_chunks) {
            // SAFETY: `u_src`, `v_src`, and `dst` are 32-byte slices.
            unsafe {
                let u_vec = _mm256_and_si256(
                    _mm256_srli_epi16(_mm256_loadu_si256(u_src.as_ptr().cast()), 2),
                    low_byte,
                );
                let v_vec = _mm256_slli_epi16(
                    _mm256_srli_epi16(_mm256_loadu_si256(v_src.as_ptr().cast()), 2),
                    8,
                );
                let uv = _mm256_or_si256(u_vec, v_vec);
                _mm256_storeu_si256(dst.as_mut_ptr().cast(), uv);
            }
        }
        for ((dst, u_src), v_src) in dst_chunks
            .into_remainder()
            .chunks_exact_mut(2)
            .zip(u_chunks.remainder().chunks_exact(2))
            .zip(v_chunks.remainder().chunks_exact(2))
        {
            dst[0] = (u16::from_le_bytes([u_src[0], u_src[1]]) >> 2) as u8;
            dst[1] = (u16::from_le_bytes([v_src[0], v_src[1]]) >> 2) as u8;
        }
    }
}

/// Pack 10-bit little-endian semi-planar `P010_10LE` (`Y`, interleaved `UV`,
/// MSB-aligned in 16-bit words) into 8-bit biplanar `NV12` (`Y`, `UV`).
pub(crate) fn pack_p010_10le_to_nv12_planes(
    y: &[u8],
    uv: &[u8],
    width: usize,
    rows: usize,
    y_stride: usize,
    uv_in_stride: usize,
) -> Result<Vec<Plane>, DecodeError> {
    let cw = width.div_ceil(2);
    let ch = rows.div_ceil(2);
    let uv_samples = cw * 2;
    check_plane_len("Y", y, width * 2, rows, y_stride)?;
    check_plane_len("UV", uv, uv_samples * 2, ch, uv_in_stride)?;
    let mut y_out = take_plane_vec(width * rows);
    let mut uv_out = take_plane_vec(uv_samples * ch);
    pack_p010_10le_plane(y, width, rows, y_stride, &mut y_out);
    pack_p010_10le_plane(uv, uv_samples, ch, uv_in_stride, &mut uv_out);
    Ok(vec![
        Plane::new(y_out, width, rows),
        Plane::new(uv_out, uv_samples, ch),
    ])
}

fn pack_p010_10le_plane(
    src: &[u8],
    row_samples: usize,
    rows: usize,
    src_stride: usize,
    dst: &mut [u8],
) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: guarded by runtime AVX2 feature detection above.
        unsafe {
            pack_p010_10le_plane_avx2(src, row_samples, rows, src_stride, dst);
        }
        return;
    }
    pack_p010_10le_plane_scalar(src, row_samples, rows, src_stride, dst);
}

fn pack_p010_10le_plane_scalar(
    src: &[u8],
    row_samples: usize,
    rows: usize,
    src_stride: usize,
    dst: &mut [u8],
) {
    let src_row_bytes = row_samples * 2;
    for row in 0..rows {
        let src_row = &src[row * src_stride..row * src_stride + src_row_bytes];
        let dst_row = &mut dst[row * row_samples..(row + 1) * row_samples];
        for (d, s) in dst_row.iter_mut().zip(src_row.chunks_exact(2)) {
            *d = (u16::from_le_bytes([s[0], s[1]]) >> 8) as u8;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn pack_p010_10le_plane_avx2(
    src: &[u8],
    row_samples: usize,
    rows: usize,
    src_stride: usize,
    dst: &mut [u8],
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        _mm256_loadu_si256, _mm256_packus_epi16, _mm256_permute4x64_epi64, _mm256_srli_epi16,
        _mm256_storeu_si256,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        _mm256_loadu_si256, _mm256_packus_epi16, _mm256_permute4x64_epi64, _mm256_srli_epi16,
        _mm256_storeu_si256,
    };

    let src_row_bytes = row_samples * 2;
    for row in 0..rows {
        let src_row = &src[row * src_stride..row * src_stride + src_row_bytes];
        let dst_row = &mut dst[row * row_samples..(row + 1) * row_samples];
        let mut src_chunks = src_row.chunks_exact(64);
        let mut dst_chunks = dst_row.chunks_exact_mut(32);
        for (d, s) in (&mut dst_chunks).zip(&mut src_chunks) {
            // SAFETY: `s` is a 64-byte slice and `d` is a 32-byte slice.
            unsafe {
                let lo = _mm256_srli_epi16(_mm256_loadu_si256(s.as_ptr().cast()), 8);
                let hi = _mm256_srli_epi16(_mm256_loadu_si256(s[32..].as_ptr().cast()), 8);
                let packed = _mm256_packus_epi16(lo, hi);
                let ordered = _mm256_permute4x64_epi64(packed, 0b11_01_10_00);
                _mm256_storeu_si256(d.as_mut_ptr().cast(), ordered);
            }
        }
        for (d, s) in dst_chunks
            .into_remainder()
            .iter_mut()
            .zip(src_chunks.remainder().chunks_exact(2))
        {
            *d = (u16::from_le_bytes([s[0], s[1]]) >> 8) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: 4K `I420_10LE` and `I420` conversion ran scalar byte loops and
    /// allocated fresh multi-megabyte `Vec<u8>` planes every frame, causing
    /// `decode_late` frame drops on 4K60 HEVC playback. Closes the class of
    /// SIMD lane/remainder mismatches across odd widths, padded strides, and
    /// full 10-bit (`0..=1023`) and 16-bit inputs.
    #[test]
    fn i420_and_i420_10le_pack_matches_scalar_across_odd_widths_and_padded_strides() {
        for width in [1usize, 2, 3, 15, 16, 17, 31, 32, 33, 63, 64, 65, 97] {
            for rows in [1usize, 2, 3, 5] {
                for pad in [0usize, 14] {
                    let cw = width.div_ceil(2);
                    let ch = rows.div_ceil(2);
                    let y_stride8 = width + pad;
                    let u_stride8 = cw + pad;
                    let v_stride8 = cw + pad / 2;
                    let y8: Vec<u8> = (0..y_stride8 * rows)
                        .map(|i| ((i * 73 + 19) & 0xff) as u8)
                        .collect();
                    let u8_in: Vec<u8> = (0..u_stride8 * ch)
                        .map(|i| ((i * 41 + 7) & 0xff) as u8)
                        .collect();
                    let v8_in: Vec<u8> = (0..v_stride8 * ch)
                        .map(|i| ((i * 97 + 131) & 0xff) as u8)
                        .collect();

                    let planes8 = pack_i420_to_nv12_planes(
                        &y8, &u8_in, &v8_in, width, rows, y_stride8, u_stride8, v_stride8,
                    )
                    .expect("pack 8-bit i420");
                    assert_eq!(planes8[0].stride, width);
                    assert_eq!(planes8[0].rows, rows);
                    for r in 0..rows {
                        assert_eq!(
                            &planes8[0].data[r * width..(r + 1) * width],
                            &y8[r * y_stride8..r * y_stride8 + width],
                            "8-bit Y mismatch at width={width} rows={rows} pad={pad} row={r}"
                        );
                    }
                    let mut expected_uv8 = vec![0u8; cw * 2 * ch];
                    interleave_i420_uv_scalar(
                        &u8_in,
                        &v8_in,
                        cw,
                        ch,
                        u_stride8,
                        v_stride8,
                        &mut expected_uv8,
                    );
                    assert_eq!(
                        planes8[1].data, expected_uv8,
                        "8-bit UV mismatch at width={width} rows={rows} pad={pad}"
                    );

                    let y_stride10 = (width + pad) * 2;
                    let u_stride10 = (cw + pad) * 2;
                    let v_stride10 = (cw + pad / 2) * 2;
                    let y10: Vec<u8> = (0..y_stride10 * rows / 2)
                        .flat_map(|i| {
                            let val = ((i * 257 + 3) as u16) & 0x03ff;
                            val.to_le_bytes()
                        })
                        .collect();
                    let u10: Vec<u8> = (0..u_stride10 * ch / 2)
                        .flat_map(|i| {
                            let val = ((i * 193 + 509) as u16) & 0x03ff;
                            val.to_le_bytes()
                        })
                        .collect();
                    let v10: Vec<u8> = (0..v_stride10 * ch / 2)
                        .flat_map(|i| {
                            // Include high bits above bit 9 to verify masking before packing.
                            let val = (i * 311 + 1021) as u16;
                            val.to_le_bytes()
                        })
                        .collect();

                    let planes10 = pack_i420_10le_to_nv12_planes(
                        &y10, &u10, &v10, width, rows, y_stride10, u_stride10, v_stride10,
                    )
                    .expect("pack 10-bit i420_10le");
                    let mut expected_y10 = vec![0u8; width * rows];
                    let mut expected_uv10 = vec![0u8; cw * 2 * ch];
                    convert_i420_10le_rows_scalar(
                        &y10,
                        &u10,
                        &v10,
                        width,
                        rows,
                        y_stride10,
                        u_stride10,
                        v_stride10,
                        &mut expected_y10,
                        &mut expected_uv10,
                    );
                    assert_eq!(
                        planes10[0].data, expected_y10,
                        "10-bit Y mismatch at width={width} rows={rows} pad={pad}"
                    );
                    assert_eq!(
                        planes10[1].data, expected_uv10,
                        "10-bit UV mismatch at width={width} rows={rows} pad={pad}"
                    );

                    let uv_stride_p010 = (cw * 2 + pad) * 2;
                    let y_p010: Vec<u8> = (0..y_stride10 * rows / 2)
                        .flat_map(|i| {
                            let val = (((i * 179 + 11) & 0x03ff) as u16) << 6;
                            val.to_le_bytes()
                        })
                        .collect();
                    let uv_p010: Vec<u8> = (0..uv_stride_p010 * ch / 2)
                        .flat_map(|i| {
                            let val = (((i * 251 + 43) & 0x03ff) as u16) << 6;
                            val.to_le_bytes()
                        })
                        .collect();
                    let planes_p010 = pack_p010_10le_to_nv12_planes(
                        &y_p010,
                        &uv_p010,
                        width,
                        rows,
                        y_stride10,
                        uv_stride_p010,
                    )
                    .expect("pack 10-bit p010_10le");
                    let mut expected_y_p010 = vec![0u8; width * rows];
                    let mut expected_uv_p010 = vec![0u8; cw * 2 * ch];
                    pack_p010_10le_plane_scalar(
                        &y_p010,
                        width,
                        rows,
                        y_stride10,
                        &mut expected_y_p010,
                    );
                    pack_p010_10le_plane_scalar(
                        &uv_p010,
                        cw * 2,
                        ch,
                        uv_stride_p010,
                        &mut expected_uv_p010,
                    );
                    assert_eq!(
                        planes_p010[0].data, expected_y_p010,
                        "P010_10LE Y mismatch at width={width} rows={rows} pad={pad}"
                    );
                    assert_eq!(
                        planes_p010[1].data, expected_uv_p010,
                        "P010_10LE UV mismatch at width={width} rows={rows} pad={pad}"
                    );
                }
            }
        }
        assert!(
            pack_i420_to_nv12_planes(&[0u8; 3], &[0u8; 2], &[0u8; 2], 4, 2, 4, 2, 2).is_err(),
            "truncated I420 Y plane must return DecodeError"
        );
        assert!(
            pack_i420_10le_to_nv12_planes(&[0u8; 7], &[0u8; 4], &[0u8; 4], 4, 2, 8, 4, 4).is_err(),
            "truncated I420_10LE Y plane must return DecodeError"
        );
        assert!(
            pack_p010_10le_to_nv12_planes(&[0u8; 16], &[0u8; 7], 4, 2, 8, 8).is_err(),
            "truncated P010_10LE UV plane must return DecodeError"
        );
    }
}
