//! Tests for [`super`]: the route transition.
//!
//! The class closed here is a transition that restarts. Two one-shots, one per
//! end, each replayed from zero whenever its route was entered, so an operator
//! who opened a project and pressed Escape saw two full fades over each other.
//! A chased value cannot restart, and these are the samples that prove it.
//!
//! Each test drives both keys to a known state on its own motion frames before
//! asserting.

use std::time::Duration;

use gpui::motion::presets;

use super::*;
use crate::motion::TestFrames;

/// Long enough that a value started now has certainly landed.
fn past_the_end() -> Duration {
    glide::ROUTE.total() + Duration::from_millis(30)
}

/// Puts both routes' values at rest with `route` fully drawn, so a test starts
/// from the state a settled application is in rather than from an empty store.
fn settled_on(frames: &TestFrames, route: UiRoute) -> f32 {
    fade(route, false);
    frames.advance(past_the_end());
    fade(route, false)
}

#[test]
fn the_route_a_settled_application_draws_is_fully_there() {
    let frames = TestFrames::new();
    assert_eq!(settled_on(&frames, UiRoute::Home), 1.0);
    assert_eq!(settled_on(&frames, UiRoute::Editor), 1.0);
}

/// Leaving a route and coming back to it mid-flight continues from the opacity
/// on screen. The old shape replayed a fade from zero, so a bounce read as a
/// flash of the shell's background between two full fades.
#[test]
fn a_route_change_during_a_route_change_continues_from_the_screen() {
    let frames = TestFrames::new();
    settled_on(&frames, UiRoute::Home);
    fade(UiRoute::Editor, false);
    frames.advance(Duration::from_millis(60));
    let arriving = fade(UiRoute::Editor, false);
    assert!(
        arriving > 0.0 && arriving < 1.0,
        "the editor has to be mid-arrival for the bounce to mean anything: {arriving}"
    );

    let back = fade(UiRoute::Home, false);
    assert!(
        back > 0.0,
        "Home was mid-departure, so it comes back from where it was, not from nothing"
    );
    assert!(
        back < 1.0,
        "and it is not instantly back either: {back} was already drawn at {arriving}"
    );
}

/// Motion off is not this file's decision (the store reads the policy), but a
/// caller that asks for it must get a route that is simply there.
#[test]
fn motion_off_puts_the_route_where_it_belongs_on_the_frame_it_changed() {
    let frames = TestFrames::new();
    settled_on(&frames, UiRoute::Home);
    assert_eq!(fade(UiRoute::Editor, true), 1.0);
    assert_eq!(fade(UiRoute::Home, true), 1.0);
}

/// Both ends run at one rate, from the catalog. The two one-shots this replaced
/// were 160 ms one way and 320 ms the other, which is why leaving felt quicker
/// than arriving.
#[test]
fn the_transition_is_one_catalog_spec_in_both_directions() {
    assert_eq!(glide::ROUTE, presets::DIALOG_IN);
}
