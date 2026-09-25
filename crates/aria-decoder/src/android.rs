//! Android video decoding via the NDK **MediaExtractor + MediaCodec** pipeline.
//!
//! Unlike the Apple/Windows backends — where one object does demux *and* decode —
//! Android splits the job: [`AMediaExtractor`](ndk_sys::AMediaExtractor) demuxes
//! the container and [`AMediaCodec`](ndk_sys::AMediaCodec) runs the hardware
//! decoder. MediaCodec is also **buffered and asynchronous**: you feed input
//! buffers and drain output buffers on separate cadences. We hide that behind the
//! synchronous [`aria_core::VideoDecoder`] pull by running a feed-input /
//! drain-output *pump* inside [`MediaCodecDecoder::next_frame`] until one frame
//! pops out (or the stream ends).
//!
//! ## What this backend does today
//!
//! - Demuxes with `AMediaExtractor`, picks the first `video/*` track, and spins
//!   up a `MediaCodec` decoder for its MIME type in **ByteBuffer mode** (no output
//!   surface), so decoded frames land in system memory.
//! - Reads each output buffer stride/slice-height-aware into [`FrameData::Cpu`],
//!   interpreting the buffer per the codec's reported `COLOR_FORMAT`
//!   (semi-planar → NV12, planar → I420).
//! - Probes size / rotation / frame-rate / colorimetry from the track format,
//!   refining size/stride/color from the codec's output format once it settles.
//! - Seeks with `AMediaExtractor_seekTo(PREVIOUS_SYNC)` + `AMediaCodec_flush`,
//!   and answers [`VideoDecoder::frame_at`] with the shared roll-forward policy
//!   ([`crate::seek`]) so sequential playback / forward scrubbing never pays a
//!   seek-plus-GOP-re-decode per frame.
//!
//! ## What it does not do
//!
//! There is **no GPU zero-copy path**. The proper one configures the codec with
//! an `ANativeWindow` from an `AImageReader`, then pulls `AHardwareBuffer`s
//! ([`aria_core::GpuSurfaceKind::AHardwareBuffer`]) for the renderer to import.
//! That needs an Android device/GPU to validate. Until it exists,
//! [`crate::decoder_capability`] reports no GPU surfaces and
//! [`MediaCodecDecoder::open`] refuses [`OutputMode::Gpu`] up front rather than
//! failing part way through a decode.
//!
//! ## Time base
//!
//! MediaExtractor/MediaCodec timestamps are **microseconds**, so every frame's
//! PTS rate — and [`SourceInfo::time_base`] — is [`US_PER_SEC`].

use core::ffi::c_char;
use core::ptr;
use core::slice;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use ndk_sys::{
    AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG, AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
    AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED, AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED,
    AMEDIAFORMAT_KEY_CHANNEL_COUNT, AMEDIAFORMAT_KEY_COLOR_FORMAT, AMEDIAFORMAT_KEY_COLOR_RANGE,
    AMEDIAFORMAT_KEY_COLOR_STANDARD, AMEDIAFORMAT_KEY_COLOR_TRANSFER, AMEDIAFORMAT_KEY_DURATION,
    AMEDIAFORMAT_KEY_FRAME_RATE, AMEDIAFORMAT_KEY_HEIGHT, AMEDIAFORMAT_KEY_MIME,
    AMEDIAFORMAT_KEY_PCM_ENCODING, AMEDIAFORMAT_KEY_ROTATION, AMEDIAFORMAT_KEY_SAMPLE_RATE,
    AMEDIAFORMAT_KEY_SLICE_HEIGHT, AMEDIAFORMAT_KEY_STRIDE, AMEDIAFORMAT_KEY_WIDTH, AMediaCodec,
    AMediaCodec_configure, AMediaCodec_createDecoderByType, AMediaCodec_delete,
    AMediaCodec_dequeueInputBuffer, AMediaCodec_dequeueOutputBuffer, AMediaCodec_flush,
    AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer, AMediaCodec_getOutputFormat,
    AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer, AMediaCodec_start,
    AMediaCodec_stop, AMediaCodecBufferInfo, AMediaExtractor, AMediaExtractor_advance,
    AMediaExtractor_delete, AMediaExtractor_getSampleTime, AMediaExtractor_getTrackCount,
    AMediaExtractor_getTrackFormat, AMediaExtractor_new, AMediaExtractor_readSampleData,
    AMediaExtractor_seekTo, AMediaExtractor_selectTrack, AMediaExtractor_setDataSourceFd,
    AMediaFormat, AMediaFormat_delete, AMediaFormat_getInt32, AMediaFormat_getInt64,
    AMediaFormat_getString, SeekMode, media_status_t,
};

