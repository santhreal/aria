//! What playback actually achieved against the rate it was asked for.
//!
//! A frame that takes longer than its share of a second is a frame the player
//! could not deliver on time: the picture either repeats or the clock slips.
//! Neither is visible from [`FrameStats`](crate::FrameStats), which reports how
//! one frame divided its cost and says nothing about whether the run kept up.
//!
//! One owner for that question, so the preview HUD, a measured run, and a
//! report on the operator's own machine all count the same way: a render is
//! late when it exceeds the target's budget, the render rate is frames over the
//! time they took to render, the achieved rate is frames shown over the run's
//! time, and a frame that was due and not shown is dropped with a
//! [`DropCause`].

use std::time::Duration;

use aria_core::Rational;

use crate::FrameStats;

/// Why a frame due on screen was not shown at its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropCause {
    /// The frame was not rendered in time, and decoding took most of the last
    /// render.
    DecodeLate,
    /// The frame was not rendered in time, and rasterizing and compositing took
    /// most of the last render.
    RenderLate,
    /// The frame was rendered in time, and the display reached its time late.
    PresentLate,
}

impl DropCause {
    /// Every cause, in the order a report lists them.
    pub const ALL: [Self; 3] = [Self::DecodeLate, Self::RenderLate, Self::PresentLate];

    /// The cause as a log line and a report print it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::DecodeLate => "decode late",
            Self::RenderLate => "render late",
            Self::PresentLate => "present late",
        }
    }

    /// The cause of a frame that was not rendered in time, from the stage split
    /// of the most recent render: decode when decoding cost at least as much as
    /// rasterizing and compositing together, else render.
    pub fn not_ready(stats: &FrameStats) -> Self {
        if stats.decode_ms >= stats.scaled_cost_ms() {
            Self::DecodeLate
        } else {
            Self::RenderLate
        }
    }
}

impl std::fmt::Display for DropCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Frames that were due on screen and not shown, counted by cause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DroppedFrames {
    pub decode_late: u32,
    pub render_late: u32,
    pub present_late: u32,
}

impl DroppedFrames {
    /// Count `frames` more drops of `cause`.
    pub fn record(&mut self, cause: DropCause, frames: u32) {
        let slot = match cause {
            DropCause::DecodeLate => &mut self.decode_late,
            DropCause::RenderLate => &mut self.render_late,
            DropCause::PresentLate => &mut self.present_late,
        };
        *slot = slot.saturating_add(frames);
    }

    /// Drops of one cause.
    pub const fn count(&self, cause: DropCause) -> u32 {
        match cause {
            DropCause::DecodeLate => self.decode_late,
            DropCause::RenderLate => self.render_late,
            DropCause::PresentLate => self.present_late,
        }
    }

    /// Drops of every cause.
    pub const fn total(&self) -> u32 {
        self.decode_late
            .saturating_add(self.render_late)
            .saturating_add(self.present_late)
    }

    /// No frame dropped.
    pub const fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Add another count to this one.
    pub fn add(&mut self, other: Self) {
        for cause in DropCause::ALL {
            self.record(cause, other.count(cause));
        }
    }
}

/// One playback run measured against a target rate: the frames shown, the
/// frames dropped, and what the shown frames cost to render.
///
/// Cheap enough to live in a playback loop: a few adds per frame, no
/// allocation, no clock of its own. The caller supplies every duration,
/// because only the caller knows where a render began and when the run did.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackMeter {
    /// A frame's share of a second at the target rate.
    budget: Duration,
    /// The target, kept for the report so a reader sees both numbers.
    target_fps: f64,
    /// Frames rendered for the run, with the time each took.
    frames: u32,
    late: u32,
    worst: Duration,
    total: Duration,
    /// Frames put on screen, and how far into the run the last one went up.
    presented: u32,
    shown_until: Duration,
    dropped: DroppedFrames,
}

