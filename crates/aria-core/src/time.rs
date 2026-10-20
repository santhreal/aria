//! Time primitives aligned with OpenTimelineIO: exact [`Rational`] rates and
//! [`RationalTime`] positions/durations.
//!
//! A [`RationalTime`] is `value` ticks at `rate` ticks-per-second, so NTSC rates
//! (23.976 = 24000/1001) stay exact and a stream's PTS (`value` in its own
//! time base) is representable without rounding. [`TimeRange`] is half-open
//! `[start, start + duration)` with both endpoints carrying the same rate.

use core::cmp::Ordering;

/// Errors from rational-time arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    /// Two operands carried different rates where the same rate was required.
    #[error("rate mismatch: expected {expected:?}, got {got:?}")]
    RateMismatch { expected: Rational, got: Rational },
    /// Tick arithmetic overflowed `i64`.
    #[error("time arithmetic overflow")]
    Overflow,
}

/// An exact frame rate (or tick rate) as `num/den` units per second.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    pub num: i32,
    pub den: i32,
}

impl Rational {
    pub const FPS_24: Rational = Rational::new(24, 1);
    pub const FPS_23_976: Rational = Rational::new(24000, 1001);
    pub const FPS_25: Rational = Rational::new(25, 1);
    pub const FPS_30: Rational = Rational::new(30, 1);
    pub const FPS_29_97: Rational = Rational::new(30000, 1001);
    pub const FPS_50: Rational = Rational::new(50, 1);
    pub const FPS_60: Rational = Rational::new(60, 1);
    pub const FPS_59_94: Rational = Rational::new(60000, 1001);

    pub const fn new(num: i32, den: i32) -> Self {
        Self { num, den }
    }

    /// Approximate units-per-second as a float (display/UI only).
    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            return 0.0;
        }
        f64::from(self.num) / f64::from(self.den)
    }

    /// Frames in one timecode second: the rate rounded up, so 24000/1001
    /// counts 24 and 30000/1001 counts 30. A tick is one frame at the rate, so
    /// this is the frame field's modulus, not the ticks in a wall-clock second
    /// ([`Rational::as_f64`] is that).
    pub fn nominal_fps(self) -> i64 {
        let den = i64::from(self.den).max(1);
        ((i64::from(self.num) + den - 1) / den).max(1)
    }

    /// Seconds-per-unit as a float.
    pub fn seconds_per_unit(self) -> f64 {
        if self.num == 0 {
            return 0.0;
        }
        f64::from(self.den) / f64::from(self.num)
    }

    pub fn is_valid(self) -> bool {
        self.num > 0 && self.den > 0
    }

    /// This rate in lowest terms, e.g. `600/25` → `24/1`.
    ///
    /// Rate equality in this crate is structural ([`rate_eq`]), but container
    /// timescales are arbitrary (a 24 fps file may report `600/25`). Reducing a
    /// probed rate before it enters the model lets it match the `FPS_*`
    /// constants and any source range authored at the canonical rate. Invalid
    /// rates (non-positive parts) are returned unchanged.
    pub fn reduced(self) -> Rational {
        if !self.is_valid() {
            return self;
        }
        let g = gcd(self.num, self.den);
        Rational::new(self.num / g, self.den / g)
    }

    /// `num : den` in lowest terms, or `None` when a reduced term is outside
    /// `i32`. For ratios computed in `i64`, such as tick counts scaled by a
    /// rate.
    pub fn from_ratio(num: i64, den: i64) -> Option<Rational> {
        let g = gcd_i64(num, den);
        Some(Rational::new(
            i32::try_from(num / g).ok()?,
            i32::try_from(den / g).ok()?,
        ))
    }

    /// `num : den` in lowest terms, with both terms halved until they fit in
    /// `i32`. For speed ratios of two positive tick counts: a span long
    /// enough to overflow `i32` is past the precision a speed is read at, so
    /// it is approximated rather than rejected. Halving stops at 1, so a
    /// positive term never reaches zero.
    pub fn from_ratio_fitted(num: i64, den: i64) -> Rational {
        let g = gcd_i64(num, den);
        let (mut num, mut den) = (num / g, den / g);
        while num > i64::from(i32::MAX) || den > i64::from(i32::MAX) {
            num = (num / 2).max(1);
            den = (den / 2).max(1);
        }
        Rational::new(num as i32, den as i32)
    }
}

