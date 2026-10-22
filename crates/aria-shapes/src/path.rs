//! Pen-tool paths: cubic-bezier outlines rasterized on the CPU.
//!
//! Arbitrary curved paths have no cheap SDF, so unlike the parametric shapes
//! they realize as bitmaps through `tiny-skia` and ride the compositor's
//! existing RGBA layer path. Paths animate through the clip *transform*
//! (position/scale/rotation/opacity on the GPU quad), so a raster is rebuilt
//! only when the path or its style is edited — [`PathRaster`] memoizes the
//! recent results exactly like `aria-text`'s renderer memoizes text runs.
//!
//! Fill rule is **non-zero winding** everywhere (raster and
//! [`path_hit_test`]), matching the pen-tool convention in Photoshop/AE.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use aria_core::RgbaImage;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Transform};

use crate::{BezierPath, PathPoint, ShapeStyle};

/// Flattening steps per cubic segment for bounds/hit-test queries. Editor
/// precision, not raster precision (tiny-skia flattens adaptively when
/// drawing): 16 chords keep the error well under a pixel at UI sizes.
const FLATTEN_STEPS: usize = 16;

/// Extra transparent margin around the inked bounds, covering tiny-skia's
/// anti-alias overhang.
const RASTER_PAD: f32 = 2.0;

/// Rasterizes pen paths, memoizing recent results. Owned by the renderer next
/// to the `TextRenderer`; reuse one across frames so unchanged paths cost a
/// memo lookup. A raster is shared with the memo, so the compositor hashes it
/// once rather than on every frame.
pub struct PathRaster {
    memo: HashMap<PathKey, Arc<RgbaImage>>,
    order: VecDeque<PathKey>,
}

impl std::fmt::Debug for PathRaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathRaster")
            .field("cached", &self.memo.len())
            .finish()
    }
}

impl Default for PathRaster {
    fn default() -> Self {
        Self::new()
    }
}