use aria_core::{
    AudioReader, ColorPrimaries, ColorRange, ColorSpace, CpuImage, DecodeError, FrameData,
    MatrixCoefficients, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation, SourceInfo,
    TransferFunction, VideoDecoder, VideoFrame,
};

use crate::OutputMode;

#[path = "android_audio.rs"]
mod android_audio;
#[path = "android_video.rs"]
mod android_video;

/// `crate::lib` re-exports both by these paths; the split is internal.
pub use android_audio::MediaCodecAudioReader;
pub use android_video::MediaCodecDecoder;

/// MediaExtractor/MediaCodec timestamp tick rate: microseconds per second.
const US_PER_SEC: i64 = 1_000_000;

/// Safety net on the feed/drain pump so a stuck codec can't hang the worker.
const MAX_PUMP_ITERS: usize = 512;

/// Don't block waiting for a free input buffer — drain output instead.
const INPUT_TIMEOUT_US: i64 = 0;
/// Give the decoder a short window to surface an output buffer each iteration.
const OUTPUT_TIMEOUT_US: i64 = 10_000;

// `android.media.MediaFormat` COLOR_STANDARD_* values (KEY_COLOR_STANDARD).
const COLOR_STANDARD_BT709: i32 = 1;
const COLOR_STANDARD_BT601_PAL: i32 = 2;
const COLOR_STANDARD_BT601_NTSC: i32 = 4;
const COLOR_STANDARD_BT2020: i32 = 6;
// COLOR_TRANSFER_* values (KEY_COLOR_TRANSFER).
const COLOR_TRANSFER_SDR_VIDEO: i32 = 3;
const COLOR_TRANSFER_ST2084: i32 = 6;
const COLOR_TRANSFER_HLG: i32 = 7;
// COLOR_RANGE_* values (KEY_COLOR_RANGE).
const COLOR_RANGE_FULL: i32 = 1;
// `MediaCodecInfo.CodecCapabilities` COLOR_Format* values (KEY_COLOR_FORMAT).
const COLOR_FORMAT_YUV420_PLANAR: i32 = 19;
const COLOR_FORMAT_YUV420_PACKED_PLANAR: i32 = 20;
const COLOR_FORMAT_YUV420_SEMIPLANAR: i32 = 21;
const COLOR_FORMAT_YUV420_PACKED_SEMIPLANAR: i32 = 39;

/// Point `extractor` at `path` via a file descriptor.
///
/// `AMediaExtractor_setDataSource(path)` parses its argument as a URL and is
/// unreliable for bare filesystem paths on many devices (it fails with no
/// detail, as seen on app-private `cache/` files). Opening the file ourselves
/// and handing MediaExtractor the fd via `AMediaExtractor_setDataSourceFd` is
/// the robust path for local files. The returned [`File`] owns the fd and must
/// outlive the extractor.
fn set_data_source_fd(extractor: *mut AMediaExtractor, path: &Path) -> Result<File, DecodeError> {
    let file = File::open(path)
        .map_err(|e| DecodeError::Open(format!("open {} failed: {e}", path.display())))?;
    let len = file
        .metadata()
        .map(|m| m.len() as i64)
        .map_err(|e| DecodeError::Open(format!("stat {} failed: {e}", path.display())))?;
    let fd = file.as_raw_fd();
    // SAFETY: `extractor` is a non-null, valid `AMediaExtractor` handle allocated via
    // `AMediaExtractor_new()`. `fd` is a valid open file descriptor owned by `file`
    // (which outlives this call and is returned to the caller), with offset 0 and length `len`.
    let status = unsafe { AMediaExtractor_setDataSourceFd(extractor, fd, 0, len) };
    if !media_ok(status) {
        return Err(DecodeError::Open(format!(
            "AMediaExtractor_setDataSourceFd failed for {}",
            path.display()
        )));
    }
    Ok(file)
}

