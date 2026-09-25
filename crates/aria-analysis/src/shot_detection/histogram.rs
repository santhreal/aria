//! Histogram calculation and Earth Mover's Distance for shot boundary detection.

use super::detector::{Rgba8Frame, ShotDetectionConfig};

#[derive(Debug)]
pub(crate) struct FrameFeatures {
    pub(crate) sampled_rgba: Vec<[u8; 4]>,
    pub(crate) luma_histogram: Vec<u32>,
    pub(crate) luma_sum: u64,
}

impl FrameFeatures {
    pub(crate) fn new(maximum_sampled_pixels: usize, histogram_bins: usize) -> Self {
        Self {
            sampled_rgba: Vec::with_capacity(maximum_sampled_pixels),
            luma_histogram: vec![0; histogram_bins],
            luma_sum: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.sampled_rgba.clear();
        self.luma_histogram.fill(0);
        self.luma_sum = 0;
    }
}

pub(crate) fn extract_features(
    frame: Rgba8Frame<'_>,
    config: ShotDetectionConfig,
    output: &mut FrameFeatures,
) {
    output.clear();
    debug_assert_eq!(output.luma_histogram.len(), config.luma_histogram_bins());

    let sample_columns = frame.width().min(config.sample_width());
    let sample_rows = frame.height().min(config.sample_height());

    for sample_y in 0..sample_rows {
        let source_y = sample_coordinate(sample_y, frame.height(), sample_rows);
        let row_start = source_y * frame.row_stride();
        for sample_x in 0..sample_columns {
            let source_x = sample_coordinate(sample_x, frame.width(), sample_columns);
            let pixel_start = row_start + source_x * 4;
            let rgba = [
                frame.bytes()[pixel_start],
                frame.bytes()[pixel_start + 1],
                frame.bytes()[pixel_start + 2],
                frame.bytes()[pixel_start + 3],
            ];
            let luma = rgb_luma(rgba);
            let histogram_bin =
                usize::from(luma) * config.luma_histogram_bins() / (usize::from(u8::MAX) + 1);

            output.sampled_rgba.push(rgba);
            output.luma_histogram[histogram_bin] += 1;
            output.luma_sum += u64::from(luma);
        }
    }
}

pub(crate) fn sample_coordinate(index: usize, source_length: usize, sample_count: usize) -> usize {
    let denominator = sample_count * 2;
    let multiplier = index * 2 + 1;
    let quotient = source_length / denominator;
    let remainder = source_length % denominator;
    quotient * multiplier + (remainder * multiplier) / denominator
}

pub(crate) fn rgb_luma(rgba: [u8; 4]) -> u8 {
    let weighted =
        77 * u32::from(rgba[0]) + 150 * u32::from(rgba[1]) + 29 * u32::from(rgba[2]) + 128;
    (weighted >> 8) as u8
}

pub(crate) fn histogram_earth_mover_distance(
    previous: &FrameFeatures,
    current: &FrameFeatures,
) -> f64 {
    let sample_count = previous.sampled_rgba.len() as u64;
    let interval_count = previous.luma_histogram.len() - 1;
    let mut cumulative_difference = 0_i64;
    let mut transport = 0_u64;

    for index in 0..interval_count {
        cumulative_difference +=
            i64::from(current.luma_histogram[index]) - i64::from(previous.luma_histogram[index]);
        transport += cumulative_difference.unsigned_abs();
    }

    transport as f64 / (sample_count * interval_count as u64) as f64
}
