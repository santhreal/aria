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

/// The ladder as repeated presses climb it: L from a stop, then each press doubling
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

    state.playback_started = Some(Instant::now() - Duration::from_secs(10));
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

type StartFn = fn(&mut crate::app::AppState);

fn transport_start_paths() -> Vec<(&'static str, &'static str, i32, StartFn)> {
    vec![
        ("toggle_playback", "toggle_playback", 1, |state| {
            state.toggle_playback();
        }),
        ("play_work_area", "play_work_area", 1, |state| {
            state.play_work_area();
        }),
        ("set_shuttle(+1)", "set_shuttle", 1, |state| {
            state.set_shuttle(1);
        }),
        ("set_shuttle(-1)", "set_shuttle", -1, |state| {
            state.set_shuttle(-1);
        }),
        ("set_shuttle_speed(+1x)", "set_shuttle_speed", 1, |state| {
            state.set_shuttle_speed(ShuttleSpeed::ONE);
        }),
        (
            "set_shuttle_speed(-1x)",
            "set_shuttle_speed",
            -1,
            |state| {
                state.set_shuttle_speed(ShuttleSpeed::hundredths(-100));
            },
        ),
        ("step_shuttle_fine(+1)", "step_shuttle_fine", 1, |state| {
            state.step_shuttle_fine(1);
        }),
        ("step_shuttle_fine(-1)", "step_shuttle_fine", -1, |state| {
            state.shuttle = ShuttleSpeed::hundredths(10);
            state.step_shuttle_fine(-1);
        }),
        (
            "shuttle_key_down(+1, false)",
            "shuttle_key_down",
            1,
            |state| {
                state.shuttle_key_down(1, false);
            },
        ),
        (
            "shuttle_key_down(-1, false)",
            "shuttle_key_down",
            -1,
            |state| {
                state.shuttle_key_down(-1, false);
            },
        ),
        (
            "shuttle_key_down(+1, true) with K",
            "shuttle_key_down",
            1,
            |state| {
                state.k_down(false);
                state.shuttle_key_down(1, true);
            },
        ),
        (
            "shuttle_key_down(-1, true) with K",
            "shuttle_key_down",
            -1,
            |state| {
                state.k_down(false);
                state.shuttle_key_down(-1, true);
            },
        ),
        ("space", "toggle_playback", 1, |state| {
            state.space();
        }),
        ("shift_space", "play_work_area", 1, |state| {
            state.shift_space();
        }),
        (
            "toggle_viewer_playback",
            "toggle_playback",
            1,
            |state| {
                state.toggle_viewer_playback();
            },
        ),
    ]
}

