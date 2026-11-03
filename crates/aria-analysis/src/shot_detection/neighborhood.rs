//! The context test a cut candidate passes after its own score clears the threshold.
//!
//! A frame-to-frame score alone cannot tell a cut from fast motion or a flash: a
//! whip pan scores high on every frame, and a camera flash scores high on the frame
//! that flashes and on the frame after it. The neighborhood test compares a
//! candidate with the scores on each side of it, so a cut must stand out from the
//! frames before it and from the frames after it, which motion that starts or
//! stops on the candidate does not. It also compares frames a few frames apart
//! across the candidate, so the new picture must last and the old one must have
//! lasted.

use crate::ConfigError;

/// Largest accepted neighborhood radius and persistence span, in frames.
///
/// Both spans set how many frames the detector retains and how long a candidate
/// waits before it resolves, so both are bounded.
pub const MAX_SHOT_CONTEXT_FRAMES: usize = 15;

/// How a cut candidate is tested against the frames around it.
///
/// A candidate at frame `k` with score `s` is a cut when:
///
/// - `s >= contrast_ratio * max(b, a) + contrast_margin`, where `b` is the median
///   score of the `radius_frames` frames before `k` and `a` is the median score
///   of the `radius_frames` frames after `k`;
/// - the change from frame `k - 1` to frame `k + persistence_frames`, and the
///   change from frame `k - 1 - persistence_frames` to frame `k`, each score at
///   least `persistence_ratio * s`.
///
/// [`ShotNeighborhood::OFF`] passes every candidate, which leaves the plain score
/// threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotNeighborhood {
    radius_frames: usize,
    contrast_ratio: f32,
    contrast_margin: f32,
    persistence_frames: usize,
    persistence_ratio: f32,
}

impl ShotNeighborhood {
    /// A neighborhood test that passes every candidate.
    pub const OFF: Self = Self {
        radius_frames: 0,
        contrast_ratio: 0.0,
        contrast_margin: 0.0,
        persistence_frames: 0,
        persistence_ratio: 0.0,
    };

    /// Constructs a validated neighborhood test.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a span exceeds [`MAX_SHOT_CONTEXT_FRAMES`], the
    /// contrast ratio is negative or non-finite, or the contrast margin or the
    /// persistence ratio is outside `[0, 1]`.
    pub fn new(
        radius_frames: usize,
        contrast_ratio: f32,
        contrast_margin: f32,
        persistence_frames: usize,
        persistence_ratio: f32,
    ) -> Result<Self, ConfigError> {
        if radius_frames > MAX_SHOT_CONTEXT_FRAMES {
            return Err(ConfigError::new(
                "radius_frames",
                "must be at most MAX_SHOT_CONTEXT_FRAMES",
            ));
        }
        if !contrast_ratio.is_finite() || contrast_ratio < 0.0 {
            return Err(ConfigError::new(
                "contrast_ratio",
                "must be finite and non-negative",
            ));
        }
        if !(0.0..=1.0).contains(&contrast_margin) {
            return Err(ConfigError::new(
                "contrast_margin",
                "must be between 0 and 1",
            ));
        }
        if persistence_frames > MAX_SHOT_CONTEXT_FRAMES {
            return Err(ConfigError::new(
                "persistence_frames",
                "must be at most MAX_SHOT_CONTEXT_FRAMES",
            ));
        }
        if !(0.0..=1.0).contains(&persistence_ratio) {
            return Err(ConfigError::new(
                "persistence_ratio",
                "must be between 0 and 1",
            ));
        }
        Ok(Self {
            radius_frames,
            contrast_ratio,
            contrast_margin,
            persistence_frames,
            persistence_ratio,
        })
    }

    /// Frames on each side of a candidate whose scores form that side's median.
    #[must_use]
    pub const fn radius_frames(self) -> usize {
        self.radius_frames
    }

    /// Multiple of the larger side median a candidate's score must reach.
    #[must_use]
    pub const fn contrast_ratio(self) -> f32 {
        self.contrast_ratio
    }

    /// Score a candidate must clear above the scaled side median.
    #[must_use]
    pub const fn contrast_margin(self) -> f32 {
        self.contrast_margin
    }

    /// Frames across a candidate over which its new picture must last and its old
    /// picture must have lasted. Zero skips the persistence test.
    #[must_use]
    pub const fn persistence_frames(self) -> usize {
        self.persistence_frames
    }

    /// Share of a candidate's score each comparison across it must keep.
    #[must_use]
    pub const fn persistence_ratio(self) -> f32 {
        self.persistence_ratio
    }

    /// Frames a candidate waits for before it resolves: the larger span.
    #[must_use]
    pub const fn lookahead_frames(self) -> usize {
        if self.radius_frames > self.persistence_frames {
            self.radius_frames
        } else {
            self.persistence_frames
        }
    }

    /// Frames the detector retains: from the earliest frame the oldest unresolved
    /// candidate compares with (at least the frame before it) to the newest frame.
    pub(crate) const fn retained_frames(self) -> usize {
        let behind = if self.radius_frames > self.persistence_frames {
            self.radius_frames
        } else {
            self.persistence_frames + 1
        };
        self.lookahead_frames() + behind + 1
    }

    /// Whether `score` stands out from the median of the scores `before` it and
    /// from the median of the scores `after` it by the configured contrast.
    /// Reorders both slices.
    pub(crate) fn stands_out(self, score: f32, before: &mut [f32], after: &mut [f32]) -> bool {
        score >= self.contrast_ratio * median(before).max(median(after)) + self.contrast_margin
    }

    /// Whether a comparison across a candidate scoring `lasting` keeps enough of
    /// `score`.
    pub(crate) fn persists(self, score: f32, lasting: f32) -> bool {
        lasting >= self.persistence_ratio * score
    }
}

impl Default for ShotNeighborhood {
    fn default() -> Self {
        Self {
            radius_frames: 2,
            contrast_ratio: 1.75,
            contrast_margin: 0.05,
            persistence_frames: 5,
            persistence_ratio: 0.6,
        }
    }
}

/// The median of `values`, the mean of the middle pair for an even count, and
/// zero for none. Reorders `values`.
fn median(values: &mut [f32]) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    let middle = values.len() / 2;
    match values.len() {
        0 => 0.0,
        len if len % 2 == 1 => values[middle],
        _ => (values[middle - 1] + values[middle]) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_takes_the_middle_value_or_the_mean_of_the_middle_pair() {
        assert_eq!(median(&mut []), 0.0);
        assert_eq!(median(&mut [0.4]), 0.4);
        assert_eq!(median(&mut [0.9, 0.1, 0.3]), 0.3);
        assert_eq!(median(&mut [0.9, 0.1, 0.3, 0.5]), 0.4);
    }
}
