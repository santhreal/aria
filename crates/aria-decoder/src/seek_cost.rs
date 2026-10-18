//! Per-decoder seek cost model: how far forward rolling beats seeking.
//!
//! ## Exact roll window
//!
//! How far forward rolling beats seeking depends entirely on the source: a
//! seek costs a keyframe-prefix re-decode (GOP length × per-frame cost),
//! rolling costs one decode per frame skipped. With ~8-second GOPs (common in
//! downloaded/CDN 4K) a fixed 1-second window turns a 30-frame forward drag
//! into a multi-second GOP re-decode; with all-keyframe content rolling more
//! than a couple frames is pure waste. So each decoder carries a [`SeekStats`]
//! cost model: EMAs of the observed seek-path cost and the per-frame roll
//! cost, measured by [`crate::frame_at_rolling`] itself. The window is
//! the break-even point `seek_cost / frame_cost` (in frames, converted through
//! the frame rate), clamped between two frame periods and
//! [`MAX_ROLL_WINDOW_SECS`]. Cold-start defaults reproduce the old fixed
//! window (~1 s at 30 fps) until real measurements arrive.
//!
//! Measured (`scrub_bench`, 4K H.264 fragmented MP4 with ~8 s GOPs, Intel
//! UHD 630, zero-copy GPU output, mean/p95 per target): a +30-frame forward
//! drag — 1.001 s at 29.97 fps, one tick past the old fixed window — went
//! from 449/1 239 ms to 175/238 ms; the CPU path from 1 530/4 282 ms to
//! 646/1 185 ms. Small steps (+2, +8) and the never-rolling patterns
//! (backward, random) are unchanged.
//!
//! ## Nearest roll window
//!
//! [`crate::frame_at_nearest_rolling`] competes with a different
//! alternative: a snap, one seek plus one decode, with no GOP-prefix walk.
//! Its window is the break-even `snap_cost / frame_cost`, never wider than
//! the exact window. Until both a snap and a rolled frame are observed the
//! snap is assumed to cost [`SNAP_PRIOR_FRAMES`] rolled frames: a decoder
//! that only ever snaps would otherwise divide its measured snap by the
//! 10 ms frame default, never roll, and never measure a frame. Measured on
//! `h264-1080p60` (1 s GOP, i7-11700K CPU decode): a snap costs 12 ms and a
//! rolled frame 1.15 ms, while the exact window had grown past 3 s, so a
//! 90-frame nearest jump rolled 90 frames (103 ms) where a snap costs 12 ms.
//! With the snap divided by the frame default, the renderer's 3-frame
//! interactive scrub snapped every step: 12.7 ms against 5.0 ms rolled.

use aria_core::Rational;

/// Upper bound on the roll window: a pathological cost estimate (huge seek
/// EMA, tiny frame EMA) must never make the decoder crawl forever instead of
/// seeking.
pub(crate) const MAX_ROLL_WINDOW_SECS: f64 = 10.0;

/// EMA smoothing factor: heavy enough on the newest sample to track a
/// scene-complexity change within a few observations, light enough that one
/// outlier (cache-cold seek, page fault) doesn't swing the window.
const EMA_ALPHA: f64 = 0.3;

/// Frame rate assumed for the window when the source doesn't report one.
const FALLBACK_FPS: f64 = 30.0;

/// Snap cost, in rolled frames, assumed before a decoder observes a snap.
pub(crate) const SNAP_PRIOR_FRAMES: f64 = 8.0;

