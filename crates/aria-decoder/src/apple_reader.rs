//! Reader lifecycle for the AVFoundation backend: building an `AVAssetReader`
//! at a start time, the [`VideoDecoder`] surface it drives, and the open-GOP
//! decode-failure recovery described in `apple.rs`'s module docs.
//!
//! Declared from `apple.rs` with `#[path]`; `use super::*` reaches that module's
//! private items and its own imports, so the move changed no name resolution.
//! The module calls AVFoundation and Foundation through Objective-C FFI and is
//! `unsafe` throughout.

#![allow(unsafe_code)]

use super::*;

impl AvfDecoder {
    /// Pull the next decoded frame, recovering from reader decode failures
    /// (see the module docs on open-GOP sources). Non-reader errors — an
    /// unsupported pixel format, a lock failure — pass through untouched;
    /// reader failures that survive recovery surface only after
    /// [`Self::reset_failed_reader`] has put the decoder back into a usable
    /// state.
    fn next_pixel_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let result = self.ensure_started().and_then(|()| self.pull_frame(None));
        match result {
            Err(err) if unsafe { self.reader.status() } == AVAssetReaderStatus::Failed => {
                self.recover_read_failure(err)
            }
            other => other,
        }
    }

    /// Recover from a reader that failed mid-read. On valid files that is
    /// VideoToolbox refusing an open-GOP start point (see the module docs):
    /// the reader dies on the sync sample's leading B-frames — sometimes
    /// after vending a few decodable frames — even though every frame the
    /// caller actually wants decodes fine from an earlier, sounder start.
    ///
    /// Rebuild the reader progressively earlier (doubling back-off, clamped
    /// to the start of the stream, where the first sample must be a real
    /// sync frame) and walk forward to where the caller was: just past the
    /// last emitted frame, or the requested seek start if nothing was
    /// emitted yet. Attempts that fail at their own start point are
    /// near-free (VideoToolbox rejects them before decoding much); the
    /// successful attempt pays one GOP-prefix walk, which is what a
    /// closed-GOP file would have charged for the seek anyway.
    ///
    /// Gives up — surfacing `original` after [`Self::reset_failed_reader`] —
    /// when there is nowhere to back away from (a failure at the very start
    /// of the stream) or when even the start of the stream won't decode
    /// (real corruption; the bounded back-off makes the total give-up cost a
    /// couple of decode walks, paid once per caller-visible error).
    fn recover_read_failure(
        &mut self,
        original: DecodeError,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        // Resume just past the last emitted frame (mid-walk failure), or at
        // the requested start (nothing emitted since the seek).
        let resume = match (self.last_pts, self.start) {
            (Some(pts), _) => EmitFrom::After(pts),
            (None, Some(start)) => EmitFrom::At(start),
            (None, None) => {
                // Cold open failing at the head of the stream: nothing to
                // back away from.
                self.reset_failed_reader();
                return Err(original);
            }
        };
        let anchor = resume.bound();
        let mut backoff = one_second_ticks(anchor.rate);
        loop {
            let earlier = RationalTime::new((anchor.value - backoff).max(0), anchor.rate);
            let attempt = self
                .rebuild_reader_at(Some(earlier))
                .and_then(|()| self.ensure_started())
                .and_then(|()| self.pull_frame(Some(resume)));
            match attempt {
                // Recovered (`Some`), or the resume point really is past the
                // end of the stream (`None`) — a healthy reader's answer.
                Ok(frame) => return Ok(frame),
                Err(err) if unsafe { self.reader.status() } != AVAssetReaderStatus::Failed => {
                    // Not a reader failure (unsupported format, lock error):
                    // an earlier start cannot change it.
                    return Err(err);
                }
                Err(_) => {
                    if earlier.value == 0 {
                        // Even the start of the stream won't decode.
                        self.reset_failed_reader();
                        return Err(original);
                    }
                    backoff = backoff.saturating_mul(2);
                }
            }
        }
    }

    /// Put the decoder back into a usable state after a read failure: a
    /// `Failed` reader can never vend again, so replace it (best-effort) with
    /// a fresh, un-started one at the last requested position, and clear the
    /// roll-forward anchor so the next [`VideoDecoder::frame_at`] takes the
    /// seek path on the healthy reader instead of rolling onto the dead one.
    /// The decode *position* after a failure is unspecified; callers re-drive
    /// through `frame_at`/`seek`, which both re-position.
    fn reset_failed_reader(&mut self) {
        self.last_pts = None;
        // On rebuild failure the cancelled reader stays; the next pull then
        // errors through `ensure_started` and lands back here — no loop, the
        // recovery path is bounded and `frame_at` callers see the error.
        let _ = self.rebuild_reader_at(self.start);
    }

    /// Replace the current reader with one whose time range starts at
    /// `range_start` (`None` = whole clip). Builds the replacement first so a
    /// failure leaves the decoder unchanged, then cancels the old reader
    /// before releasing it (see the module docs on reader lifecycle).
    fn rebuild_reader_at(&mut self, range_start: Option<RationalTime>) -> Result<(), DecodeError> {
        let start = range_start.map(rational_to_cmtime);
        let (reader, output) = build_reader(&self.asset, &self.track, start)?;
        self.cancel_current_reader();
        self.reader = reader;
        self.output = output;
        self.started = false;
        Ok(())
    }

    /// Cancel the current reader's in-flight decode work before it is replaced
    /// or dropped.
    ///
    /// Releasing a started reader *without* cancelling leaves its pipeline
    /// (VideoToolbox session, queued IOSurfaces) to unwind asynchronously.
    /// Sustained scrubbing rebuilds readers faster than those zombies drain,
    /// and once enough pile up the process hits its decode-resource ceiling and
    /// every subsequent read fails with "Cannot Decode".
    fn cancel_current_reader(&mut self) {
        if self.started {
            unsafe { self.reader.cancelReading() };
            self.started = false;
        }
    }

    /// Build a [`DecodeError`] from the reader's `NSError`, if any, carrying
    /// enough to diagnose a failure from a log line alone: error domain +
    /// code, the underlying error (for `AVErrorDecodeFailed` that is the
    /// VideoToolbox `OSStatus`), and where the reader was positioned.
    pub(super) fn reader_error(&self, context: &str) -> DecodeError {
        let detail = unsafe { self.reader.error() }
            .map(|e| describe_ns_error(&e))
            .unwrap_or_else(|| "no NSError".to_string());
        let start = match self.start {
            Some(t) => format!("{:.3}s", rt_secs(t)),
            None => "clip start".to_string(),
        };
        let position = match self.last_pts {
            Some(t) => format!(", last pts {:.3}s", rt_secs(t)),
            None => ", no frames delivered".to_string(),
        };
        DecodeError::Decode(format!(
            "{context}: {detail} (reader start {start}{position})"
        ))
    }
}