impl PathRaster {
    pub fn new() -> Self {
        Self {
            memo: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Rasterize `path` (shape-local pixels) scaled by `scale` into a
    /// straight-alpha bitmap sized to the inked bounds plus stroke overhang.
    /// The bounds center lands on the image center — the point the renderer
    /// places at the layer center. Returns a zero-area image for paths with
    /// nothing to draw (fewer than 2 points, or no fill and no stroke).
    pub fn rasterize(
        &mut self,
        path: &BezierPath,
        style: &ShapeStyle,
        scale: f32,
    ) -> Arc<RgbaImage> {
        let key = PathKey::new(path, style, scale);
        self.memoized(key, || rasterize_uncached(path, style, scale))
    }

    /// Rasterize a closed mask `points` (in normalized `0..1` layer fractions)
    /// onto a `width × height` stencil image. `center` shifts the path in
    /// layer fractions and `rotation_rad` rotates around `0.5 + center`.
    /// `expansion` offsets the edge by that many pixels, outward when
    /// positive, with round corners.
    pub fn rasterize_mask(
        &mut self,
        width: u32,
        height: u32,
        points: &[PathPoint],
        center: [f32; 2],
        rotation_rad: f32,
        expansion: f32,
    ) -> Arc<RgbaImage> {
        let (w, h) = (width.max(1), height.max(1));
        let (wf, hf) = (w as f32, h as f32);
        let map = |v: [f32; 2]| [(v[0] + center[0]) * wf, (v[1] + center[1]) * hf];
        let pixel_path = BezierPath {
            points: points
                .iter()
                .map(|p| PathPoint {
                    anchor: map(p.anchor),
                    handle_in: map(p.handle_in),
                    handle_out: map(p.handle_out),
                })
                .collect(),
            closed: true,
        };
        let style = ShapeStyle {
            fill: Some([255, 255, 255, 255]),
            stroke: Some(crate::Stroke {
                rgba: w.to_be_bytes(),
                width: f32::from_bits(h),
            }),
        };
        let expansion = if expansion.is_finite() {
            expansion
        } else {
            0.0
        };
        let key = PathKey {
            expansion: expansion.to_bits(),
            ..PathKey::new(&pixel_path, &style, rotation_rad)
        };
        self.memoized(key, || {
            rasterize_mask_uncached(&pixel_path, w, h, center, rotation_rad, expansion)
        })
    }

    fn memoized(&mut self, key: PathKey, build: impl FnOnce() -> RgbaImage) -> Arc<RgbaImage> {
        if let Some(hit) = self.memo.get(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
                self.order.push_back(key);
            }
            return Arc::clone(hit);
        }
        let image = Arc::new(build());
        const MEMO_CAP: usize = 32;
        while self.memo.len() >= MEMO_CAP {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.memo.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.memo.insert(key, Arc::clone(&image));
        image
    }
}

/// The pixel size [`PathRaster::rasterize`] returns for `path` drawn with
/// `style` at `scale`, without drawing it: the inked bounds plus the stroke
/// overhang and the anti-alias margin. `(0, 0)` for a path with nothing to
/// draw.
pub fn path_raster_size(path: &BezierPath, style: &ShapeStyle, scale: f32) -> (u32, u32) {
    raster_frame(path, style, scale).map_or((0, 0), |frame| (frame.width, frame.height))
}

/// What one raster of a path covers. Built once and read by both the size
/// query and the rasterizer, so the two cannot disagree on a pixel.
struct RasterFrame {
    /// The outline at raster scale.
    outline: tiny_skia::Path,
    /// The stroke width at raster scale, `0` without a stroke.
    stroke_w: f32,
    /// The transparent margin on every side of the inked bounds.
    pad: f32,
    /// The raster scale, a non-finite or non-positive request read as `1`.
    scale: f32,
    width: u32,
    height: u32,
}

/// The raster frame of `path` drawn with `style` at `scale`, or `None` when
/// the path has no drawable segment or neither a fill nor a stroke.
fn raster_frame(path: &BezierPath, style: &ShapeStyle, scale: f32) -> Option<RasterFrame> {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    if style.fill.is_none() && style.stroke.is_none() {
        return None;
    }
    let outline = build_skia_path(path, scale)?;
    let stroke_w = style.stroke.map_or(0.0, |s| (s.width * scale).max(0.0));
    let bounds = outline.bounds();
    let pad = stroke_w * 0.5 + RASTER_PAD;
    Some(RasterFrame {
        width: (bounds.width() + 2.0 * pad).ceil().max(1.0) as u32,
        height: (bounds.height() + 2.0 * pad).ceil().max(1.0) as u32,
        outline,
        stroke_w,
        pad,
        scale,
    })
}

/// Whether a point on a path's raster lands on the path's ink.
///
/// `offset` is measured from the raster's center in raster pixels, the frame
/// the renderer places the bitmap in. It is mapped back to shape-local pixels
/// through the same frame [`PathRaster::rasterize`] draws with and tested with
/// [`path_hit_test`], so a closed path is a hitbox inside its outline whatever
/// its fill colour. `false` for a path with nothing to draw.
pub fn path_raster_hit(
    path: &BezierPath,
    style: &ShapeStyle,
    scale: f32,
    offset: [f32; 2],
) -> bool {
    let Some(frame) = raster_frame(path, style, scale) else {
        return false;
    };
    // `rasterize` centres the inked bounds in the bitmap.
    let bounds = frame.outline.bounds();
    let local = [
        (bounds.x() + bounds.width() * 0.5 + offset[0]) / frame.scale,
        (bounds.y() + bounds.height() * 0.5 + offset[1]) / frame.scale,
    ];
    let width = style.stroke.map_or(0.0, |stroke| stroke.width.max(0.0));
    path_hit_test(path, local, width)
}

fn rasterize_uncached(path: &BezierPath, style: &ShapeStyle, scale: f32) -> RgbaImage {
    let Some(RasterFrame {
        outline: skia_path,
        stroke_w,
        pad,
        width: w,
        height: h,
        ..
    }) = raster_frame(path, style, scale)
    else {
        return RgbaImage::transparent(0, 0);
    };
    let bounds = skia_path.bounds();
    let Some(mut pixmap) = Pixmap::new(w, h) else {
        return RgbaImage::transparent(0, 0);
    };
    // Center the inked bounds in the padded image.
    let translate = Transform::from_translate(
        pad - bounds.x() + (w as f32 - bounds.width() - 2.0 * pad) * 0.5,
        pad - bounds.y() + (h as f32 - bounds.height() - 2.0 * pad) * 0.5,
    );

    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };

    // Fill only closes visually on closed paths; an open pen path is
    // stroke-only even if a fill color is set (matches AE/Photoshop).
    if let (Some(fill), true) = (style.fill, path.closed) {
        paint.set_color_rgba8(fill[0], fill[1], fill[2], fill[3]);
        pixmap.fill_path(&skia_path, &paint, FillRule::Winding, translate, None);
    }
    if let Some(stroke) = style.stroke
        && stroke_w > 0.0
    {
        paint.set_color_rgba8(
            stroke.rgba[0],
            stroke.rgba[1],
            stroke.rgba[2],
            stroke.rgba[3],
        );
        let sk_stroke = tiny_skia::Stroke {
            width: stroke_w,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..tiny_skia::Stroke::default()
        };
        pixmap.stroke_path(&skia_path, &paint, &sk_stroke, translate, None);
    }

    // tiny-skia stores premultiplied alpha; the compositor's RGBA path takes
    // straight alpha (it premultiplies on upload).
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    for (i, px) in pixmap.pixels().iter().enumerate() {
        let c = px.demultiply();
        let o = i * 4;
        out[o] = c.red();
        out[o + 1] = c.green();
        out[o + 2] = c.blue();
        out[o + 3] = c.alpha();
    }
    RgbaImage::new(w, h, out)
}

fn rasterize_mask_uncached(
    pixel_path: &BezierPath,
    w: u32,
    h: u32,
    center: [f32; 2],
    rotation_rad: f32,
    expansion: f32,
) -> RgbaImage {
    let Some(mut pixmap) = Pixmap::new(w, h) else {
        return RgbaImage::transparent(0, 0);
    };
    if let Some(skia_path) = build_skia_path(pixel_path, 1.0) {
        let paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        let transform = if rotation_rad.abs() > 1e-6 {
            Transform::from_rotate_at(
                rotation_rad.to_degrees(),
                (0.5 + center[0]) * w as f32,
                (0.5 + center[1]) * h as f32,
            )
        } else {
            Transform::identity()
        };
        pixmap.fill_path(&skia_path, &paint, FillRule::Winding, transform, None);
        // A stroke twice the expansion wide reaches that far either side of
        // the edge: drawn over the fill it grows the mask, erased out of it
        // it shrinks the mask.
        if expansion != 0.0 {
            let edge = Paint {
                blend_mode: if expansion > 0.0 {
                    tiny_skia::BlendMode::SourceOver
                } else {
                    tiny_skia::BlendMode::DestinationOut
                },
                ..paint
            };
            let stroke = tiny_skia::Stroke {
                width: expansion.abs() * 2.0,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };
            pixmap.stroke_path(&skia_path, &edge, &stroke, transform, None);
        }
    }
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    for (i, px) in pixmap.pixels().iter().enumerate() {
        let a = px.alpha();
        let o = i * 4;
        out[o] = 255;
        out[o + 1] = 255;
        out[o + 2] = 255;
        out[o + 3] = a;
    }
    RgbaImage::new(w, h, out)
}

/// Build the tiny-skia path for `path` scaled by `scale`. `None` when the
/// path has no drawable segment or a coordinate is non-finite.
fn build_skia_path(path: &BezierPath, scale: f32) -> Option<tiny_skia::Path> {
    if !path.is_drawable() {
        return None;
    }
    let s = |v: [f32; 2]| (v[0] * scale, v[1] * scale);
    let mut pb = PathBuilder::new();
    let first = path.points[0];
    let (x0, y0) = s(first.anchor);
    pb.move_to(x0, y0);
    for pair in path.points.windows(2) {
        push_cubic(&mut pb, &pair[0], &pair[1], scale);
    }
    if path.closed {
        let last = path.points[path.points.len() - 1];
        push_cubic(&mut pb, &last, &first, scale);
        pb.close();
    }
    pb.finish()
}

/// Append the cubic segment `a → b` (out-handle of `a`, in-handle of `b`).
/// Two collapsed handles degrade to a straight line, which keeps corner
/// points sharp instead of numerically flat cubics.
fn push_cubic(pb: &mut PathBuilder, a: &PathPoint, b: &PathPoint, scale: f32) {
    let straight = a.handle_out == a.anchor && b.handle_in == b.anchor;
    if straight {
        pb.line_to(b.anchor[0] * scale, b.anchor[1] * scale);
    } else {
        pb.cubic_to(
            a.handle_out[0] * scale,
            a.handle_out[1] * scale,
            b.handle_in[0] * scale,
            b.handle_in[1] * scale,
            b.anchor[0] * scale,
            b.anchor[1] * scale,
        );
    }
}

/// Tight bounds of the drawn curve in shape-local pixels (`(min, max)`
/// corners), or `None` for non-drawable paths. Computed on the flattened
/// curve, so it hugs the ink (control points may lie far outside). The pen
/// tool uses this to normalize a committed path around its center; selection
/// UIs use it for the content box.
pub fn path_bounds(path: &BezierPath) -> Option<([f32; 2], [f32; 2])> {
    let pts = flatten(path)?;
    let mut min = pts[0];
    let mut max = pts[0];
    for p in &pts {
        min = [min[0].min(p[0]), min[1].min(p[1])];
        max = [max[0].max(p[0]), max[1].max(p[1])];
    }
    Some((min, max))
}

/// True when `point` (shape-local pixels) hits the path: inside the fill
/// (non-zero winding; closed paths only) or within half the stroke width of
/// the curve. Editor hit-testing — not a per-frame call.
pub fn path_hit_test(path: &BezierPath, point: [f32; 2], stroke_width: f32) -> bool {
    let Some(pts) = flatten(path) else {
        return false;
    };

    // Distance to the polyline (squared, for the stroke test).
    let tol = (stroke_width * 0.5).max(1.0);
    let tol2 = tol * tol;
    let segment_hit = |a: [f32; 2], b: [f32; 2]| -> bool {
        let e = [b[0] - a[0], b[1] - a[1]];
        let w = [point[0] - a[0], point[1] - a[1]];
        let len2 = e[0] * e[0] + e[1] * e[1];
        if !len2.is_finite() || len2 <= 1e-12 {
            return w[0] * w[0] + w[1] * w[1] <= tol2;
        }
        let t = ((w[0] * e[0] + w[1] * e[1]) / len2).clamp(0.0, 1.0);
        let d = [w[0] - e[0] * t, w[1] - e[1] * t];
        d[0] * d[0] + d[1] * d[1] <= tol2
    };
    for pair in pts.windows(2) {
        if segment_hit(pair[0], pair[1]) {
            return true;
        }
    }
    if path.closed && segment_hit(pts[pts.len() - 1], pts[0]) {
        return true;
    }

    // Fill: non-zero winding number over the closed flattened loop.
    if path.closed {
        let mut winding = 0i32;
        let n = pts.len();
        for i in 0..n {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            if a[1] <= point[1] {
                if b[1] > point[1] && cross(a, b, point) > 0.0 {
                    winding += 1;
                }
            } else if b[1] <= point[1] && cross(a, b, point) < 0.0 {
                winding -= 1;
            }
        }
        if winding != 0 {
            return true;
        }
    }
    false
}

/// Cross product `(b - a) × (p - a)`: positive when `p` is left of `a→b`.
fn cross(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Flatten the path's cubics into a polyline (shape-local pixels), `None`
/// for non-drawable paths.
fn flatten(path: &BezierPath) -> Option<Vec<[f32; 2]>> {
    if !path.is_drawable() {
        return None;
    }
    let mut out = Vec::with_capacity(path.points.len() * FLATTEN_STEPS);
    out.push(path.points[0].anchor);
    for pair in path.points.windows(2) {
        flatten_segment(&pair[0], &pair[1], &mut out);
    }
    if path.closed {
        let last = path.points[path.points.len() - 1];
        let first = path.points[0];
        flatten_segment(&last, &first, &mut out);
        out.pop(); // the loop's last point duplicates the first
    }
    Some(out)
}

/// Append the flattened chords of segment `a → b` (excluding `a`'s anchor).
fn flatten_segment(a: &PathPoint, b: &PathPoint, out: &mut Vec<[f32; 2]>) {
    if a.handle_out == a.anchor && b.handle_in == b.anchor {
        out.push(b.anchor);
        return;
    }
    let (p0, p1, p2, p3) = (a.anchor, a.handle_out, b.handle_in, b.anchor);
    for k in 1..=FLATTEN_STEPS {
        let t = k as f32 / FLATTEN_STEPS as f32;
        let u = 1.0 - t;
        let c0 = u * u * u;
        let c1 = 3.0 * u * u * t;
        let c2 = 3.0 * u * t * t;
        let c3 = t * t * t;
        out.push([
            c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0],
            c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1],
        ]);
    }
}

/// Memo identity of a raster request: every coordinate/color/width by bit
/// pattern, so any edit is a new key (never a wrong hit).
#[derive(Clone, PartialEq, Eq, Hash)]
struct PathKey {
    points: Vec<[u32; 6]>,
    closed: bool,
    fill: Option<[u8; 4]>,
    stroke: Option<([u8; 4], u32)>,
    scale: u32,
    /// Mask edge offset by bit pattern; `0` for a raster that is not a mask.
    expansion: u32,
}

impl PathKey {
    fn new(path: &BezierPath, style: &ShapeStyle, scale: f32) -> Self {
        Self {
            points: path
                .points
                .iter()
                .map(|p| {
                    [
                        p.anchor[0].to_bits(),
                        p.anchor[1].to_bits(),
                        p.handle_in[0].to_bits(),
                        p.handle_in[1].to_bits(),
                        p.handle_out[0].to_bits(),
                        p.handle_out[1].to_bits(),
                    ]
                })
                .collect(),
            closed: path.closed,
            fill: style.fill,
            stroke: style.stroke.map(|s| (s.rgba, s.width.to_bits())),
            scale: scale.to_bits(),
            expansion: 0,
        }
    }
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