/// Greatest common divisor of two `i32`s, never zero so it is safe to divide
/// by. `gcd(i32::MIN, i32::MIN)` is 2³¹, which saturates to `i32::MAX`.
fn gcd(a: i32, b: i32) -> i32 {
    i32::try_from(gcd_i64(i64::from(a), i64::from(b))).unwrap_or(i32::MAX)
}

/// Greatest common divisor of two `i64`s (Euclid), never zero so it is safe to
/// divide by. `gcd_i64(i64::MIN, 0)` is 2⁶³, which saturates to `i64::MAX`.
fn gcd_i64(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    i64::try_from(a.max(1)).unwrap_or(i64::MAX)
}

/// A time position or duration as integer ticks at an exact rate.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RationalTime {
    pub value: i64,
    pub rate: Rational,
}

impl RationalTime {
    pub const fn new(value: i64, rate: Rational) -> Self {
        Self { value, rate }
    }

    pub fn zero(rate: Rational) -> Self {
        Self::new(0, rate)
    }

    /// Position in seconds as a float (display/UI; not for exact comparisons).
    pub fn seconds(self) -> f64 {
        if self.rate.num == 0 {
            return 0.0;
        }
        (self.value as f64) * f64::from(self.rate.den) / f64::from(self.rate.num)
    }

    /// Exact ordering against another time, **even at a different rate** — the
    /// primitive the scrub/playback path uses to ask "is this decoded frame at
    /// or after the requested timeline position?" without float drift.
    ///
    /// Compares `value · rate.den` cross-multiplied (rates are units/second with
    /// positive `num`), in `i128` so it never overflows for real media.
    pub fn compare(self, other: RationalTime) -> Ordering {
        let lhs = i128::from(self.value) * i128::from(self.rate.den) * i128::from(other.rate.num);
        let rhs = i128::from(other.value) * i128::from(other.rate.den) * i128::from(self.rate.num);
        lhs.cmp(&rhs)
    }

    /// This time in whole units of `1 / units_per_second` seconds:
    /// `value · den · units_per_second / num`, exact in `i128`, truncated
    /// toward zero and saturated to `i64`. Platform clocks read time this way:
    /// GStreamer in nanoseconds (`1_000_000_000`), Media Foundation in 100 ns
    /// units (`10_000_000`). A rate that is not [`Rational::is_valid`] gives 0.
    pub fn in_units(self, units_per_second: i64) -> i64 {
        if !self.rate.is_valid() {
            return 0;
        }
        let scaled = i128::from(self.value) * i128::from(self.rate.den);
        let units = match scaled.checked_mul(i128::from(units_per_second)) {
            Some(product) => product / i128::from(self.rate.num),
            None if (scaled < 0) == (units_per_second < 0) => i128::MAX,
            None => i128::MIN,
        };
        units.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }

    /// This time as `(ticks, timescale)` with `seconds = ticks / timescale`,
    /// the shape of CoreMedia's `CMTime`. Exact at timescale `rate.num` when
    /// `value · den` fits `i64`; past that, truncated to microseconds at
    /// timescale `1_000_000` rather than wrapping. A rate term below 1 counts
    /// as 1.
    pub fn as_timescaled(self) -> (i64, i32) {
        let rate = Rational::new(self.rate.num.max(1), self.rate.den.max(1));
        let time = RationalTime::new(self.value, rate);
        match time.at_whole_rate() {
            Ok(whole) => (whole.value, rate.num),
            Err(_) => (time.in_units(1_000_000), 1_000_000),
        }
    }

    /// This time at the whole-number rate `rate.num / 1`: `value * rate.den`
    /// ticks, the same instant at every rate, 30000/1001 included. Fails with
    /// [`TimeError::Overflow`] when the tick count leaves `i64`.
    pub fn at_whole_rate(self) -> Result<RationalTime, TimeError> {
        let value = self
            .value
            .checked_mul(i64::from(self.rate.den))
            .ok_or(TimeError::Overflow)?;
        Ok(RationalTime::new(value, Rational::new(self.rate.num, 1)))
    }
}

/// Half-open range `[start, start + duration)`.
///
/// `start` and `duration` must share the same `rate`; use [`TimeRange::at_rate`]
/// when constructing from tick counts at one rate.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub start: RationalTime,
    pub duration: RationalTime,
}