impl PlaybackMeter {
    /// A meter for playback at `rate`.
    ///
    /// A non-positive or absurd rate would make every frame late or none, so
    /// the budget falls back to 1/24 s, the same floor the report shows as its
    /// target, rather than measuring against infinity.
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
            presented: 0,
            shown_until: Duration::ZERO,
            dropped: DroppedFrames::default(),
        }
    }

    /// A frame's share of a second at the target rate.
    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// Record one rendered frame, which took `took` to produce.
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

    /// Record a frame put on screen `at` into the run.
    pub fn present(&mut self, at: Duration) {
        self.presented = self.presented.saturating_add(1);
        self.shown_until = self.shown_until.max(at);
    }

    /// Record frames that were due and not shown.
    pub fn drop_frames(&mut self, dropped: DroppedFrames) {
        self.dropped.add(dropped);
    }

    /// Forget what was measured, keeping the target. Used when playback stops
    /// and starts again: the previous run's drops are not this run's.
    pub fn reset(&mut self) {
        *self = Self {
            budget: self.budget,
            target_fps: self.target_fps,
            ..Self::at_rate(Rational::FPS_24)
        };
    }

    /// What the run achieved so far.
    pub fn report(&self) -> PlaybackReport {
        let rendering = self.total.as_secs_f64();
        let showing = self.shown_until.as_secs_f64();
        PlaybackReport {
            frames: self.frames,
            late: self.late,
            target_fps: self.target_fps,
            render_fps: if rendering > 0.0 {
                f64::from(self.frames) / rendering
            } else {
                0.0
            },
            worst_ms: self.worst.as_secs_f64() * 1000.0,
            presented: self.presented,
            achieved_fps: if showing > 0.0 {
                f64::from(self.presented) / showing
            } else {
                0.0
            },
            dropped: self.dropped,
        }
    }
}

/// A measured playback run: the rate it showed, the frames it dropped, and what
/// its frames cost to render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackReport {
    /// Frames rendered for the run.
    pub frames: u32,
    /// Of those, the ones that took longer than the target's budget.
    pub late: u32,
    /// The rate playback was asked for.
    pub target_fps: f64,
    /// Rendered frames over the time they took to render.
    pub render_fps: f64,
    /// The slowest single render, in milliseconds.
    pub worst_ms: f64,
    /// Frames put on screen.
    pub presented: u32,
    /// Frames shown over the run's time up to the last one shown.
    pub achieved_fps: f64,
    /// Frames that were due and not shown, by cause.
    pub dropped: DroppedFrames,
}

impl PlaybackReport {
    /// Nothing rendered or shown yet: an empty run is not a failing one.
    pub fn is_empty(&self) -> bool {
        self.frames == 0 && self.presented == 0 && self.dropped.is_empty()
    }

