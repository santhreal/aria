//! Tests for [`gpui::motion::fling`] as the timeline pan spends it: what a
//! release is worth.
//!
//! The class closed here is a throw that does not answer to the gesture that
//! made it: one sample deciding the distance, a hand that stopped still
//! throwing, a placement drifting on, or a reduced-motion session moving on its
//! own. Every rule is checked at its boundary, because every one of them is a
//! threshold.

use gpui::motion::fling::{
    FLING_COAST_MS as COAST_MS, FLING_MAX_RATE as MAX_RATE, FLING_MIN_RATE as MIN_RATE,
    FLING_STALE_MS as STALE_MS, fling_distance, sample_velocity,
};

/// A steady drag converges on the rate it is actually moving at, and does not
/// overshoot it: the blend is a weighted mean, so it approaches from below.
#[test]
fn a_steady_drag_converges_on_its_own_rate() {
    let mut v = 0.0;
    for _ in 0..12 {
        v = sample_velocity(v, 8.0, 8.0);
        assert!(v <= 1.0 + 1e-6, "converged past the rate it was fed: {v}");
    }
    assert!(v > 0.95, "twelve samples at 1 px/ms only reached {v}");
}

/// One absurd sample cannot decide the throw. A pointer that jumps 400px in a
/// millisecond is a warp, not a hand, and the ceiling plus the blend leave the
/// running velocity inside what a hand can do.
#[test]
fn one_jump_cannot_decide_the_throw() {
    let v = sample_velocity(1.0, 400.0, 1.0);
    assert!(v <= MAX_RATE, "a single jump reached {v}");
    assert!(v < 3.5, "a single jump moved the velocity to {v}");
}

/// A gap mid-drag starts the velocity over rather than blending across the
/// pause, so the rate after the pause is the rate the hand is moving at now.
#[test]
fn a_gap_replaces_the_velocity_instead_of_blending_into_it() {
    let after_gap = sample_velocity(4.0, 10.0, STALE_MS);
    assert!(
        (after_gap - 10.0 / STALE_MS).abs() < 1e-6,
        "the gap blended into the old rate: {after_gap}"
    );
}

/// A sample with no time behind it has no rate to report.
#[test]
fn a_sample_without_time_leaves_the_velocity_alone() {
    assert_eq!(sample_velocity(0.75, 12.0, 0.0), 0.75);
    assert_eq!(sample_velocity(0.75, 12.0, -1.0), 0.75);
    assert_eq!(sample_velocity(0.75, 12.0, f32::NAN), 0.75);
}

/// The throw is the release rate spent over the coast, and it keeps the sign of
/// the movement: a drag one way must not fling the other.
#[test]
fn the_throw_follows_the_rate_and_its_direction() {
    assert_eq!(fling_distance(1.0, 0.0, false), COAST_MS);
    assert_eq!(fling_distance(-1.0, 0.0, false), -COAST_MS);
    assert!(fling_distance(2.0, 0.0, false) > fling_distance(1.0, 0.0, false));
}

/// Every way a release is not a throw, at its boundary. Each of these would
/// otherwise move the view after the operator had stopped asking it to.
#[test]
fn a_release_that_is_not_a_throw_is_worth_nothing() {
    // Motion off: nothing coasts, however hard it was thrown.
    assert_eq!(fling_distance(4.0, 0.0, true), 0.0);
    // The hand had already stopped.
    assert_eq!(fling_distance(4.0, STALE_MS, false), 0.0);
    assert_eq!(fling_distance(4.0, STALE_MS - 1.0, false), 4.0 * COAST_MS);
    // A placement, not a throw.
    assert_eq!(fling_distance(MIN_RATE / 2.0, 0.0, false), 0.0);
    assert!(fling_distance(MIN_RATE, 0.0, false) > 0.0);
    // A velocity nothing can be computed from.
    assert_eq!(fling_distance(f32::NAN, 0.0, false), 0.0);
    assert_eq!(fling_distance(f32::INFINITY, 0.0, false), 0.0);
}

/// The ceiling is what bounds the throw, so the longest possible one is a
/// screen's worth of travel rather than a whole sequence.
#[test]
fn the_longest_throw_is_bounded_by_the_rate_ceiling() {
    let fastest = sample_velocity(MAX_RATE, 10_000.0, 1.0);
    let throw = fling_distance(fastest, 0.0, false);
    assert!(
        throw <= MAX_RATE * COAST_MS,
        "the fastest sample threw {throw}"
    );
    assert!(throw <= 1_400.0, "the fastest throw is {throw} pixels");
}
