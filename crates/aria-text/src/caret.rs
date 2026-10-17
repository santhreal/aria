//! Caret stops: where a text cursor between two characters is drawn.
//!
//! The stops come from the layout [`TextRenderer::shape`] places glyphs from,
//! so a caret sits at the pen position the shaper advanced to, not at a guess
//! from character counts: kerning, tracking, ligatures, per-range sizes and
//! wrapping are all already in it.

use crate::{CaretStop, PendingCluster, ShapeKey, TextRenderer, TextStyle};

/// A laid-out line with no glyphs: an empty paragraph. It still holds a caret.
pub(crate) struct EmptyLine {
    pub line: usize,
    pub byte: usize,
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

impl TextRenderer {
    /// Every caret stop of `text` laid out with `style`, by line and then by
    /// byte, at most one per byte per line.
    ///
    /// Coordinates are in the ink box [`ShapedText::extent`](crate::ShapedText::extent)
    /// measures, the space [`ShapedText::clusters`](crate::ShapedText::clusters)
    /// are placed in. The bitmap [`rasterize`](Self::rasterize) returns has
    /// [`raster_padding`] around that box.
    ///
    /// Memoized beside [`shape`](Self::shape): a run already shaped costs one
    /// lookup, and a miss shapes once for both.
    pub fn carets(&mut self, text: &str, style: &TextStyle) -> Vec<CaretStop> {
        let key = ShapeKey::new(text, style);
        if let Some(hit) = self.caret_memo.get(&key) {
            return hit.clone();
        }
        let shaped = self.shape_uncached(text, style);
        Self::memo_insert(&mut self.shape_memo, ShapeKey::new(text, style), shaped);
        self.caret_memo.get(&key).cloned().unwrap_or_default()
    }
}

/// The transparent margin [`TextRenderer::rasterize`] puts around the ink
/// box for `style`: padding, stroke, shadow and card headroom.
pub fn raster_padding(style: &TextStyle) -> u32 {
    crate::effects::effect_padding(style)
}

/// The stop nearest `point`, in the space the stops are in.
///
/// The line is the one whose span holds `point`'s y, or the nearest line when
/// none does; the stop is the one on that line nearest in x. `None` only for
/// an empty list.
pub fn caret_at(stops: &[CaretStop], [x, y]: [f32; 2]) -> Option<&CaretStop> {
    let line = stops
        .iter()
        .min_by(|a, b| line_distance(a, y).total_cmp(&line_distance(b, y)))?
        .line;
    stops
        .iter()
        .filter(|stop| stop.line == line)
        .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
}

/// How far `y` is outside the span of `stop`'s line, `0` inside it.
fn line_distance(stop: &CaretStop, y: f32) -> f32 {
    if y < stop.top {
        stop.top - y
    } else if y > stop.bottom {
        y - stop.bottom
    } else {
        0.0
    }
}

/// The caret stops of one layout, offset into the ink box at `(gx, gy)`.
///
/// Each cluster gives a stop at its leading edge and one at its trailing edge.
/// A left-to-right cluster leads at its pen position; a right-to-left one
/// leads at its far edge. Neighbouring clusters share a byte, and the first
/// stop for a byte on a line is kept.
pub(crate) fn stops(
    clusters: &[PendingCluster],
    empty: &[EmptyLine],
    gx: f32,
    gy: f32,
) -> Vec<CaretStop> {
    let mut out = Vec::with_capacity(clusters.len() * 2 + empty.len());
    for cluster in clusters {
        let (lead, trail) = if cluster.rtl {
            (cluster.pen_right, cluster.pen_x)
        } else {
            (cluster.pen_x, cluster.pen_right)
        };
        let top = cluster.line_top - gy;
        let bottom = cluster.line_bottom - gy;
        out.push(CaretStop {
            byte: cluster.range.start,
            line: cluster.line,
            x: lead - gx,
            top,
            bottom,
        });
        out.push(CaretStop {
            byte: cluster.range.end,
            line: cluster.line,
            x: trail - gx,
            top,
            bottom,
        });
    }
    out.extend(empty.iter().map(|line| CaretStop {
        byte: line.byte,
        line: line.line,
        x: line.x - gx,
        top: line.top - gy,
        bottom: line.bottom - gy,
    }));
    out.sort_by_key(|stop| (stop.line, stop.byte));
    out.dedup_by(|later, kept| later.line == kept.line && later.byte == kept.byte);
    out
}

#[cfg(test)]
#[path = "caret_tests.rs"]
mod tests;
