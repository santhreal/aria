//! The Auto resolution controller's steps and bounds, and the fit sizes.

use std::time::{Duration, Instant};

use super::*;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Auto stepped down from Full to `level` at `at`, with the drop window empty.
fn auto_at(level: Resolution, at: Instant) -> AutoResolution {
    let mut auto = AutoResolution::default();
    auto.start(at);
    while auto.level() != level {
        assert!(
            auto.dropped(AUTO_DROPS as u32, at),
            "cannot reach {level:?}"
        );
    }
    auto
}

#[test]
fn three_drops_inside_a_second_step_down_one_level() {
    let t0 = Instant::now();
    let mut auto = AutoResolution::default();
    auto.start(t0);
    assert!(!auto.dropped(1, t0));
    assert!(!auto.dropped(1, t0 + ms(400)));
    assert!(auto.dropped(1, t0 + ms(900)));
    assert_eq!(auto.level(), Resolution::Half);
    // The window empties on a step: the next step needs three new drops.
    assert!(!auto.dropped(1, t0 + ms(950)));
    assert!(!auto.dropped(1, t0 + ms(960)));
    assert!(auto.dropped(1, t0 + ms(970)));
    assert_eq!(auto.level(), Resolution::Quarter);
}

#[test]
fn drops_spread_past_a_second_do_not_step() {
    let t0 = Instant::now();
    let mut auto = AutoResolution::default();
    auto.start(t0);
    for at in [0, 600, 1_200, 1_800, 2_400, 3_000] {
        assert!(!auto.dropped(1, t0 + ms(at)), "drop at {at} ms stepped");
    }
    assert_eq!(auto.level(), Resolution::Full);
}

#[test]
fn one_present_dropping_three_frames_steps_once() {
    let t0 = Instant::now();
    let mut auto = AutoResolution::default();
    auto.start(t0);
    assert!(auto.dropped(40, t0));
    assert_eq!(auto.level(), Resolution::Half);
    assert!(!auto.dropped(0, t0 + ms(10)));
}

#[test]
fn auto_stops_at_one_eighth() {
    let t0 = Instant::now();
    let mut auto = auto_at(Resolution::Eighth, t0);
    assert!(!auto.dropped(10, t0 + ms(10)));
    assert_eq!(auto.level(), Resolution::Eighth);
}

#[test]
fn five_clean_seconds_step_up_one_level_at_a_time() {
    let t0 = Instant::now();
    let mut auto = auto_at(Resolution::Eighth, t0);
    assert!(!auto.presented(t0 + ms(4_999)));
    assert!(auto.presented(t0 + ms(5_000)));
    assert_eq!(auto.level(), Resolution::Quarter);
    // The step restarts the clock.
    assert!(!auto.presented(t0 + ms(9_999)));
    assert!(auto.presented(t0 + ms(10_000)));
    assert_eq!(auto.level(), Resolution::Half);
    assert!(auto.presented(t0 + ms(15_000)));
    assert_eq!(auto.level(), Resolution::Full);
    assert!(!auto.presented(t0 + ms(60_000)));
    assert_eq!(auto.level(), Resolution::Full);
}

#[test]
fn a_drop_restarts_the_clean_clock() {
    let t0 = Instant::now();
    let mut auto = auto_at(Resolution::Half, t0);
    assert!(!auto.dropped(1, t0 + ms(3_000)));
    assert!(!auto.presented(t0 + ms(5_000)));
    assert!(!auto.presented(t0 + ms(7_999)));
    assert!(auto.presented(t0 + ms(8_000)));
    assert_eq!(auto.level(), Resolution::Full);
}

#[test]
fn time_paused_does_not_count_toward_a_step_up() {
    let t0 = Instant::now();
    let mut auto = auto_at(Resolution::Half, t0);
    let resumed = t0 + Duration::from_secs(600);
    auto.start(resumed);
    assert_eq!(auto.level(), Resolution::Half, "a new run keeps the level");
    assert!(!auto.presented(resumed + ms(100)));
    assert!(auto.presented(resumed + ms(5_000)));
}

#[test]
fn fit_divides_each_side_and_rounds_up() {
    assert_eq!(Resolution::Full.fit((1920, 1080)), None);
    assert_eq!(Resolution::Half.fit((1920, 1080)), Some((960, 540)));
    assert_eq!(Resolution::Quarter.fit((1920, 1080)), Some((480, 270)));
    assert_eq!(Resolution::Eighth.fit((1921, 1081)), Some((241, 136)));
    assert_eq!(Resolution::Eighth.fit((1, 1)), Some((1, 1)));
    assert_eq!(Resolution::Half.fit((0, 0)), Some((1, 1)));
}

#[test]
fn levels_step_in_order_and_stop_at_the_ends() {
    for pair in Resolution::ALL.windows(2) {
        assert_eq!(pair[0].lower(), pair[1]);
        assert_eq!(pair[1].higher(), pair[0]);
        assert!(pair[0].divisor() < pair[1].divisor());
    }
    assert_eq!(Resolution::Full.higher(), Resolution::Full);
    assert_eq!(Resolution::Eighth.lower(), Resolution::Eighth);
}

#[test]
fn settings_name_each_level_once() {
    let fixed: Vec<Resolution> = PlaybackResolution::ALL
        .iter()
        .filter_map(|choice| choice.fixed())
        .collect();
    assert_eq!(fixed, Resolution::ALL);
    for choice in PlaybackResolution::ALL {
        let label = choice.fixed().map_or("Auto", Resolution::label);
        assert_eq!(choice.label(), label);
        let text = serde_json::to_string(&choice).unwrap_or_default();
        let back: PlaybackResolution = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, choice, "{text}");
    }
    assert_eq!(
        serde_json::to_string(&PlaybackResolution::Auto).unwrap_or_default(),
        "\"auto\""
    );
    for choice in PausedResolution::ALL {
        assert_eq!(choice.label(), choice.resolution().label());
    }
}
