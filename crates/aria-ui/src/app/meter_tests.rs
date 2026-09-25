//! What the output meter reports.
//!
//! The class these close: the mix was inaudible until export, and the first
//! meter that gets written is usually wrong in one of three ways — it eases
//! upward and misses transients, its fall rate follows the device's block size
//! rather than the clock, or it goes blank on a device that is not stereo. Each
//! of those is asserted here, on the pure arithmetic, so none needs a device.

use std::time::{Duration, Instant};

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::meter::decay;
use crate::audio::block_peaks;
use crate::scratch_pool::scratch;

fn state() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("meter")).expect("state");
    state.new_project().expect("project");
    state
}

#[test]
fn a_stereo_block_reports_each_side() {
    let block = [0.25, 0.75, -0.5, 0.1];
    let (peaks, clipped) = block_peaks(&block, 2);
    assert_eq!(
        peaks,
        [0.5, 0.75],
        "left is the loudest of the even samples"
    );
    assert!(!clipped);
}

#[test]
fn a_mono_device_lights_both_bars() {
    // A meter that reads one channel on a mono device shows half a mix.
    let (peaks, _) = block_peaks(&[0.4, -0.9, 0.2], 1);
    assert_eq!(peaks, [0.9, 0.9]);
}

#[test]
fn a_surround_device_folds_into_two_bars() {
    // Six channels: the first three fold left, the rest right. Nothing is lost
    // and neither bar goes blank.
    let block = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
    let (peaks, _) = block_peaks(&block, 6);
    assert_eq!(peaks, [0.3, 0.6]);
}

#[test]
fn full_scale_is_reported_as_a_clip() {
    let (_, clipped) = block_peaks(&[0.2, 1.0], 2);
    assert!(clipped, "a sample at full scale is a clip");
    let (_, quiet) = block_peaks(&[0.2, 0.999], 2);
    assert!(!quiet, "just under full scale is not");
}

#[test]
fn an_over_scale_sample_is_a_clip_too() {
    let (peaks, clipped) = block_peaks(&[1.7, -2.4], 2);
    assert!(clipped);
    assert_eq!(peaks, [1.7, 2.4], "the overshoot is reported, not hidden");
}

#[test]
fn a_broken_decoder_cannot_poison_the_meter() {
    let (peaks, clipped) = block_peaks(&[f32::NAN, f32::INFINITY, 0.3, 0.4], 2);
    assert_eq!(peaks, [0.3, 0.4]);
    assert!(!clipped, "an infinity is not a clip report");
}

#[test]
fn an_empty_or_channelless_block_reads_silent() {
    assert_eq!(block_peaks(&[], 2), ([0.0, 0.0], false));
    assert_eq!(block_peaks(&[0.9], 0), ([0.0, 0.0], false));
}

#[test]
fn a_rise_is_immediate() {
    // Easing upward under-reports a transient, which is the whole point of a
    // peak meter.
    assert_eq!(decay(0.1, 0.9, 0.001), 0.9);
}

#[test]
fn a_fall_is_rate_limited_by_elapsed_time() {
    // Two seconds of fall at 2.0 per second reaches the floor; a tenth does not.
    assert_eq!(decay(1.0, 0.0, 2.0), 0.0);
    let after = decay(1.0, 0.0, 0.1);
    assert!(after > 0.7 && after < 0.9, "fell to {after}");
}

#[test]
fn a_fall_never_undershoots_the_target_or_zero() {
    assert_eq!(decay(0.5, 0.4, 10.0), 0.4);
    assert_eq!(decay(0.5, 0.0, 10.0), 0.0);
    assert_eq!(decay(0.0, 0.0, -1.0), 0.0, "a backwards clock is inert");
}

#[test]
fn a_silent_app_repaints_nothing() {
    let mut state = state();
    let now = Instant::now();
    assert!(
        !state.meter_tick(now),
        "the first tick of silence is a no-op"
    );
    assert!(!state.meter_tick(now + Duration::from_millis(50)));
    assert!(!state.meter().active());
}

#[test]
fn the_reading_falls_towards_silence_over_ticks() {
    let mut state = state();
    let now = Instant::now();
    // A loud block reads immediately, then the device goes quiet and the clock
    // takes the bar down rather than dropping it.
    state.audio.publish_block(&[1.0, 1.0], 2);
    assert!(state.meter_tick(now), "a loud block is a repaint");
    assert_eq!(state.meter().peaks, [1.0, 1.0], "a rise is immediate");
    state.audio.publish_block(&[0.0, 0.0], 2);
    assert!(
        state.meter_tick(now + Duration::from_millis(100)),
        "a falling bar is a repaint"
    );
    let mid = state.meter().peaks[0];
    assert!(mid > 0.0 && mid < 1.0, "held at {mid}");
    state.meter_tick(now + Duration::from_secs(2));
    assert_eq!(state.meter().peaks, [0.0, 0.0]);
    assert!(
        !state.meter_tick(now + Duration::from_secs(3)),
        "silence stops asking for repaints"
    );
}

#[test]
fn a_clip_is_acknowledged_rather_than_forgotten_on_its_own() {
    let mut state = state();
    state.audio.publish_block(&[0.2, 1.0], 2);
    state.meter_tick(Instant::now());
    assert!(state.meter().clipped, "the clip reaches the reading");
    // Later quiet blocks do not clear it: a clip that vanished before it was
    // seen is a clip that never got fixed.
    state.audio.publish_block(&[0.1, 0.1], 2);
    state.meter_tick(Instant::now());
    assert!(state.meter().clipped);
    state.clear_clip();
    assert!(!state.meter().clipped);
    state.meter_tick(Instant::now());
    assert!(!state.meter().clipped, "clearing survives the next tick");
}

#[test]
fn stopping_playback_drops_the_bars_and_keeps_the_clip() {
    let handle = crate::audio::AudioHandle::new();
    handle.publish_block(&[0.8, 1.0], 2);
    assert_eq!(handle.peaks(), ([0.8, 1.0], true));
    handle.publish_stop();
    let (peaks, clipped) = handle.peaks();
    assert_eq!(
        peaks,
        [0.0, 0.0],
        "a stopped stream leaves no bars standing"
    );
    assert!(clipped, "the clip is a receipt for what was heard");
    handle.clear_clip();
    assert!(!handle.peaks().1);
}
