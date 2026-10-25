//! The quads that draw a graph's lines: a sampled curve and a handle's lever.
//!
//! The graph used to paint one quad per sample pair, two pixels tall, parked at
//! the pair's mean y — so a diagonal read as a staircase of horizontal bars and
//! the one surface for judging an ease could not show the ease. A
//! segment is drawn as a run of pixel-wide columns instead, each spanning the y
//! the segment crosses in that column, which is a line at any slope.
//!
//! The geometry is here rather than in the paint closure so it can be asserted
//! against the curve's own samples without a window. Both functions return
//! iterators, so a paint closure draws each column as it is produced and a
//! frame allocates nothing for its lines.

/// Drawn thickness of the curve, in pixels. A column never draws thinner than
/// this, so a flat stretch is still a line.
pub const CURVE_STROKE: f32 = 2.0;

/// Drawn thickness of a handle's lever, the line from its keyframe to its tip,
/// in pixels.
pub const LEVER_STROKE: f32 = 1.0;

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

/// The columns that draw `samples` inside a graph `height` pixels tall, at
/// [`CURVE_STROKE`].
///
/// `samples` are `(pixel_x, pixel_y, tick, value)` as
/// [`crate::app::curve_geometry::build_curve_polyline`] produces them: pixel
/// coordinates are graph-local, y grows downward. Each pair of samples is cut
/// the way [`segment_columns`] cuts a segment. A stretch outside the graph is
/// drawn along the edge it left by, so a curve that leaves the range still
/// shows where it went.
pub fn curve_columns(
    samples: &[(f32, f32, f64, f32)],
    height: f32,
) -> impl Iterator<Item = CurveColumn> + '_ {
    samples.windows(2).flat_map(move |pair| {
        let (x0, y0, _, _) = pair[0];
        let (x1, y1, _, _) = pair[1];
        slices((x0, y0), (x1, y1)).map(move |slice| {
            let top = slice.top.clamp(0.0, height);
            let bottom = slice.bottom.clamp(0.0, height);
            let y = (top - CURVE_STROKE * 0.5).clamp(0.0, height);
            CurveColumn {
                x: slice.x,
                w: slice.w,
                y,
                h: (bottom - top + CURVE_STROKE).min(height - y),
            }
        })
    })
}

/// The columns that draw the straight line from `from` to `to`, `stroke`
/// pixels thick.
///
/// Points are graph-local `(x, y)` pixels, y down, in either order. A segment
/// wider than a pixel is cut into one column per pixel of x, each covering the
/// slice of the segment that crosses it. A column is never narrower than a
/// pixel, so neighbours overlap rather than meet at a fractional x, where each
/// would cover the shared pixel by half. A segment narrower than a pixel is one
/// column centered on it, so an upright lever stands on its keyframe rather
/// than beside it. Nothing is clamped: the painter's content mask cuts a line
/// at the graph's edge.
pub fn segment_columns(
    from: (f32, f32),
    to: (f32, f32),
    stroke: f32,
) -> impl Iterator<Item = CurveColumn> {
    slices(from, to).map(move |slice| CurveColumn {
        x: slice.x,
        w: slice.w,
        y: slice.top - stroke * 0.5,
        h: slice.bottom - slice.top + stroke,
    })
}

/// One column of x a segment crosses, and the y range it covers there.
struct Slice {
    x: f32,
    w: f32,
    top: f32,
    bottom: f32,
}

/// The slices [`segment_columns`] draws. Half the stroke goes either side of
/// a slice's range, so a segment meets its neighbour instead of leaving a
/// notch at a slope break.
fn slices(from: (f32, f32), to: (f32, f32)) -> impl Iterator<Item = Slice> {
    let ((x0, y0), (x1, y1)) = if to.0 < from.0 {
        (to, from)
    } else {
        (from, to)
    };
    let (dx, dy) = (x1 - x0, y1 - y0);
    let steps = dx.ceil().max(1.0);
    let w = (dx / steps).max(1.0);
    let left = if dx < 1.0 { x0 + dx * 0.5 - 0.5 } else { x0 };
    (0..steps as usize).map(move |step| {
        let at = step as f32 / steps;
        let next = (step + 1) as f32 / steps;
        let (a, b) = (y0 + dy * at, y0 + dy * next);
        Slice {
            x: left + dx * at,
            w,
            top: a.min(b),
            bottom: a.max(b),
        }
    })
}
