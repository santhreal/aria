//! What a shuttle rate means to the mixer, how fine shuttle and K+J/L behave,
//! how work-area and loop playback advance, and how audio scrubbing queues grains.

use std::time::{Duration, Instant};

use super::premiere_tests::with_clip;
use crate::transport::ShuttleSpeed;

/// The mixer follows the transport at every non-zero forward and reverse speed
/// up to 4x (fractional speeds included), and goes silent above 4x or at 0.
#[test]
fn the_mixer_follows_the_transport_up_to_four_times_speed_in_both_directions() {
    let (mut state, _) = with_clip();
    for hundredths in [
        -800, -410, -401, -400, -200, -110, -100, -50, -25, -10, 0, 10, 25, 50, 100, 110, 200, 400,
        401, 410, 800,
    ] {
        state.shuttle = ShuttleSpeed::hundredths(hundredths);
        let expected = hundredths != 0 && hundredths.abs() <= 400;
        assert_eq!(
            state.audio_follows(),
            expected,
            "the mixer's answer at {}",
            state.shuttle
        );
    }
}

/// The ladder as an operator climbs it: L from a stop, then each press doubling
/// until it clamps at 8x, with audio following up to 4x and muting at 8x.
#[test]
fn climbing_the_ladder_follows_audio_through_4x_and_mutes_at_8x() {
    let (mut state, _) = with_clip();
    state.set_shuttle(1);
    assert_eq!(state.shuttle, 1);
    assert!(state.audio_follows());

    state.set_shuttle(1);
    assert_eq!(state.shuttle, 2);
    assert!(state.audio_follows());

    state.set_shuttle(1);
    assert_eq!(state.shuttle, 4);
    assert!(state.audio_follows());

    for _ in 0..2 {
        state.set_shuttle(1);
        assert_eq!(state.shuttle, 8);
        assert!(!state.audio_follows(), "8x is above the 4x audible ceiling");
    }

    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -1);
    assert!(state.audio_follows(), "-1x plays reverse shuttle audio");
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -2);
    assert!(state.audio_follows(), "-2x plays reverse shuttle audio");
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -4);
    assert!(state.audio_follows(), "-4x plays reverse shuttle audio");
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -8);
    assert!(!state.audio_follows(), "-8x mutes shuttle audio");
}

/// Shift+J and Shift+L step shuttle speed by 10% across zero and clamp at [-8x, 8x].
#[test]
fn shift_j_and_shift_l_step_by_ten_percent_across_zero_and_clamp() {
    let (mut state, _) = with_clip();
    state.step_shuttle_fine(1);
    assert!(state.playing);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(110));
    state.step_shuttle_fine(1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(120));

    // Step down from 1.2x through 0.1x and across zero to -0.1x.
    for expected in [110, 100, 90, 80, 70, 60, 50, 40, 30, 20, 10, -10, -20] {
        state.step_shuttle_fine(-1);
        assert_eq!(state.shuttle, ShuttleSpeed::hundredths(expected));
        assert!(state.playing);
    }

    // Cross back from -0.2x -> -0.1x -> +0.1x.
    state.step_shuttle_fine(1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(-10));
    state.step_shuttle_fine(1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(10));

    // Clamp at +8x and -8x.
    state.set_shuttle_speed(ShuttleSpeed::hundredths(795));
    state.step_shuttle_fine(1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(800));
    state.step_shuttle_fine(1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(800));

    state.set_shuttle_speed(ShuttleSpeed::hundredths(-795));
    state.step_shuttle_fine(-1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(-800));
    state.step_shuttle_fine(-1);
    assert_eq!(state.shuttle, ShuttleSpeed::hundredths(-800));
}

