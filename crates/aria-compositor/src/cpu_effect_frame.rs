//! The `RGBA16F` frames a [`CpuEffect`](crate::CpuEffect) reads and writes,
//! and the binary16 conversions between them and 8-bit channels.

/// IEEE-754 binary16 representation of `1.0`.
pub const F16_ONE: u16 = half::f16::ONE.to_bits();

/// Convert an IEEE-754 binary16 (`f16`) bit pattern to `f32`. Exact for every
/// non-NaN pattern; a NaN decodes to a quiet NaN of the same sign.
#[inline]
pub const fn f16_to_f32(bits: u16) -> f32 {
    half::f16::from_bits(bits).to_f32_const()
}

/// Convert an `f32` to an IEEE-754 binary16 (`f16`) bit pattern (round to
/// nearest, ties to even).
#[inline]
pub const fn f32_to_f16(value: f32) -> u16 {
    half::f16::from_f32_const(value).to_bits()
}

/// Precomputed `u8` (`0..=255`) to normalized `f16` (`0.0..=1.0`) table.
pub const U8_TO_F16: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        table[i] = f32_to_f16((i as f32) / 255.0);
        i += 1;
    }
    table
};

/// Convert a normalized `f16` channel (`0.0..=1.0`) to `u8` (`0..=255`).
#[inline]
pub fn f16_to_u8(bits: u16) -> u8 {
    let v = f16_to_f32(bits);
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= 1.0 {
        255
    } else {
        (v * 255.0 + 0.5) as u8
    }
}

/// The geometry both frame views share: `width`×`height` `RGBA16F` pixels,
/// `row_stride` `u16` elements per row.
#[derive(Debug, Clone, Copy)]
struct FrameShape {
    width: u32,
    height: u32,
    row_stride: usize,
}

impl FrameShape {
    /// Panics if `row_stride < width as usize * 4` or `len` is shorter than
    /// `height as usize * row_stride`.
    fn new(len: usize, width: u32, height: u32, row_stride: usize) -> Self {
        let row_elems = (width as usize) * 4;
        assert!(
            row_stride >= row_elems,
            "row_stride ({row_stride}) must be at least width * 4 ({row_elems})"
        );
        let needed = (height as usize) * row_stride;
        assert!(
            len >= needed,
            "data length ({len}) is shorter than height * row_stride ({needed})"
        );
        Self {
            width,
            height,
            row_stride,
        }
    }

    /// The `u16` index range of row `y`'s `width * 4` active channel values.
    #[inline]
    fn row(&self, y: u32) -> std::ops::Range<usize> {
        assert!(
            y < self.height,
            "row {y} out of bounds for height {}",
            self.height
        );
        let start = (y as usize) * self.row_stride;
        start..start + (self.width as usize) * 4
    }

    /// The offset of pixel `x`'s first channel within a row.
    #[inline]
    fn column(&self, x: u32) -> usize {
        assert!(
            x < self.width,
            "x {x} out of bounds for width {}",
            self.width
        );
        (x as usize) * 4
    }
}

/// Input frame handed to [`CpuEffect::process`](crate::CpuEffect::process): premultiplied `RGBA16F` pixels
/// stored as four IEEE-754 binary16 `u16` channels (`[R, G, B, A]`) per pixel.
#[derive(Debug, Clone, Copy)]
pub struct CpuFrameIn<'a> {
    data: &'a [u16],
    shape: FrameShape,
}

impl<'a> CpuFrameIn<'a> {
    /// Wrap `data` as a `width`×`height` `RGBA16F` input frame with
    /// `row_stride` `u16` elements per row.
    ///
    /// Panics if `row_stride < width as usize * 4` or `data` is shorter than
    /// `height as usize * row_stride`.
    pub fn new(data: &'a [u16], width: u32, height: u32, row_stride: usize) -> Self {
        let shape = FrameShape::new(data.len(), width, height, row_stride);
        Self { data, shape }
    }