    /// The share of rendered frames that missed the budget, 0.0 to 1.0.
    pub fn late_share(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            f64::from(self.late) / f64::from(self.frames)
        }
    }

    /// Whether rendering kept the rate it was asked for, allowing `late_share`
    /// up to `tolerance`.
    ///
    /// Both halves count: a run can average the target rate and still stutter
    /// on a handful of frames, and a run with no late frame at all can still be
    /// below rate because every frame sat just under the budget.
    pub fn keeps_up(&self, tolerance: f64) -> bool {
        self.frames > 0 && self.render_fps >= self.target_fps && self.late_share() <= tolerance
    }

    /// One line for a HUD or a log: rate, target, drops, and render cost.
    pub fn label(&self) -> String {
        if self.is_empty() {
            return "no frames measured".to_string();
        }
        format!(
            "{:.1} of {:.0} fps, {} dropped ({} decode late, {} render late, {} present late), \
             {} of {} renders over budget, worst {:.1} ms",
            self.achieved_fps,
            self.target_fps,
            self.dropped.total(),
            self.dropped.decode_late,
            self.dropped.render_late,
            self.dropped.present_late,
            self.late,
            self.frames,
            self.worst_ms
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

    /// The render rate is frames over the time they took, so half-budget
    /// frames read as twice the target.
    #[test]
    fn the_render_rate_is_frames_over_the_time_they_took() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        let half = meter.budget() / 2;
        for _ in 0..24 {
            meter.frame(half);
        }
        let report = meter.report();
        assert!((report.render_fps - 48.0).abs() < 0.5, "{}", report.label());
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
        assert!(report.render_fps > report.target_fps, "{}", report.label());
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
        assert_eq!(report.render_fps, 0.0);
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
        meter.present(Duration::from_millis(500));
        let mut dropped = DroppedFrames::default();
        dropped.record(DropCause::RenderLate, 2);
        meter.drop_frames(dropped);
        meter.reset();
        let report = meter.report();
        assert!(report.is_empty(), "{}", report.label());
        assert_eq!(report.worst_ms, 0.0);
        assert_eq!(report.presented, 0);
        assert!(report.dropped.is_empty());
        assert!((report.target_fps - 30.0).abs() < 1e-9);
        assert_eq!(meter.budget(), Duration::from_secs_f64(1.0 / 30.0));
    }

    /// The achieved rate is frames shown over the run's time, independent of
    /// what the frames cost: a run that shows 24 frames in one second achieved
    /// 24 fps whether each took 1 ms or 30 ms to render.
    #[test]
    fn the_achieved_rate_is_frames_shown_over_the_run() {
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        for frame in 1..=24u32 {
            meter.frame(Duration::from_millis(1));
            meter.present(Duration::from_secs_f64(f64::from(frame) / 24.0));
        }
        let report = meter.report();
        assert_eq!(report.presented, 24);
        assert!(
            (report.achieved_fps - 24.0).abs() < 1e-9,
            "{}",
            report.label()
        );
        assert!(report.render_fps > 900.0, "{}", report.label());
    }

    /// Every cause is counted in its own slot and the total is their sum, so a
    /// report never charges a present-late drop to the decoder.
    #[test]
    fn drops_are_counted_by_cause() {
        let mut dropped = DroppedFrames::default();
        for (cause, frames) in DropCause::ALL.into_iter().zip([1u32, 2, 4]) {
            dropped.record(cause, frames);
        }
        for (cause, frames) in DropCause::ALL.into_iter().zip([1u32, 2, 4]) {
            assert_eq!(dropped.count(cause), frames, "{cause}");
        }
        assert_eq!(dropped.total(), 7);
        let mut meter = PlaybackMeter::at_rate(Rational::FPS_24);
        meter.drop_frames(dropped);
        meter.drop_frames(dropped);
        let report = meter.report();
        assert!(!report.is_empty());
        assert_eq!(report.dropped.total(), 14);
        assert_eq!(report.dropped.present_late, 8);
        assert!(
            report
                .label()
                .contains("14 dropped (2 decode late, 4 render late, 8 present late)"),
            "{}",
            report.label()
        );
    }

    /// The cause of a frame that was not ready follows the larger half of the
    /// last render's split, and a tie is the decoder's.
    #[test]
    fn a_frame_not_ready_is_charged_to_the_larger_stage() {
        let decode = FrameStats {
            decode_ms: 30.0,
            raster_ms: 5.0,
            composite_ms: 5.0,
            ..FrameStats::default()
        };
        let render = FrameStats {
            decode_ms: 2.0,
            raster_ms: 10.0,
            composite_ms: 20.0,
            ..FrameStats::default()
        };
        let tie = FrameStats {
            decode_ms: 10.0,
            raster_ms: 4.0,
            composite_ms: 6.0,
            ..FrameStats::default()
        };
        assert_eq!(DropCause::not_ready(&decode), DropCause::DecodeLate);
        assert_eq!(DropCause::not_ready(&render), DropCause::RenderLate);
        assert_eq!(DropCause::not_ready(&tie), DropCause::DecodeLate);
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