/// Holding K and tapping J or L steps one frame; holding K and holding J or L
/// plays at +-1/4x until release.
#[test]
fn k_plus_j_or_l_taps_one_frame_and_holds_quarter_speed_until_release() {
    let (mut state, _) = with_clip();
    state.set_playhead(10);
    state.k_down(false);
    assert!(!state.playing);

    // Tap L while K is held: +1 frame, still paused.
    state.shuttle_key_down(1, false);
    state.shuttle_key_up(1);
    assert_eq!(state.playhead.value, 11);
    assert!(!state.playing);

    // Tap J while K is held: -1 frame, still paused.
    state.shuttle_key_down(-1, false);
    state.shuttle_key_up(-1);
    assert_eq!(state.playhead.value, 10);
    assert!(!state.playing);

    // Hold L while K is held: plays at +1/4x until L is released.
    state.shuttle_key_down(1, true);
    assert!(state.playing);
    assert_eq!(state.shuttle, ShuttleSpeed::QUARTER);
    assert!(state.audio_follows());
    state.shuttle_key_up(1);
    assert!(!state.playing);
    assert_eq!(state.shuttle, ShuttleSpeed::ONE);

    // Hold J while K is held: plays at -1/4x until K is released.
    state.shuttle_key_down(-1, true);
    assert!(state.playing);
    assert_eq!(state.shuttle, ShuttleSpeed::REVERSE_QUARTER);
    assert!(state.audio_follows());
    state.k_up();
    assert!(!state.playing);
    assert_eq!(state.shuttle, ShuttleSpeed::ONE);
}

/// Shift+Space plays from mark_in to mark_out and stops at mark_out when loop
/// is off, or wraps back to mark_in when loop is on; Ctrl+Shift+L toggles loop
/// and Space playback wraps at mark_out when loop is on.
#[test]
fn work_area_and_loop_playback_stop_or_wrap_at_mark_out() {
    let (mut state, _) = with_clip();
    state.mark_in = Some(5);
    state.mark_out = Some(15);
    state.set_playhead(0);

    // Shift+Space with loop off starts at mark_in (5) and stops at mark_out (15).
    state.shift_space();
    assert!(state.playing);
    assert_eq!(state.playhead.value, 5);
    state.playback_started = Some(Instant::now() - Duration::from_secs(2));
    assert!(
        !state.advance_playhead(),
        "stopped at mark_out with loop off"
    );
    assert!(!state.playing);
    assert_eq!(state.playhead.value, 15);

    // Toggle loop on (Ctrl+Shift+L): Shift+Space wraps back to mark_in (5).
    state.toggle_loop();
    assert!(state.looping);
    state.shift_space();
    assert!(state.playing);
    assert_eq!(state.playhead.value, 5);
    state.playback_started = Some(Instant::now() - Duration::from_secs(2));
    assert!(state.advance_playhead(), "wrapped to mark_in with loop on");
    assert!(state.playing);
    assert_eq!(state.playhead.value, 5);

    // Space playback with loop on also wraps at mark_out (15) back to mark_in (5).
    state.toggle_playback();
    assert!(!state.playing);
    state.set_playhead(10);
    state.toggle_playback();
    assert!(state.playing);
    state.playback_started = Some(Instant::now() - Duration::from_secs(2));
    assert!(
        state.advance_playhead(),
        "Space playback wrapped at mark_out with loop on"
    );
    assert!(state.playing);
    assert_eq!(state.playhead.value, 5);
}

/// Audio scrubbing queues at most one grain across a 50-move scrub burst when
/// `audio_scrubbing` is on, and queues nothing when `audio_scrubbing` is off.
#[test]
fn scrub_playhead_respects_audio_scrubbing_setting_and_bounds_queue_to_one() {
    let (mut state, _) = with_clip();
    assert!(state.ui_settings.audio_scrubbing);
    for tick in 1..=50 {
        state.scrub_playhead(tick);
    }
    assert_eq!(state.audio.pending_scrub_grains(), 1);
    let grain = state.audio.take_pending_scrub().expect("one latest grain");
    assert_eq!(grain.tick, 50);
    assert_eq!(state.audio.pending_scrub_grains(), 0);

    state.ui_settings.audio_scrubbing = false;
    for tick in 1..=50 {
        state.scrub_playhead(tick);
    }
    assert_eq!(state.audio.pending_scrub_grains(), 0);
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
