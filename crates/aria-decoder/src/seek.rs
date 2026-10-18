//! Shared seek policy: roll forward instead of re-seeking for near targets.
//!
//! None of the native readers keeps a keyframe index (the old FFmpeg stack
//! did), so the [`VideoDecoder`] default `frame_at` — seek, then walk — makes
//! the codec re-decode the whole GOP prefix on *every* call. Across a playback
//! or forward-scrub run that is O(GOP²) work per GOP, and scrubbing is the
//! primary touch interaction on mobile.
//!
//! Every backend therefore remembers the PTS of the last frame it emitted and
//! answers `frame_at` with [`frame_at_rolling`]: when the target lies ahead of
//! that frame by no more than the **roll window**, keep decoding forward from
//! where the codec already is — every in-between frame would have to be
//! decoded after a seek anyway. Backward targets, long forward jumps, and
//! fresh decoders fall back to a real seek, byte-identical to the default
//! seek-then-walk.
//!
//! The roll window is cost-adaptive: see [`crate::seek_cost`].
//!
//! ## Tick-truncation slack
//!
//! The walk treats a frame as satisfying the target when its PTS is within
//! **one tick** of the frame's own time base *before* it ([`frame_covers`]),
//! not only at/after it. Backends whose native clocks are integer ticks —
//! Media Foundation's 100-ns units, MediaCodec's microseconds — **truncate**
//! frame times that aren't representable: at 30000/1001 fps frame 1's true
//! time 333 666.⅔ hns arrives as 333 666. Under an exact comparison that
//! frame sits *before* its own rational target, so `frame_at(i)` returns
//! frame `i + 1`, and the off-by-one compounds: targets that exactly equal a
//! truncated PTS (1 in 3 at NTSC rates) then compare `Equal` to `last_pts`,
//! fall off the roll path, and pay a full seek + GOP re-decode *during
//! ordinary sequential playback*. One tick of slack returns the intended
//! frame and keeps playback on the roll path, while being far too small to
//! ever skip a real frame — as a guard for hypothetical frame-granular time
//! bases the slack is additionally capped at half a frame period. Exact-clock
//! backends (Apple's rational `CMTime`) are unaffected: their neighboring
//! frames sit whole periods apart, never within a tick.

use core::cmp::Ordering;
use std::time::Instant;

use aria_core::{DecodeError, Rational, RationalTime, VideoDecoder, VideoFrame, resample};

use crate::seek_cost::{SeekStats, frames_per_sec};

/// `sec(a) − sec(b)`, exactly, scaled by the (positive) product of both rate
/// numerators: positive iff `a` is later. `i128` keeps the triple products
/// exact (values ≤ 2⁶³, rate parts ≤ 2³¹ ⇒ products < 2¹²⁵).
fn scaled_delta(a: RationalTime, b: RationalTime) -> i128 {
    i128::from(a.value) * i128::from(a.rate.den) * i128::from(b.rate.num)
        - i128::from(b.value) * i128::from(b.rate.den) * i128::from(a.rate.num)
}

/// The tick-truncation slack: whether `delta` — a non-negative
/// [`scaled_delta`] between a requested time and a decoded PTS on
/// `tick_rate`, with `scale = other_num · tick_rate.num` — is at most **one
/// tick** of `tick_rate`, additionally capped at **half a frame period** so a
/// coarse time base (ticks ≈ frames) can never absorb a real frame.
///
/// One tick in the scaled units is `tick_rate.den · other_num` (a tick is
/// `den/num` seconds; the scale contributes `num · other_num`). The
/// half-frame cap compares `delta / scale ≤ fr.den / (2·fr.num)`
/// cross-multiplied; `checked_mul` guards the one product that can exceed
/// `i128`, which is unambiguously "way more than half a frame". An invalid
/// `frame_rate` skips the cap (the tick bound alone still applies).
fn within_tick_slack(
    delta: i128,
    tick_rate: Rational,
    other_num: i32,
    scale: i128,
    frame_rate: Rational,
) -> bool {
    debug_assert!(delta >= 0 && scale > 0);
    if delta > i128::from(tick_rate.den) * i128::from(other_num) {
        return false;
    }
    if !frame_rate.is_valid() {
        return true;
    }
    let rhs = scale * i128::from(frame_rate.den);
    match delta.checked_mul(2 * i128::from(frame_rate.num)) {
        Some(lhs) => lhs <= rhs,
        None => false,
    }
}

