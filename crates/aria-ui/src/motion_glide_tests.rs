//! Tests for [`super`]: the glide store.
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

use std::time::Duration;

use super::*;
use crate::motion::TestFrames;

/// A key for one test value. The store is per thread and every test runs on
/// its own thread, so a name only has to be unique within its test.
fn key(name: &'static str) -> MotionKey {
    MotionKey::new("glide-test", name)
}

/// Long enough after a start that the value has certainly landed: the spec's
/// total at the test app's duration scale of 1, and one frame more.
fn past_the_end(spec: MotionSpec) -> Duration {
    spec.total() + Duration::from_millis(30)
}

#[test]
fn the_first_sighting_of_a_value_does_not_travel() {
    let frames = TestFrames::new();
    let k = key("first-sighting");
    assert_eq!(
        glide(k, 100.0, presets::RESIZE, false),
        100.0,
        "a value seen for the first time draws where it is"
    );
    assert!(!frames.end(), "and nothing is moving because of it");
}

#[test]
fn a_changed_target_travels_and_then_lands() {
    let frames = TestFrames::new();
    let k = key("travels");
    glide(k, 0.0, presets::RESIZE, false);
    let mid = glide(k, 100.0, presets::RESIZE, false);
    assert!(
        (0.0..100.0).contains(&mid),
        "the frame the target changed draws the old value, not the new one: {mid}"
    );
    assert!(frames.end(), "and the store reports it is still moving");

    frames.advance(past_the_end(presets::RESIZE));
    assert_eq!(
        glide(k, 100.0, presets::RESIZE, false),
        100.0,
        "past its own duration the value is its target"
    );
    assert!(!frames.end(), "and nothing is moving any more");
}

#[test]
fn a_travelling_value_only_moves_toward_its_target() {
    let frames = TestFrames::new();
    let k = key("monotone");
    glide(k, 0.0, presets::SCROLL_GLIDE, false);
    let step = presets::SCROLL_GLIDE.total() / 24;
    let mut previous = 0.0;
    for _ in 0..=24 {
        let current = glide(k, 1000.0, presets::SCROLL_GLIDE, false);
        assert!(
            current >= previous - 1e-3,
            "the value fell from {previous} to {current}"
        );
        assert!(current <= 1000.0, "the value overshot: {current}");
        previous = current;
        frames.advance(step);
    }
    frames.advance(past_the_end(presets::SCROLL_GLIDE));
    assert_eq!(
        glide(k, 1000.0, presets::SCROLL_GLIDE, false),
        1000.0,
        "and past its span the value lands on its target"
    );
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
    let frames = TestFrames::new();
    let k = key("retarget");
    glide(k, 0.0, presets::SCROLL_GLIDE, false);
    glide(k, 1000.0, presets::SCROLL_GLIDE, false);
    frames.advance(presets::SCROLL_GLIDE.total() / 2);
    let interrupted = glide(k, 1000.0, presets::SCROLL_GLIDE, false);
    assert!(
        interrupted > 1.0 && interrupted < 999.0,
        "the value has to be in flight for the reversal to mean anything: {interrupted}"
    );

    // The reversal is the same frame, so the value it hands back is the one
    // already on screen: not near it, it.
    let reversed = glide(k, 0.0, presets::SCROLL_GLIDE, false);
    assert_eq!(
        reversed, interrupted,
        "the reversal jumped instead of continuing from the drawn value"
    );

    frames.advance(past_the_end(presets::SCROLL_GLIDE));
    assert_eq!(
        glide(k, 0.0, presets::SCROLL_GLIDE, false),
        0.0,
        "and the interrupted leg still lands on the target that replaced it"
    );
}

#[test]
fn the_instant_override_snaps_without_travelling() {
    let frames = TestFrames::new();
    let k = key("instant");
    glide(k, 0.0, presets::RESIZE, false);
    assert_eq!(
        glide(k, 100.0, presets::RESIZE, true),
        100.0,
        "a value under the hand is where the hand put it"
    );
    assert!(!frames.end(), "and it never became something to animate");
}

/// The store reads reduced motion from the frame's policy, so a call site that
/// requests animation still respects motion turned off.
#[test]
fn reduced_motion_stops_the_store_even_when_the_caller_asks_to_animate() {
    let frames = TestFrames::new();
    let k = key("reduced");
    glide(k, 0.0, presets::RESIZE, false);
    frames.update(|cx| cx.set_reduce_motion(true));
    let value = glide(k, 100.0, presets::RESIZE, false);
    assert_eq!(
        value, 100.0,
        "motion is off, so the value is already where it was going"
    );
    assert!(!frames.end(), "and no frame was requested for it");
}