impl TimeRange {
    pub fn new(start: RationalTime, duration: RationalTime) -> Result<Self, TimeError> {
        check_same_rate(start.rate, duration.rate)?;
        Ok(Self { start, duration })
    }

    /// Build a range from tick counts at a single rate.
    pub fn at_rate(start: i64, duration: i64, rate: Rational) -> Self {
        Self {
            start: RationalTime::new(start, rate),
            duration: RationalTime::new(duration, rate),
        }
    }

    /// Exclusive end tick (`start + duration`) at the range's rate.
    pub fn end_tick(self) -> i64 {
        self.start.value + self.duration.value
    }

    /// Exclusive end as a [`RationalTime`].
    pub fn end(self) -> Result<RationalTime, TimeError> {
        time_add(&self.start, &self.duration)
    }

    pub fn is_empty(self) -> bool {
        self.duration.value <= 0
    }

    /// True if `t` lies within `[start, end)`.
    pub fn contains(self, t: RationalTime) -> Result<bool, TimeError> {
        check_same_rate(self.start.rate, t.rate)?;
        Ok(t.value >= self.start.value && t.value < self.end_tick())
    }

    /// True if the two ranges share at least one tick.
    pub fn overlaps(self, other: Self) -> Result<bool, TimeError> {
        check_same_rate(self.start.rate, other.start.rate)?;
        if self.is_empty() || other.is_empty() {
            return Ok(false);
        }
        Ok(self.start.value < other.end_tick() && other.start.value < self.end_tick())
    }

    /// The overlapping region, if any.
    pub fn intersection(self, other: Self) -> Result<Option<Self>, TimeError> {
        if !self.overlaps(other)? {
            return Ok(None);
        }
        let start = self.start.value.max(other.start.value);
        let end = self.end_tick().min(other.end_tick());
        Ok(Some(Self::at_rate(start, end - start, self.start.rate)))
    }
}

/// Structural equality on the rate pair (no gcd reduction).
pub fn rate_eq(a: Rational, b: Rational) -> bool {
    a.num == b.num && a.den == b.den
}

/// Errors when `actual` does not match `expected`.
pub fn check_same_rate(actual: Rational, expected: Rational) -> Result<(), TimeError> {
    if rate_eq(actual, expected) {
        Ok(())
    } else {
        Err(TimeError::RateMismatch {
            expected,
            got: actual,
        })
    }
}

/// Same-rate addition with checked overflow.
pub fn time_add(a: &RationalTime, b: &RationalTime) -> Result<RationalTime, TimeError> {
    check_same_rate(a.rate, b.rate)?;
    let value = a.value.checked_add(b.value).ok_or(TimeError::Overflow)?;
    Ok(RationalTime::new(value, a.rate))
}

/// Same-rate subtraction with checked overflow.
pub fn time_sub(a: &RationalTime, b: &RationalTime) -> Result<RationalTime, TimeError> {
    check_same_rate(a.rate, b.rate)?;
    let value = a.value.checked_sub(b.value).ok_or(TimeError::Overflow)?;
    Ok(RationalTime::new(value, a.rate))
}

