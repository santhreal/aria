//! What the badge says a rate is.
//!
//! The readout is the rate itself, not an index into a list of speeds, so it
//! must round-trip: an operator reading `-4x` off the badge and pressing J four
//! times has to arrive where the badge said they already were. The label is
//! asserted over every rate the transport can hold rather than a sample of them.
//!
//! Not covered: that the badge is drawn, which needs a window. Its presence rule
//! is `AppState::audio_follows`, asserted in `app/shuttle_tests.rs`.

use super::rate_label;

/// The ladder the transport clamps to, both directions.
const RATES: &[i32] = &[-8, -4, -2, -1, 2, 4, 8];

#[test]
fn a_rate_reads_back_as_the_rate_it_is() {
    for &rate in RATES {
        let label = rate_label(rate);
        let stripped = label
            .strip_suffix('x')
            .unwrap_or_else(|| panic!("{label} does not say what unit it is in"));
        assert_eq!(
            stripped.parse::<i32>().ok(),
            Some(rate),
            "{label} does not read back as {rate}"
        );
    }
}

#[test]
fn a_reversed_rate_says_so_before_the_number() {
    assert_eq!(rate_label(-2), "-2x");
    assert_eq!(rate_label(2), "2x");
}

/// 1x is the one rate the badge is never drawn at, and the label must not be the
/// thing that decides that: it answers for any rate it is handed.
#[test]
fn the_label_answers_for_the_rate_the_badge_hides_too() {
    assert_eq!(rate_label(1), "1x");
}
