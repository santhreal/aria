//! Apple backend: `AVAssetReader` (demux + VideoToolbox decode) behind
//! [`aria_core::VideoDecoder`].
//!
//! `AVAssetReader` reads decoded `CVPixelBuffer`s forward from a configurable
//! start time. The output settings list every format we can ingest, so
//! VideoToolbox vends frames in its native decoded format (NV12 video/full
//! range, or 10-bit/`BGRA` for some sources); the exact format is detected per
//! frame from the pixel buffer. Sample data is *not* copied out of the decoder
//! (`alwaysCopiesSampleData = NO`): the CPU path copies planes itself and the
//! GPU path hands the renderer the decoder's own IOSurface, retained.
//!
//! ## Seeking
//!
//! Playback and forward scrubbing never re-seek: [`VideoDecoder::frame_at`]
//! rolls forward from the last emitted frame when the target is just ahead
//! (see [`crate::seek`]), which is what avoids the O(GOP²) re-decode of a
//! seek-per-frame loop. A true [`AvfDecoder::seek`] (backward, or a long jump)
//! tears down and rebuilds the reader at the new start time against the
//! already-parsed `AVURLAsset` — `AVAssetReader` is forward-only, so this is
//! the correct random-access primitive. (`resetForReadingTimeRanges:` was
//! evaluated and rejected: it only re-arms after the *current* range is fully
//! drained, so a scrub seek would decode-and-drop the rest of the in-flight
//! range first, costing more than the reader rebuild it saves.)
//!
//! One caveat other backends don't share: `AVAssetReader` trims its output to
//! the time range, decoding the GOP prefix *internally*, so
//! [`VideoDecoder::frame_at_nearest`]'s "seek + one decode" here returns the
//! frame at/after the target at the cost of the full prefix decode — there is
//! no cheap snap-to-sync-frame on this backend. On long-GOP 4K that is
//! hundreds of milliseconds per cold target (thumbnails, filmstrips); if that
//! ever matters it needs demux/decode split apart (compressed sample output +
//! our own `VTDecompressionSession`), not tuning here.
//!
//! Two pieces of lifecycle hygiene keep sustained scrubbing stable: an
//! outgoing reader is [`AVAssetReader::cancelReading`]-ed when replaced or
//! dropped (releasing a started reader without cancelling leaves its decode
//! pipeline — VideoToolbox session plus queued IOSurfaces — to wind down
//! asynchronously; under scrub churn hundreds of those pile up and exhaust
//! process-wide decode resources, after which *every* decoder in the process
//! fails with "Cannot Decode"), and each frame pull runs inside an
//! [`autoreleasepool`] because decode workers are plain Rust threads with no
//! enclosing pool — without one, AVFoundation's autoreleased bookkeeping
//! accumulates for the life of the thread.
//!
//! ## Open-GOP sources: seek-point decode recovery
//!
//! `AVAssetReader` starts decode at the container sync sample at/before the
//! time range's start and trusts it blindly. Open-GOP H.264 (x264
//! `open_gop=1`, some hardware encoders) marks **non-IDR I-frames** as sync
//! samples, and most of those are followed in decode order by leading
//! B-frames that display *before* the I-frame and reference the **previous**
//! GOP. Starting decode there, VideoToolbox hits references that don't exist
//! and — unlike FFmpeg, which drops the undecodable leading frames — fails
//! the entire reader session (`AVErrorDecodeFailed`, "Cannot Decode") before
//! vending a single frame. On a real-world 4K export with ~10 s GOPs this
//! made *every seek into 8 of the file's 11 GOPs* a hard error, while
//! sequential decode of the same file was flawless (the references exist by
//! the time the boundary is crossed in order).
//!
//! The recovery ([`AvfDecoder::recover_read_failure`]): when the reader
//! fails, rebuild it with the start pulled back from where the caller was —
//! just past the last emitted frame, or the requested seek start — doubling
//! the back-off (1 s, 2 s, 4 s, … clamped to 0, where the first sample must
//! be genuinely decodable) until decode sticks, then walk forward and trim
//! back up to that position. The failure can strike *at* the seek point
//! (start sync sample has leading B-frames; the reader dies before vending
//! anything) or *mid-walk* while crossing such a sync sample after a start
//! at an earlier non-IDR one — resuming past the last emitted frame handles
//! both. Failed attempts are cheap (VideoToolbox dies within a few frames of
//! a bad start, and pre-bound frames skip conversion); the successful
//! attempt pays one GOP-prefix walk, which is what a closed-GOP file would
//! have charged for the seek anyway. If even the start of the stream won't
//! decode (real corruption), the error surfaces — but through
//! [`AvfDecoder::reset_failed_reader`], which replaces the dead reader (a
//! `Failed` reader can never vend again) and clears the roll-forward anchor,
//! so the next `frame_at` re-seeks on a healthy reader instead of re-reading
//! the corpse forever.
//!
//! The module calls AVFoundation, CoreMedia, and CoreVideo through Objective-C
//! and C FFI and is `unsafe` throughout.

