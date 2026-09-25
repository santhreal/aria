//! Tests for [`super`]: the clip card's chased states.
//!
//! The class closed here is a state change that queues instead of retargeting.
//! Selection and lock are both reversible under the hand, so the symptom is a
//! ring that snaps in after the click that deselected it, or a badge that plays
//! a full fade for a lock the operator has already undone.
//!
//! What these do not catch: what the card draws with the value. That is a colour
//! alpha in the builder chain, and it is asserted only in the sense that a value
//! outside `0.0..=1.0` cannot leave here.

use super::*;

/// Ids no other test uses, so the process-wide glide store cannot carry a value
/// between them. The projection's ids are raw, and a real clip id is allocated
/// from a process counter, so a fixed high offset cannot collide with one.
fn id(n: u64) -> u64 {
    n + 9_000
}

#[test]
fn a_card_that_has_never_been_selected_draws_no_ring() {
    let t = selection_t(id(1), false, false);
    assert_eq!(t, 0.0, "a first sighting of `false` is at rest");
    assert!(!showing(t), "so the ring is not built at all");
}

#[test]
fn a_locked_card_arrives_at_a_full_badge() {
    let clip = id(2);
    // A first sighting is at rest, so the badge has to start from unlocked for
    // its arrival to be an arrival: set the target, then let its own span pass.
    lock_t(clip, false, false);
    lock_t(clip, true, false);
    std::thread::sleep(motion::HOVER_FADE.total().mul_f32(motion::speed_scale()));
    std::thread::sleep(std::time::Duration::from_millis(30));
    let t = lock_t(clip, true, false);
    assert_eq!(t, 1.0, "past its own span the badge is fully there");
    assert!(showing(t));
}

/// The reason both of these are chased rather than played: a reversal continues
/// from where the fade is. A marquee that sweeps across a lane and back changes
/// a card's selection on consecutive frames, and a queue of one-shots would show
/// that as a flicker at full opacity.
#[test]
fn a_selection_reversed_mid_fade_continues_from_where_it_got_to() {
    let clip = id(3);
    selection_t(clip, false, false);
    selection_t(clip, true, false);
    std::thread::sleep(std::time::Duration::from_millis(40));
    let rising = selection_t(clip, true, false);
    assert!(
        rising > 0.0 && rising < 1.0,
        "the ring has to be mid-flight for the reversal to mean anything: {rising}"
    );

    let reversed = selection_t(clip, false, false);
    assert!(
        (reversed - rising).abs() < 0.25,
        "the reversal jumped from {rising} to {reversed} instead of continuing"
    );
    assert!(
        reversed <= rising + 1e-3,
        "it kept rising after deselection"
    );
}

/// Reduced motion is the one case where a state is allowed to snap, and it has
/// to snap in both directions: an operator who turned motion off gets the ring,
/// not a ring that is on its way.
#[test]
fn motion_off_puts_a_state_where_it_belongs_on_the_frame_it_changed() {
    let clip = id(4);
    assert_eq!(selection_t(clip, false, true), 0.0);
    assert_eq!(selection_t(clip, true, true), 1.0, "selection is immediate");
    assert_eq!(selection_t(clip, false, true), 0.0, "and so is losing it");
}

/// Two cards mid-flight in opposite directions is the shape of every click that
/// moves the selection, so the keys have to be per clip.
#[test]
fn two_cards_hold_their_own_state() {
    let (a, b) = (id(5), id(6));
    selection_t(a, false, false);
    selection_t(b, false, false);
    assert_eq!(selection_t(a, true, true), 1.0);
    assert_eq!(selection_t(b, false, true), 0.0, "b kept its own value");
}

#[test]
fn the_threshold_is_below_one_step_of_eight_bit_alpha() {
    assert!(!showing(0.0));
    assert!(!showing(1.0 / 512.0));
    assert!(showing(1.0 / 200.0));
    assert!(showing(1.0));
}
