//! Curve and gradient parameter values, and their GPU lookup tables.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::EffectParamKind;

/// Number of samples in a GPU curve lookup table.
pub const EFFECT_CURVE_LUT_SAMPLES: usize = 64;

/// Number of samples in a GPU gradient lookup table.
pub const EFFECT_GRADIENT_LUT_SAMPLES: usize = 64;

/// One control point `(x, y)` on a 1D [`EffectParamKind::Curve`] parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EffectCurvePoint {
    pub x: f32,
    pub y: f32,
}

impl EffectCurvePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Sample a piecewise-linear 1D curve at `t` in `[0.0, 1.0]`.
pub fn sample_effect_curve(points: &[EffectCurvePoint], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if points.is_empty() {
        return t;
    }
    if points.len() == 1 {
        return points[0].y;
    }
    if t <= points[0].x {
        return points[0].y;
    }
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.x {
            let span = b.x - a.x;
            if span.abs() <= 1e-6 {
                return b.y;
            }
            let u = ((t - a.x) / span).clamp(0.0, 1.0);
            return a.y + (b.y - a.y) * u;
        }
    }
    points[points.len() - 1].y
}

/// Bake a 1D curve into [`EFFECT_CURVE_LUT_SAMPLES`] evenly-spaced samples over `[0, 1]`.
pub fn evaluate_effect_curve_lut(points: &[EffectCurvePoint]) -> [f32; EFFECT_CURVE_LUT_SAMPLES] {
    let mut lut = [0.0_f32; EFFECT_CURVE_LUT_SAMPLES];
    let denom = (EFFECT_CURVE_LUT_SAMPLES - 1) as f32;
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = sample_effect_curve(points, i as f32 / denom);
    }
    lut
}

/// One color stop on a [`EffectParamKind::Gradient`] parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EffectGradientStop {
    /// Normalized position in `[0.0, 1.0]`.
    #[serde(alias = "position")]
    pub offset: f32,
    /// Linear RGBA color at this stop.
    pub color: [f32; 4],
}

impl EffectGradientStop {
    pub const fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

/// Sample a piecewise-linear RGBA gradient at `t` in `[0.0, 1.0]`.
pub fn sample_effect_gradient(stops: &[EffectGradientStop], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    if stops.is_empty() {
        return [t, t, t, 1.0];
    }
    if stops.len() == 1 {
        return stops[0].color;
    }
    if t <= stops[0].offset {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = b.offset - a.offset;
            if span.abs() <= 1e-6 {
                return b.color;
            }
            let u = ((t - a.offset) / span).clamp(0.0, 1.0);
            return [
                a.color[0] + (b.color[0] - a.color[0]) * u,
                a.color[1] + (b.color[1] - a.color[1]) * u,
                a.color[2] + (b.color[2] - a.color[2]) * u,
                a.color[3] + (b.color[3] - a.color[3]) * u,
            ];
        }
    }
    stops[stops.len() - 1].color
}

/// Bake a gradient into [`EFFECT_GRADIENT_LUT_SAMPLES`] linear RGBA samples over `[0, 1]`.
pub fn evaluate_effect_gradient_lut(
    stops: &[EffectGradientStop],
) -> [[f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES] {
    let mut lut = [[0.0_f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES];
    let denom = (EFFECT_GRADIENT_LUT_SAMPLES - 1) as f32;
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = sample_effect_gradient(stops, i as f32 / denom);
    }
    lut
}