/// True when the decoder should keep pulling frames instead of seeking:
/// `target` lies after `last_pts` by no more than `window_us` microseconds,
/// and by more than the truncation slack — a target within one tick of the
/// last emitted frame *is* that frame (see the module docs), and re-emitting
/// it takes a seek, not a roll. Invalid rates disable rolling — a seek is
/// always correct.
pub(crate) fn should_roll_forward(
    last_pts: Option<RationalTime>,
    target: RationalTime,
    frame_rate: Rational,
    window_us: i64,
) -> bool {
    let Some(last) = last_pts else {
        return false;
    };
    if !last.rate.is_valid() || !target.rate.is_valid() {
        return false;
    }
    if target.compare(last) != Ordering::Greater {
        return false;
    }
    let ahead = scaled_delta(target, last);
    let scale = i128::from(target.rate.num) * i128::from(last.rate.num);
    // ahead/scale secs ≤ window_us µs ⟺ ahead·10⁶ ≤ window_us·scale. The
    // left product can overflow i128 only for astronomically far targets,
    // where seeking is the right answer anyway.
    match ahead.checked_mul(1_000_000) {
        Some(lhs) if lhs <= i128::from(window_us) * scale => {}
        _ => return false,
    }
    !within_tick_slack(ahead, last.rate, target.rate.num, scale, frame_rate)
}

/// Whether a frame at `later` is a different, later frame than one at
/// `earlier`: after it by more than the truncation slack. Invalid rates fall
/// back to an exact comparison.
pub(crate) fn is_after(later: RationalTime, earlier: RationalTime, frame_rate: Rational) -> bool {
    if later.compare(earlier) != Ordering::Greater {
        return false;
    }
    if !later.rate.is_valid() || !earlier.rate.is_valid() {
        return true;
    }
    let ahead = scaled_delta(later, earlier);
    let scale = i128::from(later.rate.num) * i128::from(earlier.rate.num);
    !within_tick_slack(ahead, earlier.rate, later.rate.num, scale, frame_rate)
}

/// Whether `target` lies before `at` by more than half a frame period: the
/// frame for `target` precedes the frame at `at`. Without a valid frame rate,
/// [`is_after`] decides.
pub(crate) fn is_frame_before(
    target: RationalTime,
    at: RationalTime,
    frame_rate: Rational,
) -> bool {
    if !frame_rate.is_valid() || !target.rate.is_valid() || !at.rate.is_valid() {
        return is_after(at, target, frame_rate);
    }
    if at.compare(target) != Ordering::Greater {
        return false;
    }
    // (at − target) secs > den / (2·num) secs, cross-multiplied by the scale.
    let behind = scaled_delta(at, target);
    let scale = i128::from(at.rate.num) * i128::from(target.rate.num);
    match behind.checked_mul(2 * i128::from(frame_rate.num)) {
        Some(lhs) => lhs > scale * i128::from(frame_rate.den),
        None => true,
    }
}

/// Whether the decoded frame at `frame_pts` satisfies a request for `target`:
/// at/after it, or before it by no more than the truncation slack (see the
/// module docs).
pub(crate) fn frame_covers(
    frame_pts: RationalTime,
    target: RationalTime,
    frame_rate: Rational,
) -> bool {
    if frame_pts.compare(target) != Ordering::Less {
        return true;
    }
    if !frame_pts.rate.is_valid() || !target.rate.is_valid() {
        return false;
    }
    let behind = scaled_delta(target, frame_pts);
    let scale = i128::from(target.rate.num) * i128::from(frame_pts.rate.num);
    within_tick_slack(behind, frame_pts.rate, target.rate.num, scale, frame_rate)
}

