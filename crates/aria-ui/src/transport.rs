//! Playback transport tick math.
//!
//! The playback clock stores an anchor (tick + wall-clock ms) when play
//! starts and computes the current tick from elapsed time. Slow decode
//! drops frames instead of stretching the playhead.

/// Pixels per second at zoom 1. Must match the timeline ruler.
pub const TIMELINE_BASE_PPS: f32 = 80.0;

/// The widest a second may be drawn: sixteen times base, where a frame at 24fps
/// is 53px and a single frame can be trimmed by eye.
pub const ZOOM_MAX: f32 = 16.0;
/// The narrowest fixed zoom: a tenth of base, where an hour of sequence is
/// 2880px. A sequence too long to fit the view at this zoom lowers the floor to
/// its fit (`AppState::zoom_floor`), so a zoom out always reaches all of it.
pub const ZOOM_MIN: f32 = 0.1;
/// One notch of zoom from a key or a menu row.
pub const ZOOM_STEP: f32 = 1.15;
/// One notch from the wheel. Finer than a key press, because a wheel delivers
/// several notches in the time a key delivers one.
pub const ZOOM_WHEEL_STEP: f32 = 1.1;
/// Pixels one wheel line counts for, where the platform reports lines.
pub const WHEEL_LINE_PX: f32 = 24.0;

/// Zoom that fits `duration_ticks` into `viewport_px` at `base_pps`, at most
/// [`ZOOM_MAX`]. A long sequence fits below [`ZOOM_MIN`].
pub fn fit_zoom(duration_ticks: i64, fps: f32, viewport_px: f32, base_pps: f32) -> f32 {
    let seconds = duration_ticks as f32 / fps.max(1.0);
    if seconds <= f32::EPSILON || base_pps <= f32::EPSILON {
        return 1.0;
    }
    (viewport_px / (seconds * base_pps)).min(ZOOM_MAX)
}

/// `ticks` as px from the timeline's start, at `fps` ticks and `pps` px per
/// second.
pub fn ticks_to_px_f(ticks: f32, fps: f32, pps: f32) -> f32 {
    (ticks / fps.max(1.0)) * pps
}

/// Timeline body width after chrome: window minus side panes, splits, header.
pub fn editor_timeline_viewport_px(
    window_w: f32,
    sidebar_w: f32,
    inspector_w: f32,
    header_w: f32,
) -> f32 {
    (window_w - sidebar_w - inspector_w - 8.0 - header_w).max(160.0)
}

/// Signed playback speed as a rational multiplier of the sequence rate.
///
/// Stored in hundredths (`100 / 100` = 1x, `25 / 100` = 1/4x,
/// `110 / 100` = 1.1x, `-200 / 100` = -2x).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShuttleSpeed {
    pub num: i32,
    pub den: i32,
}

impl ShuttleSpeed {
    pub const STOP: Self = Self::hundredths(0);
    pub const ONE: Self = Self::hundredths(100);
    pub const REVERSE_ONE: Self = Self::hundredths(-100);
    pub const QUARTER: Self = Self::hundredths(25);
    pub const REVERSE_QUARTER: Self = Self::hundredths(-25);
    pub const MIN_HUNDREDTHS: i32 = -800;
    pub const MAX_HUNDREDTHS: i32 = 800;
    pub const MAX_AUDIBLE_HUNDREDTHS: i32 = 400;

    pub const fn new(num: i32, den: i32) -> Self {
        Self {
            num,
            den: if den <= 0 { 1 } else { den },
        }
    }

    pub const fn hundredths(hundredths: i32) -> Self {
        Self {
            num: hundredths,
            den: 100,
        }
    }

    pub const fn steps(steps: i32) -> Self {
        Self {
            num: steps.saturating_mul(100),
            den: 100,
        }
    }

    pub const fn as_hundredths(self) -> i32 {
        if self.den == 100 {
            self.num
        } else {
            let den = if self.den > 1 { self.den } else { 1 };
            ((self.num as i64 * 100) / (den as i64)) as i32
        }
    }

    pub const fn signum(self) -> i32 {
        self.num.signum()
    }

    pub const fn is_audible(self) -> bool {
        let den = if self.den > 1 { self.den } else { 1 };
        self.num != 0 && (self.num as i64).abs() <= 4 * (den as i64)
    }

    pub const fn clamp_range(self) -> Self {
        let h = self.as_hundredths();
        let clamped = if h < Self::MIN_HUNDREDTHS {
            Self::MIN_HUNDREDTHS
        } else if h > Self::MAX_HUNDREDTHS {
            Self::MAX_HUNDREDTHS
        } else {
            h
        };
        Self::hundredths(clamped)
    }

    /// Integer speed reported on plugin `ViewFrame::speed` (`1`, `2`, `4`, `8`, `-1`, ...).
    pub const fn as_plugin_speed(self) -> i32 {
        let h = self.as_hundredths();
        if h.abs() >= 100 { h / 100 } else { h.signum() }
    }

    /// Step shuttle speed by 10% (`10` hundredths) in the sign of `delta_sign`,
    /// crossing zero directly (`0.1x <-> -0.1x`) and clamping to `[-8x, 8x]`.
    pub const fn step_ten_percent(self, delta_sign: i32) -> Self {
        if delta_sign == 0 {
            return self;
        }
        let step = if delta_sign < 0 { -10 } else { 10 };
        let mut next = self.as_hundredths() + step;
        if next == 0 {
            next = step;
        }
        Self::hundredths(next).clamp_range()
    }
}

impl Default for ShuttleSpeed {
    fn default() -> Self {
        Self::ONE
    }
}

