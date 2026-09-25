//! What the editor reports about the rate playback held.
//!
//! The class these close: frame cost was measured per frame and against a fixed
//! 150 ms log threshold, so a preview that ran at half the sequence's rate was
//! invisible — every frame was well inside the threshold and no counter compared
//! the run against a rate at all. `aria_render::PlaybackMeter` answers the
//! operator's question instead, and these assert the wiring around it: which
//! frames are charged to playback, which rate they are charged against, and
//! that a run never inherits the previous one's drops.
//!
//! Not covered: that the panel draws the line, which needs a window. What is
//! asserted is the report it draws from.

use std::time::Duration;

use super::premiere_tests::with_clip;

#[test]
fn a_paused_editor_reports_no_rate_at_all() {
    let (state, _) = with_clip();
    let report = state.playback_report();
    assert!(
        report.is_empty(),
        "a session that never played reported {}",
        report.label()
    );
    assert!(
        !report.keeps_up(0.05),
        "an empty run claimed to keep up, which would make the panel green before a frame existed"
    );
}

#[test]
fn a_frame_delivered_under_playback_counts_against_the_sequence_rate() {
    let (mut state, _) = with_clip();
    state.toggle_playback();
    let budget = state.playback.budget();
    state.playback.frame(budget / 2);
    state.playback.frame(budget / 2);

    let report = state.playback_report();
    assert_eq!(report.frames, 2);
    assert_eq!(
        report.late, 0,
        "two frames inside the budget were called late"
    );
    assert_eq!(
        report.target_fps.round(),
        state.playhead.rate.as_f64().round(),
        "playback at 1x was measured against {} fps",
        report.target_fps
    );
    assert!(
        report.keeps_up(0.0),
        "a run of half-budget frames did not keep up: {}",
        report.label()
    );
}

#[test]
fn a_frame_over_its_budget_is_late_and_the_worst_one_is_kept() {
    let (mut state, _) = with_clip();
    state.toggle_playback();
    let budget = state.playback.budget();
    state.playback.frame(budget / 4);
    state.playback.frame(budget * 3);

    let report = state.playback_report();
    assert_eq!(
        report.late,
        1,
        "the slow frame was not counted: {}",
        report.label()
    );
    assert!(
        report.worst_ms >= (budget * 3).as_secs_f64() * 1000.0 - 0.001,
        "the worst frame was reported as {:.1} ms",
        report.worst_ms
    );
    assert!(
        !report.keeps_up(0.05),
        "half the frames were late and the run still claimed the rate: {}",
        report.label()
    );
}

/// A run that starts again starts empty: the drops of the run the operator
/// already stopped are not this run's, and carrying them over would leave the
/// panel amber for the rest of the session.
#[test]
fn starting_playback_again_forgets_the_previous_run() {
    let (mut state, _) = with_clip();
    state.toggle_playback();
    let budget = state.playback.budget();
    state.playback.frame(budget * 4);
    assert_eq!(state.playback_report().late, 1);

    state.toggle_playback();
    state.toggle_playback();
    assert!(
        state.playback_report().is_empty(),
        "the second run inherited {} frames from the first",
        state.playback_report().frames
    );
}

/// The shuttle ladder is the target, not a decoration: at 4x the player has to
/// produce four frames in the time one frame usually takes, so a frame that was
/// comfortable at 1x is late at 4x. Measuring against the sequence rate instead
/// would report a smooth 4x that visibly stutters.
#[test]
fn the_shuttle_step_raises_the_rate_playback_is_measured_against() {
    let (mut state, _) = with_clip();
    state.set_shuttle(1);
    let at_one = state.playback.budget();

    state.set_shuttle(1);
    assert_eq!(state.shuttle, 2);
    let at_two = state.playback.budget();
    // Halved, within the nanosecond the two divisions round differently by:
    // asserting exact equality would test `Duration::from_secs_f64`, not the
    // rate the transport asked for.
    let doubled = at_two.as_secs_f64() * 2.0;
    assert!(
        (doubled - at_one.as_secs_f64()).abs() < 1e-6,
        "2x asked for a {doubled:.6}s budget where 1x asks for {:.6}s",
        at_one.as_secs_f64()
    );

    state.playback.frame(at_one);
    let report = state.playback_report();
    assert_eq!(
        report.late,
        1,
        "a 1x-sized frame was on time at 2x: {}",
        report.label()
    );
    assert_eq!(
        report.target_fps.round(),
        (state.playhead.rate.as_f64() * 2.0).round()
    );
}

/// Reverse costs the same as forward: the direction changes which frame is
/// decoded, not how many arrive per second. A negative target would make every
/// frame late while J is held.
#[test]
fn playing_backwards_is_measured_against_the_same_rate_as_forwards() {
    let (mut state, _) = with_clip();
    state.set_shuttle(1);
    let forward = state.playback.budget();
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -1);
    assert_eq!(state.playback.budget(), forward);
    assert!(state.playback_report().target_fps > 0.0);
}

/// A stop leaves the last run readable. The transport clears `playback_started`
/// and the audio, and the report stays: the operator asks "why was that
/// stuttery" after stopping, not during.
#[test]
fn stopping_keeps_the_run_that_just_finished_readable() {
    let (mut state, _) = with_clip();
    state.toggle_playback();
    state.playback.frame(Duration::from_millis(1));
    state.toggle_playback();

    assert!(!state.playing);
    let report = state.playback_report();
    assert_eq!(
        report.frames,
        1,
        "the stop threw the run away: {}",
        report.label()
    );
}