#![allow(unsafe_code)]

use core::cmp::Ordering;
use std::path::Path;
use std::ptr::NonNull;
use std::slice;

use objc2::AllocAnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeAudio,
    AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_foundation::{CFRetained, CFString, CFType, CGAffineTransform, CGSize};
use objc2_core_media::{
    CMFormatDescription, CMTime, CMTimeFlags, CMTimeRange,
    kCMFormatDescriptionColorPrimaries_DCI_P3, kCMFormatDescriptionColorPrimaries_EBU_3213,
    kCMFormatDescriptionColorPrimaries_ITU_R_709_2, kCMFormatDescriptionColorPrimaries_ITU_R_2020,
    kCMFormatDescriptionColorPrimaries_P3_D65, kCMFormatDescriptionColorPrimaries_SMPTE_C,
    kCMFormatDescriptionExtension_ColorPrimaries, kCMFormatDescriptionExtension_TransferFunction,
    kCMFormatDescriptionExtension_YCbCrMatrix, kCMFormatDescriptionTransferFunction_ITU_R_709_2,
    kCMFormatDescriptionTransferFunction_ITU_R_2020,
    kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG,
    kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ,
    kCMFormatDescriptionTransferFunction_sRGB, kCMFormatDescriptionYCbCrMatrix_ITU_R_601_4,
    kCMFormatDescriptionYCbCrMatrix_ITU_R_709_2, kCMFormatDescriptionYCbCrMatrix_ITU_R_2020,
    kCMTimePositiveInfinity,
};
use objc2_core_video::{
    CVImageBuffer, CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL, NSUnderlyingErrorKey,
};

use aria_core::{
    ColorPrimaries, ColorRange, ColorSpace, CpuImage, DecodeError, FrameData, GpuSurface,
    GpuSurfaceKind, MatrixCoefficients, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation,
    SourceInfo, TransferFunction, VideoDecoder, VideoFrame,
};

use crate::OutputMode;

#[path = "apple_probe.rs"]
mod apple_probe;
#[path = "apple_reader.rs"]
mod apple_reader;

/// The audio-only duration probe `crate::probe` reaches for by this path.
pub(crate) use apple_probe::audio_track_duration;
// Glob so this module, `apple_tests`, and the integration tests all reach the
// split-out helpers by the same bare names they used before the move.
use apple_probe::*;
use apple_reader::*;

// FourCC pixel format codes (CoreVideo OSType constants), matched against
// `CVPixelBufferGetPixelFormatType`.
const FOURCC_420V: u32 = u32::from_be_bytes(*b"420v"); // NV12, video (limited) range
const FOURCC_420F: u32 = u32::from_be_bytes(*b"420f"); // NV12, full range
const FOURCC_X420: u32 = u32::from_be_bytes(*b"x420"); // 10-bit 4:2:0, video range
const FOURCC_BGRA: u32 = u32::from_be_bytes(*b"BGRA"); // 32-bit BGRA

/// A decoded video source backed by AVFoundation / VideoToolbox.
pub struct AvfDecoder {
    asset: Retained<AVURLAsset>,
    track: Retained<AVAssetTrack>,
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
    info: SourceInfo,
    mode: OutputMode,
    started: bool,
    /// The requested start of the current reader (`None` = whole clip):
    /// where a replacement reader is rebuilt after a failure, and the trim
    /// point the failure recovery walks back up to (see the module docs).
    start: Option<RationalTime>,
    /// PTS of the last emitted frame — the [`crate::seek`] roll-forward anchor.
    /// Cleared on [`VideoDecoder::seek`] (decode position moved) and on read
    /// failure (the rebuilt reader is not at that position).
    last_pts: Option<RationalTime>,
    /// Observed seek/decode costs driving the adaptive roll window.
    seek_stats: crate::seek_cost::SeekStats,
}

