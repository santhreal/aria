//! RGBA ↔ BGRA pixel conversions for hardware encoders.
//!
//! Apple VideoToolbox expects 32-bit BGRA (`kCVPixelFormatType_32BGRA`).
//! This conversion is chunked via `as_chunks::<16>()` to allow auto-vectorization.

/// Swizzle packed 8-bit RGBA row into BGRA in-place or into a separate destination.
#[inline]
pub fn swizzle_rgba_to_bgra(src: &[u8], dst: &mut [u8], width: usize) {
    let row_bytes = width * 4;
    let s = &src[..row_bytes];
    let d = &mut dst[..row_bytes];
    let (s_chunks, s_rem) = s.as_chunks::<16>();
    let (d_chunks, d_rem) = d.as_chunks_mut::<16>();
    for (sc, dc) in s_chunks.iter().zip(d_chunks.iter_mut()) {
        dc[0] = sc[2];
        dc[1] = sc[1];
        dc[2] = sc[0];
        dc[3] = sc[3];
        dc[4] = sc[6];
        dc[5] = sc[5];
        dc[6] = sc[4];
        dc[7] = sc[7];
        dc[8] = sc[10];
        dc[9] = sc[9];
        dc[10] = sc[8];
        dc[11] = sc[11];
        dc[12] = sc[14];
        dc[13] = sc[13];
        dc[14] = sc[12];
        dc[15] = sc[15];
    }
    let (s_px, _) = s_rem.as_chunks::<4>();
    let (d_px, _) = d_rem.as_chunks_mut::<4>();
    for (sp, dp) in s_px.iter().zip(d_px.iter_mut()) {
        dp[0] = sp[2];
        dp[1] = sp[1];
        dp[2] = sp[0];
        dp[3] = sp[3];
    }
}

/// Copy packed BGRA row without channel re-ordering.
#[inline]
pub fn copy_bgra(src: &[u8], dst: &mut [u8], width: usize) {
    let row_bytes = width * 4;
    dst[..row_bytes].copy_from_slice(&src[..row_bytes]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzle_rgba_to_bgra_converts_correctly() {
        let mut src = vec![0u8; 32];
        for i in 0..8 {
            src[i * 4] = 10; // R
            src[i * 4 + 1] = 20; // G
            src[i * 4 + 2] = 30; // B
            src[i * 4 + 3] = 40; // A
        }
        let mut dst = vec![0u8; 32];
        swizzle_rgba_to_bgra(&src, &mut dst, 8);
        for i in 0..8 {
            assert_eq!(dst[i * 4], 30); // B
            assert_eq!(dst[i * 4 + 1], 20); // G
            assert_eq!(dst[i * 4 + 2], 10); // R
            assert_eq!(dst[i * 4 + 3], 40); // A
        }
    }
}
