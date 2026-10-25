//! Contracts for gain arithmetic.
//!
//! The class closed here: a decibel conversion written at the call site. Each
//! copy picked its own base, its own handling of silence, and its own answer for
//! a target that would clip, so the same −6 dB meant two different faders. These
//! pin the conversion, the round trip, and every refusal.
//!
//! Not covered: what a mixer does with the multiplier — `aria-render`'s mixer
//! tests own that.

use super::{
    MAX_NORMALISATION_BOOST_DB, NORMALISATION_FLOOR_DBFS, db_from_linear, linear_from_db,
    normalisation_gain,
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
