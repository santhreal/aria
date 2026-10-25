//! The painted scopes are asserted against pictures whose reading is known.
//!
//! The class of defect this closes: a scope drawn upside down, mirrored, or
//! scaled to nothing still looks like a scope. These pin the orientation (code
//! 255 at the top, neutral chroma at the centre) and that a trace is actually
//! laid down, so a rewrite of the painter cannot silently flip it.

use aria_analysis::{Rgba8Frame, ScopeConfig, measure_scopes};

use super::*;

/// Measure a flat frame of one colour.
fn flat_scopes(rgba: [u8; 4]) -> FrameScopes {
    traced_scopes(rgba, WaveformTrace::Luma)
}

/// Measure a flat frame of one colour with the waveform counting `trace`.
fn traced_scopes(rgba: [u8; 4], trace: WaveformTrace) -> FrameScopes {
    let bytes: Vec<u8> = rgba.iter().copied().cycle().take(16 * 8 * 4).collect();
    let frame = Rgba8Frame::new(&bytes, 16, 8, 16 * 4, 0.0).expect("frame");
    measure_scopes(frame, ScopeConfig::default().with_waveform_trace(trace))
}

/// Rows carrying a trace in `channel`, thresholded above the graticule so a
/// level line is not mistaken for a measurement.
fn lit_rows(image: &RgbaImage, channel: usize) -> Vec<u32> {
    lit_rows_in(image, channel, 0..image.width)
}

/// [`lit_rows`] over the columns `xs` alone.
fn lit_rows_in(image: &RgbaImage, channel: usize, xs: std::ops::Range<u32>) -> Vec<u32> {
    let mut rows = Vec::new();
    for y in 0..image.height {
        let mut lit = false;
        for x in xs.clone() {
            let at = ((y * image.width + x) * 4) as usize;
            if image.pixels[at + channel] > GRATICULE[channel] + 40 {
                lit = true;
                break;
            }
        }
        if lit {
            rows.push(y);
        }
    }
    rows
}

#[test]
fn a_white_frame_traces_the_top_of_the_waveform() {
    let scopes = flat_scopes([255, 255, 255, 255]);
    let image = waveform_image(&scopes.waveform);
    assert_eq!(image.height, SCOPE_PX);
    assert!(image.is_well_formed());
    let rows = lit_rows(&image, 1);
    assert_eq!(rows.first().copied(), Some(0), "code 255 draws at the top");
    assert!(
        rows.iter().all(|y| *y < SCOPE_PX / 4),
        "a white frame must not trace the bottom half: {rows:?}"
    );
}

#[test]
fn a_black_frame_traces_the_bottom_of_the_waveform() {
    let scopes = flat_scopes([0, 0, 0, 255]);
    let image = waveform_image(&scopes.waveform);
    let rows = lit_rows(&image, 1);
    assert_eq!(
        rows.last().copied(),
        Some(SCOPE_PX - 1),
        "code 0 draws at the bottom"
    );
}

#[test]
fn a_mid_grey_frame_traces_the_middle() {
    let scopes = flat_scopes([128, 128, 128, 255]);
    let image = waveform_image(&scopes.waveform);
    let rows = lit_rows(&image, 1);
    let mean = rows.iter().sum::<u32>() as f32 / rows.len() as f32;
    let centre = (SCOPE_PX - 1) as f32 / 2.0;
    assert!(
        (mean - centre).abs() < 6.0,
        "mid gray traced at {mean}, expected near {centre}"
    );
}

#[test]
fn the_waveform_keeps_one_column_per_measured_column() {
    let scopes = flat_scopes([200, 200, 200, 255]);
    let image = waveform_image(&scopes.waveform);
    assert_eq!(image.width as usize, scopes.waveform.columns());
}

#[test]
fn a_monochrome_frame_lands_on_the_vectorscope_centre() {
    let scopes = flat_scopes([90, 90, 90, 255]);
    let image = vectorscope_image(&scopes.vectorscope);
    assert_eq!(image.width, SCOPE_PX);
    assert_eq!(image.height, SCOPE_PX);
    let (x, y) = brightest_trace(&image);
    let centre = (SCOPE_PX - 1) as f32 / 2.0;
    let distance = (x as f32 - centre).hypot(y as f32 - centre);
    assert!(
        distance <= 2.0,
        "neutral chroma traced {distance} px from the center of the grid"
    );
}

/// The pixel where the yellow trace is strongest. The graticule lifts every
/// channel a little, so red plus green is what separates a trace from a line.
fn brightest_trace(image: &RgbaImage) -> (u32, u32) {
    let mut best = (0u32, 0u32, 0u16);
    for y in 0..image.height {
        for x in 0..image.width {
            let at = ((y * image.width + x) * 4) as usize;
            let lift = u16::from(image.pixels[at]) + u16::from(image.pixels[at + 1]);
            if lift > best.2 {
                best = (x, y, lift);
            }
        }
    }
    (best.0, best.1)
}

