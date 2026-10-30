//! Contracts for gain arithmetic.
//!
//! The class closed here: a decibel conversion written at the call site. Each
//! copy picked its own base, its own handling of silence, and its own answer for
//! a target that would clip, so the same −6 dB meant two different faders. These
//! pin the conversion, the round trip, and every refusal. The travel tests pin
//! the one floor and the one taper a fader, a clip's volume row and its volume
//! line stand on: where unity, the floor, silence and the ceiling stand, the
//! round trip, and a travel or ceiling with no meaning.
//!
//! Not covered: what a mixer does with the multiplier — `aria-render`'s mixer
//! tests own that — and whether each control calls the travel, which the
//! controls' own tests measure.

use super::{
    GAIN_FLOOR_DB, MAX_NORMALISATION_BOOST_DB, NORMALISATION_FLOOR_DBFS, db_from_linear,
    gain_at_travel, gain_travel, linear_from_db, normalisation_gain,
};

/// Decibels are logarithmic, so equality is a tolerance.
fn close(left: f32, right: f32, tolerance: f32) -> bool {
    (left - right).abs() <= tolerance
}

#[test]
fn zero_decibels_is_unity() {
    assert_eq!(linear_from_db(0.0), 1.0);
    assert_eq!(db_from_linear(1.0), Some(0.0));
}

#[test]
fn six_decibels_is_a_doubling() {
    assert!(
        close(linear_from_db(6.0206), 2.0, 0.001),
        "got {}",
        linear_from_db(6.0206)
    );
    assert!(
        close(linear_from_db(-6.0206), 0.5, 0.001),
        "got {}",
        linear_from_db(-6.0206)
    );
}

#[test]
fn the_conversion_round_trips() {
    for decibels in [-48.0, -24.0, -6.0, -0.5, 0.0, 3.0, 12.0] {
        let linear = linear_from_db(decibels);
        let back = db_from_linear(linear).expect("a positive multiplier has a level");
        assert!(close(back, decibels, 0.001), "{decibels} became {back}");
    }
}

#[test]
fn silence_has_no_decibel_value() {
    assert_eq!(db_from_linear(0.0), None);
    assert_eq!(db_from_linear(-1.0), None);
    assert_eq!(db_from_linear(f32::NAN), None);
    assert_eq!(db_from_linear(f32::INFINITY), None);
}

#[test]
fn an_infinite_level_saturates_rather_than_producing_a_nan() {
    assert_eq!(linear_from_db(f32::NEG_INFINITY), 0.0);
    assert_eq!(linear_from_db(f32::INFINITY), f32::INFINITY);
}

/// The ceiling of a clip's volume, +20 dB, so unity stands three quarters of
/// the way along a travel from -60 dB.
const TEN_TIMES: f32 = 10.0;

#[test]
fn a_gain_stands_where_its_decibels_fall_between_the_floor_and_the_ceiling() {
    assert_eq!(GAIN_FLOOR_DB, -60.0);
    for (decibels, travel) in [(0.0, 0.75), (-6.0, 0.675), (-30.0, 0.375), (10.0, 0.875)] {
        let at = gain_travel(linear_from_db(decibels), TEN_TIMES);
        assert!(
            close(at, travel, 1e-4),
            "{decibels} dB stands at {at}, expected {travel}"
        );
    }
}

#[test]
fn the_floor_silence_and_the_ceiling_bound_the_travel() {
    let floor = linear_from_db(GAIN_FLOOR_DB);
    assert!(close(gain_travel(floor, TEN_TIMES), 0.0, 1e-5));
    for under in [linear_from_db(GAIN_FLOOR_DB - 12.0), 0.0, -1.0, f32::NAN] {
        assert_eq!(
            gain_travel(under, TEN_TIMES),
            0.0,
            "{under} is under the floor"
        );
    }
    for over in [TEN_TIMES, 40.0, f32::INFINITY] {
        assert_eq!(
            gain_travel(over, TEN_TIMES),
            1.0,
            "{over} is over the ceiling"
        );
    }
}

#[test]
fn the_travel_round_trips_above_the_floor() {
    for travel in [0.05, 0.25, 0.5, 0.75, 0.99] {
        for ceiling in [1.0, 4.0, TEN_TIMES] {
            let back = gain_travel(gain_at_travel(travel, ceiling), ceiling);
            assert!(
                close(back, travel, 1e-4),
                "{travel} of a travel to {ceiling} came back as {back}"
            );
        }
    }
}

