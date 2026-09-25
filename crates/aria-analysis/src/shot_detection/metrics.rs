//! Metrics and scoring for scene change detection.

use super::histogram::FrameFeatures;
use crate::ConfigError;

/// Relative weights for the two normalized scene-change components.
///
/// Weights are normalized by their sum when scoring, so they need not add to
/// one. At least one must be positive. The histogram component measures
/// luminance-distribution movement; the pixel component measures sampled RGBA
/// change after compensating RGB channels for the global mean-luma shift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotScoreWeights {
    histogram_weight: f32,
    pixel_difference_weight: f32,
}

impl ShotScoreWeights {
    /// Constructs validated relative score weights.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when either weight is non-finite or negative, or
    /// when both weights are zero.
    pub fn new(histogram_weight: f32, pixel_difference_weight: f32) -> Result<Self, ConfigError> {
        if !histogram_weight.is_finite() || histogram_weight < 0.0 {
            return Err(ConfigError::new(
                "histogram_weight",
                "must be finite and non-negative",
            ));
        }
        if !pixel_difference_weight.is_finite() || pixel_difference_weight < 0.0 {
            return Err(ConfigError::new(
                "pixel_difference_weight",
                "must be finite and non-negative",
            ));
        }
        if histogram_weight == 0.0 && pixel_difference_weight == 0.0 {
            return Err(ConfigError::new(
                "score_weights",
                "at least one score weight must be positive",
            ));
        }

        Ok(Self {
            histogram_weight,
            pixel_difference_weight,
        })
    }

    /// Relative weight of normalized luminance-histogram distance.
    #[must_use]
    pub const fn histogram_weight(self) -> f32 {
        self.histogram_weight
    }

    /// Relative weight of brightness-compensated sampled RGBA difference.
    #[must_use]
    pub const fn pixel_difference_weight(self) -> f32 {
        self.pixel_difference_weight
    }
}

impl Default for ShotScoreWeights {
    fn default() -> Self {
        Self {
            histogram_weight: 0.4,
            pixel_difference_weight: 0.6,
        }
    }
}

pub(crate) fn scene_change_score(
    previous: &FrameFeatures,
    current: &FrameFeatures,
    weights: ShotScoreWeights,
) -> f32 {
    debug_assert_eq!(previous.sampled_rgba.len(), current.sampled_rgba.len());
    debug_assert_eq!(previous.luma_histogram.len(), current.luma_histogram.len());

    let histogram_distance = super::histogram::histogram_earth_mover_distance(previous, current);
    let pixel_difference = compensated_pixel_difference(previous, current);
    let histogram_weight = f64::from(weights.histogram_weight);
    let pixel_weight = f64::from(weights.pixel_difference_weight);
    let weight_sum = histogram_weight + pixel_weight;

    ((histogram_weight * histogram_distance + pixel_weight * pixel_difference) / weight_sum)
        .clamp(0.0, 1.0) as f32
}

pub(crate) fn compensated_pixel_difference(
    previous: &FrameFeatures,
    current: &FrameFeatures,
) -> f64 {
    let sample_count = previous.sampled_rgba.len();
    let mean_luma_shift =
        (current.luma_sum as f64 - previous.luma_sum as f64) / sample_count as f64;
    let mut absolute_difference = 0.0_f64;

    for (&previous_rgba, &current_rgba) in previous.sampled_rgba.iter().zip(&current.sampled_rgba) {
        for channel in 0..3 {
            let channel_delta =
                f64::from(current_rgba[channel]) - f64::from(previous_rgba[channel]);
            absolute_difference += (channel_delta - mean_luma_shift).abs();
        }
        absolute_difference += (f64::from(current_rgba[3]) - f64::from(previous_rgba[3])).abs();
    }

    let maximum_uncompensated_difference = sample_count as f64 * 4.0 * f64::from(u8::MAX);
    (absolute_difference / maximum_uncompensated_difference).clamp(0.0, 1.0)
}
