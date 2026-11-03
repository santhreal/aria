//! Metrics and scoring for scene change detection.

use super::histogram::FrameFeatures;
use crate::ConfigError;

/// Relative weights for the two normalized scene-change components.
///
/// Weights are normalized by their sum when scoring, so they need not add to
/// one. At least one must be positive. The histogram component measures
/// luminance-distribution movement; the pixel component measures sampled RGBA
/// change after compensating for exposure shifts and fades.
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

    let weighted =
        (histogram_weight * histogram_distance + pixel_weight * pixel_difference) / weight_sum;
    // A cut inside a still frame region moves few luma-histogram bins, so when the
    // pixel term weighs at least as much as the histogram term it also scores alone.
    let score = if pixel_weight >= histogram_weight {
        weighted.max(pixel_difference)
    } else {
        weighted
    };
    score.clamp(0.0, 1.0) as f32
}

/// Mean luma at or below which a frame is black, so no exposure ratio is formed
/// against it.
const BLACK_MEAN_LUMA: f64 = 4.0;

/// Smallest exposure ratio compensated as a gain: a frame darker than this share
/// of the other's mean compares as new content.
const MIN_EXPOSURE_RATIO: f64 = 0.15;

/// Share of samples, in percent, averaged as the most-changed group: a cut that
/// replaces a part of the frame scores on the samples it replaced.
const CHANGED_SHARE_PERCENT: usize = 15;

/// Gain from the blended per-sample difference to the score scale.
const DIFFERENCE_GAIN: f64 = 2.10;

/// Sampled RGBA change with exposure changes compensated.
///
/// Each sample takes the smaller of two channel differences: one after removing
/// the mean luma shift (an additive exposure change) and one after scaling the
/// brighter frame by the ratio of the means (a gain change, as in a fade). The
/// alpha difference is a floor. The score blends the mean over every sample with
/// the mean over the most-changed [`CHANGED_SHARE_PERCENT`] of samples.
pub(crate) fn compensated_pixel_difference(
    previous: &FrameFeatures,
    current: &FrameFeatures,
) -> f64 {
    let sample_count = previous.sampled_rgba.len();
    if sample_count == 0 {
        return 0.0;
    }
    let samples = sample_count as f64;
    let previous_mean = previous.luma_sum as f64 / samples;
    let current_mean = current.luma_sum as f64 / samples;
    let mean_luma_shift = current_mean - previous_mean;

    // The ratio scales the brighter frame down to the darker one's exposure.
    let (current_is_brighter, exposure_ratio) =
        if previous_mean > BLACK_MEAN_LUMA && current_mean > BLACK_MEAN_LUMA {
            if current_mean >= previous_mean {
                (true, (previous_mean / current_mean).max(MIN_EXPOSURE_RATIO))
            } else {
                (
                    false,
                    (current_mean / previous_mean).max(MIN_EXPOSURE_RATIO),
                )
            }
        } else {
            (true, 1.0)
        };

    let mut difference_sum = 0.0_f64;
    let mut difference_counts = [0_u32; 256];
    for (&previous_rgba, &current_rgba) in previous.sampled_rgba.iter().zip(&current.sampled_rgba) {
        let mut shifted = 0.0_f64;
        let mut scaled = 0.0_f64;
        for channel in 0..3 {
            let before = f64::from(previous_rgba[channel]);
            let after = f64::from(current_rgba[channel]);
            let shifted_channel = ((after - before) - mean_luma_shift).abs();
            let scaled_channel = if exposure_ratio < 1.0 {
                if current_is_brighter {
                    (after * exposure_ratio - before).abs()
                } else {
                    (after - before * exposure_ratio).abs()
                }
            } else {
                shifted_channel
            };
            shifted = shifted.max(shifted_channel);
            scaled = scaled.max(scaled_channel);
        }
        let alpha = (f64::from(current_rgba[3]) - f64::from(previous_rgba[3])).abs();
        let difference = shifted.min(scaled).max(alpha);
        difference_sum += difference;
        difference_counts[(difference.round() as usize).min(255)] += 1;
    }

    let mean_difference = difference_sum / (samples * f64::from(u8::MAX));
    let changed_count = (sample_count * CHANGED_SHARE_PERCENT / 100).max(1);
    let mut remaining = changed_count;
    let mut changed_sum = 0_u64;
    for (difference, &count) in difference_counts.iter().enumerate().rev() {
        let taken = (count as usize).min(remaining);
        changed_sum += taken as u64 * difference as u64;
        remaining -= taken;
        if remaining == 0 {
            break;
        }
    }
    let changed_difference = changed_sum as f64 / (changed_count as f64 * f64::from(u8::MAX));

    ((mean_difference + changed_difference) / 2.0 * DIFFERENCE_GAIN).clamp(0.0, 1.0)
}