/// Resample `time` to `to`, rounding to the nearest tick.
pub fn resample(time: RationalTime, to: Rational) -> RationalTime {
    if !time.rate.is_valid() || !to.is_valid() {
        return RationalTime::zero(to);
    }
    if rate_eq(time.rate, to) {
        return time;
    }
    let numer = i128::from(time.value) * i128::from(to.num) * i128::from(time.rate.den);
    let denom = i128::from(to.den) * i128::from(time.rate.num);
    if denom == 0 {
        return RationalTime::zero(to);
    }
    let half = denom.abs() / 2;
    let magnitude = (numer.abs() + half) / denom.abs();
    let q = if (numer >= 0) == (denom >= 0) {
        magnitude
    } else {
        -magnitude
    };
    RationalTime::new(q as i64, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R24: Rational = Rational::FPS_24;
    const R30: Rational = Rational::FPS_30;

    fn rt(value: i64, rate: Rational) -> RationalTime {
        RationalTime::new(value, rate)
    }

    #[test]
    fn rational_fps_constants_are_canonical() {
        assert_eq!(Rational::FPS_23_976, Rational::new(24000, 1001));
        assert_eq!(Rational::FPS_29_97, Rational::new(30000, 1001));
        assert_eq!(Rational::FPS_59_94, Rational::new(60000, 1001));
    }

    #[test]
    fn rational_as_f64_approximates_ntsc() {
        assert!((Rational::FPS_23_976.as_f64() - 23.976).abs() < 0.001);
        assert!((Rational::FPS_29_97.as_f64() - 29.97).abs() < 0.001);
    }

    #[test]
    fn rational_is_valid() {
        assert!(Rational::new(24, 1).is_valid());
        assert!(!Rational::new(0, 1).is_valid());
        assert!(!Rational::new(24, 0).is_valid());
        assert!(!Rational::new(-24, 1).is_valid());
    }

    #[test]
    fn rational_reduced_canonicalizes_container_timescales() {
        // A 24 fps file commonly reports its rate against a 600 timescale.
        assert_eq!(Rational::new(600, 25).reduced(), R24);
        assert_eq!(Rational::new(60, 2).reduced(), R30);
        assert_eq!(Rational::new(24000, 1000).reduced(), R24);
        // Already-canonical NTSC rates are coprime and survive untouched.
        assert_eq!(Rational::FPS_29_97.reduced(), Rational::FPS_29_97);
        assert_eq!(R24.reduced(), R24);
        // Invalid rates pass through rather than dividing by a zero gcd.
        assert_eq!(Rational::new(24, 0).reduced(), Rational::new(24, 0));
    }

    #[test]
    fn rational_time_seconds() {
        assert!((rt(48, R24).seconds() - 2.0).abs() < f64::EPSILON);
        // One 29.97 frame is value=1 at rate 30000/1001 -> exactly 1001/30000 s.
        assert!((rt(1, Rational::FPS_29_97).seconds() - 1001.0 / 30000.0).abs() < 1e-12);
    }

    #[test]
    fn compare_is_exact_across_rates() {
        // 1 second expressed two ways must compare equal.
        assert_eq!(rt(24, R24).compare(rt(30, R30)), Ordering::Equal);
        // One 24fps frame (1/24 s) is later than one 30fps frame (1/30 s).
        assert_eq!(rt(1, R24).compare(rt(1, R30)), Ordering::Greater);
        assert_eq!(rt(1, R30).compare(rt(1, R24)), Ordering::Less);
    }

    /// `at_whole_rate` sets the time a plugin encoder receives, so it must
    /// keep the instant for fractional rates, not only for `n/1` ones.
    #[test]
    fn at_whole_rate_keeps_the_instant() {
        for (value, rate) in [
            (0, R24),
            (48, R24),
            (7, Rational::FPS_23_976),
            (300, Rational::FPS_29_97),
            (-5, Rational::FPS_59_94),
            (96_000, Rational::new(48_000, 1)),
        ] {
            let time = rt(value, rate);
            let whole = time.at_whole_rate().expect("in range");
            assert_eq!(whole.rate, Rational::new(rate.num, 1));
            assert_eq!(whole.value, value * i64::from(rate.den));
            assert_eq!(
                whole.compare(time),
                Ordering::Equal,
                "{time:?} -> {whole:?}"
            );
        }
        assert_eq!(
            rt(i64::MAX / 1000, Rational::FPS_29_97).at_whole_rate(),
            Err(TimeError::Overflow)
        );
    }

    #[test]
    fn time_add_same_rate() {
        let sum = time_add(&rt(30, R30), &rt(45, R30)).unwrap();
        assert_eq!(sum.value, 75);
        assert_eq!(sum.rate, R30);
    }

    #[test]
    fn time_add_rate_mismatch_errors() {
        // time_add(a, b) checks check_same_rate(a.rate, b.rate): got = a.rate
        // (what was found), expected = b.rate.
        assert_eq!(
            time_add(&rt(1, R30), &rt(1, R24)),
            Err(TimeError::RateMismatch {
                expected: R24,
                got: R30,
            })
        );
    }

    #[test]
    fn time_add_overflow_is_checked() {
        assert_eq!(
            time_add(&rt(i64::MAX, R30), &rt(1, R30)),
            Err(TimeError::Overflow)
        );
    }

    #[test]
    fn resample_rounds_to_nearest_tick() {
        // 1 tick at 24fps -> nearest 30fps tick is 1 (1.25 rounds to 1).
        assert_eq!(resample(rt(1, R24), R30), rt(1, R30));
        // 2 ticks at 24fps (1/12 s) -> 2.5 at 30fps, rounds to 3 (half-up).
        assert_eq!(resample(rt(2, R24), R30), rt(3, R30));
        // Same rate is identity.
        assert_eq!(resample(rt(7, R30), R30), rt(7, R30));
    }

    #[test]
    fn resample_handles_negative_and_degenerate() {
        assert_eq!(resample(rt(-2, R24), R30).value, -3);
        assert_eq!(resample(rt(5, Rational::new(0, 1)), R30), rt(0, R30));
    }

    #[test]
    fn time_range_contains_and_overlaps() {
        let a = TimeRange::at_rate(0, 30, R30);
        assert!(a.contains(rt(0, R30)).unwrap());
        assert!(a.contains(rt(29, R30)).unwrap());
        assert!(!a.contains(rt(30, R30)).unwrap()); // half-open
        let b = TimeRange::at_rate(15, 30, R30);
        assert!(a.overlaps(b).unwrap());
        let c = TimeRange::at_rate(30, 30, R30);
        assert!(!a.overlaps(c).unwrap());
    }

    #[test]
    fn time_range_intersection() {
        let a = TimeRange::at_rate(0, 30, R30);
        let b = TimeRange::at_rate(15, 30, R30);
        assert_eq!(
            a.intersection(b).unwrap(),
            Some(TimeRange::at_rate(15, 15, R30))
        );
        let c = TimeRange::at_rate(100, 10, R30);
        assert_eq!(a.intersection(c).unwrap(), None);
    }

    #[test]
    fn gcd_handles_i32_min_without_overflow() {
        assert_eq!(gcd(i32::MIN, 1), 1);
        assert_eq!(gcd(1, i32::MIN), 1);
        assert_eq!(gcd(i32::MIN, i32::MIN), i32::MAX);
        assert_eq!(gcd(i32::MIN, 2), 2);
        assert_eq!(gcd(0, 0), 1);
        assert_eq!(gcd_i64(i64::MIN, 0), i64::MAX);
        assert_eq!(gcd_i64(i64::MIN, 4), 4);
        assert_eq!(gcd_i64(0, 0), 1);
        assert_eq!(gcd_i64(-9, 6), 3);
    }

    #[test]
    fn in_units_is_exact_truncates_toward_zero_and_saturates() {
        const NS: i64 = 1_000_000_000;
        const HNS: i64 = 10_000_000;
        assert_eq!(rt(0, R24).in_units(NS), 0);
        assert_eq!(rt(24, R24).in_units(NS), NS);
        assert_eq!(rt(1, Rational::FPS_29_97).in_units(NS), 33_366_666);
        assert_eq!(rt(-1, Rational::FPS_29_97).in_units(NS), -33_366_666);
        // An hour on a nanosecond clock overflows `i64` once multiplied by
        // 1e9, so the product has to be taken in `i128`.
        let hour_ns = 3_600 * NS;
        assert_eq!(
            rt(hour_ns, Rational::new(1_000_000_000, 1)).in_units(NS),
            hour_ns
        );
        assert_eq!(
            rt(3_600 * 48_000, Rational::new(48_000, 1)).in_units(NS),
            hour_ns
        );
        // Already on the 100 ns base: unchanged.
        assert_eq!(rt(3, Rational::new(10_000_000, 1)).in_units(HNS), 3);
        assert_eq!(rt(30, R30).in_units(HNS), HNS);
        // 5/30 s is 1_666_666.67 hns: truncated toward zero on both sides.
        assert_eq!(rt(5, R30).in_units(HNS), 1_666_666);
        assert_eq!(rt(-5, R30).in_units(HNS), -1_666_666);
        // NTSC: 30 frames at 30000/1001 is 1.001 s.
        assert_eq!(rt(30, Rational::FPS_29_97).in_units(HNS), 10_010_000);
        // Past `i64`, and past `i128` before the division: saturated.
        assert_eq!(rt(i64::MAX, Rational::new(1, 1)).in_units(NS), i64::MAX);
        assert_eq!(rt(i64::MIN, Rational::new(1, 1)).in_units(NS), i64::MIN);
        assert_eq!(
            rt(i64::MAX, Rational::new(1, i32::MAX)).in_units(i64::MAX),
            i64::MAX
        );
        assert_eq!(
            rt(i64::MIN, Rational::new(1, i32::MAX)).in_units(i64::MAX),
            i64::MIN
        );
        // No rate, no time.
        assert_eq!(rt(24, Rational::new(0, 1)).in_units(NS), 0);
        assert_eq!(rt(24, Rational::new(-24, 1)).in_units(NS), 0);
        assert_eq!(rt(24, Rational::new(24, 0)).in_units(NS), 0);
        assert_eq!(rt(24, Rational::new(24, -1)).in_units(NS), 0);
    }

    #[test]
    fn as_timescaled_is_exact_until_value_times_den_overflows() {
        assert_eq!(rt(48, R24).as_timescaled(), (48, 24));
        assert_eq!(
            rt(30, Rational::FPS_29_97).as_timescaled(),
            (30_030, 30_000)
        );
        let half_max = i64::MAX / 2;
        assert_eq!(rt(half_max, R30).as_timescaled(), (half_max, 30));
        // `half_max · 3` leaves `i64`: microseconds instead, still positive and
        // still the same instant to the microsecond.
        let (value, timescale) = rt(half_max, Rational::new(30, 3)).as_timescaled();
        assert_eq!(timescale, 1_000_000);
        assert_eq!(
            value,
            rt(half_max, Rational::new(30, 3)).in_units(1_000_000)
        );
        assert!(value > 0);
        // An invalid rate is read as 1 per term instead of dividing by zero.
        assert_eq!(rt(5, Rational::new(0, 0)).as_timescaled(), (5, 1));
    }

    /// A ratio of two tick counts reduces to the rate it names, and a term
    /// that cannot fit an `i32` is rejected by [`Rational::from_ratio`] and
    /// halved by [`Rational::from_ratio_fitted`], whose result keeps the ratio.
    #[test]
    fn a_tick_ratio_reduces_and_an_oversized_one_is_rejected_or_halved() {
        assert_eq!(Rational::from_ratio(600, 25), Some(R24));
        assert_eq!(Rational::from_ratio_fitted(600, 25), R24);
        assert_eq!(
            Rational::from_ratio(48_048, 48_000),
            Some(Rational::new(1001, 1000))
        );
        assert_eq!(Rational::from_ratio(0, 5), Some(Rational::new(0, 1)));
        assert_eq!(Rational::from_ratio(-6, 4), Some(Rational::new(-3, 2)));

        let over = i64::from(i32::MAX) + 1;
        assert_eq!(Rational::from_ratio(over, 1), None);
        assert_eq!(Rational::from_ratio(1, over), None);
        assert_eq!(Rational::from_ratio(over * 2, 2), None);
        assert_eq!(Rational::from_ratio(i64::from(i32::MIN) - 1, 1), None);
        assert_eq!(
            Rational::from_ratio(i64::from(i32::MAX), 1),
            Some(Rational::new(i32::MAX, 1))
        );
        // Reduction brings a large pair back into range.
        assert_eq!(
            Rational::from_ratio(i64::from(i32::MAX) * 4, 8),
            Some(Rational::new(i32::MAX, 2))
        );

        // 2^32 : 1 halves twice to 2^30 : 1, the first power of two that fits.
        assert_eq!(
            Rational::from_ratio_fitted(1 << 32, 1),
            Rational::new(1 << 30, 1)
        );
        // 3 * 2^32 : 2^32 reduces to 3 : 1 before any halving.
        assert_eq!(
            Rational::from_ratio_fitted(3 << 32, 1 << 32),
            Rational::new(3, 1)
        );
        // Coprime and past `i32`: both terms halve together until both fit.
        assert_eq!(
            Rational::from_ratio_fitted(6_000_000_001, 4_000_000_000),
            Rational::new(1_500_000_000, 1_000_000_000)
        );
        // A term halved past one is held at one instead of becoming a zero rate.
        assert_eq!(
            Rational::from_ratio_fitted(over * 4, 3),
            Rational::new(1 << 30, 1)
        );
        assert_eq!(
            Rational::from_ratio_fitted(7, over),
            Rational::new(3, 1 << 30)
        );
        // Two large terms of similar size keep the ratio to well under a
        // millionth, which is the case a speed from two tick counts is.
        let (num, den) = (over * 3, over + 1);
        let r = Rational::from_ratio_fitted(num, den);
        let want = num as f64 / den as f64;
        assert!(
            (r.as_f64() - want).abs() / want < 1e-6,
            "{num}:{den} -> {r:?}"
        );
    }
}
