//! Tests for [`super`]: the eased-geometry store.
//!
//! The class closed here is a value that travels when it should not, or does not
//! travel when it should. Both read as a layout bug rather than a motion bug,
//! which is why they survived: a pane that keeps sliding after motion was turned
//! off, and a pane that snaps because a call site passed `instant` it did not
//! mean.
//!
//! What these do not catch: whether a surface passes the right `instant` for a
//! gesture. That stays per-call-site by design — a splitter under the pointer
//! must not lag the hand — and only reduced motion is decided by the store.

use super::*;

/// A key nothing else in the process uses, so the thread-local store cannot
/// carry state between tests.
fn key(name: &'static str) -> motion::MotionKey {
    motion::MotionKey::new("glide-test", name)
}

/// The span a value travels over: the spec's total through the speed
/// multiplier, which is exactly what the store divides by.
fn span(spec: MotionSpec) -> std::time::Duration {
    spec.total().mul_f32(motion::speed_scale())
}

/// Long enough after a start that the value has certainly landed.
fn past_the_end(spec: MotionSpec) -> std::time::Duration {
    span(spec).saturating_add(std::time::Duration::from_millis(30))
}

#[test]
fn the_first_sighting_of_a_value_does_not_travel() {
    let k = key("first-sighting");
    let t0 = Instant::now();
    assert_eq!(
        glide_at(k, 100.0, motion::RESIZE, false, t0),
        100.0,
        "a value seen for the first time draws where it is"
    );
    assert!(!tick_at(t0), "and nothing is moving because of it");
}

#[test]
fn a_changed_target_travels_and_then_lands() {
    let k = key("travels");
    let t0 = Instant::now();
    let landed = t0 + past_the_end(motion::RESIZE);
    glide_at(k, 0.0, motion::RESIZE, false, t0);
    let mid = glide_at(k, 100.0, motion::RESIZE, false, t0);
    assert!(
        (0.0..100.0).contains(&mid),
        "the frame the target changed draws the old value, not the new one: {mid}"
    );
    assert!(tick_at(t0), "and the store reports it is still moving");

    assert_eq!(
        glide_at(k, 100.0, motion::RESIZE, false, landed),
        100.0,
        "past its own duration the value is its target"
    );
    assert!(!tick_at(landed), "and nothing is moving any more");
}

#[test]
fn a_travelling_value_only_moves_toward_its_target() {
    let k = key("monotone");
    let t0 = Instant::now();
    glide_at(k, 0.0, motion::SCROLL_GLIDE, false, t0);
    glide_at(k, 1000.0, motion::SCROLL_GLIDE, false, t0);
    let step = span(motion::SCROLL_GLIDE) / 24;
    let mut previous = 0.0;
    for frame in 0..=24 {
        let current = glide_at(k, 1000.0, motion::SCROLL_GLIDE, false, t0 + step * frame);
        assert!(
            current >= previous - 1e-3,
            "the value fell from {previous} to {current}"
        );
        assert!(current <= 1000.0, "the value overshot: {current}");
        previous = current;
    }
}

/// A target that changes mid-flight continues from where the value is, not from
/// where the last leg started and not from the target it never reached.
///
/// The class this closes is the one that reads as a glitch rather than as
/// motion: a reversal that jumps to the far end first, or one that restarts the
/// interrupted leg from its own origin and so travels backwards through ground
/// it had already covered. Both are what a queue of one-shots does; a chased
/// value cannot do either, and this is the proof it is chased.
#[test]
fn a_target_that_changes_mid_flight_continues_from_the_current_value() {
    let k = key("retarget");
    let t0 = Instant::now();
    let halfway = t0 + span(motion::SCROLL_GLIDE) / 2;
    glide_at(k, 0.0, motion::SCROLL_GLIDE, false, t0);
    glide_at(k, 1000.0, motion::SCROLL_GLIDE, false, t0);
    let interrupted = glide_at(k, 1000.0, motion::SCROLL_GLIDE, false, halfway);
    assert!(
        interrupted > 1.0 && interrupted < 999.0,
        "the value has to be in flight for the reversal to mean anything: {interrupted}"
    );

    // The reversal is the same frame, so the value it hands back is the one
    // already on screen: not near it, it.
    let reversed = glide_at(k, 0.0, motion::SCROLL_GLIDE, false, halfway);
    assert_eq!(
        reversed, interrupted,
        "the reversal jumped instead of continuing from the drawn value"
    );

    assert_eq!(
        glide_at(
            k,
            0.0,
            motion::SCROLL_GLIDE,
            false,
            halfway + past_the_end(motion::SCROLL_GLIDE)
        ),
        0.0,
        "and the interrupted leg still lands on the target that replaced it"
    );
}

#[test]
fn the_instant_override_snaps_without_travelling() {
    let k = key("instant");
    let t0 = Instant::now();
    glide_at(k, 0.0, motion::RESIZE, false, t0);
    assert_eq!(
        glide_at(k, 100.0, motion::RESIZE, true, t0),
        100.0,
        "a value under the hand is where the hand put it"
    );
    assert!(!tick_at(t0), "and it never became something to animate");
}

/// The point of the mirror: the store decides, so a call site that asks to
/// animate still respects an operator who turned motion off.
#[test]
fn reduced_motion_stops_the_store_even_when_the_caller_asks_to_animate() {
    let k = key("reduced");
    let t0 = Instant::now();
    glide_at(k, 0.0, motion::RESIZE, false, t0);
    motion::set_reduced_mirror(true);
    let value = glide_at(k, 100.0, motion::RESIZE, false, t0);
    let moving = tick_at(t0);
    motion::set_reduced_mirror(false);
    assert_eq!(
        value, 100.0,
        "motion is off, so the value is already where it was going"
    );
    assert!(!moving, "and no frame was requested for it");
}

/// A key nothing draws is dropped, so a timeline with ten thousand clips does
/// not keep an entry for every card that ever scrolled past.
#[test]
fn a_value_nothing_draws_is_forgotten() {
    let k = key("forgotten");
    let t0 = Instant::now();
    glide_at(k, 0.0, motion::RESIZE, false, t0);
    glide_at(k, 100.0, motion::RESIZE, false, t0);
    assert!(tick_at(t0), "it is moving on the frame it changed");
    tick_at(t0);
    tick_at(t0);
    assert_eq!(
        glide_at(
            k,
            50.0,
            motion::RESIZE,
            false,
            t0 + past_the_end(motion::RESIZE)
        ),
        50.0,
        "the entry was dropped, so this is a first sighting again"
    );
}

/// The specs the store spends are catalog specs, not numbers of its own. A raw
/// duration cannot compile here, but the wrong catalog entry can, and the feel
/// of the timeline is the difference.
#[test]
fn every_glide_spec_is_a_catalog_spec() {
    assert_eq!(PLAYHEAD.duration_ms(), motion::FLICK.ms());
    assert_eq!(PLAYHEAD.curve(), motion::EASE_OUT);
    assert_eq!(LANE, motion::COLLAPSE);
    assert_eq!(EDIT, motion::RESIZE);
    assert_eq!(PANE, motion::RESIZE);
    assert_eq!(ZOOM, motion::RESIZE);
    assert_eq!(SCROLL, motion::SCROLL_GLIDE);
    assert_eq!(FLING, motion::FLING);
    assert!(FLING.curve().overshoots(), "a fling that cannot overshoot");
    assert_eq!(STATE, motion::HOVER_FADE);
    assert_eq!(ROUTE, motion::DIALOG_IN);
    assert_eq!(PROGRESS, motion::RESIZE);
}
