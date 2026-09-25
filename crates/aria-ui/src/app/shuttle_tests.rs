//! What a shuttle rate means to the mixer, and what a stop does to the rate.
//!
//! The class closed here: J and L moved the picture off 1x while the mixer went
//! on playing forward at 1x, and because the audio clock drives the playhead
//! whenever it is running, reverse did not even move backwards. The rule is one
//! predicate — the mixer follows the transport at 1x and nowhere else — so every
//! site that republishes during playback reads the same answer, and a rate left
//! standing over a stopped playhead is a state the transport cannot be in.
//!
//! Not covered: that the audio device actually goes silent, which needs a
//! device. What is asserted is the decision that silences it.

use super::premiere_tests::with_clip;

/// Every rate the transport can hold, including the ones only the doubling
/// ladder reaches. Swept rather than sampled: a second speed the mixer claimed
/// to follow would be a silent 2x that plays 1x audio over it.
#[test]
fn the_mixer_follows_the_transport_at_one_speed_and_no_other() {
    let (mut state, _) = with_clip();
    for rate in -8..=8 {
        state.shuttle = rate;
        assert_eq!(
            state.audio_follows(),
            rate == 1,
            "the mixer's answer at {rate}x"
        );
    }
}

/// The ladder as an operator climbs it: L from a stop, then each press doubling
/// until it clamps, and J turning it around.
#[test]
fn climbing_the_ladder_leaves_the_mixer_behind_after_the_first_press() {
    let (mut state, _) = with_clip();
    state.set_shuttle(1);
    assert_eq!(state.shuttle, 1);
    assert!(state.audio_follows(), "1x is the speed the mixer can play");

    for expected in [2, 4, 8, 8] {
        state.set_shuttle(1);
        assert_eq!(state.shuttle, expected);
        assert!(!state.audio_follows(), "{expected}x has no audio to play");
    }

    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -1);
    assert!(!state.audio_follows(), "the mixer cannot read backwards");
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -2);
    assert!(!state.audio_follows());
}

/// K stops, and a stop is at 1x: the rate describes what playback is doing, and
/// nothing is playing.
#[test]
fn stopping_returns_the_transport_to_one_speed() {
    let (mut state, _) = with_clip();
    state.set_shuttle(-1);
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -2);

    state.set_shuttle(0);
    assert!(!state.playing);
    assert_eq!(state.shuttle, 1);
    assert!(state.audio_follows());
}

/// Reaching the end of the sequence is the other way playback stops, and it
/// leaves the transport in the same place K does.
#[test]
fn running_out_of_sequence_returns_the_transport_to_one_speed() {
    let (mut state, _) = with_clip();
    // Reverse from tick zero: the stop branch is reached on the first advance,
    // with no dependence on how long a test takes to run.
    state.set_playhead(0);
    state.set_shuttle(-1);
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -2);
    assert!(state.playing);

    assert!(
        !state.advance_playhead(),
        "the playhead ran out of sequence"
    );
    assert!(!state.playing);
    assert_eq!(state.store.playhead_ticks, 0);
    assert_eq!(state.shuttle, 1);
    assert!(state.audio_follows());
}

/// Space is not a rate change: it pauses whatever was running and plays
/// forward from there, whatever J and L left behind.
#[test]
fn a_toggle_plays_forward_from_any_rate() {
    let (mut state, _) = with_clip();
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -1);

    state.toggle_playback();
    assert!(!state.playing, "the first press stops what J started");
    assert_eq!(state.shuttle, 1);

    state.toggle_playback();
    assert!(state.playing);
    assert_eq!(state.shuttle, 1);
    assert!(state.audio_follows(), "and the mixer is back");
}
