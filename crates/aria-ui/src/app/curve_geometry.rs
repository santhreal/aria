//! Curve geometry: keyframe projection, bezier handle mapping, and polyline sampling.
//!
//! True-to-playback curve rendering: the polyline is built directly by querying
//! [`aria_models::Param::sample_at`] across the clip time window, so the graph
//! never diverges from engine playback evaluation.
//!
//! Lives beside the state, not in the panel that draws it: the drag owner
//! ([`super::curve_drag`]) hit-tests these pixels, and the state may not name a
//! panel.

use aria_models::{Clip, Easing, ParamSpec, ParamValue};

/// Viewport and coordinate mapping parameters for the curve editor canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveBounds {
    pub min_tick: i64,
    pub max_tick: i64,
    pub min_value: f32,
    pub max_value: f32,
    pub width: f32,
    pub height: f32,
}

impl CurveBounds {
    /// Compute mapping bounds from a keyframe sequence and declared property range.
    pub fn new(
        min_tick: i64,
        max_tick: i64,
        min_value: f32,
        max_value: f32,
        width: f32,
        height: f32,
    ) -> Self {
        let max_tick = if max_tick > min_tick {
            max_tick
        } else {
            min_tick + 1
        };
        let (min_val, max_val) = if (max_value - min_value).abs() < 1e-4 {
            (min_value - 1.0, min_value + 1.0)
        } else if min_value > max_value {
            (max_value, min_value)
        } else {
            (min_value, max_value)
        };
        Self {
            min_tick,
            max_tick,
            min_value: min_val,
            max_value: max_val,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    /// Map timeline tick and property value to canvas pixel coordinates (x, y).
    /// Canvas origin (0, 0) is top-left, so higher values map to smaller y.
    pub fn to_pixel(&self, tick: i64, value: f32) -> (f32, f32) {
        self.to_pixel_f64(tick as f64, value)
    }

    /// Fractional tick version of [`Self::to_pixel`].
    pub fn to_pixel_f64(&self, tick: f64, value: f32) -> (f32, f32) {
        let tick_span = (self.max_tick - self.min_tick) as f64;
        let t_frac = if tick_span > 0.0 {
            ((tick - self.min_tick as f64) / tick_span) as f32
        } else {
            0.0
        };
        let val_span = self.max_value - self.min_value;
        let v_frac = if val_span.abs() > 1e-6 {
            (value - self.min_value) / val_span
        } else {
            0.5
        };

        let px_x = t_frac * self.width;
        let px_y = (1.0 - v_frac) * self.height;
        (px_x, px_y)
    }

    /// Convert canvas pixel coordinates back to (tick, value).
    pub fn from_pixel(&self, x: f32, y: f32) -> (i64, f32) {
        let t_frac = (x / self.width).clamp(0.0, 1.0);
        let v_frac = (1.0 - (y / self.height)).clamp(0.0, 1.0);

        let tick_span = (self.max_tick - self.min_tick) as f64;
        let tick = (self.min_tick as f64 + t_frac as f64 * tick_span).round() as i64;
        let val_span = self.max_value - self.min_value;
        let value = self.min_value + v_frac * val_span;

        (tick, value)
    }
}

/// One point on the keyframe polyline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveKeyPoint {
    pub tick: i64,
    pub value: f32,
    pub easing: Easing,
    pub pixel_x: f32,
    pub pixel_y: f32,
}

/// Which control handle on a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BezierHandleKind {
    /// Outgoing tangent handle leaving keyframe 0: (x1, y1)
    Control1,
    /// Incoming tangent handle arriving at keyframe 1: (x2, y2)
    Control2,
}

/// A draggable bezier control handle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveHandle {
    pub segment_index: usize,
    pub kind: BezierHandleKind,
    /// Anchor point (keyframe position)
    pub anchor_x: f32,
    pub anchor_y: f32,
    /// Handle tip position
    pub handle_x: f32,
    pub handle_y: f32,
    /// Normalized control point coordinates in 0..=1 box
    pub u: f32,
    pub v: f32,
}