    /// Frame width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.shape.width
    }

    /// Frame height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.shape.height
    }

    /// Row stride in `u16` elements (`>= width * 4`).
    #[inline]
    pub fn row_stride(&self) -> usize {
        self.shape.row_stride
    }

    /// Underlying `u16` slice containing `height * row_stride` elements.
    #[inline]
    pub fn data(&self) -> &'a [u16] {
        self.data
    }

    /// The `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row(&self, y: u32) -> &'a [u16] {
        &self.data[self.shape.row(y)]
    }

    /// Raw `RGBA16F` (`[u16; 4]`) pixel at `(x, y)`.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        let idx = self.shape.column(x);
        let row = self.row(y);
        [row[idx], row[idx + 1], row[idx + 2], row[idx + 3]]
    }

    /// Pixel at `(x, y)` converted from `RGBA16F` to `f32` `[R, G, B, A]`.
    #[inline]
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [f16_to_f32(r), f16_to_f32(g), f16_to_f32(b), f16_to_f32(a)]
    }
}

/// Output frame handed to [`CpuEffect::process`](crate::CpuEffect::process): premultiplied `RGBA16F`
/// pixels stored as four IEEE-754 binary16 `u16` channels (`[R, G, B, A]`) per
/// pixel, of the same dimensions as the input frame.
#[derive(Debug)]
pub struct CpuFrameOut<'a> {
    data: &'a mut [u16],
    shape: FrameShape,
}

impl<'a> CpuFrameOut<'a> {
    /// Wrap `data` as a mutable `width`×`height` `RGBA16F` output frame with
    /// `row_stride` `u16` elements per row.
    ///
    /// Panics if `row_stride < width as usize * 4` or `data` is shorter than
    /// `height as usize * row_stride`.
    pub fn new(data: &'a mut [u16], width: u32, height: u32, row_stride: usize) -> Self {
        let shape = FrameShape::new(data.len(), width, height, row_stride);
        Self { data, shape }
    }

