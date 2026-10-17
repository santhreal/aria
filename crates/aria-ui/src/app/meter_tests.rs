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
use crate::app::meter::{
    METER_FLOOR_DBFS, MeterReading, PEAK_HOLD_SECONDS, decay, hold_peak, meter_fraction,
};
use crate::audio::{BlockLevels, block_levels};
use crate::scratch_pool::scratch;

fn state() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("meter")).expect("state");
    state.new_project().expect("project");
    state
}

/// The peaks and clip flag of a block.
fn block_peaks(data: &[f32], channels: usize) -> ([f32; 2], bool) {
    let levels = block_levels(data, channels);
    (levels.peak, levels.clipped)
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
    let levels = handle.levels();
    assert_eq!((levels.peak, levels.clipped), ([0.8, 1.0], true));
    assert_eq!(levels.rms, [0.8, 1.0]);
    handle.publish_stop();
    let levels = handle.levels();
    assert_eq!(
        (levels.peak, levels.rms),
        ([0.0, 0.0], [0.0, 0.0]),
        "a stopped stream leaves no bars standing"
    );
    assert!(levels.clipped, "the clip is a receipt for what was heard");
    handle.clear_clip();
    assert!(!handle.levels().clipped);
}

#[test]
fn rms_is_the_root_mean_square_of_each_side() {
    // Left 0.6 and -0.8: sqrt((0.36 + 0.64) / 2). Right one sample of 0.5 and
    // one of zero: sqrt(0.25 / 2).
    let levels = block_levels(&[0.6, 0.5, -0.8, 0.0], 2);
    assert_eq!(levels.peak, [0.8, 0.5]);
    assert!(
        (levels.rms[0] - 0.5f32.sqrt()).abs() < 1e-6,
        "{:?}",
        levels.rms
    );
    assert!(
        (levels.rms[1] - 0.125f32.sqrt()).abs() < 1e-6,
        "{:?}",
        levels.rms
    );
    assert!(levels.rms[0] <= levels.peak[0] && levels.rms[1] <= levels.peak[1]);
}

#[test]
fn rms_skips_what_a_broken_decoder_sends_and_doubles_a_mono_side() {
    let levels = block_levels(&[f32::NAN, 0.3, 0.3, f32::INFINITY], 2);
    assert_eq!(levels.rms, [0.3, 0.3], "a non-finite sample is not counted");
    let mono = block_levels(&[0.5, -0.5], 1);
    assert_eq!(mono.rms, [0.5, 0.5]);
    assert_eq!(block_levels(&[], 2), BlockLevels::default());
    let unread = block_levels(&[f32::NAN, f32::NAN], 2);
    assert_eq!(
        unread.rms,
        [0.0, 0.0],
        "no finite sample reads silent, not NaN"
    );
}

#[test]
fn the_hold_line_stays_up_for_the_hold_time_then_follows_the_bar() {
    assert_eq!(hold_peak(0.5, 1.0, 0.9, 0.02), (0.9, 0.0), "a rise re-arms");
    assert_eq!(
        hold_peak(0.9, 0.0, 0.2, 1.0),
        (0.9, 1.0),
        "held inside the time"
    );
    let (line, age) = hold_peak(0.9, 1.0, 0.2, 0.6);
    assert_eq!(
        (line, age),
        (0.2, PEAK_HOLD_SECONDS),
        "expired: follows the bar"
    );
    assert_eq!(
        hold_peak(0.2, PEAK_HOLD_SECONDS, 0.1, 100.0),
        (0.1, PEAK_HOLD_SECONDS),
        "the age is capped, so a reading at rest is stable"
    );
    assert_eq!(
        hold_peak(0.9, 0.0, 0.2, -5.0),
        (0.9, 0.0),
        "a backwards clock is inert"
    );
}

#[test]
fn the_scale_runs_from_the_floor_to_full_scale() {
    assert_eq!(meter_fraction(1.0), 1.0, "0 dBFS is the top");
    assert_eq!(meter_fraction(2.0), 1.0, "an over clamps to the top");
    let floor = 10f32.powf(METER_FLOOR_DBFS / 20.0);
    assert!(
        meter_fraction(floor).abs() < 1e-4,
        "{}",
        meter_fraction(floor)
    );
    assert_eq!(
        meter_fraction(floor / 10.0),
        0.0,
        "below the floor reads empty"
    );
    assert_eq!(meter_fraction(0.0), 0.0);
    assert_eq!(meter_fraction(f32::NAN), 0.0);
    // -6 dBFS sits a tenth of the way down a 60 dB scale.
    assert!((meter_fraction(10f32.powf(-6.0 / 20.0)) - 0.9).abs() < 1e-4);
}

#[test]
fn a_held_peak_outlives_the_bar_and_then_drops_with_one_repaint() {
    let mut state = state();
    let now = Instant::now();
    state.audio.publish_block(&[1.0, 1.0], 2);
    state.meter_tick(now);
    state.audio.publish_block(&[0.0, 0.0], 2);
    // The bar has fallen to silence while the line still holds.
    assert!(state.meter_tick(now + Duration::from_millis(1_000)));
    let reading = state.meter();
    assert_eq!(reading.peaks, [0.0, 0.0]);
    assert_eq!(reading.hold, [1.0, 1.0]);
    assert!(
        reading.active(),
        "a standing hold line keeps the meter drawn"
    );
    // Ageing alone changes no pixel.
    assert!(!state.meter_tick(now + Duration::from_millis(1_200)));
    // The hold time runs out: one repaint takes the line down.
    assert!(state.meter_tick(now + Duration::from_millis(1_600)));
    assert_eq!(
        state.meter(),
        MeterReading {
            hold_age: [PEAK_HOLD_SECONDS; 2],
            clipped: true,
            ..MeterReading::default()
        }
    );
    assert!(!state.meter_tick(now + Duration::from_millis(2_000)));
}