/// Cleans up the half-built native objects if `open` bails partway through.
/// On success `open` `mem::forget`s the guard and hands the pointers to the
/// [`MediaCodecDecoder`], whose own `Drop` then owns teardown.
struct PartialCleanup {
    extractor: *mut AMediaExtractor,
    codec: *mut AMediaCodec,
    format: *mut AMediaFormat,
}

impl Drop for PartialCleanup {
    fn drop(&mut self) {
        if !self.codec.is_null() {
            // SAFETY: `self.codec` is a non-null, owned `AMediaCodec` handle. `AMediaCodec_stop`
            // transitions it out of executing state so `AMediaCodec_delete` can safely release it.
            unsafe {
                AMediaCodec_stop(self.codec);
                AMediaCodec_delete(self.codec);
            }
        }
        if !self.format.is_null() {
            // SAFETY: `self.format` is a non-null, owned `AMediaFormat` handle allocated by NDK
            // (`AMediaExtractor_getTrackFormat` / `AMediaCodec_getOutputFormat`) and not yet deleted.
            unsafe {
                AMediaFormat_delete(self.format);
            }
        }
        if !self.extractor.is_null() {
            // SAFETY: `self.extractor` is a non-null, owned `AMediaExtractor` handle allocated via
            // `AMediaExtractor_new()` and not yet deleted.
            unsafe {
                AMediaExtractor_delete(self.extractor);
            }
        }
    }
}

/// Whether the file at `path` carries at least one `audio/*` track. Used by the
/// probe to set [`crate::MediaProbe::has_audio`]; `false` on any error.
pub(crate) fn has_audio_track(path: &Path) -> bool {
    // SAFETY: `AMediaExtractor_new()` allocates a new native extractor instance or returns null on OOM.
    let extractor = unsafe { AMediaExtractor_new() };
    if extractor.is_null() {
        return false;
    }
    // `_source` keeps the fd open while we read track formats below.
    let mut found = false;
    if let Ok(_source) = set_data_source_fd(extractor, path) {
        // SAFETY: `extractor` is non-null and configured with a data source via `set_data_source_fd`.
        let count = unsafe { AMediaExtractor_getTrackCount(extractor) };
        for index in 0..count {
            // SAFETY: `extractor` is initialized with a data source and `index` is within `0..count`.
            let format = unsafe { AMediaExtractor_getTrackFormat(extractor, index) };
            if format.is_null() {
                continue;
            }
            // SAFETY: `format` is a non-null `AMediaFormat` pointer returned by `getTrackFormat`.
            // `AMEDIAFORMAT_KEY_MIME` is an NDK-provided static null-terminated C string constant.
            let is_audio = unsafe { format_string(format, AMEDIAFORMAT_KEY_MIME) }
                .is_some_and(|mime| mime.starts_with("audio/"));
            // SAFETY: `format` was allocated by `AMediaExtractor_getTrackFormat` and must be
            // deleted to release native format memory.
            unsafe { AMediaFormat_delete(format) };
            if is_audio {
                found = true;
                break;
            }
        }
    }
    // SAFETY: `extractor` is the non-null `AMediaExtractor` handle allocated above and not yet deleted.
    unsafe { AMediaExtractor_delete(extractor) };
    found
}

/// Scan the extractor's tracks and return the first `video/*` one: its index,
/// its (caller-owned) format, and its MIME type.
fn find_video_track(
    extractor: *mut AMediaExtractor,
) -> Result<(usize, *mut AMediaFormat, String), DecodeError> {
    // SAFETY: `extractor` is a non-null, valid `AMediaExtractor` pointer with an active data source.
    let track_count = unsafe { AMediaExtractor_getTrackCount(extractor) };
    for index in 0..track_count {
        // SAFETY: `extractor` is initialized with a data source and `index` is within `0..track_count`.
        let format = unsafe { AMediaExtractor_getTrackFormat(extractor, index) };
        if format.is_null() {
            continue;
        }
        // SAFETY: `format` is a non-null `AMediaFormat` pointer returned by `getTrackFormat`;
        // `AMEDIAFORMAT_KEY_MIME` is a static null-terminated C string constant.
        let mime = unsafe { format_string(format, AMEDIAFORMAT_KEY_MIME) };
        match mime {
            Some(mime) if mime.starts_with("video/") => return Ok((index, format, mime)),
            // SAFETY: `format` is non-null and owned; we delete non-video track formats to avoid leaks.
            _ => unsafe {
                AMediaFormat_delete(format);
            },
        }
    }
    Err(DecodeError::Unsupported("source has no video track".into()))
}