    /// Frame width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.shape.width
    }

    /// Frame height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.shape.height
    }

    /// Row stride in `u16` elements (`>= width * 4`).
    #[inline]
    pub fn row_stride(&self) -> usize {
        self.shape.row_stride
    }

    /// Immutable view of the underlying `u16` slice.
    #[inline]
    pub fn data(&self) -> &[u16] {
        self.data
    }

    /// Mutable view of the underlying `u16` slice.
    #[inline]
    pub fn data_mut(&mut self) -> &mut [u16] {
        self.data
    }

    /// The `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row(&self, y: u32) -> &[u16] {
        &self.data[self.shape.row(y)]
    }

    /// Mutable slice of the `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [u16] {
        &mut self.data[self.shape.row(y)]
    }

    /// Raw `RGBA16F` (`[u16; 4]`) pixel at `(x, y)`.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        let idx = self.shape.column(x);
        let row = self.row(y);
        [row[idx], row[idx + 1], row[idx + 2], row[idx + 3]]
    }

    /// Pixel at `(x, y)` converted from `RGBA16F` to `f32` `[R, G, B, A]`.
    #[inline]
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [f16_to_f32(r), f16_to_f32(g), f16_to_f32(b), f16_to_f32(a)]
    }

    /// Write raw `RGBA16F` (`[u16; 4]`) channels to `(x, y)`.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u16; 4]) {
        let idx = self.shape.column(x);
        let row = self.row_mut(y);
        row[idx..idx + 4].copy_from_slice(&rgba);
    }

    /// Convert `rgba` (`f32` `[R, G, B, A]`) to `RGBA16F` and write to `(x, y)`.
    #[inline]
    pub fn set_pixel_f32(&mut self, x: u32, y: u32, rgba: [f32; 4]) {
        self.set_pixel(
            x,
            y,
            [
                f32_to_f16(rgba[0]),
                f32_to_f16(rgba[1]),
                f32_to_f16(rgba[2]),
                f32_to_f16(rgba[3]),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: CPU effects read and write `RGBA16F` converted from and to `u8`
    /// channel values. If any `0..=255` byte fails to round-trip through `f16`
    /// exactly, a passthrough CPU effect shifts colors on every frame.
    #[test]
    fn every_u8_channel_round_trips_through_f16_exactly() {
        for byte in 0u8..=255 {
            let half = U8_TO_F16[usize::from(byte)];
            let back = f16_to_u8(half);
            assert_eq!(back, byte, "byte {byte} became {back} via f16 {half:#06x}");
        }
        assert_eq!(f32_to_f16(1.0), F16_ONE);
        assert_eq!(f16_to_f32(F16_ONE), 1.0);
        assert_eq!(f32_to_f16(0.0), 0);
        assert_eq!(f16_to_f32(0), 0.0);
    }

    #[test]
    fn frame_views_respect_row_stride_and_pixel_accessors() {
        // 2x2 frame with a padded stride of 12 u16s per row (active width is 8 u16s).
        let mut storage = vec![0u16; 24];
        {
            let mut out = CpuFrameOut::new(&mut storage, 2, 2, 12);
            assert_eq!(out.width(), 2);
            assert_eq!(out.height(), 2);
            assert_eq!(out.row_stride(), 12);
            out.set_pixel_f32(1, 0, [0.25, 0.5, 0.75, 1.0]);
            out.set_pixel(0, 1, [U8_TO_F16[10], U8_TO_F16[20], U8_TO_F16[30], F16_ONE]);
        }
        let input = CpuFrameIn::new(&storage, 2, 2, 12);
        assert_eq!(input.row(0).len(), 8);
        assert_eq!(input.row(1).len(), 8);
        assert_eq!(input.pixel_f32(1, 0), [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(
            input.pixel(0, 1),
            [U8_TO_F16[10], U8_TO_F16[20], U8_TO_F16[30], F16_ONE]
        );
        // The padding between rows is never part of a row or a pixel.
        assert_eq!(&input.row(1)[..4], &storage[12..16]);
        assert!(storage[8..12].iter().all(|&c| c == 0));
    }

    /// WHY: both frame views take their bounds from one shape. Each case is a
    /// read or write a CPU effect can make with a bad index or a short buffer;
    /// each must panic with the bound it broke instead of reading the next
    /// row's pixels or the padding, or failing later on a bare slice index.
    #[test]
    fn frame_views_panic_on_every_out_of_bounds_access() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        fn panic_text(f: impl FnOnce()) -> String {
            let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("the access panics");
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default()
        }
        const STRIDE: &str = "row_stride (7) must be at least width * 4 (8)";
        const SHORT: &str = "data length (23) is shorter than height * row_stride (24)";
        const X: &str = "x 2 out of bounds for width 2";
        const ROW: &str = "row 2 out of bounds for height 2";
        let mut storage = vec![0u16; 24];
        assert_eq!(
            panic_text(|| {
                CpuFrameIn::new(&storage, 2, 2, 7);
            }),
            STRIDE
        );
        assert_eq!(
            panic_text(|| {
                CpuFrameIn::new(&storage[..23], 2, 2, 12);
            }),
            SHORT
        );
        let input = CpuFrameIn::new(&storage, 2, 2, 12);
        assert_eq!(
            panic_text(|| {
                input.pixel(2, 0);
            }),
            X
        );
        assert_eq!(
            panic_text(|| {
                input.pixel(0, 2);
            }),
            ROW
        );
        assert_eq!(
            panic_text(|| {
                input.row(2);
            }),
            ROW
        );
        assert_eq!(
            panic_text(|| {
                CpuFrameOut::new(&mut storage[..23], 2, 2, 12);
            }),
            SHORT
        );
        assert_eq!(
            panic_text(|| {
                CpuFrameOut::new(&mut storage, 2, 2, 7);
            }),
            STRIDE
        );
        let mut out = CpuFrameOut::new(&mut storage, 2, 2, 12);
        assert_eq!(panic_text(|| out.set_pixel(2, 0, [0; 4])), X);
        assert_eq!(panic_text(|| out.set_pixel(0, 2, [0; 4])), ROW);
        assert_eq!(
            panic_text(|| {
                out.row_mut(2);
            }),
            ROW
        );
        out.set_pixel(1, 1, [1, 2, 3, 4]);
        assert_eq!(out.pixel(1, 1), [1, 2, 3, 4]);
    }
}
