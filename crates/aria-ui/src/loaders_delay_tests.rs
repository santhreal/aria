//! Tests for [`super`]: the delay before a loader appears.
//!
//! The class closed here is the flash — a loader that appears and disappears
//! inside a few frames. It is invisible in a screenshot and obvious in use, so
//! it survived a long time; what makes it testable is that the decision is one
//! function over a clock rather than a condition spread across every surface.
//!
//! What these do not catch: a caller that asks intermittently. That restarts the
//! span by design, and the contract is documented rather than enforced, because
//! every keyed value in the app is kept alive by being drawn.

use std::time::Duration;

use super::*;

fn key(name: &str) -> motion::ControlId {
    motion::ControlId::Key(motion::MotionKey::new("delay-test", name))
}

/// The span of [`glide::WAIT`] and one frame more, on the test clock.
fn past_the_wait() -> Duration {
    glide::WAIT.total() + Duration::from_millis(30)
}

/// The first frame of a wait shows nothing. This is the whole feature: a fast
/// load must leave no trace.
#[test]
fn a_wait_that_has_just_started_shows_no_loader() {
    let _frames = motion::TestFrames::new();
    assert!(!worth_showing(key("fresh")));
}

/// A wait that outlasts the span gets its loader, and keeps it.
#[test]
fn a_wait_that_outlasts_the_span_shows_its_loader_and_keeps_it() {
    let frames = motion::TestFrames::new();
    let wait = key("slow");
    assert!(!worth_showing(wait));
    frames.advance(glide::WAIT.total() - Duration::from_millis(30));
    assert!(!worth_showing(wait), "the span is not up yet");
    frames.advance(Duration::from_millis(60));
    assert!(
        worth_showing(wait),
        "the span is up, so the wait is visible"
    );
    assert!(worth_showing(wait), "and it does not blink back out");
}

/// Two waits are two ages. A panel that starts a second request while the first
/// is running must not inherit the first one's age, or the new loader appears
/// instantly — which is the flash, arriving by a different route.
#[test]
fn two_waits_age_separately() {
    let frames = motion::TestFrames::new();
    let first = key("first");
    worth_showing(first);
    frames.advance(past_the_wait());
    let second = key("second");
    assert!(worth_showing(first));
    assert!(
        !worth_showing(second),
        "a wait that started now is new, whatever else is running"
    );
}

/// The span is the duration scale's: a slower policy waits longer before a
/// loader appears, so a loader never shows sooner than the motion around it.
#[test]
fn the_span_follows_the_duration_scale() {
    let frames = motion::TestFrames::new();
    frames.update(|cx| cx.set_motion_policy(cx.motion_policy().with_duration_scale(2.0)));
    let wait = key("scaled");
    assert!(!worth_showing(wait));
    frames.advance(past_the_wait());
    assert!(
        !worth_showing(wait),
        "one authored span is half the scaled one"
    );
    frames.advance(glide::WAIT.total());
    assert!(
        worth_showing(wait),
        "two authored spans cover the scaled one"
    );
}

/// Reduced motion does not shorten a wait: a loader is information, and a
/// wait that ends inside the span must still leave no trace.
#[test]
fn reduced_motion_does_not_shorten_the_wait() {
    let frames = motion::TestFrames::new();
    frames.update(|cx| cx.set_reduce_motion(true));
    let wait = key("reduced");
    assert!(!worth_showing(wait), "no loader on the first frame");
    frames.advance(past_the_wait());
    assert!(worth_showing(wait));
}

/// The span is one opening stage, not a number this file chose.
#[test]
fn the_span_is_one_opening_stage() {
    assert_eq!(glide::WAIT, motion::OPEN);
    assert_eq!(glide::WAIT.duration_ms, 320);
}