/// Probe static properties from the container track format at open time. Size,
/// stride, and exact color are refined later from the codec's output format.
///
/// # Safety
/// `format` must be a non-null, valid `AMediaFormat` pointer that has not been deleted.
unsafe fn probe_container(format: *mut AMediaFormat) -> Result<SourceInfo, DecodeError> {
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_WIDTH` is an NDK static C string.
    let width = unsafe { format_i32(format, AMEDIAFORMAT_KEY_WIDTH) }
        .filter(|w| *w > 0)
        .ok_or_else(|| DecodeError::Open("track format has no width".into()))?
        as u32;
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_HEIGHT` is an NDK static C string.
    let height = unsafe { format_i32(format, AMEDIAFORMAT_KEY_HEIGHT) }
        .filter(|h| *h > 0)
        .ok_or_else(|| DecodeError::Open("track format has no height".into()))?
        as u32;

    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_ROTATION` is an NDK static C string.
    let rotation = unsafe { format_i32(format, AMEDIAFORMAT_KEY_ROTATION) }
        .and_then(Rotation::from_degrees)
        .unwrap_or(Rotation::None);

    // FRAME_RATE is usually an int; some containers store a float (which the
    // int getter rejects), so fall back to 30 fps.
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_FRAME_RATE` is an NDK static C string.
    let frame_rate = unsafe { format_i32(format, AMEDIAFORMAT_KEY_FRAME_RATE) }
        .filter(|r| *r > 0)
        .map(|r| Rational::new(r, 1))
        .unwrap_or(Rational::FPS_30);

    let time_base = Rational::new(US_PER_SEC as i32, 1);
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_DURATION` is an NDK static C string.
    let duration = unsafe { format_i64(format, AMEDIAFORMAT_KEY_DURATION) }
        .filter(|d| *d > 0)
        .map(|us| RationalTime::new(us, time_base));

    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_COLOR_FORMAT` is an NDK static C string.
    let pixel_format = match unsafe { format_i32(format, AMEDIAFORMAT_KEY_COLOR_FORMAT) } {
        Some(value) => color_format_to_pixel(value).ok_or_else(|| {
            DecodeError::unsupported(format!("MediaCodec color format {value:#x}"))
        })?,
        None => PixelFormat::Nv12,
    };

    // SAFETY: `format` is a valid non-null AMediaFormat pointer per caller contract.
    let color = unsafe { color_from_format(format) };

    Ok(SourceInfo {
        coded_size: (width, height),
        display_size: (width, height),
        rotation,
        pixel_format,
        color,
        frame_rate,
        time_base,
        duration,
    })
}