// SAFETY: AVFoundation/CoreVideo objects here are owned and touched by a single
// thread at a time — the decode-ahead worker the engine parks this decoder on.
// We never share `&self` across threads, satisfying the `VideoDecoder: Send`
// requirement without needing `Sync`.
unsafe impl Send for AvfDecoder {}

impl std::fmt::Debug for AvfDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AvfDecoder")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

/// Parks a retained CoreVideo buffer in a [`GpuSurface`]'s `keep_alive` so the
/// decoder's IOSurface outlives the renderer's reads. The raw handle in the
/// surface points at this same object.
struct RetainedImageBuffer(#[allow(dead_code)] CFRetained<CVImageBuffer>);

// SAFETY: a decoded `CVImageBuffer` is immutable, and CoreVideo's
// retain/release is atomic, so the wrapper can travel to the render thread and
// drop there. We only ever hold (never mutate) the buffer.
unsafe impl Send for RetainedImageBuffer {}
unsafe impl Sync for RetainedImageBuffer {}

/// Lower PTS bound for frames the failure-recovery walk may emit: the
/// rebuilt reader starts earlier than the caller's position, and everything
/// below the bound is trimmed without conversion.
#[derive(Clone, Copy)]
enum EmitFrom {
    /// Emit the first frame at/after the bound (a requested seek start).
    At(RationalTime),
    /// Emit the first frame strictly after the bound (resuming past the
    /// last frame the caller already has).
    After(RationalTime),
}

impl EmitFrom {
    fn bound(self) -> RationalTime {
        match self {
            EmitFrom::At(t) | EmitFrom::After(t) => t,
        }
    }

    fn admits(self, pts: RationalTime) -> bool {
        match self {
            EmitFrom::At(t) => pts.compare(t) != Ordering::Less,
            EmitFrom::After(t) => pts.compare(t) == Ordering::Greater,
        }
    }
}

impl AvfDecoder {
    /// Open `path` and probe its first video track.
    pub fn open(path: &Path, mode: OutputMode) -> Result<Self, DecodeError> {
        let path_str = path
            .to_str()
            .ok_or_else(|| DecodeError::unsupported("path is not valid UTF-8"))?;

        let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
        let asset = unsafe { AVURLAsset::initWithURL_options(AVURLAsset::alloc(), &url, None) };

        let track = first_video_track(&asset)
            .ok_or_else(|| DecodeError::unsupported("no video track found"))?;

        let info = probe(&track);
        // No explicit time range -> AVFoundation's default (the whole clip).
        let (reader, output) = build_reader(&asset, &track, None)?;

        Ok(Self {
            asset,
            track,
            reader,
            output,
            info,
            mode,
            started: false,
            start: None,
            last_pts: None,
            seek_stats: Default::default(),
        })
    }

    /// Whether this decoder's asset also carries an audio track. Reads the
    /// already-parsed asset, so the probe doesn't pay a second open of the
    /// file.
    pub(crate) fn has_audio_track(&self) -> bool {
        let Some(media_audio) = (unsafe { AVMediaTypeAudio }) else {
            return false;
        };
        #[allow(deprecated)]
        let tracks = unsafe { self.asset.tracksWithMediaType(media_audio) };
        tracks.firstObject().is_some()
    }

    /// Start the reader if it hasn't been started yet.
    fn ensure_started(&mut self) -> Result<(), DecodeError> {
        if !self.started {
            if !unsafe { self.reader.startReading() } {
                return Err(self.reader_error("startReading failed"));
            }
            self.started = true;
        }
        Ok(())
    }

    /// Pull the next decoded sample buffer's pixel buffer, skipping
    /// marker-only buffers — and, when `emit_from` is set, frames below that
    /// bound (the failure recovery reads from an earlier start and trims back
    /// up to where the caller was) — and convert it to a [`VideoFrame`].
    ///
    /// Runs inside an autorelease pool: decode workers are plain Rust threads,
    /// so without one AVFoundation's per-pull autoreleased objects would
    /// accumulate until the thread exits. The returned frame only holds
    /// explicitly retained references, so it safely outlives the pool.
    fn pull_frame(
        &mut self,
        emit_from: Option<EmitFrom>,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        autoreleasepool(|_| {
            loop {
                let Some(sample) = (unsafe { self.output.copyNextSampleBuffer() }) else {
                    // Exhausted, or an error mid-stream — distinguish via status.
                    return match unsafe { self.reader.status() } {
                        AVAssetReaderStatus::Failed => Err(self.reader_error("read failed")),
                        _ => Ok(None),
                    };
                };

                // Marker-only buffers carry no image; skip them.
                if unsafe { sample.num_samples() } == 0 {
                    continue;
                }

                let Some(image_buffer) = (unsafe { sample.image_buffer() }) else {
                    continue;
                };

                let pts = cmtime_to_rational(unsafe { sample.presentation_time_stamp() })
                    .unwrap_or_else(|| RationalTime::zero(self.info.time_base));

                // Trim frames below the recovery bound without paying the
                // plane copy / surface wrap for them.
                if let Some(bound) = emit_from
                    && !bound.admits(pts)
                {
                    continue;
                }

                let frame = self.image_buffer_to_frame(image_buffer, pts)?;
                self.last_pts = Some(pts);
                return Ok(Some(frame));
            }
        })
    }

    /// Convert one decoded `CVImageBuffer` into a [`VideoFrame`]. Takes ownership
    /// of the retained buffer so the GPU path can park it as the surface's
    /// keep-alive; the CPU path copies its planes and lets it drop.
    fn image_buffer_to_frame(
        &self,
        image_buffer: CFRetained<CVImageBuffer>,
        pts: RationalTime,
    ) -> Result<VideoFrame, DecodeError> {
        // A decoded `CVImageBuffer` *is* a `CVPixelBuffer` (same CFType);
        // reinterpret to reach the pixel-buffer accessors.
        let pb: &CVPixelBuffer = unsafe {
            CFRetained::as_ptr(&image_buffer)
                .cast::<CVPixelBuffer>()
                .as_ref()
        };

        let fourcc = CVPixelBufferGetPixelFormatType(pb);
        let (format, range) = detect_format(fourcc).ok_or_else(|| {
            DecodeError::unsupported(format!("unsupported pixel format {}", fourcc_str(fourcc)))
        })?;

        let width = CVPixelBufferGetWidth(pb) as u32;
        let height = CVPixelBufferGetHeight(pb) as u32;

        let color = ColorSpace {
            range,
            ..self.info.color
        };

        let data = match self.mode {
            OutputMode::Cpu => FrameData::Cpu(CpuImage::new(read_cpu_planes(pb)?)),
            OutputMode::Gpu => {
                // Hand the renderer the IOSurface-backed buffer's address and
                // keep our retained reference alive alongside it. The pointer
                // and the `keep_alive` refer to the same object.
                let handle = CFRetained::as_ptr(&image_buffer).as_ptr() as usize as u64;
                let surface = GpuSurface::new(
                    GpuSurfaceKind::CoreVideoPixelBuffer,
                    handle,
                    Some(Box::new(RetainedImageBuffer(image_buffer))),
                );
                FrameData::Gpu(surface)
            }
        };

        Ok(VideoFrame::new(
            pts,
            format,
            color,
            (width, height),
            Rect::from_size(width, height),
            self.info.rotation,
            data,
        ))
    }
}

/// `CMTime` (value/timescale seconds) -> exact [`RationalTime`].
pub(crate) fn cmtime_to_rational(t: CMTime) -> Option<RationalTime> {
    (t.timescale > 0 && t.flags.contains(CMTimeFlags::Valid))
        .then(|| RationalTime::new(t.value, Rational::new(t.timescale, 1)))
}

/// [`RationalTime`] → `CMTime`, through [`RationalTime::as_timescaled`].
pub(crate) fn rational_to_cmtime(t: RationalTime) -> CMTime {
    let (value, timescale) = t.as_timescaled();
    CMTime {
        value,
        timescale,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    }
}

#[cfg(test)]
fn cm_zero() -> CMTime {
    CMTime {
        value: 0,
        timescale: 1,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    }
}

/// Human-readable FourCC for error messages.
fn fourcc_str(code: u32) -> String {
    String::from_utf8_lossy(&code.to_be_bytes()).into_owned()
}

#[cfg(test)]
#[path = "apple_tests.rs"]
mod tests;

/// Integration tests that round-trip through real AVFoundation: synthesize a
/// tiny H.264 movie with `AVAssetWriter`, then decode it back. Hermetic — no
/// external asset needed.
#[cfg(test)]
#[path = "apple_integration_tests.rs"]
mod integration;
