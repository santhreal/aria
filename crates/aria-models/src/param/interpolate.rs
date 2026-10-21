//! How a [`Param`] interpolates between keyframes: the [`Lerp`] value trait,
//! per-segment sampling ([`SegmentSample`]), and the arc-length bezier that a
//! 2-d motion path with [`SpatialTangents`] follows.

use serde::{Deserialize, Serialize};

use super::Keyframe;
#[cfg(doc)]
use super::Param;
use crate::error::ModelError;

/// Values a [`Param`] can animate: lerp-able, plain-old-data.
pub trait Lerp: Copy {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

/// Marker for [`Lerp`] types whose `lerp(a, b, t)` is safe for `t` outside
/// `[0, 1]` (overshoot for elastic / back presets). Implemented for scalars,
/// vec2, and [`crate::Scale2`]. Not implemented for colors (`[u8; 4]`) or
/// [`crate::CropRect`] — those clamp / wrap and must not receive piecewise
/// overshoot presets.
pub trait Extrapolate: Lerp {}

impl Extrapolate for f32 {}
impl Extrapolate for [f32; 2] {}
impl Extrapolate for [f32; 3] {}

impl Lerp for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Lerp for [f32; 2] {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        [f32::lerp(a[0], b[0], t), f32::lerp(a[1], b[1], t)]
    }
}

impl Lerp for [f32; 3] {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        [
            f32::lerp(a[0], b[0], t),
            f32::lerp(a[1], b[1], t),
            f32::lerp(a[2], b[2], t),
        ]
    }
}

impl Lerp for [f32; 4] {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        [
            f32::lerp(a[0], b[0], t),
            f32::lerp(a[1], b[1], t),
            f32::lerp(a[2], b[2], t),
            f32::lerp(a[3], b[3], t),
        ]
    }
}

/// RGBA colors (shape fill/stroke animation): per-channel lerp in encoded
/// 8-bit space — what CapCut-class editors do; a perceptual space would be
/// overkill for UI color fades.
impl Lerp for [u8; 4] {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let mut out = [0u8; 4];
        for (o, (&x, &y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
            *o = (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        out
    }
}

/// Segment interpolation with access to the bounding keyframes; the default
/// forwards to plain [`Lerp`]. `[f32; 2]` overrides it to follow cubic bezier
/// motion paths when spatial tangents are present.
///
/// Temporal easing controls SPEED along the path; spatial tangents control
/// SHAPE (After Effects semantics). Both tangents zero/`None` → exact legacy
/// straight-line lerp (early-return, bit-identical).
///
/// Public because it bounds [`Param::sample`]; outside this crate, implement
/// the default (`impl SegmentSample for T {}`) for any custom [`Lerp`] type.
pub trait SegmentSample: Lerp {
    fn segment_sample(a: &Keyframe<Self>, b: &Keyframe<Self>, eased_t: f32) -> Self {
        Lerp::lerp(a.value, b.value, eased_t)
    }

    /// Signed scalar value change from `a` to `b` for temporal speed-to-Bezier
    /// conversion, or arc/chord distance for multi-dimensional values.
    fn segment_delta(_a: &Keyframe<Self>, _b: &Keyframe<Self>) -> f32 {
        1.0
    }

    /// Non-negative spatial/value distance from `a` to `b`, used to distribute
    /// roving keyframe ticks proportionally to segment length.
    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        Self::segment_delta(a, b).abs()
    }
}

impl SegmentSample for f32 {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        b.value - a.value
    }
}

impl SegmentSample for [u8; 4] {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let mut sum = 0.0f32;
        for (&x, &y) in a.value.iter().zip(b.value.iter()) {
            let d = (f32::from(y) - f32::from(x)) / 255.0;
            sum += d * d;
        }
        sum.sqrt()
    }
}

impl SegmentSample for [f32; 3] {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let dx = b.value[0] - a.value[0];
        let dy = b.value[1] - a.value[1];
        let dz = b.value[2] - a.value[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

impl SegmentSample for [f32; 4] {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let mut sum = 0.0f32;
        for (&x, &y) in a.value.iter().zip(b.value.iter()) {
            let d = y - x;
            sum += d * d;
        }
        sum.sqrt()
    }
}

impl SegmentSample for [f32; 2] {
    fn segment_sample(a: &Keyframe<Self>, b: &Keyframe<Self>, eased_t: f32) -> Self {
        let out_t = a.tangents.map(|t| t.out_t).unwrap_or([0.0, 0.0]);
        let in_t = b.tangents.map(|t| t.in_t).unwrap_or([0.0, 0.0]);
        if is_zero_vec2(&out_t) && is_zero_vec2(&in_t) {
            return Lerp::lerp(a.value, b.value, eased_t);
        }
        let p0 = a.value;
        let p1 = [p0[0] + out_t[0], p0[1] + out_t[1]];
        let p3 = b.value;
        let p2 = [p3[0] + in_t[0], p3[1] + in_t[1]];
        let u = arc_length_parameter(p0, p1, p2, p3, eased_t.clamp(0.0, 1.0));
        eval_cubic_bezier2(p0, p1, p2, p3, u)
    }

    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        Self::segment_distance(a, b)
    }

    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let out_t = a.tangents.map(|t| t.out_t).unwrap_or([0.0, 0.0]);
        let in_t = b.tangents.map(|t| t.in_t).unwrap_or([0.0, 0.0]);
        if is_zero_vec2(&out_t) && is_zero_vec2(&in_t) {
            let dx = b.value[0] - a.value[0];
            let dy = b.value[1] - a.value[1];
            return (dx * dx + dy * dy).sqrt();
        }
        let p0 = a.value;
        let p1 = [p0[0] + out_t[0], p0[1] + out_t[1]];
        let p3 = b.value;
        let p2 = [p3[0] + in_t[0], p3[1] + in_t[1]];
        bezier2_arc_length(p0, p1, p2, p3)
    }
}

