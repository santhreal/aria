//! The geometry of a mask path: the mapping between path space and window
//! pixels, and the curve arithmetic the Pen and the vertex drags edit with.

use aria_models::ShapePathPoint;

use crate::app::canvas_overlay::Rect;
use crate::app::canvas_overlay::motion_path::TangentSide;

/// How far from a segment a Pen press inserts a vertex, in window pixels.
pub const SEGMENT_HIT: f32 = 5.0;
/// Pointer travel, in window pixels, below which a press is a click.
pub(super) const CLICK_SLOP: f32 = 3.0;
/// Fewest vertices a mask path keeps: fewer encloses no area.
pub const MIN_MASK_VERTICES: usize = 3;
/// How far a mask path coordinate may sit from the layer, as the model allows.
const PATH_REACH: f32 = 10.0;
/// Samples per segment for the segment hit test.
const SEGMENT_SAMPLES: usize = 32;

/// The mapping between a mask's path space and window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathSpace {
    layer: Rect,
    center: [f32; 2],
    /// Clockwise radians, as the renderer turns the stencil.
    rotation: f32,
}

impl PathSpace {
    /// The space a mask with `center` and `rotation_deg` (clockwise degrees)
    /// draws in over the `layer` quad.
    pub fn new(layer: Rect, center: [f32; 2], rotation_deg: f32) -> Self {
        Self {
            layer,
            center,
            rotation: rotation_deg.to_radians(),
        }
    }

    /// The point the stencil turns about, in layer pixels.
    fn pivot(&self) -> [f32; 2] {
        [
            (0.5 + self.center[0]) * self.layer.w,
            (0.5 + self.center[1]) * self.layer.h,
        ]
    }

    /// A path-space point in window pixels.
    pub fn to_window(&self, [vx, vy]: [f32; 2]) -> [f32; 2] {
        let pivot = self.pivot();
        let shifted = [
            (vx + self.center[0]) * self.layer.w - pivot[0],
            (vy + self.center[1]) * self.layer.h - pivot[1],
        ];
        let [x, y] = turn(shifted, self.rotation);
        [self.layer.x + pivot[0] + x, self.layer.y + pivot[1] + y]
    }

    /// A window point in path space, held to the reach the model accepts.
    pub fn to_path(&self, [ax, ay]: [f32; 2]) -> [f32; 2] {
        let pivot = self.pivot();
        let [x, y] = turn(
            [ax - self.layer.x - pivot[0], ay - self.layer.y - pivot[1]],
            -self.rotation,
        );
        let across = |extent: f32| {
            if extent.abs() > f32::EPSILON {
                extent
            } else {
                1.0
            }
        };
        [
            ((pivot[0] + x) / across(self.layer.w) - self.center[0]).clamp(-PATH_REACH, PATH_REACH),
            ((pivot[1] + y) / across(self.layer.h) - self.center[1]).clamp(-PATH_REACH, PATH_REACH),
        ]
    }
}

/// `v` turned clockwise by `radians` in a y-down space.
pub(super) fn turn([x, y]: [f32; 2], radians: f32) -> [f32; 2] {
    let (sin, cos) = radians.sin_cos();
    [x * cos - y * sin, x * sin + y * cos]
}

pub(super) fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

pub(super) fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

pub(super) fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub(super) fn length([x, y]: [f32; 2]) -> f32 {
    x.hypot(y)
}

/// Whether a vertex bends the path: a tangent handle off its anchor.
pub fn is_smooth(vertex: &ShapePathPoint) -> bool {
    vertex.handle_in != vertex.anchor || vertex.handle_out != vertex.anchor
}

/// The handle on `side` of `vertex`.
pub(super) fn handle(vertex: &ShapePathPoint, side: TangentSide) -> [f32; 2] {
    match side {
        TangentSide::In => vertex.handle_in,
        TangentSide::Out => vertex.handle_out,
    }
}

/// Move the handle on `side` of `vertex` to `to`. A vertex whose two handles
/// lie on one line through the anchor stays smooth: the other handle turns to
/// stay opposite and keeps its own length. A cusp keeps its other handle.
pub(super) fn set_handle(vertex: &mut ShapePathPoint, side: TangentSide, to: [f32; 2]) {
    let other_side = match side {
        TangentSide::In => TangentSide::Out,
        TangentSide::Out => TangentSide::In,
    };
    let mine = sub(handle(vertex, side), vertex.anchor);
    let other = sub(handle(vertex, other_side), vertex.anchor);
    let cross = mine[0] * other[1] - mine[1] * other[0];
    let dot = mine[0] * other[0] + mine[1] * other[1];
    let reach = length(mine) * length(other);
    let opposed = reach > f32::EPSILON && cross.abs() <= 1e-3 * reach && dot < 0.0;
    let moved = to;
    let turned = if opposed {
        let away = sub(vertex.anchor, moved);
        let along = length(away);
        (along > f32::EPSILON).then(|| {
            let scale = length(other) / along;
            add(vertex.anchor, [away[0] * scale, away[1] * scale])
        })
    } else {
        None
    };
    match side {
        TangentSide::In => vertex.handle_in = moved,
        TangentSide::Out => vertex.handle_out = moved,
    }
    if let Some(turned) = turned {
        match other_side {
            TangentSide::In => vertex.handle_in = turned,
            TangentSide::Out => vertex.handle_out = turned,
        }
    }
}

