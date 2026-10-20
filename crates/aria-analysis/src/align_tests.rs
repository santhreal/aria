//! Alignment of one envelope against another.
//!
//! # Why
//!
//! A camera group whose files did not start together cuts to the wrong moment,
//! and the offset that fixes it was a field nothing computed. These pin the
//! measurement that computes it: the sign of the lag, what a weak match looks
//! like, and every input that must be refused rather than answered with a
//! number. The refusals matter more than the match: an offset applied from a
//! guess moves every camera in the group away from sync, and the result looks
//! the same as a real answer.

use super::{
    ALIGN_BUDGET, AlignConfig, MAX_ALIGN_LAG, MAX_ALIGN_SAMPLES, RIVAL_GAP, align, align_anywhere,
};

/// A repeatable envelope with sharp events: a slow hum with spikes on top, so
/// correlation has something to lock onto and a flat stretch to reject.
///
/// The spikes fall where the noise happens to peak rather than on a grid. A
/// spike every N samples is a metronome: it matches at every repeat, so the
/// search has no single answer and the rival score equals the winner. Two of
/// these tests were written against a fixture that did exactly that, and it
/// reported lags off by thousands of samples with a score of 0.96.
fn envelope(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed | 1;
    (0..len)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 16) as f32 / 65_535.0;
            let hum = ((i as f32) * 0.05).sin().abs() * 0.2;
            if noise > 0.97 {
                0.9 + hum
            } else {
                noise * 0.1 + hum
            }
        })
        .collect()
}

fn config() -> AlignConfig {
    AlignConfig::new(200, 64).expect("a 200 sample search with a 64 sample floor is valid")
}

#[test]
fn a_later_start_reports_a_positive_lag() {
    let reference = envelope(1_000, 7);
    // The candidate is the same recording that began 40 samples later, so its
    // first sample belongs 40 samples into the reference's clock.
    let candidate = reference[40..].to_vec();
    let found = align(&reference, &candidate, config()).expect("a shifted copy aligns");
    assert_eq!(found.lag, 40, "score {}", found.score);
    assert!(
        found.score > 0.99,
        "an exact copy correlates fully, got {}",
        found.score
    );
    assert_eq!(found.overlap, 960);
}

#[test]
fn an_earlier_start_reports_a_negative_lag() {
    let full = envelope(1_000, 11);
    // The reference is the late one this time: the candidate holds 40 samples
    // the reference never saw, so it begins before the reference's zero.
    let reference = full[40..].to_vec();
    let found = align(&reference, &full, config()).expect("a shifted copy aligns");
    assert_eq!(found.lag, -40, "score {}", found.score);
    assert!(found.score > 0.99, "got {}", found.score);
}

#[test]
fn an_identical_pair_reports_no_lag() {
    let reference = envelope(500, 3);
    let found = align(&reference, &reference, config()).expect("a signal aligns with itself");
    assert_eq!(found.lag, 0);
    assert!(found.score > 0.99, "got {}", found.score);
}

#[test]
fn two_unrelated_recordings_score_low() {
    let reference = envelope(1_000, 5);
    let mut other = envelope(1_000, 9);
    // Break the shared spike grid: with it, two unrelated envelopes still share
    // a rhythm, and the test would be measuring the fixture.
    for (i, value) in other.iter_mut().enumerate() {
        if i % 37 == 0 {
            *value = 0.1;
        }
        if i % 23 == 0 {
            *value = 0.9;
        }
    }
    let found = align(&reference, &other, config()).expect("both envelopes vary");
    assert!(
        found.score < 0.5,
        "unrelated envelopes must not read as a match, got {} at lag {}",
        found.score,
        found.lag
    );
}

#[test]
fn silence_is_refused_rather_than_aligned() {
    let reference = envelope(500, 13);
    let silent = vec![0.0f32; 500];
    assert!(
        align(&reference, &silent, config()).is_none(),
        "a silent angle carries no evidence about time"
    );
    assert!(
        align(&silent, &reference, config()).is_none(),
        "and neither does a silent reference"
    );
}

