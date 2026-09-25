//! Placing a group's cameras on its clock from their sound.
//!
//! # Why
//!
//! An angle's sync offset was a field nothing wrote, so a group of cameras that
//! did not start rolling together cut to the wrong moment on every camera but
//! the first. The measurement that writes it now is here: these pin the sign of
//! what it writes, the reference it writes against, and every input it must
//! report rather than answer.
//!
//! The refusals carry the weight. An offset applied from a weak match moves a
//! camera that was already in sync, and nothing on screen distinguishes that
//! from a real answer, so a measurement that cannot be trusted has to decline by
//! name instead.

use super::measure;
use crate::app::decline::Decline;

/// A repeatable envelope with sharp events, the shape a peak array has.
///
/// The events fall where the noise happens to peak, never on a grid: a spike
/// every N samples is the repeating sound this module refuses, so a fixture
/// built that way would be testing the refusal in every test.
fn envelope(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 16) as f32 / 65_535.0;
            if noise > 0.97 { 0.95 } else { noise * 0.1 }
        })
        .collect()
}

/// Two peaks per tick: the rate the timeline decodes peaks at (40 per second)
/// over a 20 fps sequence, so a lag of two samples is one frame and the
/// conversion is visible in the numbers.
const TICKS_PER_SAMPLE: f64 = 0.5;

#[test]
fn a_camera_that_started_late_is_placed_later_on_the_clock() {
    let reference = envelope(2_000, 3);
    // The second camera began 120 envelope samples into the first, so its own
    // first frame belongs 120 samples along the group's clock: 60 ticks here.
    let candidate = reference[120..].to_vec();
    let measurement = measure(&[Ok(&reference), Ok(&candidate)], 0, TICKS_PER_SAMPLE)
        .expect("two recordings of one sound measure");
    assert_eq!(measurement.moved.len(), 1);
    assert_eq!(measurement.moved[0].angle, 1);
    assert_eq!(measurement.moved[0].offset, 60);
    assert!(measurement.refused.is_none());
}

#[test]
fn a_camera_that_started_early_is_placed_earlier() {
    let full = envelope(2_000, 5);
    let reference = full[120..].to_vec();
    let measurement = measure(&[Ok(&reference), Ok(&full)], 0, TICKS_PER_SAMPLE)
        .expect("two recordings of one sound measure");
    assert_eq!(measurement.moved[0].offset, -60);
}

#[test]
fn the_first_camera_is_the_clock_the_others_are_placed_against() {
    let reference = envelope(2_000, 7);
    let candidate = reference[120..].to_vec();
    // The first camera already sits at tick 500. Placing the second against it
    // means 500 plus the measured lag, not the lag alone: syncing a group must
    // not drag it back to zero.
    let measurement = measure(&[Ok(&reference), Ok(&candidate)], 500, TICKS_PER_SAMPLE)
        .expect("two recordings of one sound measure");
    assert_eq!(measurement.moved[0].offset, 560);
    // And the reference itself is never in the list of angles to move.
    assert!(measurement.moved.iter().all(|moved| moved.angle != 0));
}

#[test]
fn every_camera_after_the_first_is_placed() {
    let reference = envelope(2_000, 11);
    let second = reference[40..].to_vec();
    let third = reference[300..].to_vec();
    let measurement = measure(
        &[Ok(&reference), Ok(&second), Ok(&third)],
        0,
        TICKS_PER_SAMPLE,
    )
    .expect("three recordings of one sound measure");
    let placed: Vec<(usize, i64)> = measurement
        .moved
        .iter()
        .map(|moved| (moved.angle, moved.offset))
        .collect();
    assert_eq!(placed, vec![(1, 20), (2, 150)]);
}

#[test]
fn a_group_of_one_camera_has_nothing_to_measure_against() {
    let only = envelope(500, 13);
    assert_eq!(
        measure(&[Ok(&only)], 0, TICKS_PER_SAMPLE),
        Err(Decline::OneAngleOnly)
    );
}

