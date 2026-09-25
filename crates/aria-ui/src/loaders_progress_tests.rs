//! Tests for [`super`]: where the two fills are drawn.
//!
//! The class closed here is a fill that leaves its track. Both are placed by a
//! fraction of the parent, so a value outside `0.0..=1.0` is not a wrong pixel:
//! it is a fill drawn over the row beside it. The two ways to get one are a job
//! that over-reports and a phase that ran past one, and both are exercised here.
//!
//! What these do not catch: the drawn colour, and whether the bar is mounted at
//! all. The first is a theme token, and the second is the caller's condition.

use super::*;

/// A key nothing else uses, so the process-wide chased value cannot be carried
/// in from another test.
fn key(name: &str) -> motion::ControlId {
    motion::ControlId::Key(motion::MotionKey::new("progress-test", name))
}

#[test]
fn the_shuttle_stays_inside_its_track_at_every_phase() {
    for step in 0..=256 {
        let phase = step as f32 / 64.0;
        let left = shuttle_left(phase);
        assert!(
            (0.0..=1.0 - SHUTTLE).contains(&left),
            "phase {phase} put the segment at {left}, ending at {}",
            left + SHUTTLE
        );
    }
}

/// The shuttle turns around rather than wrapping, so there is no frame on which
/// it jumps from one edge to the other. A wrap reads as a second segment.
#[test]
fn the_shuttle_reverses_instead_of_wrapping() {
    let mut previous = shuttle_left(0.0);
    let mut rising = true;
    let mut reversals = 0;
    for step in 1..=128 {
        let current = shuttle_left(step as f32 / 128.0);
        assert!(
            (current - previous).abs() < 0.1,
            "the segment jumped from {previous} to {current}"
        );
        let now_rising = current >= previous;
        if now_rising != rising {
            reversals += 1;
            rising = now_rising;
        }
        previous = current;
    }
    assert_eq!(reversals, 1, "one turn per cycle, not a wrap");
}

/// A job that reports more than it has done fills the bar, and one that reports
/// a negative fraction empties it. Neither draws outside the track.
#[test]
fn a_fraction_a_job_could_not_have_meant_still_draws_inside_the_track() {
    for reported in [-1.0, -0.0, 0.0, 0.5, 1.0, 2.0, f32::MAX] {
        let width = eased_width(key("clamp"), reported);
        assert!(
            (0.0..=1.0).contains(&width),
            "{reported} drew a fill {width} wide"
        );
    }
}

/// The determinate fill travels to a new report instead of stepping to it. A
/// render reports every few hundred frames, so the raw fraction arrives in jumps
/// large enough to read as a bar that had been stuck and then caught up.
#[test]
fn a_new_report_is_travelled_to_rather_than_jumped_to() {
    let bar = key("travel");
    assert_eq!(eased_width(bar, 0.0), 0.0, "a first report draws as it is");
    let jumped = eased_width(bar, 0.9);
    assert!(
        jumped < 0.9,
        "the frame the report arrived still draws the old width, not the new one: {jumped}"
    );

    std::thread::sleep(
        glide::PROGRESS
            .total()
            .mul_f32(motion::speed_scale())
            .saturating_add(std::time::Duration::from_millis(30)),
    );
    assert_eq!(
        eased_width(bar, 0.9),
        0.9,
        "and past its own span the fill is where the job said it was"
    );
}

/// Two bars on screen at once are two values, not one drawn twice.
#[test]
fn two_bars_hold_their_own_width() {
    let (a, b) = (key("own-a"), key("own-b"));
    eased_width(a, 0.0);
    eased_width(b, 0.0);
    eased_width(a, 1.0);
    assert_eq!(eased_width(b, 0.0), 0.0, "b was not moved by a");
}

/// Both fills run on timings the catalog owns.
#[test]
fn both_fills_run_on_catalog_timings() {
    assert_eq!(glide::PROGRESS, motion::RESIZE);
    assert_eq!(ZERON_PULSE.duration_ms(), motion::BREATH.ms());
}