/// A value already travelling when motion is turned off lands on the next
/// frame instead of finishing its leg.
#[test]
fn turning_motion_off_lands_a_value_already_in_flight() {
    let frames = TestFrames::new();
    let k = key("in-flight");
    glide(k, 0.0, presets::SCROLL_GLIDE, false);
    glide(k, 1000.0, presets::SCROLL_GLIDE, false);
    frames.advance(presets::SCROLL_GLIDE.total() / 4);
    let moving = glide(k, 1000.0, presets::SCROLL_GLIDE, false);
    assert!(moving < 1000.0, "the value is in flight: {moving}");
    frames.update(|cx| cx.set_reduce_motion(true));
    assert_eq!(glide(k, 1000.0, presets::SCROLL_GLIDE, false), 1000.0);
    assert!(!frames.end(), "and nothing is left moving");
}

/// A key nothing draws is dropped, so a timeline with ten thousand clips does
/// not keep an entry for every card that ever scrolled past.
#[test]
fn a_value_nothing_draws_is_forgotten() {
    let frames = TestFrames::new();
    let k = key("forgotten");
    glide(k, 0.0, presets::RESIZE, false);
    glide(k, 100.0, presets::RESIZE, false);
    assert!(frames.end(), "it is moving on the frame it changed");
    frames.end();
    frames.end();
    frames.advance(past_the_end(presets::RESIZE));
    assert_eq!(
        glide(k, 50.0, presets::RESIZE, false),
        50.0,
        "the entry was dropped, so this is a first sighting again"
    );
}

/// A value read on every frame is kept, however many frames end: eviction is
/// for values nothing draws, not for values that stopped moving.
#[test]
fn a_value_drawn_every_frame_is_kept() {
    let frames = TestFrames::new();
    let k = key("kept");
    glide(k, 0.0, presets::RESIZE, false);
    glide(k, 100.0, presets::RESIZE, false);
    for _ in 0..4 {
        frames.end();
        frames.advance(Duration::from_millis(16));
        glide(k, 100.0, presets::RESIZE, false);
    }
    let mid = glide(k, 100.0, presets::RESIZE, false);
    assert!(
        0.0 < mid && mid < 100.0,
        "the value continued its leg instead of restarting as a first sighting: {mid}"
    );
}

/// The specs the store spends are catalog specs, not numbers of its own. The
/// wrong catalog entry compiles, and the feel of the timeline is the difference.
#[test]
fn every_glide_spec_is_a_catalog_spec() {
    assert_eq!(PLAYHEAD.duration_ms, presets::MENU_OUT.duration_ms);
    assert_eq!(PLAYHEAD.delay_ms, 0);
    assert_eq!(PLAYHEAD.easing, Easing::EaseOut);
    assert_eq!(LANE, presets::COLLAPSE);
    assert_eq!(EDIT, presets::RESIZE);
    assert_eq!(ZOOM, presets::RESIZE);
    assert_eq!(SCROLL, presets::SCROLL_GLIDE);
    assert_eq!(FLING, crate::motion::FLING);
    assert!(
        FLING
            .easing
            .bezier()
            .is_some_and(|curve| curve.overshoots()),
        "a fling that cannot overshoot"
    );
    assert_eq!(STATE, presets::HOVER_FADE);
    assert_eq!(ROUTE, presets::DIALOG_IN);
    assert_eq!(PROGRESS, presets::RESIZE);
    assert_eq!(WAIT, crate::motion::LOADER_WAIT);
}

/// Playback holds every glide and hover fade at its end state, and the hold
/// lifts when playback stops.
///
/// The class this closes: chrome that keeps easing while the picture plays,
/// which spends the frame budget `docs/spec/look.md` gives the picture, and a
/// playhead that chases the frame on screen instead of standing on it. It does
/// not cover one-shot entrances, which playback does not hold.
#[test]
fn playback_holds_glides_and_hover_fades_at_their_end_state() {
    use crate::motion::{hold_for_playback, hover_t, set_hover};

    let frames = TestFrames::new();
    let k = key("held");
    let lit = key("held-hover");
    glide(k, 0.0, presets::RESIZE, false);

    hold_for_playback(true);
    assert_eq!(
        glide(k, 100.0, presets::RESIZE, false),
        100.0,
        "a glide during playback draws its target at once"
    );
    frames.update(|cx| set_hover(lit, true, cx.motion_policy(), cx.frame_instant()));
    assert_eq!(hover_t(lit), 1.0, "a hover during playback is lit at once");
    assert!(!frames.end(), "and nothing is left moving");

    hold_for_playback(false);
    let leaving = glide(k, 0.0, presets::RESIZE, false);
    assert!(
        leaving > 0.0,
        "once playback stops a change travels again: {leaving}"
    );
    assert!(frames.end(), "and the store reports it moving");
    frames.advance(past_the_end(presets::RESIZE));
    assert_eq!(glide(k, 0.0, presets::RESIZE, false), 0.0);
}