/// [`VideoDecoder::frame_at`] with the roll-forward fast path.
///
/// `last_pts` is the PTS of the last frame the decoder emitted (backends
/// record it in `next_frame` and clear it in `seek`). When rolling, the walk
/// continues from the decoder's current position; hitting end of stream while
/// rolling means the target lies past the end, which is the same `Ok(None)`
/// the seek path would produce.
///
/// Times its own work to feed `stats`: a seek-path call updates the seek-cost
/// EMA with the whole `seek()`-plus-walk duration, a roll-path call updates
/// the per-frame EMA with `elapsed / frames_decoded`. The two `Instant`
/// reads are nanoseconds against millisecond-scale decode work.
pub fn frame_at_rolling<D: VideoDecoder + ?Sized>(
    decoder: &mut D,
    last_pts: Option<RationalTime>,
    stats: &mut SeekStats,
    target: RationalTime,
) -> Result<Option<VideoFrame>, DecodeError> {
    let frame_rate = decoder.info().frame_rate;
    let window = stats.roll_window_us(frame_rate);
    let start = Instant::now();
    if should_roll_forward(last_pts, target, frame_rate, window) {
        let fps = frames_per_sec(frame_rate);
        let max_roll_frames = ((window as f64 * fps / 1e6).ceil() as u32 * 2).clamp(16, 240);
        let mut decoded = 0u32;
        loop {
            // Rolling into the end of the stream means the target lies past
            // it: the same miss the seek path returns.
            let Some(frame) = decoder.next_frame()? else {
                return Ok(None);
            };
            decoded += 1;
            if frame_covers(frame.pts, target, frame_rate) {
                let elapsed_ms = start.elapsed().as_secs_f64() * 1e3;
                stats.observe_roll_frame(elapsed_ms / f64::from(decoded));
                return Ok(Some(frame));
            }
            if decoded >= max_roll_frames {
                // Walked too far forward on non-advancing or non-monotonic
                // timestamps: seek instead.
                break;
            }
        }
    }
    let found = seek_walk(decoder, target, frame_rate, |_| {})?;
    if found.is_some() {
        stats.observe_seek(start.elapsed().as_secs_f64() * 1e3);
    }
    Ok(found)
}

/// Frames a seek walk decodes past its landing point before it gives up, so a
/// corrupt stream whose timestamps never reach the target ends.
const MAX_SEEK_WALK: u32 = 600;

/// Seek to the sync sample before `target` and decode forward to the first
/// frame covering it, passing each earlier frame to `before` in stream order.
///
/// A seek that lands past `target`, or at the end of the stream, hit a sync
/// sample that cannot start a decode (an open GOP); the walk seeks earlier by
/// [`Backoff`] until it lands at or before `target`, then walks from there.
/// Frames reach `before` only after the final landing, so they are the
/// unbroken run that precedes the returned frame. `Ok(None)` when no frame
/// covers `target` within [`MAX_SEEK_WALK`] frames of the landing point plus
/// the back-off span.
pub(crate) fn seek_walk<D: VideoDecoder + ?Sized>(
    decoder: &mut D,
    target: RationalTime,
    frame_rate: Rational,
    mut before: impl FnMut(VideoFrame),
) -> Result<Option<VideoFrame>, DecodeError> {
    let fps = frames_per_sec(frame_rate);
    let mut backoff = Backoff::new(target);
    let mut walk_limit = MAX_SEEK_WALK;
    decoder.seek(target)?;
    let mut frame = decoder.next_frame()?;
    while frame
        .as_ref()
        .is_none_or(|frame| is_frame_before(target, frame.pts, frame_rate))
    {
        let Some((point, secs)) = backoff.next() else {
            break;
        };
        decoder.seek(point)?;
        walk_limit = MAX_SEEK_WALK.saturating_add((secs as f64 * fps).ceil() as u32);
        frame = decoder.next_frame()?;
    }
    let mut decoded = 0u32;
    while let Some(next) = frame {
        decoded += 1;
        if frame_covers(next.pts, target, frame_rate) {
            return Ok(Some(next));
        }
        if decoded >= walk_limit {
            break;
        }
        before(next);
        frame = decoder.next_frame()?;
    }
    Ok(None)
}

/// Longest back-off, in seconds, for a seek that lands past its target.
const MAX_BACKOFF_SECS: i64 = 64;

/// Earlier seek points for a target whose seek landed past it: one second
/// before it, then two, four, and so on to [`MAX_BACKOFF_SECS`] or the start
/// of the stream, whichever is first.
pub(crate) struct Backoff {
    target: RationalTime,
    secs: i64,
    at_start: bool,
}

impl Backoff {
    pub(crate) fn new(target: RationalTime) -> Self {
        Self {
            target,
            secs: 0,
            at_start: false,
        }
    }

