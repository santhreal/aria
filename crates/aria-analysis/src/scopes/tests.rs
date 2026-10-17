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

/// WHY: chroma computed in `f32` put greys one cell left of neutral, because
/// the Cb coefficients do not cancel in `f32`, so a monochrome frame drew a
/// smear beside the centre instead of a dot on it. Every grey, through
/// [`measure`], on grids of both parities up to the ceiling.
#[test]
fn every_grey_lands_on_the_centre_of_every_grid() {
    let bytes: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, 255]).collect();
    for size in [1, 2, 3, 127, 128, 129, MAX_VECTORSCOPE_SIZE] {
        let config = ScopeConfig::new(16, size, 200_000).expect("config");
        let scopes = measure(frame(&bytes, 16, 16), config);
        let centre = scopes.vectorscope.centre();
        assert_eq!(
            scopes.vectorscope.cell(centre, centre),
            256,
            "a grey left the centre of a {size}-cell grid"
        );
    }
}

/// The fixed point stays within one code of BT.709 luma and within one cell
/// of BT.709 chroma for every colour on a 5-step lattice, so a mistyped weight
/// or a swapped axis fails here. Greys read their own code exactly.
#[test]
fn fixed_point_tracks_bt709_within_one_step() {
    for v in 0..=255u8 {
        assert_eq!(luma709(v, v, v), v, "grey {v}");
    }
    let size = MAX_VECTORSCOPE_SIZE;
    let last = (size - 1) as f64;
    let steps = (0..=255u8).step_by(5);
    for red in steps.clone() {
        for green in steps.clone() {
            for blue in steps.clone() {
                let (r, g, b) = (f64::from(red), f64::from(green), f64::from(blue));
                let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                let read = f64::from(luma709(red, green, blue));
                assert!((read - luma).abs() <= 1.0, "luma of {red},{green},{blue}");
                let (cb, cr) = (
                    (-0.114_572 * r - 0.385_428 * g + 0.5 * b) / 255.0,
                    (0.5 * r - 0.454_153 * g - 0.045_847 * b) / 255.0,
                );
                let (x, y) = vectorscope_cell(red, green, blue, size);
                assert!(
                    (x as f64 - (cb + 0.5) * last).abs() <= 1.0
                        && (y as f64 - (0.5 - cr) * last).abs() <= 1.0,
                    "{red},{green},{blue} sits at ({x}, {y})"
                );
            }
        }
    }
}

/// The corners of the colour cube stay on the grid at the largest size, where
/// the fixed-point products are largest.
#[test]
fn the_most_saturated_colours_stay_on_the_largest_grid() {
    let last = MAX_VECTORSCOPE_SIZE - 1;
    for rgb in [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [0, 255, 255],
        [255, 0, 255],
    ] {
        let (x, y) = vectorscope_cell(rgb[0], rgb[1], rgb[2], MAX_VECTORSCOPE_SIZE);
        assert!(
            x <= last && y <= last,
            "{rgb:?} left the grid at ({x}, {y})"
        );
    }
    // Blue is the most positive Cb and red the most positive Cr.
    assert_eq!(vectorscope_cell(0, 0, 255, MAX_VECTORSCOPE_SIZE).0, last);
    assert_eq!(vectorscope_cell(255, 255, 0, MAX_VECTORSCOPE_SIZE).0, 0);
    assert_eq!(vectorscope_cell(255, 0, 0, MAX_VECTORSCOPE_SIZE).1, 0);
    assert_eq!(vectorscope_cell(0, 255, 255, MAX_VECTORSCOPE_SIZE).1, last);
}

/// Counts taken apart and put back read as the measurement they came from,
/// and a buffer of the wrong length is named rather than read out of bounds.
#[test]
fn counts_round_trip_and_a_wrong_length_is_named() {
    let bytes: Vec<u8> = (0..64u8)
        .flat_map(|v| [v * 4, 255 - v * 4, v * 2, 255])
        .collect();
    let config = ScopeConfig::new(8, 16, 200_000).expect("config");
    let measured = measure(frame(&bytes, 8, 8), config);
    let parts = || {
        (
            [
                measured.histogram.red.clone(),
                measured.histogram.green.clone(),
                measured.histogram.blue.clone(),
                measured.histogram.luma.clone(),
            ],
            measured.waveform.cells.clone(),
            measured.vectorscope.cells.clone(),
        )
    };
    let (histogram, waveform, vectorscope) = parts();
    let rebuilt = FrameScopes::from_counts(
        config,
        histogram,
        waveform,
        vectorscope,
        measured.sampled,
        measured.timestamp_seconds,
    )
    .expect("counts of this config");
    assert_eq!(rebuilt, measured);

    let (mut histogram, waveform, vectorscope) = parts();
    histogram[3].pop();
    let err = FrameScopes::from_counts(config, histogram, waveform, vectorscope, 0, 0.0)
        .expect_err("a short luma channel");
    assert_eq!(err.field(), "histogram");
    let (histogram, mut waveform, vectorscope) = parts();
    waveform.push(0);
    let err = FrameScopes::from_counts(config, histogram, waveform, vectorscope, 0, 0.0)
        .expect_err("a long waveform");
    assert_eq!(err.field(), "waveform");
    let (histogram, waveform, mut vectorscope) = parts();
    vectorscope.truncate(16);
    let err = FrameScopes::from_counts(config, histogram, waveform, vectorscope, 0, 0.0)
        .expect_err("one vectorscope row");
    assert_eq!(err.field(), "vectorscope");
}

/// Two columns of four rows: red on the left, blue on the right.
fn red_beside_blue() -> Vec<u8> {
    (0..4)
        .flat_map(|_| {
            [
                [255, 0, 0, 255],
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                [0, 0, 255, 255],
            ]
        })
        .flatten()
        .collect()
}

