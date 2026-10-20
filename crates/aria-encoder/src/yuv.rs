//! CPU RGBA / BGRA → I420 / NV12 (YUV 4:2:0) conversion.
//!
//! The renderer hands the export loop packed RGBA, but hardware video encoders
//! take YUV. Apple's VideoToolbox accepts BGRA directly (see the Apple
//! backend); MediaCodec and Media Foundation do not, so the Android backend
//! converts to I420 and the Windows and Linux backends to NV12 (same 4:2:0
//! samples, interleaved UV) here first. Kept platform-independent so it's
//! unit-testable on the host.
//!
//! Limited-range, with the matrix chosen by the caller ([`YuvMatrix`]): the
//! encoder must convert with the same coefficients decoders will use to
//! convert back, or every round-trip shifts colors. Chroma is 2×2
//! box-averaged.
//!
//! Both entry points write into a caller-owned buffer and walk the source by
//! row slices rather than by pixel index: at 1080p60 an owned return value is
//! a 3 MB allocation and free sixty times a second, and a per-pixel index is
//! two bounds checks per channel. [`ChannelOrder`] folds the BGRA channel swap
//! into the same pass, so a BGRA export no longer copies the whole frame into
//! a re-ordered scratch buffer first.

use aria_core::EncodeError;

/// RGB↔YUV conversion matrix (limited range). Encoder backends derive it
/// from the config's color tag ([`matrix_for`]) so the coefficients used to
/// build the stream match what it's tagged as — and match the **BT.709**
/// fallback decoders (including this workspace's own decode path) apply to
/// HD H.264 that doesn't signal otherwise. Mismatched matrices shift every
/// round-tripped color visibly (blues drift teal, skin drifts orange).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YuvMatrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// Fixed-point (`/256`) limited-range coefficients for one [`YuvMatrix`].
///
/// `Y = ((luma · rgb) + 128) >> 8) + 16`; `U`/`V` add a 128 offset instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct YuvCoefficients {
    /// Luma row `[R, G, B]`.
    pub luma: [i32; 3],
    /// `U` (Cb) row. Sums to zero so grays stay chroma-neutral exactly.
    pub u: [i32; 3],
    /// `V` (Cr) row. Sums to zero.
    pub v: [i32; 3],
}

/// One row per [`YuvMatrix`], in [`YuvMatrix::ALL`] order.
///
/// A table indexed once per frame rather than a `match` per call, so the
/// conversion loops read the coefficients out of registers instead of
/// re-deriving them.
const COEFFICIENTS: [YuvCoefficients; YuvMatrix::ALL.len()] = [
    // Y = 0.257R + 0.504G + 0.098B (+16);
    // U = -0.148R - 0.291G + 0.439B; V = 0.439R - 0.368G - 0.071B.
    YuvCoefficients {
        luma: [66, 129, 25],
        u: [-38, -74, 112],
        v: [112, -94, -18],
    },
    // Y = 0.183R + 0.614G + 0.062B (+16);
    // U = -0.115R - 0.385G + 0.500B; V = 0.500R - 0.454G - 0.046B
    // (scaled by 224/255).
    YuvCoefficients {
        luma: [47, 157, 16],
        u: [-26, -87, 113],
        v: [112, -102, -10],
    },
    // BT.2020 non-constant-luminance, limited range.
    YuvCoefficients {
        luma: [58, 149, 13],
        u: [-31, -81, 112],
        v: [112, -103, -9],
    },
];

impl YuvMatrix {
    /// Every matrix this converter can apply, in [`COEFFICIENTS`] row order.
    pub(crate) const ALL: [YuvMatrix; 3] = [YuvMatrix::Bt601, YuvMatrix::Bt709, YuvMatrix::Bt2020];

    /// Row of [`COEFFICIENTS`] this matrix uses. The match is exhaustive, so a
    /// new matrix variant cannot compile until it has a row.
    const fn index(self) -> usize {
        match self {
            YuvMatrix::Bt601 => 0,
            YuvMatrix::Bt709 => 1,
            YuvMatrix::Bt2020 => 2,
        }
    }

    /// This matrix's fixed-point coefficients.
    pub(crate) const fn coefficients(self) -> YuvCoefficients {
        COEFFICIENTS[self.index()]
    }
}

