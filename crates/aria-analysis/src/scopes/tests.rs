//! Scope measurements are asserted against known pictures.
//!
//! Each test builds a frame whose correct scope reading is arithmetic — flat
//! colours, a horizontal ramp, a vertical ramp — so a failure names a wrong
//! number rather than a changed look. The class of defect this closes: a scope
//! that plots something plausible but not the frame it was handed, which no
//! amount of looking at it catches.
//!
//! Not covered: how the scopes are drawn. That is the panel's problem, and a
//! drawn scope is reviewed against these numbers.

use super::*;

/// A frame of one colour, no padding.
fn flat(width: usize, height: usize, rgba: [u8; 4]) -> Vec<u8> {
    rgba.iter()
        .copied()
        .cycle()
        .take(width * height * 4)
        .collect()
}

fn frame(bytes: &[u8], width: usize, height: usize) -> Rgba8Frame<'_> {
    Rgba8Frame::new(bytes, width, height, width * 4, 0.0).expect("frame")
}

/// Bin holding every sample, or `None` when the mass is spread.
fn only_bin(bins: &[u32]) -> Option<usize> {
    let hit: Vec<usize> = bins
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(bin, _)| bin)
        .collect();
    match hit.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

#[test]
fn a_black_frame_sits_in_bin_zero_and_at_neutral_chroma() {
    let bytes = flat(8, 4, [0, 0, 0, 255]);
    let scopes = measure(frame(&bytes, 8, 4), ScopeConfig::default());
    assert_eq!(scopes.sampled, 32);
    assert_eq!(only_bin(&scopes.histogram.luma), Some(0));
    assert_eq!(only_bin(&scopes.histogram.red), Some(0));
    let centre = scopes.vectorscope.centre();
    assert_eq!(scopes.vectorscope.cell(centre, centre), 32);
    assert_eq!(scopes.vectorscope.peak(), 32);
}

#[test]
fn a_white_frame_sits_in_the_top_bin_and_stays_neutral() {
    let bytes = flat(8, 4, [255, 255, 255, 255]);
    let scopes = measure(frame(&bytes, 8, 4), ScopeConfig::default());
    assert_eq!(only_bin(&scopes.histogram.luma), Some(255));
    assert_eq!(only_bin(&scopes.histogram.blue), Some(255));
    let centre = scopes.vectorscope.centre();
    assert_eq!(
        scopes.vectorscope.cell(centre, centre),
        32,
        "white is unsaturated, so it belongs at the centre with black"
    );
}

#[test]
fn primary_red_reads_its_own_luma_and_lands_upper_right() {
    let bytes = flat(4, 4, [255, 0, 0, 255]);
    let scopes = measure(frame(&bytes, 4, 4), ScopeConfig::default());
    // 0.2126 * 255 = 54.2.
    assert_eq!(only_bin(&scopes.histogram.luma), Some(54));
    assert_eq!(only_bin(&scopes.histogram.red), Some(255));
    assert_eq!(only_bin(&scopes.histogram.green), Some(0));

    let (cb, cr) = chroma709(255, 0, 0);
    assert!(cb < 0.0, "red is below neutral on Cb, got {cb}");
    assert!(cr > 0.0, "red is above neutral on Cr, got {cr}");
    let centre = scopes.vectorscope.centre();
    let mut hit = None;
    for y in 0..scopes.vectorscope.size() {
        for x in 0..scopes.vectorscope.size() {
            if scopes.vectorscope.cell(x, y) > 0 {
                assert!(hit.is_none(), "one colour must occupy one cell");
                hit = Some((x, y));
            }
        }
    }
    let (x, y) = hit.expect("red plotted somewhere");
    assert!(x < centre, "negative Cb draws left of neutral");
    assert!(y < centre, "positive Cr draws above neutral");
}

#[test]
fn the_three_primaries_land_in_three_different_places() {
    let mut seen = Vec::new();
    for colour in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]] {
        let bytes = flat(4, 4, colour);
        let scopes = measure(frame(&bytes, 4, 4), ScopeConfig::default());
        let mut at = None;
        for y in 0..scopes.vectorscope.size() {
            for x in 0..scopes.vectorscope.size() {
                if scopes.vectorscope.cell(x, y) > 0 {
                    at = Some((x, y));
                }
            }
        }
        let at = at.expect("primary plotted");
        assert!(!seen.contains(&at), "two primaries share cell {at:?}");
        seen.push(at);
    }
    assert_eq!(seen.len(), 3);
}

#[test]
fn a_horizontal_ramp_reads_dark_on_the_left_and_bright_on_the_right() {
    let (width, height) = (256, 4);
    let mut bytes = Vec::with_capacity(width * height * 4);
    for _ in 0..height {
        for x in 0..width {
            let code = x as u8;
            bytes.extend_from_slice(&[code, code, code, 255]);
        }
    }
    let config = ScopeConfig::new(16, 64, 200_000).expect("config");
    let scopes = measure(frame(&bytes, width, height), config);
    assert_eq!(scopes.waveform.columns(), 16);

    let mut previous = -1.0_f32;
    for column in 0..scopes.waveform.columns() {
        let mean = scopes.waveform.column_mean(column).expect("column sampled");
        assert!(
            mean > previous,
            "column {column} mean {mean} did not rise above {previous}"
        );
        previous = mean;
    }
    let first = scopes.waveform.column_mean(0).expect("first");
    let last = scopes.waveform.column_mean(15).expect("last");
    assert!(first < 10.0, "left edge should be near black, got {first}");
    assert!(last > 245.0, "right edge should be near white, got {last}");
}