/// Each parade channel counts its own code per column, and a channel the trace
/// does not count reads zero. The class this closes: a parade that plots one
/// channel's codes under another's place, or a parade plane read as luma.
#[test]
fn a_parade_counts_every_channel_by_its_own_code() {
    let bytes = red_beside_blue();
    let config = ScopeConfig::new(2, 16, 200_000).expect("config");
    let luma = measure(frame(&bytes, 4, 4), config);
    let rgb = measure(
        frame(&bytes, 4, 4),
        config.with_waveform_trace(WaveformTrace::RgbParade),
    );
    assert_eq!(rgb.waveform.trace(), WaveformTrace::RgbParade);
    for (channel, left, right) in [
        (WaveformChannel::Red, 255, 0),
        (WaveformChannel::Green, 0, 0),
        (WaveformChannel::Blue, 0, 255),
    ] {
        assert_eq!(
            rgb.waveform.channel_cell(channel, 0, left),
            8,
            "{channel:?} left"
        );
        assert_eq!(
            rgb.waveform.channel_cell(channel, 1, right),
            8,
            "{channel:?} right"
        );
    }
    let red_luma = usize::from(luma709(255, 0, 0));
    assert_eq!(luma.waveform.cell(0, red_luma), 8);
    assert_eq!(
        rgb.waveform.cell(0, red_luma),
        0,
        "an RGB parade counts no luma"
    );
    assert_eq!(rgb.waveform.column_mean(0), None);
    assert_eq!(
        luma.waveform.channel_cell(WaveformChannel::Red, 0, 255),
        0,
        "a luma waveform counts no red"
    );

    let ycbcr = measure(
        frame(&bytes, 4, 4),
        config.with_waveform_trace(WaveformTrace::YcbcrParade),
    );
    for (column, rgb_value) in [(0, (255, 0, 0)), (1, (0, 0, 255))] {
        let (r, g, b) = rgb_value;
        let (cb, cr) = chroma_codes709(r, g, b);
        assert_eq!(
            ycbcr
                .waveform
                .channel_cell(WaveformChannel::Cb, column, usize::from(cb)),
            8
        );
        assert_eq!(
            ycbcr
                .waveform
                .channel_cell(WaveformChannel::Cr, column, usize::from(cr)),
            8
        );
    }
    // The luma plane of a parade is the luma waveform, and the trace changes
    // neither the histogram nor the vectorscope.
    for column in 0..2 {
        for bin in 0..SCOPE_BINS {
            assert_eq!(
                ycbcr.waveform.cell(column, bin),
                luma.waveform.cell(column, bin)
            );
        }
    }
    for parade in [&rgb, &ycbcr] {
        assert_eq!(parade.histogram, luma.histogram);
        assert_eq!(parade.vectorscope, luma.vectorscope);
    }
}

/// Every grey is neutral chroma, the primaries reach both ends of the code
/// range, and the fixed point agrees with the float chroma within one code.
#[test]
fn chroma_codes_are_neutral_for_grey_and_span_the_code_range() {
    for value in 0..=255u8 {
        assert_eq!(
            chroma_codes709(value, value, value),
            (128, 128),
            "grey {value}"
        );
    }
    assert_eq!(chroma_codes709(0, 0, 255).0, 255, "blue is the most Cb");
    assert_eq!(chroma_codes709(255, 255, 0).0, 0, "yellow is the least Cb");
    assert_eq!(chroma_codes709(255, 0, 0).1, 255, "red is the most Cr");
    assert_eq!(chroma_codes709(0, 255, 255).1, 0, "cyan is the least Cr");
    for r in (0..=255u8).step_by(15) {
        for g in (0..=255u8).step_by(15) {
            for b in (0..=255u8).step_by(15) {
                let (cb, cr) = chroma709(r, g, b);
                let (code_b, code_r) = chroma_codes709(r, g, b);
                for (code, value) in [(code_b, cb), (code_r, cr)] {
                    let expected = (value + 0.5) * 255.0;
                    assert!(
                        (f32::from(code) - expected).abs() <= 1.0,
                        "({r}, {g}, {b}) coded {code}, chroma reads {expected}"
                    );
                }
            }
        }
    }
}

/// Every trace's counts round-trip through `from_counts`, and a parade handed
/// one plane is refused by name rather than read as a short buffer.
#[test]
fn a_parade_needs_a_plane_per_channel() {
    let bytes = red_beside_blue();
    for trace in WaveformTrace::ALL {
        let config = ScopeConfig::new(4, 16, 200_000)
            .expect("config")
            .with_waveform_trace(trace);
        let measured = measure(frame(&bytes, 4, 4), config);
        let planes = trace.channels().len();
        assert_eq!(measured.waveform.cells.len(), planes * 4 * SCOPE_BINS);
        let histogram = || {
            [
                measured.histogram.red.clone(),
                measured.histogram.green.clone(),
                measured.histogram.blue.clone(),
                measured.histogram.luma.clone(),
            ]
        };
        let rebuilt = FrameScopes::from_counts(
            config,
            histogram(),
            measured.waveform.cells.clone(),
            measured.vectorscope.cells.clone(),
            measured.sampled,
            measured.timestamp_seconds,
        )
        .expect("counts of this config");
        assert_eq!(rebuilt, measured, "{trace:?}");
        if planes > 1 {
            let one_plane = measured.waveform.cells[..4 * SCOPE_BINS].to_vec();
            let err = FrameScopes::from_counts(
                config,
                histogram(),
                one_plane,
                measured.vectorscope.cells.clone(),
                0,
                0.0,
            )
            .expect_err("one plane of a parade");
            assert_eq!(err.field(), "waveform", "{trace:?}");
        }
    }
}