/// Channel order of the packed 32-bit source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelOrder {
    /// `R, G, B, A` — what the compositor reads back.
    Rgba,
    /// `B, G, R, A` — what some platform surfaces hand over.
    Bgra,
}

/// The conversion matrix for an [`EncoderConfig`](aria_core::EncoderConfig)
/// color tag. Explicit BT.601 and BT.2020 requests retain their matrices.
/// BT.709, identity, and unspecified tags use the SDR BT.709 conversion.
#[cfg_attr(
    not(any(target_os = "windows", target_os = "android", target_os = "linux")),
    allow(dead_code)
)]
pub(crate) fn matrix_for(color: &aria_core::ColorSpace) -> YuvMatrix {
    match color.matrix {
        aria_core::MatrixCoefficients::Bt601 => YuvMatrix::Bt601,
        aria_core::MatrixCoefficients::Bt2020Ncl => YuvMatrix::Bt2020,
        aria_core::MatrixCoefficients::Bt709
        | aria_core::MatrixCoefficients::Identity
        | aria_core::MatrixCoefficients::Unspecified => YuvMatrix::Bt709,
    }
}

/// Bytes one 4:2:0 frame occupies: `w*h + 2*(w/2)*(h/2)`, with `width` and
/// `height` rounded down to even for the chroma grid. I420 and NV12 are the
/// same size — only the chroma layout differs.
#[cfg_attr(
    not(any(target_os = "windows", target_os = "android", target_os = "linux")),
    allow(dead_code)
)]
pub(crate) const fn yuv420_len(width: u32, height: u32) -> usize {
    let w = (width & !1) as usize;
    let h = (height & !1) as usize;
    w * h + 2 * (w / 2) * (h / 2)
}

/// Convert a packed 32-bit image to a tightly-packed I420 frame in `out`
/// (`Y` plane, then `U`, then `V`), using `matrix` limited-range coefficients.
///
/// `stride` is the source row length in bytes (≥ `width * 4`); `out` must hold
/// at least [`yuv420_len`] bytes and is written in full, so a caller can keep
/// one buffer for the whole export.
// The Android encoder is the only non-test caller; allow dead code on hosts
// where that backend is cfg'd out (the unit tests below still exercise it).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn rgba_to_i420_into(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    matrix: YuvMatrix,
    order: ChannelOrder,
    out: &mut [u8],
) -> Result<(), EncodeError> {
    let plan = Plan::new(width, height, stride, src.len(), out.len())?;
    let src = &src[..plan.src_len()];
    let (y_plane, chroma) = out[..plan.frame_len()].split_at_mut(plan.y_len());
    let (u_plane, v_plane) = chroma.split_at_mut(plan.c_len());
    let coeffs = matrix.coefficients();
    match order {
        ChannelOrder::Rgba => write_i420::<false>(src, y_plane, u_plane, v_plane, &plan, &coeffs),
        ChannelOrder::Bgra => write_i420::<true>(src, y_plane, u_plane, v_plane, &plan, &coeffs),
    }
    Ok(())
}

/// Convert a packed 32-bit image to a tightly-packed NV12 frame in `out`
/// (`Y` plane, then interleaved `UV`) — the input format hardware H.264
/// encoder MFTs accept universally on Windows, and what `x264enc` takes on
/// Linux.
///
/// Same sampling, coefficients, and buffer contract as
/// [`rgba_to_i420_into`]; only the chroma layout differs.
///
/// A frame of more than [`BAND_PIXELS`] pixels is converted in horizontal
/// bands, one per core up to [`MAX_BANDS`], on scoped threads: on one core a
/// 1080p frame took 3.2 ms, most of the 4.2 ms an export of 60 fps video has
/// per frame at four times real time.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
pub(crate) fn rgba_to_nv12_into(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    matrix: YuvMatrix,
    order: ChannelOrder,
    out: &mut [u8],
) -> Result<(), EncodeError> {
    let plan = Plan::new(width, height, stride, src.len(), out.len())?;
    let src = &src[..plan.src_len()];
    let (y_plane, uv_plane) = out[..plan.frame_len()].split_at_mut(plan.y_len());
    let coeffs = matrix.coefficients();
    // Each band is a whole number of row pairs, so the chroma rows it writes
    // read only its own source rows.
    let pairs = plan.ch.div_ceil(bands(&plan));
    let convert = |((src, y), uv): ((&[u8], &mut [u8]), &mut [u8])| match order {
        ChannelOrder::Rgba => write_nv12::<false>(src, y, uv, &plan, &coeffs),
        ChannelOrder::Bgra => write_nv12::<true>(src, y, uv, &plan, &coeffs),
    };
    let mut parts = src
        .chunks(pairs * 2 * plan.stride)
        .zip(y_plane.chunks_mut(pairs * 2 * plan.w))
        .zip(uv_plane.chunks_mut(pairs * 2 * plan.cw));
    let first = parts.next();
    let convert = &convert;
    std::thread::scope(|scope| {
        for part in parts {
            scope.spawn(move || convert(part));
        }
        if let Some(first) = first {
            convert(first);
        }
    });
    Ok(())
}