/// Sampled polyline segments ready for rendering or geometric assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct CurvePolyline {
    /// Sampled curve points: (pixel_x, pixel_y, tick, value)
    pub samples: Vec<(f32, f32, f64, f32)>,
    /// Keyframe markers
    pub key_points: Vec<CurveKeyPoint>,
    /// Active bezier handles for keyframes with bezier easings
    pub handles: Vec<CurveHandle>,
    /// Coordinate mapping bounds used
    pub bounds: CurveBounds,
}

/// Sample a clip property scalar value at a local tick (integer or fractional).
///
/// The catalog row is the caller's, read off the projected property list. It
/// used to be looked up here, which meant enumerating and cloning the whole
/// property catalog once per sample: a hundred-point curve did it a hundred
/// times per frame.
pub fn sample_param_scalar(
    spec: &ParamSpec,
    clip: &Clip,
    component: Option<u8>,
    local_tick: f64,
) -> Option<f32> {
    // Use integer sample when tick is integral, or sample from Param directly.
    let local_i64 = local_tick.round() as i64;
    let val = spec.sample(clip, local_i64)?;
    match (val, component) {
        (ParamValue::Scalar(v), None) => Some(v),
        (ParamValue::Vec2(v), Some(idx)) => v.get(idx as usize).copied(),
        _ => None,
    }
}

/// Build polyline samples from model keyframes using the exact model sampler.
pub fn build_curve_polyline(
    spec: &ParamSpec,
    clip: &Clip,
    component: Option<u8>,
    bounds: CurveBounds,
    sample_count: usize,
) -> Option<CurvePolyline> {
    let raw_keyframes = spec.keyframes(clip);
    if raw_keyframes.is_empty() {
        return None;
    }

    let mut key_points = Vec::with_capacity(raw_keyframes.len());
    for (local_tick, easing) in &raw_keyframes {
        let abs_tick = clip.param_timeline_tick(*local_tick);
        let val = sample_param_scalar(spec, clip, component, *local_tick as f64).unwrap_or(0.0);
        let (px_x, px_y) = bounds.to_pixel(abs_tick, val);
        key_points.push(CurveKeyPoint {
            tick: abs_tick,
            value: val,
            easing: *easing,
            pixel_x: px_x,
            pixel_y: px_y,
        });
    }

    // Build polyline samples across the bounds tick range
    let count = sample_count.max(2);
    let mut samples = Vec::with_capacity(count);
    let tick_span = (bounds.max_tick - bounds.min_tick) as f64;

    for i in 0..count {
        let frac = i as f64 / (count - 1) as f64;
        let abs_tick = bounds.min_tick as f64 + frac * tick_span;
        let local_tick = clip.param_tick_f(abs_tick);
        let val = sample_param_scalar(spec, clip, component, local_tick).unwrap_or(0.0);
        let (px_x, px_y) = bounds.to_pixel_f64(abs_tick, val);
        samples.push((px_x, px_y, abs_tick, val));
    }

    // Build bezier handles for adjacent keyframes
    let mut handles = Vec::new();
    for i in 0..key_points.len().saturating_sub(1) {
        let k0 = &key_points[i];
        let k1 = &key_points[i + 1];
        if let Some([x1, y1, x2, y2]) = k0.easing.control_points() {
            let dx = k1.pixel_x - k0.pixel_x;
            let dy = k1.pixel_y - k0.pixel_y;

            // Handle 1 (leaving k0)
            let h1_x = k0.pixel_x + x1 * dx;
            let h1_y = k0.pixel_y + (1.0 - y1) * dy; // dy is in canvas pixel direction
            handles.push(CurveHandle {
                segment_index: i,
                kind: BezierHandleKind::Control1,
                anchor_x: k0.pixel_x,
                anchor_y: k0.pixel_y,
                handle_x: h1_x,
                handle_y: h1_y,
                u: x1,
                v: y1,
            });

            // Handle 2 (entering k1)
            let h2_x = k0.pixel_x + x2 * dx;
            let h2_y = k0.pixel_y + (1.0 - y2) * dy;
            handles.push(CurveHandle {
                segment_index: i,
                kind: BezierHandleKind::Control2,
                anchor_x: k1.pixel_x,
                anchor_y: k1.pixel_y,
                handle_x: h2_x,
                handle_y: h2_y,
                u: x2,
                v: y2,
            });
        }
    }

    Some(CurvePolyline {
        samples,
        key_points,
        handles,
        bounds,
    })
}