impl PartialEq<i32> for ShuttleSpeed {
    fn eq(&self, other: &i32) -> bool {
        i64::from(self.num) == i64::from(*other) * i64::from(self.den.max(1))
    }
}

impl std::fmt::Display for ShuttleSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let h = self.as_hundredths();
        if h % 100 == 0 {
            write!(f, "{}x", h / 100)
        } else {
            let sign = if h < 0 { "-" } else { "" };
            let abs = h.abs();
            if abs % 10 == 0 {
                write!(f, "{sign}{}.{}x", abs / 100, (abs % 100) / 10)
            } else {
                write!(f, "{sign}{}.{:02}x", abs / 100, abs % 100)
            }
        }
    }
}

/// Playhead tick after `elapsed_ms` milliseconds of playback starting at
/// `anchor_tick`, at `fps_num / fps_den` ticks per second, scaled by
/// `speed_num / speed_den` (JKL shuttle; 1/1 is plain playback).
///
/// Negative `speed_num` runs the playhead in reverse. Mid-frame time
/// truncates toward the anchor. Elapsed time clamps at 0. A zero or
/// invalid speed or rate returns the anchor unchanged.
pub fn playback_tick_scaled(
    anchor_tick: i64,
    elapsed_ms: i64,
    fps_num: i32,
    fps_den: i32,
    speed_num: i32,
    speed_den: i32,
) -> i64 {
    if fps_num <= 0 || fps_den <= 0 || speed_num == 0 || speed_den <= 0 {
        return anchor_tick;
    }
    let elapsed_ms = i128::from(elapsed_ms.max(0));
    let ticks = elapsed_ms * i128::from(fps_num) * i128::from(speed_num)
        / (i128::from(fps_den) * i128::from(speed_den) * 1000);
    (i128::from(anchor_tick) + ticks).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// 1x [`playback_tick_scaled`].
pub fn playback_tick(anchor_tick: i64, elapsed_ms: i64, fps_num: i32, fps_den: i32) -> i64 {
    playback_tick_scaled(anchor_tick, elapsed_ms, fps_num, fps_den, 1, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_rate_advances_exactly() {
        assert_eq!(playback_tick(0, 1000, 24, 1), 24);
        assert_eq!(playback_tick(10, 2000, 24, 1), 58);
        assert_eq!(playback_tick(0, 1000, 60, 1), 60);
    }

    #[test]
    fn ntsc_rate_is_exact() {
        assert_eq!(playback_tick(0, 1001, 30000, 1001), 30);
        assert_eq!(playback_tick(0, 3_600_000, 30000, 1001), 107_892);
    }

    #[test]
    fn mid_frame_floors() {
        assert_eq!(playback_tick(0, 41, 24, 1), 0);
        assert_eq!(playback_tick(0, 42, 24, 1), 1);
    }

    #[test]
    fn negative_elapsed_never_rewinds() {
        assert_eq!(playback_tick(100, -1000, 24, 1), 100);
    }

    #[test]
    fn invalid_rate_returns_anchor() {
        assert_eq!(playback_tick(7, 1000, 0, 1), 7);
        assert_eq!(playback_tick(7, 1000, 24, 0), 7);
        assert_eq!(playback_tick(7, 1000, -24, 1), 7);
    }

    #[test]
    fn scaled_speeds_multiply_the_rate() {
        assert_eq!(playback_tick_scaled(0, 1000, 24, 1, 2, 1), 48);
        assert_eq!(playback_tick_scaled(0, 1000, 24, 1, 8, 1), 192);
        assert_eq!(playback_tick_scaled(0, 1000, 24, 1, 1, 2), 12);
    }

    #[test]
    fn reverse_speed_walks_backward() {
        assert_eq!(playback_tick_scaled(100, 1000, 24, 1, -1, 1), 76);
        assert_eq!(playback_tick_scaled(100, 1000, 24, 1, -2, 1), 52);
        assert_eq!(playback_tick_scaled(100, 41, 24, 1, -1, 1), 100);
        assert_eq!(playback_tick_scaled(100, 42, 24, 1, -1, 1), 99);
    }

    #[test]
    fn zero_speed_returns_anchor() {
        assert_eq!(playback_tick_scaled(7, 1000, 24, 1, 0, 1), 7);
        assert_eq!(playback_tick_scaled(7, 1000, 24, 1, 1, 0), 7);
    }

    #[test]
    fn fit_zoom_fills_the_viewport() {
        assert!((fit_zoom(240, 24.0, 800.0, 80.0) - 1.0).abs() < f32::EPSILON);
        assert!((fit_zoom(480, 24.0, 800.0, 80.0) - 0.5).abs() < f32::EPSILON);
        assert_eq!(fit_zoom(0, 24.0, 800.0, 80.0), 1.0);
        assert_eq!(fit_zoom(24, 24.0, 8.0, 80.0), 0.1);
        // Three hours in 800px: below the fixed floor, and not raised to it.
        assert!((fit_zoom(24 * 10_800, 24.0, 800.0, 80.0) - 800.0 / 864_000.0).abs() < 1e-9);
        assert_eq!(fit_zoom(1, 24.0, 800.0, 80.0), ZOOM_MAX);
    }

    #[test]
    fn editor_viewport_subtracts_chrome() {
        assert!((editor_timeline_viewport_px(1440.0, 256.0, 520.0, 132.0) - 524.0).abs() < 0.5);
        assert_eq!(
            editor_timeline_viewport_px(200.0, 256.0, 520.0, 132.0),
            160.0
        );
    }
}