/// Pixels below which a band is not worth a thread of its own.
const BAND_PIXELS: usize = 1 << 18;

/// The most bands a frame is split into.
const MAX_BANDS: usize = 8;

/// How many bands `plan` is converted in.
fn bands(plan: &Plan) -> usize {
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cores = *CORES.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    (plan.y_len() / BAND_PIXELS)
        .clamp(1, cores.clamp(1, MAX_BANDS))
        .min(plan.ch)
}

/// Validated geometry of one conversion: even-rounded picture size, chroma
/// size, and the source row stride. Every length check happens here so the
/// writers below can walk whole rows with no per-pixel test.
struct Plan {
    w: usize,
    h: usize,
    cw: usize,
    ch: usize,
    stride: usize,
}

impl Plan {
    fn new(
        width: u32,
        height: u32,
        stride: usize,
        src_len: usize,
        out_len: usize,
    ) -> Result<Self, EncodeError> {
        let w = (width & !1) as usize;
        let h = (height & !1) as usize;
        if w == 0 || h == 0 {
            return Err(EncodeError::Encode(format!(
                "a {width}x{height} frame has no 4:2:0 picture to convert"
            )));
        }
        if stride < w * 4 {
            return Err(EncodeError::Encode(format!(
                "row stride {stride} is shorter than {w} packed pixels"
            )));
        }
        let src_need = stride
            .checked_mul(h)
            .ok_or_else(|| EncodeError::Encode("source frame size overflows".into()))?;
        if src_len < src_need {
            return Err(EncodeError::Encode(format!(
                "source frame is {src_len} bytes, needs stride {stride} × {h} rows"
            )));
        }
        let out_need = yuv420_len(width, height);
        if out_len < out_need {
            return Err(EncodeError::Encode(format!(
                "destination is {out_len} bytes, needs {out_need} for {w}x{h} 4:2:0"
            )));
        }
        Ok(Self {
            w,
            h,
            cw: w / 2,
            ch: h / 2,
            stride,
        })
    }

    /// Source bytes the picture occupies (padding past the last row excluded).
    fn src_len(&self) -> usize {
        self.stride * self.h
    }

    fn y_len(&self) -> usize {
        self.w * self.h
    }

    fn c_len(&self) -> usize {
        self.cw * self.ch
    }

    fn frame_len(&self) -> usize {
        self.y_len() + 2 * self.c_len()
    }
}

/// `(R, G, B)` of one source pixel as i32. `SWAP` is a const generic so the
/// BGRA channel order costs nothing at run time.
#[inline(always)]
fn rgb<const SWAP: bool>(px: &[u8; 4]) -> (i32, i32, i32) {
    let (r, b) = if SWAP { (px[2], px[0]) } else { (px[0], px[2]) };
    (i32::from(r), i32::from(px[1]), i32::from(b))
}

/// The row's pixels grouped in horizontal pairs — one chroma sample's worth of
/// columns. `as_chunks` yields fixed-size arrays, so the inner loop indexes
/// arrays instead of slices and needs no bounds check.
#[inline(always)]
fn pixel_pairs(row: &[u8]) -> &[[[u8; 4]; 2]] {
    let (pixels, _) = row.as_chunks::<4>();
    let (pairs, _) = pixels.as_chunks::<2>();
    pairs
}

#[inline(always)]
fn luma_sample(r: i32, g: i32, b: i32, [cr, cg, cb]: [i32; 3]) -> u8 {
    let luma = (cr * r + cg * g + cb * b + 128) >> 8;
    (luma + 16).clamp(0, 255) as u8
}