#[test]
fn saturated_colour_leaves_the_vectorscope_centre() {
    let scopes = flat_scopes([255, 0, 0, 255]);
    let image = vectorscope_image(&scopes.vectorscope);
    let centre = (SCOPE_PX - 1) / 2;
    let mut brightest = (0u32, 0u32, 0u16);
    for y in 0..image.height {
        for x in 0..image.width {
            let at = ((y * image.width + x) * 4) as usize;
            let lift = u16::from(image.pixels[at]) + u16::from(image.pixels[at + 1]);
            if lift > brightest.2 {
                brightest = (x, y, lift);
            }
        }
    }
    let (x, y, _) = brightest;
    let distance = (x as f32 - centre as f32).hypot(y as f32 - centre as f32);
    assert!(
        distance > 20.0,
        "primary red traced {distance} px from neutral, which is not saturated"
    );
    assert!(x < centre, "red draws left of neutral on Cb");
    assert!(y < centre, "red draws above neutral on Cr");
}

#[test]
fn the_histogram_puts_each_channel_where_its_code_value_is() {
    // Red 32, green 128, blue 224: three separate columns, left to right.
    let scopes = flat_scopes([32, 128, 224, 255]);
    let image = histogram_image(&scopes.histogram);
    assert_eq!(image.width, HISTOGRAM_W);
    assert_eq!(image.height, HISTOGRAM_H);

    let column_of = |channel: usize| -> u32 {
        let mut best = (0u32, 0u8);
        for x in 0..image.width {
            let at = (((image.height - 1) * image.width + x) * 4) as usize;
            let value = image.pixels[at + channel];
            if value > best.1 {
                best = (x, value);
            }
        }
        best.0
    };
    let red = column_of(0);
    let green = column_of(1);
    let blue = column_of(2);
    assert!(
        red < green && green < blue,
        "channels plotted out of order: red {red}, green {green}, blue {blue}"
    );
    // 32/255 of the width, within a pixel of rounding.
    assert!(red.abs_diff(32) <= 2, "red plotted at {red}");
    assert!(blue.abs_diff(224) <= 2, "blue plotted at {blue}");
}

#[test]
fn a_scope_bitmap_is_opaque_so_the_panel_cannot_show_through() {
    let scopes = flat_scopes([10, 200, 90, 255]);
    let images = ScopeImages::paint(&scopes);
    for image in [&images.waveform, &images.vectorscope, &images.histogram] {
        assert!(image.is_well_formed());
        assert!(
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|px| px[3] == 255),
            "a scope must be opaque"
        );
    }
}

#[test]
fn repaint_reuses_every_scope_pixel_buffer() {
    let scopes = flat_scopes([10, 200, 90, 255]);
    let mut images = ScopeImages::default();
    images.repaint(&scopes);
    let allocations = [
        images.waveform.pixels.as_ptr(),
        images.vectorscope.pixels.as_ptr(),
        images.histogram.pixels.as_ptr(),
    ];

    images.repaint(&scopes);
    assert_eq!(images.waveform.pixels.as_ptr(), allocations[0]);
    assert_eq!(images.vectorscope.pixels.as_ptr(), allocations[1]);
    assert_eq!(images.histogram.pixels.as_ptr(), allocations[2]);
}

#[test]
fn vectorscope_graticules_are_rendered_once_per_geometry() {
    let first = vectorscope_graticule(128);
    let second = vectorscope_graticule(128);
    assert!(
        Arc::ptr_eq(&first, &second),
        "the static ring was recomputed for the next frame"
    );
}

#[test]
fn an_empty_measurement_paints_a_graticule_and_no_trace() {
    // A fully transparent frame measures nothing.
    let bytes = vec![0u8; 8 * 8 * 4];
    let frame = Rgba8Frame::new(&bytes, 8, 8, 32, 0.0).expect("frame");
    let scopes = measure_scopes(frame, ScopeConfig::default());
    assert_eq!(scopes.sampled, 0);
    let image = waveform_image(&scopes.waveform);
    // The level lines are still there; nothing else is.
    let level_pixels = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| **px == GRATICULE)
        .count();
    assert_eq!(
        level_pixels,
        5 * image.width as usize,
        "five level lines across the canvas"
    );
    assert!(
        lit_rows(&image, 1).is_empty(),
        "nothing was measured, so nothing is traced"
    );
}

