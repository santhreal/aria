//! WHY: a playhead moved during playback was put back by the next vsync. The
//! run's clock counted from the tick playback started at, and nothing but a
//! start or a loop wrap re-anchored it, so Home, End, the arrow keys, a typed
//! timecode, a marker jump, a ruler press and a plugin's move all snapped back
//! to wherever the clock read one frame later.
//!
//! The re-anchor is at the one place the clock is read, `advance_playhead`,
//! keyed on "the playhead is not where the clock put it", so a mover added
//! later is covered without being listed. The movers below are the shipped
//! ones plus a bare field write, which stands for an engine poll or a plugin.
//!
//! Not caught here: whether the audio device restarts. The handle's stream
//! state is private to the audio module; these tests read the transport's own
//! record of the quiet mixer (`playback_audio_resume`).

use std::time::{Duration, Instant};

use aria_models::RationalTime;

use super::SEEK_AUDIO_SETTLE;
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::transport::{ShuttleSpeed, playback_tick};

/// Run the shipped binding `id` against the state.
fn run_binding(state: &mut AppState, id: &str) {
    let binding = crate::keys::bindings::SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.id == id)
        .unwrap_or_else(|| panic!("{id} is a shipped binding"));
    let crate::keys::KeyBody::Plain(run) = binding.run else {
        panic!("{id} runs against the state alone");
    };
    run(state);
}

type Mover = fn(&mut AppState);

/// Every way the playhead moves under playback, by what moves it.
fn movers() -> Vec<(&'static str, Mover)> {
    vec![
        ("Home", |state| run_binding(state, "key-start")),
        ("Left", |state| run_binding(state, "key-step-back")),
        ("Shift+Left", |state| {
            run_binding(state, "key-step-back-fast");
        }),
        ("Right", |state| run_binding(state, "key-step-forward")),
        ("Shift+Right", |state| {
            run_binding(state, "key-step-forward-fast");
        }),
        ("typed timecode", |state| state.set_playhead(6)),
        ("Viewer strip", |state| state.scrub_playhead(6)),
        ("ruler", |state| state.scrub_playhead_on_ruler(6)),
        ("engine poll", |state| {
            state.playhead = RationalTime::new(6, state.playhead.rate);
        }),
    ]
}

/// Ticks in `ms` of 1x playback at the state's rate.
fn ticks_in(state: &AppState, ms: i64) -> i64 {
    playback_tick(0, ms, state.playhead.rate.num, state.playhead.rate.den)
}

/// A run from tick 12 whose clock has read one second, at the tick it put
/// the playhead at.
fn one_second_in() -> AppState {
    let (mut state, _) = with_clip();
    state.set_playhead(12);
    state.toggle_playback();
    assert!(state.playing);
    state.playback_started = Some(Instant::now() - Duration::from_secs(1));
    assert!(state.advance_playhead());
    assert!(state.playhead.value >= 12 + ticks_in(&state, 1000));
    state
}

#[test]
fn a_run_goes_on_from_wherever_the_playhead_is_moved() {
    for (label, mover) in movers() {
        let mut state = one_second_in();
        let clock = state.playhead.value;
        mover(&mut state);
        let moved = state.playhead.value;
        assert_ne!(moved, clock, "{label}: the move left the playhead in place");
        assert!(state.playing, "{label}: the move stopped playback");

        assert!(state.advance_playhead(), "{label}: the run ended");
        let at = state.playhead.value;
        assert!(
            (moved..=moved + 2).contains(&at),
            "{label}: the next vsync put the playhead at {at}, not at the move to {moved} \
             (the old clock read {clock})"
        );

        // And the clock counts on from the move. The moves have not settled,
        // so the frame clock is the one read.
        state.playback_started = Some(Instant::now() - Duration::from_millis(500));
        if state.playback_audio_resume.is_some() {
            state.playback_audio_resume = Some(Instant::now());
        }
        assert!(state.advance_playhead(), "{label}: the run ended");
        let want = moved + ticks_in(&state, 500);
        let at = state.playhead.value;
        assert!(
            (want..=want + 2).contains(&at),
            "{label}: half a second after the move read {at}, want {want}"
        );
    }
}

/// The clock's own moves are not moves: a run left alone keeps counting from
/// where it started, vsync after vsync.
#[test]
fn a_run_left_alone_keeps_its_anchor() {
    let mut state = one_second_in();
    for ms in [1250, 1500] {
        state.playback_started = Some(Instant::now() - Duration::from_millis(ms));
        assert!(state.advance_playhead());
        let want = 12 + ticks_in(&state, ms as i64);
        let at = state.playhead.value;
        assert!(
            (want..=want + 2).contains(&at),
            "{ms} ms in read {at}, want {want}"
        );
        assert_eq!(state.playback_anchor_tick, 12);
        assert_eq!(state.playback_audio_resume, None);
    }
}

/// A drag across the ruler keeps the mixer quiet while the moves keep coming
/// and restarts it once, from the playhead, when they stop.
#[test]
fn a_ruler_drag_under_playback_restarts_the_mixer_once_it_settles() {
    let mut state = one_second_in();
    for tick in [30, 28, 20, 10, 4] {
        state.scrub_playhead_on_ruler(tick);
        assert!(state.advance_playhead());
        assert!(
            (tick..=tick + 1).contains(&state.playhead.value),
            "the drag to {tick} read {}",
            state.playhead.value
        );
        assert!(
            state.playback_audio_resume.is_some(),
            "the mixer restarted mid-drag at {tick}"
        );
    }

    state.playback_audio_resume = Some(Instant::now() - SEEK_AUDIO_SETTLE);
    assert!(state.advance_playhead());
    assert_eq!(
        state.playback_audio_resume, None,
        "the mixer never restarted"
    );
    assert_eq!(state.playback_anchor_tick, state.playhead.value);
    assert!(
        state
            .playback_started
            .is_some_and(|started| started.elapsed() < SEEK_AUDIO_SETTLE),
        "the clock was not restarted with the mixer"
    );
}

/// Above 4x the mixer does not follow the transport, so a move leaves it
/// quiet and schedules no restart.
#[test]
fn a_move_at_a_silent_rate_schedules_no_mixer_restart() {
    let mut state = one_second_in();
    state.set_shuttle_speed(ShuttleSpeed::steps(8));
    assert!(!state.audio_follows());
    state.set_playhead(6);
    assert!(state.advance_playhead());
    assert_eq!(state.playback_audio_resume, None);
}

/// A stop forgets the run: a playhead moved while stopped starts nothing on
/// the next play but the play itself.
#[test]
fn a_stop_ends_the_runs_record_of_its_playhead() {
    let mut state = one_second_in();
    state.scrub_playhead_on_ruler(4);
    assert!(state.advance_playhead());
    state.toggle_playback();
    assert!(!state.playing);
    assert_eq!(state.playback_placed, None);
    assert_eq!(state.playback_audio_resume, None);
}