#[test]
fn the_reference_carries_the_reason_it_has_no_sound() {
    let other = envelope(500, 17);
    assert_eq!(
        measure(
            &[Err(Decline::AnglesStillDecoding), Ok(&other)],
            0,
            TICKS_PER_SAMPLE
        ),
        Err(Decline::AnglesStillDecoding),
        "a group cannot be placed against a camera whose sound is not read yet"
    );
    assert_eq!(
        measure(
            &[Err(Decline::NoAngleSound), Ok(&other)],
            0,
            TICKS_PER_SAMPLE
        ),
        Err(Decline::NoAngleSound),
        "and the reason is the store's, not one invented here"
    );
}

#[test]
fn one_silent_camera_does_not_stop_the_others() {
    let reference = envelope(2_000, 19);
    let good = reference[40..].to_vec();
    let silent = vec![0.0f32; 2_000];
    let measurement = measure(
        &[Ok(&reference), Ok(&good), Ok(&silent)],
        0,
        TICKS_PER_SAMPLE,
    )
    .expect("the camera that can be placed is placed");
    assert_eq!(
        measurement
            .moved
            .iter()
            .map(|m| m.angle)
            .collect::<Vec<_>>(),
        vec![1],
        "the silent camera is left where it is"
    );
    assert_eq!(measurement.refused, Some(Decline::NoAngleSound));
}

#[test]
fn a_camera_that_does_not_match_is_left_alone() {
    let reference = envelope(2_000, 23);
    let mut unrelated = envelope(2_000, 29);
    for (i, value) in unrelated.iter_mut().enumerate() {
        if i % 29 == 0 {
            *value = 0.05;
        }
        if i % 17 == 0 {
            *value = 0.95;
        }
    }
    assert_eq!(
        measure(&[Ok(&reference), Ok(&unrelated)], 0, TICKS_PER_SAMPLE),
        Err(Decline::AnglesDoNotMatch),
        "a weak correlation is reported, never applied"
    );
}

#[test]
fn a_copy_of_a_recording_is_placed_and_a_coincidence_is_not() {
    // Both constants are claims about two populations, so the two populations
    // are exercised rather than trusted: a copy of one recording is placed at
    // the lag it was shifted by, and the unrelated pair in the test above is
    // refused. `aria-analysis` pins the scores those two produce.
    let reference = envelope(2_000, 31);
    let candidate = reference[80..].to_vec();
    let measurement =
        measure(&[Ok(&reference), Ok(&candidate)], 0, TICKS_PER_SAMPLE).expect("a copy measures");
    assert_eq!(measurement.moved[0].offset, 40);
    assert_eq!(measurement.refused, None);
}

#[test]
fn a_camera_whose_sound_repeats_is_refused_by_name() {
    // A metronome, an alarm, a rotor: the same shape every N samples, so every
    // repeat matches as well as the right one and the winner is whichever the
    // search reached first. Applying that would move a camera that may have
    // been in sync already, and nothing on screen would say so.
    let tick: Vec<f32> = (0..2_000)
        .map(|i| if i % 23 == 0 { 0.9 } else { 0.05 })
        .collect();
    let shifted = tick[40..].to_vec();
    assert_eq!(
        measure(&[Ok(&tick), Ok(&shifted)], 0, TICKS_PER_SAMPLE),
        Err(Decline::AnglesRepeat),
        "a sound that repeats cannot say where it is, and a guess is not an answer"
    );
}

#[test]
fn a_lag_converts_by_the_rate_it_is_handed() {
    let reference = envelope(2_000, 37);
    let candidate = reference[100..].to_vec();
    // Same lag, two sequence rates: the offset is in ticks, so the number
    // changes with the rate and the measurement does not.
    let slow = measure(&[Ok(&reference), Ok(&candidate)], 0, 0.5).expect("measures");
    let fast = measure(&[Ok(&reference), Ok(&candidate)], 0, 1.5).expect("measures");
    assert_eq!(slow.moved[0].offset, 50);
    assert_eq!(fast.moved[0].offset, 150);
}