/// Copy a biplanar (NV12/P010) or triplanar (I420) 4:2:0 frame out of a
/// MediaCodec ByteBuffer, honoring the codec's `stride` and `slice_height`.
///
/// # Safety
/// `base` must point to at least `available` readable, initialized bytes for the duration
/// of this call. The memory must not be concurrently mutated or deallocated.
unsafe fn read_planes(
    base: *const u8,
    available: usize,
    format: PixelFormat,
    stride: usize,
    slice_height: usize,
) -> Result<Vec<Plane>, DecodeError> {
    let y_rows = slice_height;
    let chroma_rows = slice_height.div_ceil(2);
    let overflow = || DecodeError::Decode("plane size overflow".into());

    match format {
        // Biplanar: full-res Y, then interleaved chroma at the same pitch.
        PixelFormat::Nv12 | PixelFormat::P010 => {
            let y_len = stride.checked_mul(y_rows).ok_or_else(overflow)?;
            let chroma_len = stride.checked_mul(chroma_rows).ok_or_else(overflow)?;
            let needed = y_len.checked_add(chroma_len).ok_or_else(overflow)?;
            if available < needed {
                return Err(DecodeError::Decode(format!(
                    "output buffer too small: {available} < {needed} bytes"
                )));
            }
            // SAFETY: `base` points to `available >= needed >= y_len` readable bytes.
            let luma = unsafe { slice::from_raw_parts(base, y_len) }.to_vec();
            // SAFETY: `base` points to `available` bytes; `y_len + chroma_len <= available`,
            // so `base.add(y_len)` is in-bounds and points to `chroma_len` readable bytes.
            let chroma = unsafe { slice::from_raw_parts(base.add(y_len), chroma_len) }.to_vec();
            Ok(vec![
                Plane::new(luma, stride, y_rows),
                Plane::new(chroma, stride, chroma_rows),
            ])
        }
        // Triplanar I420: Y at full pitch, then U and V at half pitch.
        PixelFormat::Yuv420p => {
            let chroma_stride = stride / 2;
            let y_len = stride.checked_mul(y_rows).ok_or_else(overflow)?;
            let chroma_len = chroma_stride
                .checked_mul(chroma_rows)
                .ok_or_else(overflow)?;
            let total_chroma = chroma_len.checked_mul(2).ok_or_else(overflow)?;
            let needed = y_len.checked_add(total_chroma).ok_or_else(overflow)?;
            if available < needed {
                return Err(DecodeError::Decode(format!(
                    "output buffer too small: {available} < {needed} bytes"
                )));
            }
            // SAFETY: `base` points to `available >= needed >= y_len` readable bytes.
            let luma = unsafe { slice::from_raw_parts(base, y_len) }.to_vec();
            // SAFETY: `y_len + chroma_len <= needed <= available`, so `base.add(y_len)` is
            // in-bounds and points to `chroma_len` readable bytes for the U plane.
            let u = unsafe { slice::from_raw_parts(base.add(y_len), chroma_len) }.to_vec();
            // SAFETY: `y_len + 2 * chroma_len == needed <= available`, so `base.add(y_len + chroma_len)`
            // is in-bounds and points to `chroma_len` readable bytes for the V plane.
            let v =
                unsafe { slice::from_raw_parts(base.add(y_len + chroma_len), chroma_len) }.to_vec();
            Ok(vec![
                Plane::new(luma, stride, y_rows),
                Plane::new(u, chroma_stride, chroma_rows),
                Plane::new(v, chroma_stride, chroma_rows),
            ])
        }
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => Err(DecodeError::unsupported(
            "MediaCodec reported a packed-RGB output color format",
        )),
    }
}

/// Read colorimetry from a format's COLOR_STANDARD/TRANSFER/RANGE keys, leaving
/// the BT.709 SDR default for anything the source doesn't signal.
///
/// # Safety
/// `format` must be a non-null, valid `AMediaFormat` pointer that has not been deleted.
unsafe fn color_from_format(format: *mut AMediaFormat) -> ColorSpace {
    let mut color = ColorSpace::BT709;
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_COLOR_STANDARD` is an NDK static C string.
    if let Some(standard) = unsafe { format_i32(format, AMEDIAFORMAT_KEY_COLOR_STANDARD) } {
        let (primaries, matrix) = map_color_standard(standard);
        color.primaries = primaries;
        color.matrix = matrix;
    }
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_COLOR_TRANSFER` is an NDK static C string.
    if let Some(transfer) = unsafe { format_i32(format, AMEDIAFORMAT_KEY_COLOR_TRANSFER) } {
        color.transfer = map_color_transfer(transfer);
    }
    // SAFETY: `format` is valid per caller contract; `AMEDIAFORMAT_KEY_COLOR_RANGE` is an NDK static C string.
    if let Some(range) = unsafe { format_i32(format, AMEDIAFORMAT_KEY_COLOR_RANGE) } {
        color.range = map_color_range(range);
    }
    color
}

/// Map a `COLOR_STANDARD_*` value to primaries + YUV matrix (Android bundles
/// both into one "standard").
fn map_color_standard(value: i32) -> (ColorPrimaries, MatrixCoefficients) {
    match value {
        COLOR_STANDARD_BT709 => (ColorPrimaries::Bt709, MatrixCoefficients::Bt709),
        COLOR_STANDARD_BT601_PAL => (ColorPrimaries::Bt601_625, MatrixCoefficients::Bt601),
        COLOR_STANDARD_BT601_NTSC => (ColorPrimaries::Bt601_525, MatrixCoefficients::Bt601),
        COLOR_STANDARD_BT2020 => (ColorPrimaries::Bt2020, MatrixCoefficients::Bt2020Ncl),
        _ => (ColorPrimaries::Unspecified, MatrixCoefficients::Unspecified),
    }
}