#[test]
fn a_constant_tone_is_refused() {
    let reference = envelope(500, 17);
    let flat = vec![0.4f32; 500];
    assert!(
        align(&reference, &flat, config()).is_none(),
        "a flat envelope matches every lag equally well, which is no answer"
    );
}

#[test]
fn a_pair_too_short_to_overlap_is_refused() {
    let reference = envelope(100, 19);
    let candidate = envelope(30, 23);
    assert!(
        align(&reference, &candidate, config()).is_none(),
        "30 samples cannot meet a 64 sample overlap floor at any lag"
    );
}

#[test]
fn the_reported_lag_stays_inside_the_search() {
    let reference = envelope(2_000, 29);
    // A real shift of 400, searched with a radius of 200: the answer is not
    // reachable, and the search must stay inside its own bounds rather than
    // wander to the true peak.
    let candidate = reference[400..].to_vec();
    let found = align(&reference, &candidate, config()).expect("both envelopes vary");
    assert!(
        found.lag.unsigned_abs() <= config().max_lag(),
        "the search reported {} outside its radius",
        found.lag
    );
}

#[test]
fn the_overlap_floor_is_honoured() {
    let reference = envelope(1_000, 31);
    let candidate = reference[900..].to_vec();
    let strict = AlignConfig::new(200, 64).expect("valid");
    let found = align(&reference, &candidate, strict);
    // The true lag of 900 needs a 100 sample overlap but sits outside the
    // radius; whatever is reported must rest on at least the floor.
    if let Some(found) = found {
        assert!(
            found.overlap >= strict.min_overlap(),
            "a lag was reported over {} samples",
            found.overlap
        );
    }
}

#[test]
fn an_input_past_the_ceiling_is_refused() {
    let long = vec![0.5f32; MAX_ALIGN_SAMPLES + 1];
    let reference = envelope(500, 37);
    assert!(align(&long, &reference, config()).is_none());
    assert!(align(&reference, &long, config()).is_none());
}

#[test]
fn a_search_wider_than_the_ceiling_is_refused() {
    assert_eq!(
        AlignConfig::new(MAX_ALIGN_LAG + 1, 64)
            .expect_err("the ceiling is enforced")
            .field(),
        "max_lag"
    );
    assert_eq!(
        AlignConfig::new(0, 64)
            .expect_err("a search of nothing is not a search")
            .field(),
        "max_lag"
    );
    assert_eq!(
        AlignConfig::new(100, 1)
            .expect_err("one sample is not a match")
            .field(),
        "min_overlap"
    );
}

#[test]
fn a_shift_beyond_any_fixed_radius_is_found() {
    let full = envelope(20_000, 41);
    // 5000 samples apart: two minutes at the rate the editor decodes peaks, and
    // far outside the radius a fixed search would use.
    let candidate = full[5_000..].to_vec();
    let found = align_anywhere(&full, &candidate).expect("a shifted copy aligns");
    assert_eq!(found.lag, 5_000, "score {}", found.score);
    assert!(found.score > 0.99, "got {}", found.score);
}

#[test]
fn a_shift_that_is_not_a_multiple_of_the_pooling_is_exact() {
    // Longer than the budget, so the coarse pass pools by more than one and the
    // true lag falls between two coarse samples. The refinement is what makes
    // the answer exact, and a frame of picture is two peaks at 40 per second:
    // rounding to the coarse grid would be visibly out of sync.
    let full = envelope(30_000, 43);
    assert!(full.len() > ALIGN_BUDGET, "the coarse pass must pool");
    let candidate = full[3_733..].to_vec();
    let found = align_anywhere(&full, &candidate).expect("a shifted copy aligns");
    assert_eq!(found.lag, 3_733, "score {}", found.score);
}

#[test]
fn a_radius_free_search_of_an_earlier_start_is_negative() {
    let full = envelope(12_000, 47);
    let reference = full[2_500..].to_vec();
    let found = align_anywhere(&reference, &full).expect("a shifted copy aligns");
    assert_eq!(found.lag, -2_500, "score {}", found.score);
}