#[test]
fn the_ends_of_the_travel_are_silence_and_the_ceiling() {
    assert_eq!(gain_at_travel(1.0, TEN_TIMES), TEN_TIMES);
    assert_eq!(gain_at_travel(3.0, TEN_TIMES), TEN_TIMES);
    for start in [0.0, -0.5, f32::NAN, f32::NEG_INFINITY] {
        assert_eq!(gain_at_travel(start, TEN_TIMES), 0.0, "{start} is silence");
    }
    let unity = gain_at_travel(0.75, TEN_TIMES);
    assert!(
        close(unity, 1.0, 1e-5),
        "three quarters is unity, got {unity}"
    );
}

#[test]
fn a_travel_whose_ceiling_is_not_above_the_floor_has_no_length() {
    for ceiling in [
        linear_from_db(GAIN_FLOOR_DB - 6.0),
        0.0,
        -1.0,
        f32::NAN,
        f32::INFINITY,
    ] {
        assert_eq!(gain_travel(1.0, ceiling), 0.0, "ceiling {ceiling}");
        assert_eq!(gain_at_travel(0.5, ceiling), 0.0, "ceiling {ceiling}");
    }
}

#[test]
fn normalising_lifts_a_quiet_measurement_onto_the_target() {
    let gain = normalisation_gain(-18.0, -6.0).expect("a 12 dB lift");
    assert!(close(gain, linear_from_db(12.0), 0.0001), "got {gain}");
}

#[test]
fn normalising_attenuates_a_hot_measurement_without_a_cap() {
    // Turning something down cannot clip, so the boost cap must not apply.
    let gain = normalisation_gain(-1.0, -60.0).expect("a 59 dB cut");
    assert!(close(gain, linear_from_db(-59.0), 0.0001), "got {gain}");
}

#[test]
fn a_boost_is_capped() {
    let gain = normalisation_gain(-60.0, 0.0).expect("a capped lift");
    assert!(
        close(gain, linear_from_db(MAX_NORMALISATION_BOOST_DB), 0.0001),
        "got {gain}, expected the cap"
    );
}

#[test]
fn a_target_above_full_scale_is_refused() {
    let error = normalisation_gain(-12.0, 0.5).expect_err("clipping is not a target");
    assert_eq!(error.field(), "target_dbfs");
}

#[test]
fn silence_is_refused_rather_than_amplified() {
    // The floor itself and anything under it: both are silence, and the refusal
    // names the same field so a surface has one message to show.
    for measured in [NORMALISATION_FLOOR_DBFS, -120.0, -1000.0] {
        let error =
            normalisation_gain(measured, -6.0).expect_err("silence has nothing to normalize");
        assert_eq!(error.field(), "measured_dbfs", "at {measured} dBFS");
    }
}

#[test]
fn a_level_that_is_not_a_number_is_refused_on_either_side() {
    assert_eq!(
        normalisation_gain(f32::NAN, -6.0)
            .expect_err("a measurement must be finite")
            .field(),
        "measured_dbfs"
    );
    assert_eq!(
        normalisation_gain(-6.0, f32::NEG_INFINITY)
            .expect_err("a target must be finite")
            .field(),
        "target_dbfs"
    );
}

#[test]
fn a_target_at_the_measurement_is_unity() {
    let gain = normalisation_gain(-14.0, -14.0).expect("no change");
    assert!(close(gain, 1.0, 0.0001), "got {gain}");
}

#[test]
fn boundary_cases_return_typed_errors_without_panics() {
    // Extreme/boundary levels
    assert!(normalisation_gain(0.0, 0.0).is_ok());
    assert!(normalisation_gain(-0.0001, 0.0).is_ok());
    assert!(normalisation_gain(-69.999, -1.0).is_ok());
    assert!(normalisation_gain(-70.0, -1.0).is_err());
    assert!(normalisation_gain(-70.0001, -1.0).is_err());
    assert!(normalisation_gain(0.0001, 0.0).is_ok()); // Target 0 is max
    assert!(normalisation_gain(-10.0, 0.0001).is_err());
    assert!(normalisation_gain(f32::NAN, f32::NAN).is_err());
    assert!(normalisation_gain(f32::INFINITY, 0.0).is_err());
    assert!(normalisation_gain(-10.0, f32::INFINITY).is_err());
}
