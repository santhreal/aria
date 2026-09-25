//! What playback actually achieved against the rate it was asked for.
//!
//! A frame that takes longer than its share of a second is a frame the player
//! could not deliver on time: the picture either repeats or the clock slips.
//! Neither is visible from [`FrameStats`](crate::FrameStats), which reports how
//! one frame divided its cost and says nothing about whether the run kept up.
//!
//! One owner for that question, so the preview HUD, a measured run, and a
//! report on the operator's own machine all count the same way: a frame is
//! late when it exceeds the target's budget, the achieved rate is frames over
//! the wall time they took, and both come from the same recorded durations.

use std::time::Duration;

use aria_core::Rational;

/// Frame timings measured against a target rate.
///
/// Cheap enough to live in a playback loop: one add and two comparisons per
/// frame, no allocation, no clock of its own — the caller times the frame it
/// just rendered, because only the caller knows where the frame began.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackMeter {
    /// A frame's share of a second at the target rate.
    budget: Duration,
    /// The target, kept for the report so a reader sees both numbers.
    target_fps: f64,
    frames: u32,
    late: u32,
    worst: Duration,
    total: Duration,
}

impl PlaybackMeter {
    /// A meter for playback at `rate`.
    ///
    /// A non-positive or absurd rate would make every frame late or none, so
    /// the budget falls back to 1/24 s — the same floor the report shows as its
    /// target, rather than silently measuring against infinity.
    pub fn at_rate(rate: Rational) -> Self {
        let fps = rate.as_f64();
        let fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            24.0
        };
        Self {
            budget: Duration::from_secs_f64(1.0 / fps),
            target_fps: fps,
            frames: 0,
            late: 0,
            worst: Duration::ZERO,
            total: Duration::ZERO,
        }
    }

    /// A frame's share of a second at the target rate.
    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// Record one delivered frame, which took `took` to produce.
    pub fn frame(&mut self, took: Duration) {
        self.frames += 1;
        self.total += took;
        if took > self.worst {
            self.worst = took;
        }
        if took > self.budget {
            self.late += 1;
        }
    }

    /// Forget what was measured, keeping the target. Used when playback stops
    /// and starts again: the previous run's drops are not this run's.
    pub fn reset(&mut self) {
        self.frames = 0;
        self.late = 0;
        self.worst = Duration::ZERO;
        self.total = Duration::ZERO;
    }

    /// What the run achieved so far.
    pub fn report(&self) -> PlaybackReport {
        let seconds = self.total.as_secs_f64();
        PlaybackReport {
            frames: self.frames,
            late: self.late,
            target_fps: self.target_fps,
            achieved_fps: if seconds > 0.0 {
                f64::from(self.frames) / seconds
            } else {
                0.0
            },
            worst_ms: self.worst.as_secs_f64() * 1000.0,
        }
    }
}

/// A measured playback run: the rate it kept and the frames it missed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackReport {
    /// Frames delivered.
    pub frames: u32,
    /// Of those, the ones that took longer than the target's budget.
    pub late: u32,
    /// The rate playback was asked for.
    pub target_fps: f64,
    /// Frames over the time they took to produce.
    pub achieved_fps: f64,
    /// The slowest single frame, in milliseconds.
    pub worst_ms: f64,
}

impl PlaybackReport {
    /// Nothing measured yet: an empty run is not a failing one.
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    /// The share of frames that missed the budget, 0.0 to 1.0.
    pub fn late_share(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            f64::from(self.late) / f64::from(self.frames)
        }
    }

    /// Whether the run kept the rate it was asked for, allowing `late_share`
    /// up to `tolerance`.
    ///
    /// Both halves matter: a run can average the target rate and still stutter
    /// on a handful of frames, and a run with no late frame at all can still be
    /// below rate because every frame sat just under the budget.
    pub fn keeps_up(&self, tolerance: f64) -> bool {
        !self.is_empty() && self.achieved_fps >= self.target_fps && self.late_share() <= tolerance
    }

    /// One line for a HUD or a log: rate, target, and what was dropped.
    pub fn label(&self) -> String {
        if self.is_empty() {
            return "no frames measured".to_string();
        }
        format!(
            "{:.1} of {:.0} fps, {} of {} frames late, worst {:.1} ms",
            self.achieved_fps, self.target_fps, self.late, self.frames, self.worst_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame inside its budget is not late; a frame past it is. The boundary
    /// is exact so a frame exactly on budget counts as kept.
    #[test]
    fn a_frame_is_late_only_past_its_budget() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        let budget = meter.budget();
        meter.frame(budget / 2);
        meter.frame(budget);
        meter.frame(budget + Duration::from_millis(5));
        let report = meter.report();
        assert_eq!(report.frames, 3);
        assert_eq!(report.late, 1);
        assert!((report.late_share() - 1.0 / 3.0).abs() < 1e-9);
    }

    /// The achieved rate is frames over the time they took, so half-budget
    /// frames read as twice the target.
    #[test]
    fn the_achieved_rate_is_frames_over_the_time_they_took() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        let half = meter.budget() / 2;
        for _ in 0..24 {
            meter.frame(half);
        }
        let report = meter.report();
        assert!(
            (report.achieved_fps - 48.0).abs() < 0.5,
            "{}",
            report.label()
        );
        assert_eq!(report.late, 0);
        assert!(report.keeps_up(0.0), "{}", report.label());
    }

    /// Averaging the target is not keeping it: a run that stutters fails the
    /// tolerance even when its mean rate is fine.
    #[test]
    fn a_run_that_stutters_does_not_keep_up() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        let budget = meter.budget();
        for _ in 0..18 {
            meter.frame(budget / 4);
        }
        for _ in 0..2 {
            meter.frame(budget * 3);
        }
        let report = meter.report();
        assert!(
            report.achieved_fps > report.target_fps,
            "{}",
            report.label()
        );
        assert!(!report.keeps_up(0.05), "{}", report.label());
        assert!(report.keeps_up(0.2), "{}", report.label());
        assert!(report.worst_ms > 100.0, "{}", report.label());
    }

    /// An empty run reports nothing rather than a division by zero, and does not
    /// claim to keep up.
    #[test]
    fn an_empty_run_reports_nothing() {
        let report = PlaybackMeter::at_rate(Rational::FPS_30).report();
        assert!(report.is_empty());
        assert_eq!(report.achieved_fps, 0.0);
        assert_eq!(report.late_share(), 0.0);
        assert!(!report.keeps_up(1.0));
        assert_eq!(report.label(), "no frames measured");
    }

    /// A stopped and restarted run is a new measurement, keeping the target.
    #[test]
    fn a_reset_forgets_the_run_and_keeps_the_target() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_30);
        meter.frame(Duration::from_millis(500));
        meter.reset();
        let report = meter.report();
        assert!(report.is_empty());
        assert_eq!(report.worst_ms, 0.0);
        assert!((report.target_fps - 30.0).abs() < 1e-9);
    }

    /// A rate that cannot be played against falls back to 24, so a zero-rate
    /// sequence is measured rather than dividing by nothing.
    #[test]
    fn an_impossible_rate_falls_back_to_the_default() {
        let meter = PlaybackMeter::at_rate(Rational::new(0, 1));
        assert!((meter.report().target_fps - 24.0).abs() < 1e-9);
        assert_eq!(meter.budget(), Duration::from_secs_f64(1.0 / 24.0));
    }
}
