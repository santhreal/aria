//! The drawn curve must be the sampled curve.
//!
//! # Why
//!
//! The graph drew a segment as one flat bar at the pair's mean y, which is
//! shaped like a curve only when the curve is flat: a linear ramp read as a
//! staircase and a steep ease read as a single step. These cases assert the
//! drawn columns against the value the samples carry at the same x, so a
//! drawing that looks plausible but does not follow the curve fails.
//!
//! What they do not catch: colour, thickness beyond the minimum, and anything
//! about the window — this is the geometry only.

use crate::app::curve_stroke::{CURVE_STROKE, CurveColumn, curve_columns};

const HEIGHT: f32 = 120.0;

/// A straight line from `(x0, y0)` to `(x1, y1)`, sampled `count` times the way
/// the polyline builder samples a ramp.
fn line(x0: f32, y0: f32, x1: f32, y1: f32, count: usize) -> Vec<(f32, f32, f64, f32)> {
    (0..count)
        .map(|i| {
            let at = i as f32 / (count - 1) as f32;
            (x0 + (x1 - x0) * at, y0 + (y1 - y0) * at, 0.0, 0.0)
        })
        .collect()
}

/// The column covering `x`, if any.
fn at<'a>(columns: &'a [CurveColumn], x: f32) -> Option<&'a CurveColumn> {
    columns
        .iter()
        .find(|column| x >= column.x && x <= column.x + column.w)
}

#[test]
fn a_diagonal_follows_the_line_it_was_sampled_from() {
    // 90 pixels of fall over 60 of run: a mean-y bar per sample pair misses the
    // line by most of a segment, which is the staircase.
    let samples = line(0.0, 10.0, 60.0, 100.0, 6);
    let columns = curve_columns(&samples, HEIGHT);
    for step in 0..=60 {
        let x = step as f32;
        let column = at(&columns, x).unwrap_or_else(|| panic!("nothing drawn at x = {x}"));
        let want = 10.0 + 90.0 * (x / 60.0);
        let (top, bottom) = column.span();
        assert!(
            want >= top - CURVE_STROKE && want <= bottom + CURVE_STROKE,
            "at x = {x} the curve is at {want} but the drawing covers {top}..{bottom}"
        );
    }
}

#[test]
fn the_drawn_line_never_breaks() {
    // Consecutive columns must touch in y, or a steep curve is drawn as a
    // dotted line of disconnected bars.
    let samples = line(0.0, 5.0, 80.0, 115.0, 9);
    let columns = curve_columns(&samples, HEIGHT);
    assert!(columns.len() >= 80, "a pixel of x needs a column");
    for pair in columns.windows(2) {
        let (top, bottom) = pair[0].span();
        let (next_top, next_bottom) = pair[1].span();
        assert!(
            next_top <= bottom && next_bottom >= top,
            "the drawing jumps from {top}..{bottom} to {next_top}..{next_bottom}"
        );
    }
}

#[test]
fn a_flat_stretch_is_still_a_line() {
    let samples = line(0.0, 60.0, 40.0, 60.0, 5);
    let columns = curve_columns(&samples, HEIGHT);
    assert!(!columns.is_empty(), "a flat curve still draws");
    for column in &columns {
        assert!(
            column.h >= CURVE_STROKE,
            "a flat column drew {} pixels tall",
            column.h
        );
        let (top, bottom) = column.span();
        assert!(
            60.0 >= top && 60.0 <= bottom,
            "the flat value 60 is outside {top}..{bottom}"
        );
    }
}

#[test]
fn a_curve_that_leaves_the_graph_is_drawn_inside_it() {
    // A value outside the graph's range is clamped by the mapping, not by the
    // painter, so the columns must stay in bounds — and a stretch the curve
    // spends entirely outside is drawn as a line along the edge it left by,
    // not as a bar reaching back into the graph.
    let samples = line(0.0, -40.0, 50.0, HEIGHT + 40.0, 7);
    let columns = curve_columns(&samples, HEIGHT);
    let value_at = |x: f32| -40.0 + (HEIGHT + 80.0) * (x / 50.0);
    for column in &columns {
        let (top, bottom) = column.span();
        assert!(
            top >= 0.0 && bottom <= HEIGHT,
            "a column covers {top}..{bottom}, outside 0..{HEIGHT}"
        );
        let (enters, leaves) = (value_at(column.x), value_at(column.x + column.w));
        if enters.max(leaves) < 0.0 {
            assert!(
                top == 0.0 && column.h <= CURVE_STROKE,
                "the curve is above the graph here but the drawing covers {top}..{bottom}"
            );
        }
        if enters.min(leaves) > HEIGHT {
            assert!(
                bottom == HEIGHT && column.h <= CURVE_STROKE,
                "the curve is below the graph here but the drawing covers {top}..{bottom}"
            );
        }
    }
}

#[test]
fn an_ease_bends_the_way_its_samples_bend() {
    // Quadratic ease-in: the drawn slope must grow with x. A mean-y drawing
    // flattens each segment, so the measured slope stops following the curve.
    let count = 21;
    let samples: Vec<(f32, f32, f64, f32)> = (0..count)
        .map(|i| {
            let at = i as f32 / (count - 1) as f32;
            (at * 100.0, 10.0 + 100.0 * at * at, 0.0, 0.0)
        })
        .collect();
    let columns = curve_columns(&samples, HEIGHT + 40.0);
    let centre = |x: f32| {
        let column = at(&columns, x).unwrap_or_else(|| panic!("nothing drawn at x = {x}"));
        let (top, bottom) = column.span();
        (top + bottom) / 2.0
    };
    for step in 0..=100 {
        let x = step as f32;
        let want = 10.0 + 100.0 * (x / 100.0) * (x / 100.0);
        let column = at(&columns, x).unwrap_or_else(|| panic!("nothing drawn at x = {x}"));
        let (top, bottom) = column.span();
        assert!(
            want >= top - CURVE_STROKE && want <= bottom + CURVE_STROKE,
            "at x = {x} the ease is at {want} but the drawing covers {top}..{bottom}"
        );
    }
    let early = centre(30.0) - centre(20.0);
    let late = centre(90.0) - centre(80.0);
    assert!(
        late > early * 2.0,
        "the ease is drawn straight: early slope {early}, late slope {late}"
    );
}

#[test]
fn one_sample_draws_nothing() {
    assert!(curve_columns(&[], HEIGHT).is_empty());
    assert!(curve_columns(&[(0.0, 0.0, 0.0, 0.0)], HEIGHT).is_empty());
}
