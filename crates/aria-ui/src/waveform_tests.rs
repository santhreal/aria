//! Contracts for waveform slicing.
//!
//! The class closed here is "the picture does not match the clip": a clip
//! trimmed to the second half of a file must draw the second half of the peaks,
//! and a range past the decoded end must read as silence rather than repeating
//! the last peak or panicking on an out-of-range slice.
//!
//! Not covered: the decode itself and the painted quads, which need a real file
//! and a window. Everything the timeline chooses to draw is a pure function of
//! the peaks and the clip's source window, and that is what these pin.

use std::sync::Arc;

use super::{Waveform, WaveformStore, apply_gain, bars};

/// Ten seconds of peaks at one per second, rising 0.1 to 1.0.
fn ramp() -> Waveform {
    Waveform {
        peaks: Arc::new((1..=10).map(|n| n as f32 / 10.0).collect()),
        per_second: 1.0,
    }
}

#[test]
fn a_bar_is_the_loudest_peak_in_its_own_slice() {
    let wave = ramp();
    // Two bars over the whole ten seconds: each covers five peaks, and each
    // reports the loudest of them rather than their average.
    let out = bars(&wave, 0.0, 10.0, 2);
    assert_eq!(out.len(), 2);
    assert!(
        (out[0] - 0.5).abs() < 1e-6,
        "first half peaks at 0.5: {out:?}"
    );
    assert!(
        (out[1] - 1.0).abs() < 1e-6,
        "second half peaks at 1.0: {out:?}"
    );
}

#[test]
fn a_clip_draws_only_its_own_source_window() {
    let wave = ramp();
    let second_half = bars(&wave, 5.0, 10.0, 1);
    let first_half = bars(&wave, 0.0, 5.0, 1);
    assert!(
        second_half[0] > first_half[0],
        "the louder half must read louder: {first_half:?} vs {second_half:?}"
    );
}

#[test]
fn a_range_past_the_decoded_end_reads_as_silence() {
    let wave = ramp();
    let out = bars(&wave, 20.0, 30.0, 4);
    assert_eq!(out, vec![0.0; 4], "nothing was decoded there");
}

#[test]
fn a_degenerate_request_returns_no_bars_rather_than_panicking() {
    let wave = ramp();
    assert!(bars(&wave, 0.0, 10.0, 0).is_empty(), "zero bars asked for");
    assert_eq!(
        bars(&wave, 3.0, 3.0, 3),
        vec![0.0; 3],
        "an empty source window has no level to show"
    );
    let empty = Waveform {
        peaks: Arc::new(Vec::new()),
        per_second: 40.0,
    };
    assert!(bars(&empty, 0.0, 1.0, 4).is_empty(), "nothing decoded");
}

#[test]
fn every_bar_stays_inside_the_drawable_range() {
    let wild = Waveform {
        peaks: Arc::new(vec![-3.0, 0.5, 17.0]),
        per_second: 1.0,
    };
    for value in bars(&wild, 0.0, 3.0, 3) {
        assert!(
            (0.0..=1.0).contains(&value),
            "a bar height outside 0..=1 would paint outside the strip: {value}"
        );
    }
}

#[test]
fn a_source_is_asked_for_once_and_a_refusal_is_remembered() {
    let mut store = WaveformStore::default();
    assert!(store.wants(7), "nothing is known about it yet");
    store.mark_pending(7);
    assert!(!store.wants(7), "a decode is already in flight");

    store.refuse(7);
    assert!(
        !store.wants(7),
        "a source with no audio must not be decoded again every frame"
    );
    assert!(store.get(7).is_none());

    store.insert(
        8,
        Waveform {
            peaks: Arc::new(vec![0.25]),
            per_second: 40.0,
        },
    );
    assert!(!store.wants(8));
    assert_eq!(store.get(8).expect("stored").peaks.as_slice(), &[0.25]);

    // A new project reallocates media ids, so nothing may survive the clear.
    store.clear();
    assert!(store.wants(7) && store.wants(8));
}

#[test]
fn a_volume_change_scales_every_bar() {
    let mut drawn = vec![1.0, 1.0, 1.0, 1.0];
    apply_gain(&mut drawn, 0.5, 0, 0, 100);
    assert_eq!(drawn, vec![0.5, 0.5, 0.5, 0.5], "the wave follows the mix");

    let mut silent = vec![1.0, 0.4];
    apply_gain(&mut silent, 0.0, 0, 0, 100);
    assert_eq!(silent, vec![0.0, 0.0], "a muted clip draws nothing");

    let mut boosted = vec![0.8, 0.2];
    apply_gain(&mut boosted, 4.0, 0, 0, 100);
    assert_eq!(
        boosted,
        vec![1.0, 0.8],
        "a boost clips at the top of the strip rather than painting past it"
    );
}

#[test]
fn a_fade_in_ramps_the_bars_it_covers() {
    // Ten bars over 100 ticks; the fade covers the first 50.
    let mut drawn = vec![1.0; 10];
    apply_gain(&mut drawn, 1.0, 50, 0, 100);
    assert!(drawn[0] < drawn[1], "the ramp rises: {drawn:?}");
    assert!(drawn[3] < 1.0, "still inside the fade: {drawn:?}");
    for value in &drawn[5..] {
        assert!(
            (value - 1.0).abs() < 1e-6,
            "past the fade the wave is at full level: {drawn:?}"
        );
    }
}

#[test]
fn a_fade_out_ramps_down_to_the_end() {
    let mut drawn = vec![1.0; 10];
    apply_gain(&mut drawn, 1.0, 0, 50, 100);
    assert!(
        (drawn[0] - 1.0).abs() < 1e-6,
        "before the fade nothing changes: {drawn:?}"
    );
    assert!(drawn[9] < drawn[8], "the ramp falls: {drawn:?}");
    assert!(
        drawn[9] < 0.2,
        "it is nearly out by the last bar: {drawn:?}"
    );
}

#[test]
fn overlapping_fades_multiply_rather_than_fight() {
    let mut drawn = vec![1.0; 4];
    apply_gain(&mut drawn, 1.0, 100, 100, 100);
    assert!(
        drawn.iter().all(|v| *v < 1.0),
        "every bar is inside both ramps: {drawn:?}"
    );
    assert!(
        drawn[1] > drawn[0] && drawn[2] > drawn[3],
        "the shape peaks in the middle: {drawn:?}"
    );
}

#[test]
fn scaling_an_empty_wave_is_inert() {
    let mut nothing: Vec<f32> = Vec::new();
    apply_gain(&mut nothing, 1.0, 10, 10, 0);
    assert!(nothing.is_empty());
}