#[test]
fn a_vertical_ramp_gives_every_column_the_same_spread() {
    let (width, height) = (32, 256);
    let mut bytes = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let code = y as u8;
        for _ in 0..width {
            bytes.extend_from_slice(&[code, code, code, 255]);
        }
    }
    let config = ScopeConfig::new(8, 64, 200_000).expect("config");
    let scopes = measure(frame(&bytes, width, height), config);
    let reference = scopes.waveform.column_mean(0).expect("first column");
    for column in 1..scopes.waveform.columns() {
        let mean = scopes.waveform.column_mean(column).expect("column");
        assert!(
            (mean - reference).abs() < 0.001,
            "column {column} mean {mean} differs from {reference}"
        );
    }
    // Every code value appears once per row, and 32 pixel columns fold into 8
    // scope columns, so a cell holds the 4 pixels of that row in that column.
    assert_eq!(scopes.waveform.peak(), 4);
}

#[test]
fn transparent_pixels_are_not_measured() {
    let (width, height) = (4, 2);
    let mut bytes = flat(width, height, [255, 255, 255, 255]);
    // Blank the second row.
    for x in 0..width {
        bytes[(width + x) * 4 + 3] = 0;
    }
    let scopes = measure(frame(&bytes, width, height), ScopeConfig::default());
    assert_eq!(scopes.sampled, 4, "only the opaque row counts");
    assert_eq!(only_bin(&scopes.histogram.luma), Some(255));
    assert_eq!(scopes.histogram.luma[0], 0, "alpha-zero black stays out");
}

#[test]
fn a_frame_over_budget_is_strided_rather_than_read_whole() {
    let (width, height) = (400, 300);
    let bytes = flat(width, height, [120, 130, 140, 255]);
    let config = ScopeConfig::new(64, 64, 10_000).expect("config");
    let scopes = measure(frame(&bytes, width, height), config);
    assert!(
        scopes.sampled as usize <= config.maximum_samples(),
        "sampled {} exceeded the budget",
        scopes.sampled
    );
    assert!(scopes.sampled > 0, "a strided read still measures");
    // The picture is flat, so a strided read reports the same code values.
    assert_eq!(only_bin(&scopes.histogram.red), Some(120));
    assert_eq!(only_bin(&scopes.histogram.green), Some(130));
}

#[test]
fn an_extreme_aspect_ratio_still_honours_the_budget() {
    // The closed-form stride undershoots here: sqrt(pixels/budget) leaves the
    // 16-row axis reading every row.
    let (width, height) = (16_384, 16);
    let bytes = flat(width, height, [10, 20, 30, 255]);
    let config = ScopeConfig::new(32, 32, 1_000).expect("config");
    let scopes = measure(frame(&bytes, width, height), config);
    assert!(
        scopes.sampled as usize <= 1_000,
        "sampled {} exceeded the budget on a wide frame",
        scopes.sampled
    );
}

#[test]
fn scopes_are_deterministic_for_one_frame() {
    let bytes = flat(200, 150, [33, 99, 199, 255]);
    let config = ScopeConfig::new(128, 64, 5_000).expect("config");
    let first = measure(frame(&bytes, 200, 150), config);
    let second = measure(frame(&bytes, 200, 150), config);
    assert_eq!(first, second);
}

#[test]
fn the_frame_timestamp_travels_with_the_measurement() {
    let bytes = flat(4, 4, [1, 2, 3, 255]);
    let view = Rgba8Frame::new(&bytes, 4, 4, 16, 12.5).expect("frame");
    let scopes = measure(view, ScopeConfig::default());
    assert_eq!(scopes.timestamp_seconds, 12.5);
}

#[test]
fn a_histogram_peak_scales_the_display() {
    let bytes = flat(10, 10, [64, 64, 64, 255]);
    let scopes = measure(frame(&bytes, 10, 10), ScopeConfig::default());
    assert_eq!(scopes.histogram.peak(), 100);
    // 256 columns over 10 pixel columns: each occupied column holds one sample
    // per row.
    assert_eq!(scopes.waveform.peak(), 10);
}

#[test]
fn out_of_range_lookups_report_zero_rather_than_panicking() {
    let bytes = flat(4, 4, [255, 255, 255, 255]);
    let config = ScopeConfig::new(4, 8, 1_000).expect("config");
    let scopes = measure(frame(&bytes, 4, 4), config);
    assert_eq!(scopes.waveform.cell(4, 0), 0);
    assert_eq!(scopes.waveform.cell(0, SCOPE_BINS), 0);
    assert_eq!(scopes.vectorscope.cell(8, 0), 0);
    assert_eq!(scopes.waveform.column_mean(4), None);
}

#[test]
fn every_configuration_field_is_validated() {
    for (columns, size, samples, field) in [
        (0, 64, 1_000, "waveform_columns"),
        (MAX_WAVEFORM_COLUMNS + 1, 64, 1_000, "waveform_columns"),
        (64, 0, 1_000, "vectorscope_size"),
        (64, MAX_VECTORSCOPE_SIZE + 1, 1_000, "vectorscope_size"),
        (64, 64, 0, "maximum_samples"),
        (64, 64, MAX_SCOPE_SAMPLES + 1, "maximum_samples"),
    ] {
        let err = ScopeConfig::new(columns, size, samples).expect_err("rejected");
        assert_eq!(err.field(), field);
    }
    assert!(
        ScopeConfig::new(1, 1, 1).is_ok(),
        "the smallest scope is legal"
    );
}
