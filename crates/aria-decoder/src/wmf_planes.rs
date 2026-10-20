//! Copying a decoded biplanar sample out of its Media Foundation buffer, and
//! the 100-ns tick conversion the reader's seek takes.
//!
//! Declared from `wmf.rs` with `#[path]`; `use super::*` reaches that module's
//! private items and its own imports, so the move changed no name resolution.

use super::*;

/// Copy a biplanar 4:2:0 sample (NV12 / P010) out of its media buffer.
///
/// Prefers [`IMF2DBuffer`] so we honor the decoder's real row pitch (which it
/// pads for alignment), and falls back to a flat `Lock` assuming an unpadded
/// `width * bytes-per-sample` stride.
pub(super) fn read_planes(
    buffer: &IMFMediaBuffer,
    format: PixelFormat,
    width: u32,
    height: u32,
) -> Result<Vec<Plane>, DecodeError> {
    if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer>() {
        let mut scanline0: *mut u8 = core::ptr::null_mut();
        let mut pitch: i32 = 0;
        unsafe { buffer_2d.Lock2D(&mut scanline0, &mut pitch) }.map_err(decode_err)?;
        let planes = (|| -> Result<Vec<Plane>, DecodeError> {
            if scanline0.is_null() || pitch <= 0 {
                return Err(DecodeError::Decode(
                    "2D buffer returned a null base or non-positive pitch".into(),
                ));
            }
            // SAFETY: between Lock2D/Unlock2D the buffer guarantees `pitch` bytes
            // per row across the full biplanar extent.
            unsafe { copy_biplanar(scanline0, pitch as usize, height, None) }
        })();
        unsafe { buffer_2d.Unlock2D() }.map_err(decode_err)?;
        return planes;
    }

    let mut base: *mut u8 = core::ptr::null_mut();
    let mut max_len: u32 = 0;
    let mut cur_len: u32 = 0;
    unsafe { buffer.Lock(&mut base, Some(&mut max_len), Some(&mut cur_len)) }
        .map_err(decode_err)?;
    let planes = (|| -> Result<Vec<Plane>, DecodeError> {
        if base.is_null() {
            return Err(DecodeError::Decode("locked buffer is null".into()));
        }
        let stride = width as usize * format.bytes_per_sample();
        // SAFETY: the contiguous buffer holds `cur_len` valid bytes; copy_biplanar
        // verifies the biplanar extent fits before reading.
        unsafe { copy_biplanar(base, stride, height, Some(cur_len as usize)) }
    })();
    unsafe { buffer.Unlock() }.map_err(decode_err)?;
    planes
}

/// Copy `height` luma rows plus `ceil(height/2)` interleaved-chroma rows, each
/// `stride` bytes, out of a biplanar buffer starting at `base`.
///
/// # Safety
/// `base` must point at a readable region of at least `stride * (height +
/// ceil(height/2))` bytes (the caller holds the buffer lock). When `available`
/// is `Some`, that extent is checked against it first.
unsafe fn copy_biplanar(
    base: *mut u8,
    stride: usize,
    height: u32,
    available: Option<usize>,
) -> Result<Vec<Plane>, DecodeError> {
    let y_rows = height as usize;
    let chroma_rows = (height as usize).div_ceil(2);
    let needed = stride
        .checked_mul(y_rows + chroma_rows)
        .ok_or_else(|| DecodeError::Decode("plane size overflow".into()))?;
    if let Some(available) = available
        && available < needed
    {
        return Err(DecodeError::Decode(format!(
            "buffer too small for {height}-row biplanar frame: {available} < {needed} bytes"
        )));
    }

    let luma = unsafe { slice::from_raw_parts(base, stride * y_rows) }.to_vec();
    let chroma =
        unsafe { slice::from_raw_parts(base.add(stride * y_rows), stride * chroma_rows) }.to_vec();
    Ok(vec![
        Plane::new(luma, stride, y_rows),
        Plane::new(chroma, stride, chroma_rows),
    ])
}
