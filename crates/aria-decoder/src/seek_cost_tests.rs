//! Seek cost model tests: the exact and nearest roll windows.

use super::*;

const R30: Rational = Rational::FPS_30;
const R60: Rational = Rational::FPS_60;
/// The old fixed window.
const ONE_SEC_US: i64 = 1_000_000;

fn frames_us(frames: f64, fps: f64) -> i64 {
    (frames / fps * 1e6) as i64
}

#[test]
fn default_stats_reproduce_the_old_one_second_window() {
    assert_eq!(SeekStats::default().roll_window_us(R30), ONE_SEC_US);
    // Unknown frame rate falls back to 30 fps rather than disabling.
    assert_eq!(
        SeekStats::default().roll_window_us(Rational::new(0, 1)),
        ONE_SEC_US
    );
}

#[test]
fn window_tracks_observed_costs() {
    // Long-GOP source: seeks ~1.2 s, frames ~5 ms — break-even is 240
    // frames, so the window converges well past the old 1 s.
    let mut stats = SeekStats::default();
    for _ in 0..20 {
        stats.observe_seek(1200.0);
        stats.observe_roll_frame(5.0);
    }
    assert!(stats.roll_window_us(R30) > 5_000_000);

    // All-keyframe source: a seek costs the same as a frame, so rolling
    // is pointless beyond the two-frame-period floor.
    let mut stats = SeekStats::default();
    for _ in 0..20 {
        stats.observe_seek(5.0);
        stats.observe_roll_frame(5.0);
    }
    assert_eq!(stats.roll_window_us(R30), frames_us(2.0, 30.0));

    // A pathological ratio saturates at the hard cap.
    let mut stats = SeekStats::default();
    for _ in 0..40 {
        stats.observe_seek(60_000.0);
        stats.observe_roll_frame(0.01);
    }
    assert_eq!(
        stats.roll_window_us(R30),
        (MAX_ROLL_WINDOW_SECS * 1e6) as i64
    );
}

/// Before any snap, the nearest window is the snap prior in frames,
/// whatever the frame cost: the prior is a ratio, not a duration.
#[test]
fn nearest_window_starts_at_the_snap_prior() {
    let mut stats = SeekStats::default();
    assert_eq!(
        stats.nearest_window_us(R30),
        frames_us(SNAP_PRIOR_FRAMES, 30.0)
    );
    for _ in 0..20 {
        stats.observe_roll_frame(1.0);
    }
    assert_eq!(
        stats.nearest_window_us(R60),
        frames_us(SNAP_PRIOR_FRAMES, 60.0)
    );
}

/// The first snap replaces the prior outright; later snaps move the EMA.
/// The `h264-1080p60` shape: 12 ms snaps, 1.2 ms frames, a 10-frame window.
#[test]
fn the_first_snap_replaces_the_prior() {
    let mut stats = SeekStats::default();
    for _ in 0..40 {
        stats.observe_roll_frame(1.2);
    }
    stats.observe_snap(12.0);
    let window = stats.nearest_window_us(R60);
    assert!(
        (window - frames_us(10.0, 60.0)).abs() <= 1_000,
        "window {window} us"
    );
    stats.observe_snap(36.0);
    // 12 + 0.3 * (36 - 12) = 19.2 ms, 16 frames.
    let window = stats.nearest_window_us(R60);
    assert!(
        (window - frames_us(16.0, 60.0)).abs() <= 1_000,
        "window {window} us"
    );
}

/// The nearest window never exceeds the exact window: a snap slower than a
/// full exact seek must not make the nearest path roll further than the
/// exact path would.
#[test]
fn nearest_window_never_exceeds_the_exact_window() {
    let mut stats = SeekStats::default();
    for _ in 0..60 {
        stats.observe_seek(5.0);
        stats.observe_roll_frame(1.0);
        stats.observe_snap(500.0);
    }
    let exact = stats.roll_window_us(R30);
    assert!(
        (exact - frames_us(5.0, 30.0)).abs() <= 1_000,
        "exact {exact} us"
    );
    assert_eq!(stats.nearest_window_us(R30), exact);
}

/// Snap costs clamp like exact costs: a free snap still rolls two frames,
/// a pathological one stops at the cap.
#[test]
fn nearest_window_clamps_at_both_ends() {
    let mut stats = SeekStats::default();
    stats.observe_roll_frame(10.0);
    stats.observe_snap(0.0);
    assert_eq!(stats.nearest_window_us(R30), frames_us(2.0, 30.0));

    let mut stats = SeekStats::default();
    for _ in 0..40 {
        stats.observe_seek(60_000.0);
        stats.observe_roll_frame(0.01);
    }
    stats.observe_snap(60_000.0);
    assert_eq!(
        stats.nearest_window_us(R30),
        (MAX_ROLL_WINDOW_SECS * 1e6) as i64
    );
}

/// The renderer's interactive scrub on a fresh decoder: a warm-up snap
/// (11 ms), then 3-frame steps at 60 fps (1.1 ms per rolled frame). The
/// window must stay wide enough to roll those steps. Two defects shut it to
/// two frames, so every step snaps and the frame cost is never measured:
/// dividing the snap by the 10 ms frame default before any frame is
/// observed, and blending the first rolled frame into that default.
#[test]
fn a_fresh_decoder_learns_to_roll_short_nearest_steps() {
    let mut stats = SeekStats::default();
    stats.observe_snap(11.0);
    assert_eq!(
        stats.nearest_window_us(R60),
        frames_us(SNAP_PRIOR_FRAMES, 60.0),
        "a snap alone keeps the prior"
    );

    stats.observe_roll_frame(1.1);
    let window = stats.nearest_window_us(R60);
    assert!(
        (window - frames_us(10.0, 60.0)).abs() <= 1_000,
        "the first rolled frame sets the ratio: window {window} us"
    );

    stats.observe_roll_frame(2.2);
    // 1.1 + 0.3 * (2.2 - 1.1) = 1.43 ms: later frames move the EMA.
    let window = stats.nearest_window_us(R60);
    assert!(
        (window - frames_us(11.0 / 1.43, 60.0)).abs() <= 1_000,
        "window {window} us"
    );
}