#[test]
fn a_radius_free_search_refuses_what_carries_no_evidence() {
    let reference = envelope(5_000, 53);
    assert!(
        align_anywhere(&reference, &vec![0.0f32; 5_000]).is_none(),
        "silence carries no evidence about time"
    );
    assert!(
        align_anywhere(&reference, &[0.3, 0.9, 0.1]).is_none(),
        "three samples are not a recording"
    );
    assert!(
        align_anywhere(&vec![0.5f32; MAX_ALIGN_SAMPLES + 1], &reference).is_none(),
        "an input past the ceiling is refused rather than searched"
    );
}

#[test]
fn a_radius_free_search_of_unrelated_recordings_scores_low() {
    let reference = envelope(8_000, 59);
    let mut other = envelope(8_000, 61);
    for (i, value) in other.iter_mut().enumerate() {
        if i % 37 == 0 {
            *value = 0.1;
        }
        if i % 19 == 0 {
            *value = 0.9;
        }
    }
    let found = align_anywhere(&reference, &other).expect("both envelopes vary");
    assert!(
        found.score < 0.5,
        "unrelated envelopes must not read as a match, got {} at lag {}",
        found.score,
        found.lag
    );
}

#[test]
fn a_recording_beats_its_own_rivals_and_a_repeating_sound_does_not() {
    // The two populations a caller's margin has to sit between. A recording
    // matches in one place, so the best score at any lag further than
    // `RIVAL_GAP` away is far below it. A repeating sound matches at every
    // repeat, so its rival is its equal and the reported lag is whichever
    // repeat the search reached first.
    let full = envelope(20_000, 67);
    let shifted = full[1_500..].to_vec();
    let once = align_anywhere(&full, &shifted).expect("a shifted copy aligns");
    assert_eq!(once.lag, 1_500);
    assert!(
        once.score - once.rival > 0.5,
        "a recording scored {} against a rival of {}, which is not a clear answer",
        once.score,
        once.rival
    );

    let tick: Vec<f32> = (0..20_000)
        .map(|i| if i % 23 == 0 { 0.9 } else { 0.05 })
        .collect();
    let ticked = tick[1_500..].to_vec();
    let over = align_anywhere(&tick, &ticked).expect("a tick varies");
    assert!(
        over.score - over.rival < 0.01,
        "a repeating sound scored {} against a rival of {}, so it looks like a \
         single answer when it is one of many",
        over.score,
        over.rival
    );
}

/// A repeatable envelope of broad transients: each event rises and falls over
/// several samples, which is the shape a decoded peak envelope has.
///
/// Sharpness is what decides whether a peak has shoulders at all. The noise-like
/// envelope above decorrelates one sample either side of the match, so a test
/// built on it cannot tell whether [`RIVAL_GAP`] does anything: the events here
/// are four samples wide, narrower than the gap and wider than one lag, so the
/// lag next door scores nearly as well as the match and a distant one does not.
fn smooth_envelope(len: usize, seed: u32) -> Vec<f32> {
    const HALF: usize = 4;
    let mut state = seed | 1;
    let mut values = vec![0.05f32; len];
    for centre in 0..len {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if (state >> 16) as f32 / 65_535.0 <= 0.98 {
            continue;
        }
        for offset in 0..=HALF {
            let height = 1.0 - offset as f32 / (HALF + 1) as f32;
            if let Some(value) = values.get_mut(centre + offset) {
                *value = value.max(height);
            }
            if let Some(value) = centre.checked_sub(offset).and_then(|i| values.get_mut(i)) {
                *value = value.max(height);
            }
        }
    }
    values
}

#[test]
fn a_rival_needs_room_to_be_a_rival() {
    // The shoulder of the winning peak is the same answer seen from beside it,
    // so a lag inside `RIVAL_GAP` is not counted. Counting it would call every
    // match ambiguous: over an envelope with events this wide, one sample off
    // scores about 0.9 against the match's 1.0.
    let reference = smooth_envelope(2_000, 71);
    let found = align(&reference, &reference, config()).expect("a signal aligns with itself");
    assert_eq!(found.lag, 0);
    assert!(
        found.score - found.rival > 0.5,
        "an exact match scored {} against a rival of {}: the shoulders inside \
         {RIVAL_GAP} samples are being counted as answers",
        found.score,
        found.rival
    );
}