/// `NSError` → "description [domain code]", recursing one level into the
/// underlying error, which for AVFoundation decode failures holds the
/// otherwise-invisible VideoToolbox `OSStatus`.
pub(super) fn describe_ns_error(err: &NSError) -> String {
    let mut text = format!(
        "{} [{} {}]",
        err.localizedDescription(),
        err.domain(),
        err.code()
    );
    let underlying = err
        .userInfo()
        .objectForKey(unsafe { NSUnderlyingErrorKey })
        .and_then(|value| value.downcast::<NSError>().ok());
    if let Some(u) = underlying {
        text.push_str(&format!(
            " (underlying: {} [{} {}])",
            u.localizedDescription(),
            u.domain(),
            u.code()
        ));
    }
    text
}

/// [`RationalTime`] in seconds, for error messages only.
fn rt_secs(t: RationalTime) -> f64 {
    t.value as f64 * f64::from(t.rate.den.max(1)) / f64::from(t.rate.num.max(1))
}

/// One second expressed in ticks of `rate`, rounded up, at least one tick —
/// the base step of the seek-point recovery back-off.
pub(super) fn one_second_ticks(rate: Rational) -> i64 {
    let num = i64::from(rate.num.max(1));
    let den = i64::from(rate.den.max(1));
    ((num + den - 1) / den).max(1)
}

impl VideoDecoder for AvfDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        // Rebuild first so a failed seek leaves the decoder unchanged.
        self.rebuild_reader_at(Some(target))?;
        self.start = Some(target);
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        self.next_pixel_frame()
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        // Copied out and written back: `frame_at_rolling` borrows `self` for
        // the decode walk, so it can't also borrow the field.
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_nearest_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }
}

impl Drop for AvfDecoder {
    fn drop(&mut self) {
        self.cancel_current_reader();
    }
}

/// Output settings asking the reader for decoded pixel buffers in one of our
/// supported formats. Listing several lets VideoToolbox return the native depth
/// (8-bit NV12 or 10-bit) rather than forcing a conversion.
fn decode_output_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let formats = [FOURCC_420V, FOURCC_420F, FOURCC_X420, FOURCC_BGRA];
    let numbers: Vec<Retained<NSNumber>> = formats
        .iter()
        .map(|&f| NSNumber::numberWithUnsignedInt(f))
        .collect();
    let number_refs: Vec<&NSNumber> = numbers.iter().map(|n| n.as_ref()).collect();
    let array = NSArray::from_slice(&number_refs);

    // `kCVPixelBufferPixelFormatTypeKey` is a CFString; toll-free bridged to
    // NSString for use as a dictionary key.
    let key_cf: &CFString = unsafe { kCVPixelBufferPixelFormatTypeKey };
    let key: &NSString = unsafe { &*(core::ptr::from_ref(key_cf).cast::<NSString>()) };
    let value: &AnyObject = unsafe { &*(Retained::as_ptr(&array).cast::<AnyObject>()) };

    NSDictionary::from_slices(&[key], &[value])
}

/// Build a reader + track output starting at `start`, reading to end of stream.
pub(super) fn build_reader(
    asset: &AVURLAsset,
    track: &AVAssetTrack,
    start: Option<CMTime>,
) -> Result<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>), DecodeError> {
    let reader = unsafe { AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), asset) }
        .map_err(|e| DecodeError::Open(e.localizedDescription().to_string()))?;

    // Request decoded pixel buffers. With `nil` settings the reader hands back
    // *compressed* sample buffers; we list the formats we can ingest and let
    // VideoToolbox pick the one closest to its native decode output (no extra
    // conversion when the source is 8-bit 4:2:0 or 10-bit), detected per frame.
    let settings = decode_output_settings();
    let output = unsafe {
        AVAssetReaderTrackOutput::initWithTrack_outputSettings(
            AVAssetReaderTrackOutput::alloc(),
            track,
            Some(&settings),
        )
    };

    unsafe {
        // We never mutate sample data in place (the CPU path copies planes
        // itself; the GPU path retains the buffer), so skip AVFoundation's
        // defensive per-frame copy — a per-frame allocation+memcpy otherwise.
        output.setAlwaysCopiesSampleData(false);
        // Only constrain the range when seeking; otherwise read the whole clip.
        if let Some(start) = start {
            reader.setTimeRange(CMTimeRange {
                start,
                duration: kCMTimePositiveInfinity,
            });
        }
        if !reader.canAddOutput(&output) {
            return Err(DecodeError::Open("reader cannot add track output".into()));
        }
        reader.addOutput(&output);
    }

    Ok((reader, output))
}
