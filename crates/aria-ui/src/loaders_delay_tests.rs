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

use super::*;

fn key(name: &str) -> motion::ControlId {
    motion::ControlId::Key(motion::MotionKey::new("delay-test", name))
}

/// The first frame of a wait shows nothing. This is the whole feature: a fast
/// load must leave no trace.
#[test]
fn a_wait_that_has_just_started_shows_no_loader() {
    assert!(!worth_showing(key("fresh")));
}

/// A wait that outlasts the span gets its loader, and keeps it.
#[test]
fn a_wait_that_outlasts_the_span_shows_its_loader_and_keeps_it() {
    let wait = key("slow");
    assert!(!worth_showing(wait));
    std::thread::sleep(
        glide::WAIT
            .total()
            .mul_f32(motion::speed_scale())
            .saturating_add(std::time::Duration::from_millis(30)),
    );
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
    let first = key("first");
    worth_showing(first);
    std::thread::sleep(
        glide::WAIT
            .total()
            .mul_f32(motion::speed_scale())
            .saturating_add(std::time::Duration::from_millis(30)),
    );
    let second = key("second");
    assert!(worth_showing(first));
    assert!(
        !worth_showing(second),
        "a wait that started now is new, whatever else is running"
    );
}

/// The span is a catalog span, not a number this file chose.
#[test]
fn the_span_is_one_settle() {
    assert_eq!(glide::WAIT.duration_ms(), motion::SETTLE.ms());
}