/// Four luma samples (`top`, `bottom`) and one `(U, V)` chroma pair from the
/// 2×2 pixel block straddling two source rows, unpacking each source pixel once.
#[inline(always)]
fn block_420<const SWAP: bool>(
    top: &[[u8; 4]; 2],
    bottom: &[[u8; 4]; 2],
    coeffs: &YuvCoefficients,
) -> ([u8; 2], [u8; 2], u8, u8) {
    let (r00, g00, b00) = rgb::<SWAP>(&top[0]);
    let (r01, g01, b01) = rgb::<SWAP>(&top[1]);
    let (r10, g10, b10) = rgb::<SWAP>(&bottom[0]);
    let (r11, g11, b11) = rgb::<SWAP>(&bottom[1]);

    let y_top = [
        luma_sample(r00, g00, b00, coeffs.luma),
        luma_sample(r01, g01, b01, coeffs.luma),
    ];
    let y_bottom = [
        luma_sample(r10, g10, b10, coeffs.luma),
        luma_sample(r11, g11, b11, coeffs.luma),
    ];

    let r = (r00 + r01 + r10 + r11) >> 2;
    let g = (g00 + g01 + g10 + g11) >> 2;
    let b = (b00 + b01 + b10 + b11) >> 2;
    let [ur, ug, ub] = coeffs.u;
    let [vr, vg, vb] = coeffs.v;
    let u = (((ur * r + ug * g + ub * b + 128) >> 8) + 128).clamp(0, 255) as u8;
    let v = (((vr * r + vg * g + vb * b + 128) >> 8) + 128).clamp(0, 255) as u8;
    (y_top, y_bottom, u, v)
}

fn write_i420<const SWAP: bool>(
    src: &[u8],
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    plan: &Plan,
    coeffs: &YuvCoefficients,
) {
    for (((y_rows, u_row), v_row), rows) in y_plane
        .chunks_exact_mut(plan.w * 2)
        .zip(u_plane.chunks_exact_mut(plan.cw))
        .zip(v_plane.chunks_exact_mut(plan.cw))
        .zip(src.chunks_exact(plan.stride * 2))
    {
        let (y_top, y_bottom) = y_rows.split_at_mut(plan.w);
        let (y_top_pairs, _) = y_top.as_chunks_mut::<2>();
        let (y_bottom_pairs, _) = y_bottom.as_chunks_mut::<2>();
        let (top, bottom) = rows.split_at(plan.stride);
        for (((((yt, yb), u), v), t), b) in y_top_pairs
            .iter_mut()
            .zip(y_bottom_pairs.iter_mut())
            .zip(u_row.iter_mut())
            .zip(v_row.iter_mut())
            .zip(pixel_pairs(top))
            .zip(pixel_pairs(bottom))
        {
            let (luma_t, luma_b, cu, cv) = block_420::<SWAP>(t, b, coeffs);
            *yt = luma_t;
            *yb = luma_b;
            *u = cu;
            *v = cv;
        }
    }
}

fn write_nv12<const SWAP: bool>(
    src: &[u8],
    y_plane: &mut [u8],
    uv_plane: &mut [u8],
    plan: &Plan,
    coeffs: &YuvCoefficients,
) {
    for ((y_rows, uv_row), rows) in y_plane
        .chunks_exact_mut(plan.w * 2)
        .zip(uv_plane.chunks_exact_mut(plan.cw * 2))
        .zip(src.chunks_exact(plan.stride * 2))
    {
        let (y_top, y_bottom) = y_rows.split_at_mut(plan.w);
        let (y_top_pairs, _) = y_top.as_chunks_mut::<2>();
        let (y_bottom_pairs, _) = y_bottom.as_chunks_mut::<2>();
        let (top, bottom) = rows.split_at(plan.stride);
        let (samples, _) = uv_row.as_chunks_mut::<2>();
        for ((((yt, yb), sample), t), b) in y_top_pairs
            .iter_mut()
            .zip(y_bottom_pairs.iter_mut())
            .zip(samples.iter_mut())
            .zip(pixel_pairs(top))
            .zip(pixel_pairs(bottom))
        {
            let (luma_t, luma_b, u, v) = block_420::<SWAP>(t, b, coeffs);
            *yt = luma_t;
            *yb = luma_b;
            *sample = [u, v];
        }
    }
}

#[cfg(test)]
#[path = "yuv_tests.rs"]
mod tests;
