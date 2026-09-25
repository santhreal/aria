//! Reusable deterministic shot boundary detection over decoded RGBA8 frames.

mod detector;
mod histogram;
mod metrics;

pub use detector::{
    MAX_SHOT_HISTOGRAM_BINS, MAX_SHOT_SAMPLE_HEIGHT, MAX_SHOT_SAMPLE_WIDTH, Rgba8Frame,
    Rgba8FrameError, ShotBoundary, ShotBoundaryKind, ShotDetectionConfig, ShotDetectionError,
    ShotDetector, detect_shots,
};
#[cfg(test)]
pub(crate) use histogram::{FrameFeatures, histogram_earth_mover_distance, sample_coordinate};
pub use metrics::ShotScoreWeights;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_sampling_stays_in_bounds_and_hits_cell_centers() {
        assert_eq!(sample_coordinate(0, 8, 4), 1);
        assert_eq!(sample_coordinate(1, 8, 4), 3);
        assert_eq!(sample_coordinate(2, 8, 4), 5);
        assert_eq!(sample_coordinate(3, 8, 4), 7);
        assert_eq!(sample_coordinate(0, 1, 1), 0);
    }

    #[test]
    fn histogram_distance_has_normalized_endpoints() {
        let mut dark = FrameFeatures::new(1, 4);
        dark.sampled_rgba.push([0; 4]);
        dark.luma_histogram[0] = 1;

        let mut bright = FrameFeatures::new(1, 4);
        bright.sampled_rgba.push([255; 4]);
        bright.luma_histogram[3] = 1;

        assert_eq!(histogram_earth_mover_distance(&dark, &dark), 0.0);
        assert_eq!(histogram_earth_mover_distance(&dark, &bright), 1.0);
    }
}