/// Per-decoder cost model for the adaptive roll-forward windows: EMAs of the
/// observed seek cost, snap cost, and per-frame decode cost, updated by
/// [`crate::frame_at_rolling`] and
/// [`crate::frame_at_nearest_rolling`] from their own wall-clock
/// measurements. Backends keep one next to `last_pts` and hand it back on
/// every `frame_at` and `frame_at_nearest`.
///
/// The defaults (300 ms per seek, 10 ms per frame) reproduce the previous
/// fixed 1-second window at 30 fps until real measurements replace them.
#[derive(Debug, Clone, Copy)]
pub struct SeekStats {
    /// EMA of a full seek-path `frame_at`: `seek()` plus the keyframe-prefix
    /// walk to the covering frame, in milliseconds.
    seek_ms: f64,
    /// EMA of one `next_frame` on the roll path, in milliseconds.
    frame_ms: f64,
    /// Whether `frame_ms` holds a measurement rather than the default.
    frame_observed: bool,
    /// EMA of a nearest snap, `seek()` plus one `next_frame`, in
    /// milliseconds; `None` until the first snap.
    snap_ms: Option<f64>,
}

impl Default for SeekStats {
    fn default() -> Self {
        Self {
            seek_ms: 300.0,
            frame_ms: 10.0,
            frame_observed: false,
            snap_ms: None,
        }
    }
}

impl SeekStats {
    pub(crate) fn observe_seek(&mut self, ms: f64) {
        self.seek_ms += EMA_ALPHA * (ms - self.seek_ms);
    }

    /// The first measurement replaces the 10 ms default outright, as the
    /// first snap replaces the snap prior: blended with the default, one
    /// 1.15 ms frame reads as 7.3 ms, an 11 ms snap reads as 1.5 frames,
    /// and the nearest window shuts to two frames before it ever learns.
    pub(crate) fn observe_roll_frame(&mut self, ms: f64) {
        if self.frame_observed {
            self.frame_ms += EMA_ALPHA * (ms - self.frame_ms);
        } else {
            self.frame_ms = ms;
            self.frame_observed = true;
        }
    }

    pub(crate) fn observe_snap(&mut self, ms: f64) {
        self.snap_ms = Some(match self.snap_ms {
            Some(snap) => snap + EMA_ALPHA * (ms - snap),
            None => ms,
        });
    }

    /// The exact break-even roll window in microseconds: rolling `n` frames
    /// costs `n × frame_ms`, so roll while that stays at or under one seek —
    /// `seek_ms / frame_ms` frames, converted to time through the frame rate.
    /// Clamped to at least two frame periods (sequential playback must always
    /// roll) and at most [`MAX_ROLL_WINDOW_SECS`].
    pub(crate) fn roll_window_us(&self, frame_rate: Rational) -> i64 {
        window_us(self.seek_ms / self.frame_ms, frame_rate)
    }

    /// The nearest roll window in microseconds: roll while `n × frame_ms`
    /// stays at or under one snap, and never past [`Self::roll_window_us`].
    /// [`SNAP_PRIOR_FRAMES`] until both costs are measured.
    pub(crate) fn nearest_window_us(&self, frame_rate: Rational) -> i64 {
        let snap_frames = match self.snap_ms {
            Some(snap) if self.frame_observed => snap / self.frame_ms,
            _ => SNAP_PRIOR_FRAMES,
        };
        window_us(snap_frames, frame_rate).min(self.roll_window_us(frame_rate))
    }
}

/// `frames` of break-even converted to microseconds at `frame_rate`, clamped
/// to two frame periods and [`MAX_ROLL_WINDOW_SECS`].
fn window_us(frames: f64, frame_rate: Rational) -> i64 {
    let fps = frames_per_sec(frame_rate);
    let secs = (frames.max(0.0) / fps).clamp(2.0 / fps, MAX_ROLL_WINDOW_SECS);
    (secs * 1e6) as i64
}

/// Frames per second of `frame_rate`, [`FALLBACK_FPS`] without a valid,
/// positive rate.
pub(crate) fn frames_per_sec(frame_rate: Rational) -> f64 {
    let fps = f64::from(frame_rate.num) / f64::from(frame_rate.den);
    if frame_rate.is_valid() && fps.is_finite() && fps > 0.0 {
        fps
    } else {
        FALLBACK_FPS
    }
}

#[cfg(test)]
#[path = "seek_cost_tests.rs"]
mod tests;