    /// The next earlier seek point and how far before the target it is, or
    /// `None` once the start of the stream or the longest back-off was tried.
    pub(crate) fn next(&mut self) -> Option<(RationalTime, i64)> {
        if self.at_start || self.secs >= MAX_BACKOFF_SECS || !self.target.rate.is_valid() {
            return None;
        }
        self.secs = (self.secs * 2).max(1);
        let rate = self.target.rate;
        // A valid rate has a positive numerator and denominator.
        let (span, den) = (
            i128::from(self.secs) * i128::from(rate.num),
            i128::from(rate.den),
        );
        let back = (span + den - 1) / den;
        let value = i128::from(self.target.value) - back;
        if value <= 0 {
            self.at_start = true;
            if self.target.value <= 0 {
                return None;
            }
            return Some((RationalTime::new(0, rate), self.secs));
        }
        let value = i64::try_from(value).ok()?;
        Some((RationalTime::new(value, rate), self.secs))
    }
}

/// [`VideoDecoder::frame_at_nearest`] with a roll-forward fast path: a
/// target just ahead of the last emitted frame takes the exact walk, while
/// rolling there costs no more than a snap (the nearest window of
/// [`SeekStats`]).
/// Every other target snaps: one seek and one decode, landing on the sync
/// point at or before `target`.
///
/// A target at or past end of stream would otherwise miss (`Ok(None)`),
/// but "nearest" callers expect the tail frame there — containers
/// routinely over-report their frame count by one, so end-of-media
/// targets overshoot EOF in ordinary editing (transition tail frames,
/// the final tick of a clip). Such targets fall back to the last frame
/// before end of stream.
///
/// A snap feeds the snap EMA, never the seek EMA: its cost (one decode)
/// says nothing about a full exact `frame_at`, and folding it into the seek
/// EMA would shrink the roll window that exact calls rely on.
pub fn frame_at_nearest_rolling<D: VideoDecoder + ?Sized>(
    decoder: &mut D,
    last_pts: Option<RationalTime>,
    stats: &mut SeekStats,
    target: RationalTime,
) -> Result<Option<VideoFrame>, DecodeError> {
    let frame_rate = decoder.info().frame_rate;
    if should_roll_forward(
        last_pts,
        target,
        frame_rate,
        stats.nearest_window_us(frame_rate),
    ) {
        if let Some(frame) = frame_at_rolling(decoder, last_pts, stats, target)? {
            return Ok(Some(frame));
        }
        // Rolled off the end of the stream: the target lies past EOF.
    } else {
        let start = Instant::now();
        decoder.seek(target)?;
        if let Some(frame) = decoder.next_frame()? {
            stats.observe_snap(start.elapsed().as_secs_f64() * 1e3);
            return Ok(Some(frame));
        }
        // The seek landed at/past the last sample: no frame to emit.
    }
    seek_back_to_tail(decoder, target, frame_rate)
}

/// Bounded number of one-frame step-backs when a nearest-seek target lies
/// past EOF. One covers the routine over-report-by-one; a few more absorb
/// sloppier containers. Past that the stream is either empty or the target
/// is nowhere near the tail, and a miss is the honest answer.
const MAX_EOF_BACKSTEPS: i64 = 8;

/// Recover the last decodable frame for a target at/past end of stream:
/// step the target back one frame period at a time and re-seek, returning
/// the first frame that decodes. Bounded by [`MAX_EOF_BACKSTEPS`] so an
/// empty or unreadable stream can't loop.
fn seek_back_to_tail<D: VideoDecoder + ?Sized>(
    decoder: &mut D,
    target: RationalTime,
    frame_rate: Rational,
) -> Result<Option<VideoFrame>, DecodeError> {
    let rate = if frame_rate.is_valid() {
        frame_rate
    } else {
        Rational::new(30, 1)
    };
    let start = resample(target, rate).value;
    for step in 1..=MAX_EOF_BACKSTEPS {
        let tick = start - step;
        if tick < 0 {
            break;
        }
        decoder.seek(RationalTime::new(tick, rate))?;
        if let Some(frame) = decoder.next_frame()? {
            return Ok(Some(frame));
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "seek_tests.rs"]
mod tests;