/// Parse `app/transport.rs` (with test items and comments stripped) and return
/// the set of `pub fn` methods whose bodies call `apply_shuttle_speed` or
/// `set_shuttle`.
fn source_transport_start_pub_fns() -> std::collections::BTreeSet<String> {
    let sources = crate::test_support::sources::production_sources();
    let (_, transport) = sources
        .iter()
        .find(|(rel, _)| rel == "app/transport.rs")
        .expect("app/transport.rs in production_sources");
    let mut found = std::collections::BTreeSet::new();
    let mut current_pub_fn: Option<(String, i32)> = None;
    let mut depth = 0i32;
    for line in transport.lines() {
        let trimmed = line.trim();
        if current_pub_fn.is_none()
            && let Some(rest) = trimmed.strip_prefix("pub fn ")
            && let Some(name) = rest.split('(').next()
        {
            current_pub_fn = Some((name.trim().to_owned(), depth));
        }
        if let Some((name, _)) = &current_pub_fn
            && (trimmed.contains("self.apply_shuttle_speed(")
                || trimmed.contains("self.set_shuttle("))
        {
            found.insert(name.clone());
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if let Some((_, start_depth)) = current_pub_fn
            && depth <= start_depth
        {
            current_pub_fn = None;
        }
    }
    found
}

/// Every sequence playback start in the crate goes through
/// `AppState::apply_shuttle_speed`, and starting playback on an empty sequence
/// declines in the status line rather than entering a 0-tick play state.
///
/// The class closed here is a playback start path that sets `self.playing = true`
/// directly without checking the sequence bounds or rewinding from the end, or
/// a new `pub fn` start path added to `app/transport.rs` outside the test table.
/// Not caught: a plugin or external clock mutating `EditorView` without going
/// through `AppState`.
#[test]
fn every_playback_start_path_goes_through_apply_shuttle_speed_and_refuses_an_empty_sequence() {
    let sources = crate::test_support::sources::production_sources();
    let mut playing_true_sites = Vec::new();
    for (rel, code) in &sources {
        let mut current_fn = String::new();
        for (idx, line) in code.lines().enumerate() {
            let trimmed = line.trim();
            if let Some( pos) = trimmed.find("fn ") {
                let after = &trimmed[pos + 3..];
                if let Some(name) = after.split('(').next() {
                    current_fn = name.trim().to_owned();
                }
            }
            if let Some(pos) = trimmed.find(".playing") {
                let rest = trimmed[pos + ".playing".len()..].trim_start();
                if let Some(rhs) = rest.strip_prefix('=')
                    && !rhs.starts_with('=')
                {
                    let rhs = rhs.trim();
                    if !rhs.starts_with("false;") {
                        playing_true_sites.push(format!(
                            "{rel}:{}:{current_fn}: {trimmed}",
                            idx + 1
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(
        playing_true_sites.len(),
        1,
        "only AppState::apply_shuttle_speed may set `.playing` to a playing state:\n  {}",
        playing_true_sites.join("\n  ")
    );
    assert!(
        playing_true_sites[0].starts_with("app/transport.rs:")
            && playing_true_sites[0].contains(":apply_shuttle_speed:"),
        "unexpected `.playing = true` owner: {}",
        playing_true_sites[0]
    );

    let from_source = source_transport_start_pub_fns();
    assert!(
        !from_source.is_empty(),
        "found no public start paths in app/transport.rs"
    );
    let paths = transport_start_paths();
    let covered: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|(_, method, _, _)| (*method).to_owned())
        .collect();
    assert_eq!(
        covered, from_source,
        "transport_start_paths() must cover every pub fn start path in app/transport.rs"
    );

    for (label, _, _, start) in &paths {
        let mut state = crate::app::AppState::new(
            aria_plugin::PluginContext::new(),
            crate::scratch_pool::scratch("empty-seq-play"),
        )
        .expect("state");
        state.new_project().expect("empty project");
        state.status_message = None;
        start(&mut state);
        assert!(
            !state.playing,
            "{label}: entered playing state on an empty sequence"
        );
        assert_eq!(
            state.status_message.as_deref(),
            Some("The sequence has no clips"),
            "{label}: did not explain why an empty sequence cannot play"
        );
    }
}

/// Starting forward playback with the playhead parked at or past the active end
/// rewinds to the active start and plays; starting reverse playback at or
/// before the active start jumps to the active end and plays backward.
#[test]
fn starting_forward_from_the_end_or_reverse_from_the_start_rewinds_across_every_start_path() {
    for (label, _, dir, start) in transport_start_paths() {
        for looping in [false, true] {
            let (mut state, _) = with_clip();
            let duration = state.store.sequence.duration_ticks;
            assert!(duration >= 72);
            let (bound_start, bound_end) = if looping {
                state.mark_in = Some(24);
                state.mark_out = Some(72);
                state.toggle_loop();
                (24, 72)
            } else if label == "play_work_area" || label == "shift_space" {
                state.mark_in = Some(24);
                state.mark_out = Some(72);
                (24, 72)
            } else {
                (0, duration)
            };
            if dir > 0 {
                state.set_playhead(bound_end);
                assert_eq!(state.playhead.value, bound_end);
                start(&mut state);
                assert!(
                    state.playing,
                    "{label} (loop={looping}): did not start forward playback"
                );
                assert_eq!(
                    state.playhead.value, bound_start,
                    "{label} (loop={looping}): forward start at {bound_end} did not rewind to {bound_start}"
                );
                assert_eq!(
                    state.playback_anchor_tick, bound_start,
                    "{label} (loop={looping}): playback_anchor_tick stayed at {bound_end}"
                );
                state.playback_started = Some(Instant::now() - Duration::from_millis(500));
                assert!(
                    state.advance_playhead(),
                    "{label} (loop={looping}): stopped on the first advance after rewinding from end"
                );
                assert!(
                    state.playhead.value > bound_start && state.playhead.value < bound_end,
                    "{label} (loop={looping}): playhead did not advance from {bound_start} (got {})",
                    state.playhead.value
                );
            } else {
                state.set_playhead(bound_start);
                assert_eq!(state.playhead.value, bound_start);
                start(&mut state);
                assert!(
                    state.playing,
                    "{label} (loop={looping}): did not start reverse playback"
                );
                assert_eq!(
                    state.playhead.value, bound_end,
                    "{label} (loop={looping}): reverse start at {bound_start} did not jump to {bound_end}"
                );
                assert_eq!(
                    state.playback_anchor_tick, bound_end,
                    "{label} (loop={looping}): playback_anchor_tick stayed at {bound_start}"
                );
                state.playback_started = Some(Instant::now() - Duration::from_millis(500));
                assert!(
                    state.advance_playhead(),
                    "{label} (loop={looping}): stopped on the first advance after jumping to end"
                );
                assert!(
                    state.playhead.value > bound_start && state.playhead.value < bound_end,
                    "{label} (loop={looping}): playhead did not advance backward from {bound_end} (got {})",
                    state.playhead.value
                );
            }
        }
    }
}
