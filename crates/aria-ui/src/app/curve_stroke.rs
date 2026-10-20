//! The quads that draw a sampled curve.
//!
//! The graph used to paint one quad per sample pair, two pixels tall, parked at
//! the pair's mean y — so a diagonal read as a staircase of horizontal bars and
//! the one surface for judging an ease could not show the ease. A
//! segment is drawn as a run of pixel-wide columns instead, each spanning the y
//! the curve actually crosses in that column, which is a line at any slope.
//!
//! The geometry is here rather than in the paint closure so it can be asserted
//! against the curve's own samples without a window.

/// Drawn thickness of the curve, in pixels. A column never draws thinner than
/// this, so a flat stretch is still a line.
pub const CURVE_STROKE: f32 = 2.0;

/// One drawn column, in graph-local pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveColumn {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CurveColumn {
    /// The y range this column paints, top first.
    pub fn span(&self) -> (f32, f32) {
        (self.y, self.y + self.h)
    }
}

/// The columns that draw `samples` inside a graph `height` pixels tall.
///
/// `samples` are `(pixel_x, pixel_y, tick, value)` as
/// [`crate::app::curve_geometry::build_curve_polyline`] produces them: pixel
/// coordinates are graph-local, y grows downward. A pair of samples further
/// apart than a pixel is subdivided so each column covers one pixel of x and
/// the slice of the segment that crosses it; a pair closer than a pixel still
/// yields one column, so nothing is dropped.
pub fn curve_columns(samples: &[(f32, f32, f64, f32)], height: f32) -> Vec<CurveColumn> {
    let mut columns = Vec::with_capacity(samples.len().saturating_mul(2));
    for pair in samples.windows(2) {
        let (x0, y0, _, _) = pair[0];
        let (x1, y1, _, _) = pair[1];
        let dx = x1 - x0;
        let steps = dx.abs().ceil().max(1.0);
        let step_w = (dx / steps).abs().max(1.0);
        let count = steps as usize;
        for step in 0..count {
            let at = step as f32 / steps;
            let next = (step + 1) as f32 / steps;
            let top = (y0 + (y1 - y0) * at).min(y0 + (y1 - y0) * next);
            let bottom = (y0 + (y1 - y0) * at).max(y0 + (y1 - y0) * next);
            let top = top.clamp(0.0, height);
            let bottom = bottom.clamp(0.0, height);
            // Half the stroke either side of the covered range, so a segment
            // meets its neighbour instead of leaving a notch at a slope break.
            let y = (top - CURVE_STROKE * 0.5).clamp(0.0, height);
            let h = (bottom - top + CURVE_STROKE).min(height - y);
            columns.push(CurveColumn {
                x: x0 + dx * at,
                w: step_w,
                y,
                h,
            });
        }
    }
    columns
}
