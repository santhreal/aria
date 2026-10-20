//! Gain arithmetic: decibels against the linear multiplier a mixer applies, and
//! the multiplier that moves a measured level onto a target.
//!
//! A fader is linear, a meter reads decibels, and a normalisation is the
//! difference between a measurement and a target. Those three conversions were
//! being written at each call site, so a clip's gain, an export's headroom, and
//! a normalise action could disagree about what −6 dB means. They live here
//! instead, next to the measurement they act on.
//!
//! Normalisation refuses rather than guesses. A measurement below
//! [`NORMALISATION_FLOOR_DBFS`] is silence, and lifting silence onto a target
//! amplifies only noise, so it is an error rather than a large number. A boost
//! is capped at [`MAX_NORMALISATION_BOOST_DB`]; an attenuation is not, since
//! turning something down can never clip.

use crate::ConfigError;

/// The largest boost a normalisation applies, in decibels.
///
/// A quiet take can need a lift; a take that needs more than this is either
/// mis-measured or the wrong material, and a silent boost of 60 dB is a wall of
/// noise rather than a fix.
pub const MAX_NORMALISATION_BOOST_DB: f32 = 24.0;

/// The quietest measurement worth normalising, in dBFS.
pub const NORMALISATION_FLOOR_DBFS: f32 = -70.0;

/// The linear multiplier for a level in decibels. `0 dB` is unity.
#[must_use]
pub fn linear_from_db(decibels: f32) -> f32 {
    if !decibels.is_finite() {
        return if decibels < 0.0 { 0.0 } else { f32::INFINITY };
    }
    10.0_f32.powf(decibels / 20.0)
}

/// The level in decibels of a linear multiplier.
///
/// Silence has no decibel value, so a multiplier that is not a positive finite
/// number returns `None` instead of an infinity a field would have to print.
#[must_use]
pub fn db_from_linear(linear: f32) -> Option<f32> {
    if !linear.is_finite() || linear <= 0.0 {
        return None;
    }
    Some(20.0 * linear.log10())
}

/// The linear multiplier that moves `measured_dbfs` onto `target_dbfs`.
///
/// Both levels are dBFS, so a target above `0` would ask for clipping and is
/// refused. The result is a factor to apply to whatever gain the clip already
/// carries, not an absolute fader value.
///
/// # Errors
///
/// Returns [`ConfigError`] when either level is not finite, when the target is
/// above `0 dBFS`, or when the measurement is at or below
/// [`NORMALISATION_FLOOR_DBFS`].
pub fn normalisation_gain(measured_dbfs: f32, target_dbfs: f32) -> Result<f32, ConfigError> {
    if !measured_dbfs.is_finite() {
        return Err(ConfigError::new("measured_dbfs", "must be finite"));
    }
    if !target_dbfs.is_finite() {
        return Err(ConfigError::new("target_dbfs", "must be finite"));
    }
    if target_dbfs > 0.0 {
        return Err(ConfigError::new(
            "target_dbfs",
            "must not exceed 0 dBFS, which would ask for clipping",
        ));
    }
    if measured_dbfs <= NORMALISATION_FLOOR_DBFS {
        return Err(ConfigError::new(
            "measured_dbfs",
            "is silence; there is nothing to normalise",
        ));
    }
    let change = (target_dbfs - measured_dbfs).min(MAX_NORMALISATION_BOOST_DB);
    Ok(linear_from_db(change))
}

#[cfg(test)]
mod tests;