/// Spatial bezier handles for 2-d position segments (motion paths). Offsets
/// are relative to the keyframe's value, in the same units as the value.
/// `out_t` shapes the segment LEAVING this keyframe, `in_t` the segment
/// ARRIVING at it. `None` ⇔ straight-line (legacy) motion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpatialTangents {
    #[serde(rename = "o", default, skip_serializing_if = "is_zero_vec2")]
    pub out_t: [f32; 2],
    #[serde(rename = "i", default, skip_serializing_if = "is_zero_vec2")]
    pub in_t: [f32; 2],
}

impl SpatialTangents {
    /// Both handles at the origin — equivalent to a missing `tangents` slot
    /// for sampling, but distinct in serde when explicitly stored.
    pub const ZERO: Self = Self {
        out_t: [0.0, 0.0],
        in_t: [0.0, 0.0],
    };

    /// `Ok` when every component is finite and within ±4 canvas fractions
    /// (generous for motion-path handles).
    pub fn validate(self) -> Result<(), ModelError> {
        for c in [self.out_t[0], self.out_t[1], self.in_t[0], self.in_t[1]] {
            if !c.is_finite() || c.abs() > 4.0 {
                return Err(ModelError::InvalidParam(format!(
                    "spatial tangent component {c} must be finite and within ±4.0"
                )));
            }
        }
        Ok(())
    }
}

fn is_zero_vec2(v: &[f32; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}

/// Evaluate a 2-d cubic bezier at parameter `u`.
#[inline]
fn eval_cubic_bezier2(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], u: f32) -> [f32; 2] {
    let u2 = u * u;
    let u3 = u2 * u;
    let mu = 1.0 - u;
    let mu2 = mu * mu;
    let mu3 = mu2 * mu;
    [
        mu3 * p0[0] + 3.0 * mu2 * u * p1[0] + 3.0 * mu * u2 * p2[0] + u3 * p3[0],
        mu3 * p0[1] + 3.0 * mu2 * u * p1[1] + 3.0 * mu * u2 * p2[1] + u3 * p3[1],
    ]
}

/// Arc length of a 2-d cubic bezier segment using the 17-point LUT.
#[inline]
fn bezier2_arc_length(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> f32 {
    const LUT_SIZE: usize = 17;
    let mut total = 0.0f32;
    let mut prev = p0;
    for i in 1..LUT_SIZE {
        let u = i as f32 / (LUT_SIZE - 1) as f32;
        let pt = eval_cubic_bezier2(p0, p1, p2, p3, u);
        let dx = pt[0] - prev[0];
        let dy = pt[1] - prev[1];
        total += (dx * dx + dy * dy).sqrt();
        prev = pt;
    }
    total
}

/// Map eased progress `t` in `0..=1` to a cubic parameter `u` so that equal
/// `t` steps travel roughly equal arc length along the curve.
#[inline]
fn arc_length_parameter(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> f32 {
    // Collinear straight-line fast path: skip 17 bezier evaluations.
    if (p1[0] - p0[0]).abs() <= f32::EPSILON
        && (p1[1] - p0[1]).abs() <= f32::EPSILON
        && (p3[0] - p2[0]).abs() <= f32::EPSILON
        && (p3[1] - p2[1]).abs() <= f32::EPSILON
    {
        return t;
    }
    const LUT_SIZE: usize = 17;
    let mut cumlen = [0.0f32; LUT_SIZE];
    let mut prev = p0;
    for i in 1..LUT_SIZE {
        let u = i as f32 / (LUT_SIZE - 1) as f32;
        let pt = eval_cubic_bezier2(p0, p1, p2, p3, u);
        let dx = pt[0] - prev[0];
        let dy = pt[1] - prev[1];
        cumlen[i] = cumlen[i - 1] + (dx * dx + dy * dy).sqrt();
        prev = pt;
    }
    let total = cumlen[LUT_SIZE - 1];
    if total <= f32::EPSILON {
        return t;
    }
    let target = t * total;
    // First index with cumlen[i] >= target (linear scan; 17 entries).
    let mut hi = 1;
    while hi < LUT_SIZE - 1 && cumlen[hi] < target {
        hi += 1;
    }
    let lo = hi - 1;
    let span = cumlen[hi] - cumlen[lo];
    let local = if span > 0.0 {
        (target - cumlen[lo]) / span
    } else {
        0.0
    };
    let u0 = lo as f32 / (LUT_SIZE - 1) as f32;
    let u1 = hi as f32 / (LUT_SIZE - 1) as f32;
    u0 + (u1 - u0) * local
}