/// Pull `vertex`'s tangents out to `to`, mirrored about its anchor.
pub(super) fn pull(vertex: &mut ShapePathPoint, to: [f32; 2]) {
    vertex.handle_out = to;
    vertex.handle_in = sub([2.0 * vertex.anchor[0], 2.0 * vertex.anchor[1]], to);
}

/// Turn vertex `index` between corner and smooth. A smooth vertex's handles
/// return to its anchor; a corner gets handles along the line between its
/// neighbours, a third of the way to each.
pub fn toggle_smooth(points: &mut [ShapePathPoint], index: usize) {
    let count = points.len();
    let Some(vertex) = points.get(index).copied() else {
        return;
    };
    if is_smooth(&vertex) {
        points[index] = ShapePathPoint::corner(vertex.anchor);
        return;
    }
    if count < 2 {
        return;
    }
    let before = points[(index + count - 1) % count].anchor;
    let after = points[(index + 1) % count].anchor;
    let direction = sub(after, before);
    let span = length(direction);
    if span <= f32::EPSILON {
        return;
    }
    let unit = [direction[0] / span, direction[1] / span];
    let reach_in = length(sub(vertex.anchor, before)) / 3.0;
    let reach_out = length(sub(after, vertex.anchor)) / 3.0;
    points[index].handle_in = sub(vertex.anchor, [unit[0] * reach_in, unit[1] * reach_in]);
    points[index].handle_out = add(vertex.anchor, [unit[0] * reach_out, unit[1] * reach_out]);
}

/// The point at `t` on the cubic segment from `a` to `b`.
pub(super) fn segment_at(a: &ShapePathPoint, b: &ShapePathPoint, t: f32) -> [f32; 2] {
    let p01 = lerp(a.anchor, a.handle_out, t);
    let p12 = lerp(a.handle_out, b.handle_in, t);
    let p23 = lerp(b.handle_in, b.anchor, t);
    lerp(lerp(p01, p12, t), lerp(p12, p23, t), t)
}

/// The segments of a path as `(from, to)` vertex indices: every neighbour pair,
/// plus last-to-first when the path is closed.
fn segments(count: usize, closed: bool) -> impl Iterator<Item = (usize, usize)> {
    let open = count.saturating_sub(1);
    let last = usize::from(closed && count >= 2);
    (0..open + last).map(move |from| (from, (from + 1) % count))
}

/// The segment under window point `at` and how far along it, within
/// [`SEGMENT_HIT`], nearest first.
pub fn segment_hit(
    points: &[ShapePathPoint],
    closed: bool,
    space: PathSpace,
    at: [f32; 2],
) -> Option<(usize, f32)> {
    let mut best: Option<(f32, usize, f32)> = None;
    for (from, to) in segments(points.len(), closed) {
        for step in 1..SEGMENT_SAMPLES {
            let t = step as f32 / SEGMENT_SAMPLES as f32;
            let on = space.to_window(segment_at(&points[from], &points[to], t));
            let distance = length(sub(on, at));
            if distance <= SEGMENT_HIT && best.is_none_or(|(nearest, ..)| distance < nearest) {
                best = Some((distance, from, t));
            }
        }
    }
    best.map(|(_, from, t)| (from, t))
}

/// Split the segment leaving vertex `from` at `t`, keeping the curve's shape,
/// and return the new vertex's index. A straight segment between two corners
/// gains a corner.
pub fn split_segment(points: &mut Vec<ShapePathPoint>, from: usize, t: f32) -> usize {
    let count = points.len();
    let to = (from + 1) % count;
    let (a, b) = (points[from], points[to]);
    let p01 = lerp(a.anchor, a.handle_out, t);
    let p12 = lerp(a.handle_out, b.handle_in, t);
    let p23 = lerp(b.handle_in, b.anchor, t);
    let p012 = lerp(p01, p12, t);
    let p123 = lerp(p12, p23, t);
    let anchor = lerp(p012, p123, t);
    let straight = a.handle_out == a.anchor && b.handle_in == b.anchor;
    let vertex = if straight {
        ShapePathPoint::corner(anchor)
    } else {
        points[from].handle_out = p01;
        points[to].handle_in = p23;
        ShapePathPoint {
            anchor,
            handle_in: p012,
            handle_out: p123,
        }
    };
    let index = from + 1;
    points.insert(index, vertex);
    index
}