/// A parade lays each channel in its own third, left to right, with a
/// graticule column between them, and each third traces its own channel's
/// code. The class this closes: a parade drawn in the wrong order, or one
/// channel drawn into another's third.
#[test]
fn a_parade_lays_each_channel_in_its_own_third() {
    let columns = ScopeConfig::default().waveform_columns() as u32;
    let thirds = [
        0..columns,
        columns + 1..2 * columns + 1,
        2 * columns + 2..3 * columns + 2,
    ];
    let red = waveform_image(&traced_scopes([255, 0, 0, 255], WaveformTrace::RgbParade).waveform);
    assert_eq!(red.width, 3 * columns + 2);
    for gap in [columns, 2 * columns + 1] {
        for y in 0..SCOPE_PX {
            let at = ((y * red.width + gap) * 4) as usize;
            assert_eq!(red.pixels[at..at + 4], GRATICULE, "gap {gap} row {y}");
        }
    }
    // Red at code 255 in the red third; green and blue at code 0 in theirs.
    let red_rows = lit_rows_in(&red, 0, thirds[0].clone());
    assert_eq!(red_rows.first().copied(), Some(0), "red traces the top");
    assert!(red_rows.iter().all(|y| *y < SCOPE_PX / 4), "{red_rows:?}");
    for (third, channel) in [(1, 1), (2, 2)] {
        let rows = lit_rows_in(&red, channel, thirds[third].clone());
        assert_eq!(rows.last().copied(), Some(SCOPE_PX - 1), "third {third}");
        assert!(rows.iter().all(|y| *y > SCOPE_PX * 3 / 4), "{rows:?}");
    }
    // The green and blue traces lift the bottom level line in every channel,
    // so the top row is what tells red apart.
    assert!(
        !lit_rows_in(&red, 0, thirds[1].start..thirds[2].end).contains(&0),
        "red traced outside its third"
    );

    // A grey's chroma is neutral: Cb and Cr trace the middle of their thirds.
    let grey =
        waveform_image(&traced_scopes([90, 90, 90, 255], WaveformTrace::YcbcrParade).waveform);
    assert_eq!(grey.width, 3 * columns + 2);
    let middle = (SCOPE_PX - 1) as f32 / 2.0;
    for (third, channel) in [(1, 2), (2, 0)] {
        let rows = lit_rows_in(&grey, channel, thirds[third].clone());
        assert!(!rows.is_empty(), "third {third} traced nothing");
        assert!(
            rows.iter().all(|y| (*y as f32 - middle).abs() < 2.0),
            "neutral chroma traced {rows:?} in third {third}"
        );
    }
}

/// Each primary and secondary at 75 % and at 100 % traces inside its target
/// box, and the skin tone line runs from neutral at its angle. Enumerated from
/// the target tables, so a target added without a box fails here.
#[test]
fn every_colour_target_holds_its_colour_and_the_skin_line_is_drawn() {
    let empty = {
        let bytes = vec![0u8; 8 * 8 * 4];
        let frame = Rgba8Frame::new(&bytes, 8, 8, 32, 0.0).expect("frame");
        vectorscope_image(&measure_scopes(frame, ScopeConfig::default()).vectorscope)
    };
    let graticule = |x: u32, y: u32| {
        let at = ((y * empty.width + x) * 4) as usize;
        empty.pixels[at..at + 4] == GRATICULE
    };
    for (level, half) in TARGET_LEVELS.into_iter().zip(TARGET_HALF) {
        for [red, green, blue] in TARGET_COLOURS {
            let scale = |code: u8| if code == 0 { 0 } else { level };
            let rgba = [scale(red), scale(green), scale(blue), 255];
            let (x, y) = brightest_trace(&vectorscope_image(&flat_scopes(rgba).vectorscope));
            let size = ScopeConfig::default().vectorscope_size();
            let (cell_x, cell_y) = vectorscope_cell(rgba[0], rgba[1], rgba[2], size);
            let (box_x, box_y) = (cell_pixel(cell_x, size), cell_pixel(cell_y, size));
            assert!(
                x.abs_diff(box_x) <= half && y.abs_diff(box_y) <= half,
                "{rgba:?} traced at ({x}, {y}), outside its box at ({box_x}, {box_y})"
            );
            // The box's edge is drawn wherever it is on the canvas.
            let edges = [
                (box_x.checked_sub(half), Some(box_y)),
                (Some(box_x + half), Some(box_y)),
                (Some(box_x), box_y.checked_sub(half)),
                (Some(box_x), Some(box_y + half)),
            ];
            let drawn = edges
                .into_iter()
                .filter_map(|(x, y)| Some((x?, y?)))
                .filter(|(x, y)| *x < SCOPE_PX && *y < SCOPE_PX)
                .all(|(x, y)| graticule(x, y));
            assert!(drawn, "the box of {rgba:?} is not drawn");
        }
    }
    let centre = cell_pixel(64, 128) as f32;
    let (sin, cos) = SKIN_TONE_DEGREES.to_radians().sin_cos();
    for step in [10.0, 40.0, 80.0] {
        let (x, y) = (centre + step * cos, centre - step * sin);
        assert!(
            graticule(x.round() as u32, y.round() as u32),
            "the skin tone line misses ({x}, {y})"
        );
    }
    // Red and yellow sit either side of the skin tone line.
    let angle = |rgba: [u8; 3]| {
        let (cb, cr) = aria_analysis::scopes::chroma709(rgba[0], rgba[1], rgba[2]);
        cr.atan2(cb).to_degrees()
    };
    assert!(angle([255, 0, 0]) < SKIN_TONE_DEGREES);
    assert!(angle([255, 255, 0]) > SKIN_TONE_DEGREES);
}