/// Map a `COLOR_TRANSFER_*` value to a transfer function. `LINEAR` has no
/// counterpart in our SDR/HDR set, so it falls through to unspecified.
fn map_color_transfer(value: i32) -> TransferFunction {
    match value {
        COLOR_TRANSFER_SDR_VIDEO => TransferFunction::Bt709,
        COLOR_TRANSFER_ST2084 => TransferFunction::Pq,
        COLOR_TRANSFER_HLG => TransferFunction::Hlg,
        _ => TransferFunction::Unspecified,
    }
}

/// Map a `COLOR_RANGE_*` value; anything but the explicit full code is limited.
fn map_color_range(value: i32) -> ColorRange {
    if value == COLOR_RANGE_FULL {
        ColorRange::Full
    } else {
        ColorRange::Limited
    }
}

/// Map a MediaCodec `COLOR_Format*` value to a plane layout. Flexible, surface,
/// and vendor formats cannot be interpreted from a bare ByteBuffer.
fn color_format_to_pixel(value: i32) -> Option<PixelFormat> {
    crate::pixel_map::android_yuv420_layout(
        matches!(
            value,
            COLOR_FORMAT_YUV420_PLANAR | COLOR_FORMAT_YUV420_PACKED_PLANAR
        ),
        matches!(
            value,
            COLOR_FORMAT_YUV420_SEMIPLANAR | COLOR_FORMAT_YUV420_PACKED_SEMIPLANAR
        ),
    )
}

/// Convert a [`RationalTime`] to MediaCodec's microsecond ticks (`i128` math to
/// avoid overflow before truncating to `i64`).
fn to_us(target: RationalTime) -> i64 {
    let num = i128::from(target.rate.num.max(1));
    let den = i128::from(target.rate.den.max(1));
    ((i128::from(target.value) * den * i128::from(US_PER_SEC)) / num) as i64
}

/// `true` when a `media_status_t` is `AMEDIA_OK` (0).
fn media_ok(status: media_status_t) -> bool {
    status.0 == 0
}

/// Read an integer key, or `None` if the format doesn't carry it.
///
/// # Safety
/// `format` must be a non-null, valid `AMediaFormat` pointer. `key` must point to a valid
/// null-terminated C string (e.g. `AMEDIAFORMAT_KEY_*` constants).
unsafe fn format_i32(format: *mut AMediaFormat, key: *const c_char) -> Option<i32> {
    let mut out: i32 = 0;
    // SAFETY: `format` and `key` are valid per caller contract; `&mut out` points to a valid,
    // aligned local `i32` stack location.
    let ok = unsafe { AMediaFormat_getInt32(format, key, &mut out) };
    ok.then_some(out)
}

/// Read a 64-bit integer key, or `None` if the format doesn't carry it.
///
/// # Safety
/// `format` must be a non-null, valid `AMediaFormat` pointer. `key` must point to a valid
/// null-terminated C string (e.g. `AMEDIAFORMAT_KEY_*` constants).
unsafe fn format_i64(format: *mut AMediaFormat, key: *const c_char) -> Option<i64> {
    let mut out: i64 = 0;
    // SAFETY: `format` and `key` are valid per caller contract; `&mut out` points to a valid,
    // aligned local `i64` stack location.
    let ok = unsafe { AMediaFormat_getInt64(format, key, &mut out) };
    ok.then_some(out)
}

/// Read a string key as an owned `String`, or `None` if absent.
///
/// # Safety
/// `format` must be a non-null, valid `AMediaFormat` pointer. `key` must point to a valid
/// null-terminated C string (e.g. `AMEDIAFORMAT_KEY_*` constants).
unsafe fn format_string(format: *mut AMediaFormat, key: *const c_char) -> Option<String> {
    let mut out: *const c_char = ptr::null();
    // SAFETY: `format` and `key` are valid per caller contract; `&mut out` points to a valid
    // local pointer storage. `AMediaFormat_getString` sets `out` to an internal C string
    // owned by `format` that remains valid as long as `format` is not modified or deleted.
    let ok = unsafe { AMediaFormat_getString(format, key, &mut out) };
    if ok && !out.is_null() {
        Some(
            // SAFETY: `out` is non-null and points to a null-terminated C string within `format`.
            // We copy it into an owned `String` before any mutation or deletion of `format`.
            unsafe { CStr::from_ptr(out) }
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    }
}

#[cfg(test)]
#[path = "android_tests.rs"]
mod tests;
