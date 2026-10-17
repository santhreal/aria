use serde::{Deserialize, Serialize};

use crate::error::ModelError;

/// Minimum handle influence (`0.1%` of the adjacent segment duration).
pub const MIN_TEMPORAL_INFLUENCE: f32 = 0.001;

/// Maximum handle influence (`100%` of the adjacent segment duration).
pub const MAX_TEMPORAL_INFLUENCE: f32 = 1.0;

/// Default After-Effects Easy Ease influence (`33.33%`).
pub const EASY_EASE_INFLUENCE: f32 = 1.0 / 3.0;

fn is_false(flag: &bool) -> bool {
    !*flag
}

fn default_temporal_points() -> [f32; 4] {
    [
        EASY_EASE_INFLUENCE,
        EASY_EASE_INFLUENCE,
        1.0 - EASY_EASE_INFLUENCE,
        1.0 - EASY_EASE_INFLUENCE,
    ]
}

/// Per-side After-Effects temporal ease: velocity (value units per tick) and
/// handle influence (`0.001..=1.0`, corresponding to `0.1%..=100%` of the
/// adjacent segment duration).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TemporalEase {
    /// Velocity at the keyframe on this side, in value units per tick.
    pub speed: f32,
    /// Handle length as a fraction of the adjacent segment duration (`0.001..=1.0`).
    pub influence: f32,
}

impl Default for TemporalEase {
    fn default() -> Self {
        Self::EASY_EASE
    }
}

impl TemporalEase {
    /// Standard Easy Ease handle (`0.0` speed, `33.33%` influence).
    pub const EASY_EASE: Self = Self {
        speed: 0.0,
        influence: EASY_EASE_INFLUENCE,
    };

    pub const fn new(speed: f32, influence: f32) -> Self {
        Self { speed, influence }
    }

    /// Construct a handle with `influence` clamped into `0.001..=1.0`.
    pub fn clamped(speed: f32, influence: f32) -> Self {
        Self {
            speed,
            influence: influence.clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE),
        }
    }

    /// Validate finiteness and influence bounds (`0.001..=1.0`).
    pub fn validate(self) -> Result<(), ModelError> {
        if !self.speed.is_finite() || !self.influence.is_finite() {
            return Err(ModelError::InvalidParam(
                "temporal ease has non-finite speed or influence".into(),
            ));
        }
        if !(MIN_TEMPORAL_INFLUENCE..=MAX_TEMPORAL_INFLUENCE).contains(&self.influence) {
            return Err(ModelError::InvalidParam(
                "temporal ease influence must be in 0.001..=1.0".into(),
            ));
        }
        Ok(())
    }

    /// Convert an outgoing and incoming [`TemporalEase`] pair into normalized
    /// cubic-bezier control points `[x1, y1, x2, y2]` for a segment of duration
    /// `dt` ticks and signed value change `dv`.
    pub fn control_points_for_segment(
        out_ease: Option<TemporalEase>,
        in_ease: Option<TemporalEase>,
        dt: f32,
        dv: f32,
    ) -> [f32; 4] {
        let (x1, y1) = match out_ease {
            Some(out) => {
                let inf = out
                    .influence
                    .clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE);
                let y = if dv.abs() > 1e-6 && dt > 0.0 {
                    (out.speed * inf * dt) / dv
                } else {
                    0.0
                };
                (inf, y)
            }
            None => (EASY_EASE_INFLUENCE, EASY_EASE_INFLUENCE),
        };
        let (x2, y2) = match in_ease {
            Some(inp) => {
                let inf = inp
                    .influence
                    .clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE);
                let x = (1.0 - inf).clamp(0.0, 1.0 - MIN_TEMPORAL_INFLUENCE);
                let y = if dv.abs() > 1e-6 && dt > 0.0 {
                    1.0 - (inp.speed * inf * dt) / dv
                } else {
                    1.0
                };
                (x, y)
            }
            None => (1.0 - EASY_EASE_INFLUENCE, 1.0 - EASY_EASE_INFLUENCE),
        };
        [x1, y1, x2, y2]
    }

    /// Derive `(outgoing, incoming)` temporal eases from a segment's easing,
    /// duration `dt` ticks, and signed value change `dv`.
    pub fn from_segment_easing(easing: Easing, dt: f32, dv: f32) -> (Self, Self) {
        let avg = if dt > 0.0 { dv / dt } else { 0.0 };
        let Some([x1, y1, x2, y2]) = easing.control_points() else {
            return (Self::EASY_EASE, Self::EASY_EASE);
        };
        let out_inf = x1.clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE);
        let in_inf = (1.0 - x2).clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE);
        let out_speed = easing.out_ease().map_or_else(
            || {
                if x1 > 1e-6 && dt > 0.0 {
                    (y1 * dv) / (x1 * dt)
                } else {
                    avg
                }
            },
            |e| e.speed,
        );
        let in_speed = easing.in_ease().map_or_else(
            || {
                if (1.0 - x2) > 1e-6 && dt > 0.0 {
                    ((1.0 - y2) * dv) / ((1.0 - x2) * dt)
                } else {
                    avg
                }
            },
            |e| e.speed,
        );
        let out_inf = easing.out_ease().map_or(out_inf, |e| {
            e.influence
                .clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE)
        });
        let in_inf = easing.in_ease().map_or(in_inf, |e| {
            e.influence
                .clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE)
        });
        (Self::new(out_speed, out_inf), Self::new(in_speed, in_inf))
    }
}

/// Interpolation curve for the segment *leaving* a keyframe (toward the next
/// one), plus optional per-keyframe After-Effects temporal handles and roving
/// flag. The last keyframe's outgoing segment is unused until a keyframe
/// follows it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    /// Constant-velocity interpolation.
    #[default]
    Linear,
    /// Accelerate from rest (quadratic).
    EaseIn,
    /// Decelerate to rest (quadratic).
    EaseOut,
    /// Accelerate then decelerate (smoothstep).
    EaseInOut,
    /// Step interpolation — the segment keeps the departing keyframe's value
    /// until the next keyframe.
    Hold,
    /// CSS-style cubic bezier: control points `(x1, y1)`, `(x2, y2)` with
    /// `x1`/`x2` in `0..=1`. `y` outside `0..=1` overshoots, like CSS.
    Bezier { points: [f32; 4] },
    /// Keyframe carrying explicit incoming/outgoing [`TemporalEase`] handles,
    /// a roving flag, or a hold toggle, paired with the solved cubic-bezier
    /// control points for its outgoing segment.
    Temporal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        in_ease: Option<TemporalEase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        out_ease: Option<TemporalEase>,
        #[serde(default, skip_serializing_if = "is_false")]
        roving: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        hold: bool,
        #[serde(default = "default_temporal_points")]
        points: [f32; 4],
    },
}

/// A named cubic-bezier easing preset (encoded as [`Easing::Bezier`]).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EasingPreset {
    pub id: String,
    pub label: String,
    /// Control points `(x1, y1, x2, y2)`.
    pub points: [f32; 4],
}

/// Named bezier easings offered in the inspector flyout / AI wire, read once.
/// Shipped set at `data/easings.toml`. Multi-keyframe bounce / elastic / back
/// are defined in the sibling `presets` module.
pub static EASING_PRESETS: std::sync::LazyLock<Vec<EasingPreset>> =
    std::sync::LazyLock::new(|| {
        crate::catalog::load(
            "easings",
            include_str!("../../data/easings.toml"),
            |entries: &[EasingPreset]| {
                crate::catalog::unique_ids(entries, |p| &p.id, |p| &p.label)?;
                for preset in entries {
                    if preset.points.iter().any(|v| !v.is_finite()) {
                        return Err(format!("{} has a non-finite control point", preset.id));
                    }
                    if !(0.0..=1.0).contains(&preset.points[0])
                        || !(0.0..=1.0).contains(&preset.points[2])
                    {
                        return Err(format!("{} has an x control point outside 0…1", preset.id));
                    }
                }
                Ok(())
            },
        )
    });

/// Look up a named easing preset by id.
pub fn easing_preset(id: &str) -> Option<&'static EasingPreset> {
    EASING_PRESETS.iter().find(|p| p.id == id)
}

impl Easing {
    /// Build an [`Easing::Bezier`] from a catalog preset id, if known.
    pub fn from_preset_id(id: &str) -> Option<Self> {
        easing_preset(id).map(|p| Easing::Bezier { points: p.points })
    }

    /// When this is a bezier matching a named preset, return that id.
    pub fn preset_id(self) -> Option<&'static str> {
        let Easing::Bezier { points } = self else {
            return None;
        };
        EASING_PRESETS
            .iter()
            .find(|p| {
                p.points
                    .iter()
                    .zip(points.iter())
                    .all(|(a, b)| (a - b).abs() < 1e-4)
            })
            .map(|p| p.id.as_str())
    }
}

impl Easing {
    /// Whether this keyframe holds its outgoing value until the next keyframe.
    pub fn is_hold(self) -> bool {
        matches!(self, Easing::Hold | Easing::Temporal { hold: true, .. })
    }

    /// Whether this keyframe is marked roving across time.
    pub fn is_roving(self) -> bool {
        matches!(self, Easing::Temporal { roving: true, .. })
    }

    /// Explicit incoming temporal ease stored on this keyframe, if any.
    pub fn in_ease(self) -> Option<TemporalEase> {
        match self {
            Easing::Temporal { in_ease, .. } => in_ease,
            _ => None,
        }
    }

    /// Explicit outgoing temporal ease stored on this keyframe, if any.
    pub fn out_ease(self) -> Option<TemporalEase> {
        match self {
            Easing::Temporal { out_ease, .. } => out_ease,
            _ => None,
        }
    }

    /// Whether this keyframe uses Bezier / Easy Ease temporal interpolation.
    pub fn is_bezier_like(self) -> bool {
        match self {
            Easing::Linear | Easing::Hold => false,
            Easing::EaseIn | Easing::EaseOut | Easing::EaseInOut | Easing::Bezier { .. } => true,
            Easing::Temporal {
                in_ease,
                out_ease,
                hold,
                ..
            } => !hold && (in_ease.is_some() || out_ease.is_some()),
        }
    }

    /// Return a copy of this easing with the `roving` flag set to `roving`.
    pub fn with_roving(self, roving: bool) -> Self {
        match self {
            Easing::Temporal {
                in_ease,
                out_ease,
                hold,
                points,
                ..
            } => {
                if !roving && in_ease.is_none() && out_ease.is_none() {
                    if hold { Easing::Hold } else { Easing::Linear }
                } else {
                    Easing::Temporal {
                        in_ease,
                        out_ease,
                        roving,
                        hold,
                        points,
                    }
                }
            }
            other if !roving => other,
            Easing::Hold => Easing::Temporal {
                in_ease: None,
                out_ease: None,
                roving: true,
                hold: true,
                points: default_temporal_points(),
            },
            other => Easing::Temporal {
                in_ease: None,
                out_ease: None,
                roving: true,
                hold: false,
                points: other
                    .control_points()
                    .unwrap_or_else(default_temporal_points),
            },
        }
    }

    /// Return a copy of this easing with updated `in_ease`, `out_ease`, and `hold`.
    pub fn with_temporal_ease(
        self,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    ) -> Self {
        let roving = self.is_roving();
        if !roving && in_ease.is_none() && out_ease.is_none() {
            return if hold { Easing::Hold } else { Easing::Linear };
        }
        let points = self
            .control_points()
            .unwrap_or_else(default_temporal_points);
        Easing::Temporal {
            in_ease,
            out_ease,
            roving,
            hold,
            points,
        }
    }

    /// CSS-style cubic-bezier control points `(x1, y1, x2, y2)` for UI
    /// display (graph-editor handles). [`Easing::Hold`] has no handles.
    ///
    /// Named polynomial easings use their exact cubic Bézier equivalents
    /// (the same polygons as the private `subsegment` helper); [`Easing::Linear`] is the
    /// identity `(0,0)/(1,1)`. Existing [`Easing::Bezier`] values round-trip
    /// unchanged (including named presets like snappy/overshoot).
    pub fn control_points(self) -> Option<[f32; 4]> {
        match self {
            Easing::Hold | Easing::Temporal { hold: true, .. } => None,
            Easing::Linear => Some([0.0, 0.0, 1.0, 1.0]),
            // Exact Bézier forms of t², 2t−t², and smoothstep (x(s)=s).
            Easing::EaseIn => Some([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0]),
            Easing::EaseOut => Some([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0]),
            Easing::EaseInOut => Some([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0]),
            Easing::Bezier { points } | Easing::Temporal { points, .. } => Some(points),
        }
    }

    /// Map linear progress `t` in `0..=1` to eased progress.
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => t * (2.0 - t),
            Easing::EaseInOut => t * t * (3.0 - 2.0 * t),
            // The jump lands exactly at the next keyframe: `sample_at` picks
            // the following segment when tick == k1.tick (partition_point
            // uses `<=`), so k1's value already applies there; returning 0
            // through the open interval keeps k0's value everywhere before.
            Easing::Hold | Easing::Temporal { hold: true, .. } => {
                if t < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
            Easing::Bezier {
                points: [x1, y1, x2, y2],
            }
            | Easing::Temporal {
                hold: false,
                points: [x1, y1, x2, y2],
                ..
            } => cubic_bezier(t, x1, y1, x2, y2),
        }
    }

    /// Instantaneous derivative `d(apply(t))/dt` for `t` in `0..=1`.
    pub fn derivative(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => 1.0,
            Easing::EaseIn => 2.0 * t,
            Easing::EaseOut => 2.0 * (1.0 - t),
            Easing::EaseInOut => 6.0 * t * (1.0 - t),
            Easing::Hold | Easing::Temporal { hold: true, .. } => 0.0,
            Easing::Bezier {
                points: [x1, y1, x2, y2],
            }
            | Easing::Temporal {
                hold: false,
                points: [x1, y1, x2, y2],
                ..
            } => cubic_bezier_derivative(t, x1, y1, x2, y2),
        }
    }

    /// Definite integral `∫₀ᵗ apply(τ) dτ` for `t` in `0..=1`.
    ///
    /// Speed is a *rate*, so the source position swept by a keyframed speed
    /// segment is the integral of the eased curve, not the eased value
    /// itself (M2 speed ramps). The preset easings integrate in closed form;
    /// bezier falls back to Simpson's rule (smooth and monotonic over the
    /// unit interval, so a fixed step count is accurate and allocation-free).
    pub fn integral_to(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            // ∫ τ        = t²/2
            Easing::Linear => 0.5 * t * t,
            // ∫ τ²       = t³/3
            Easing::EaseIn => t * t * t / 3.0,
            // ∫ (2τ−τ²)  = t² − t³/3
            Easing::EaseOut => t * t - t * t * t / 3.0,
            // ∫ (3τ²−2τ³)= t³ − t⁴/2
            Easing::EaseInOut => {
                let t3 = t * t * t;
                t3 - 0.5 * t3 * t
            }
            // The eased value is 0 through the whole open interval; the
            // measure-zero jump at t = 1 contributes nothing to the integral.
            Easing::Hold | Easing::Temporal { hold: true, .. } => 0.0,
            Easing::Bezier {
                points: [x1, y1, x2, y2],
            }
            | Easing::Temporal {
                hold: false,
                points: [x1, y1, x2, y2],
                ..
            } => {
                const STEPS: usize = 32;
                let h = t / STEPS as f32;
                if h == 0.0 {
                    return 0.0;
                }
                let f = |s: f32| cubic_bezier(s, x1, y1, x2, y2);
                let mut sum = f(0.0) + f(t);
                for i in 1..STEPS {
                    let s = h * i as f32;
                    sum += if i % 2 == 0 { 2.0 } else { 4.0 } * f(s);
                }
                sum * h / 3.0
            }
        }
    }

    /// Reparameterize the portion `from..=to` of this easing back onto
    /// `0..=1`. The returned easing satisfies
    ///
    /// `slice(u) = (self(from + (to-from)u) - self(from)) /
    ///             (self(to) - self(from))`.
    ///
    /// Structural edits use this when a normalized speed-ramp segment is cut
    /// at an arbitrary point. Polynomial presets are first represented as
    /// equivalent cubic Béziers; de Casteljau subdivision then preserves the
    /// exact curve shape. A subrange whose endpoint progress is identical but
    /// whose interior is not flat cannot be represented by one `Easing`, so it
    /// fails closed.
    pub(crate) fn subsegment(self, from: f32, to: f32) -> Result<Self, ModelError> {
        if !from.is_finite()
            || !to.is_finite()
            || !(0.0..1.0).contains(&from)
            || !(0.0..=1.0).contains(&to)
            || from >= to
        {
            return Err(ModelError::InvalidParam(
                "easing subsegment must satisfy 0 <= from < to <= 1".into(),
            ));
        }
        if from == 0.0 && to == 1.0 {
            return Ok(self);
        }
        if self == Easing::Linear {
            return Ok(Easing::Linear);
        }
        // Any subrange of a hold is still a hold: the value stays at the
        // start throughout, and a subrange ending at `to == 1.0` keeps the
        // jump at its own end.
        if self.is_hold() {
            return Ok(Easing::Hold);
        }

        let [x1, y1, x2, y2] = self
            .control_points()
            .expect("named / bezier easings expose control points");
        let points = [
            BezierPoint { x: 0.0, y: 0.0 },
            BezierPoint { x: x1, y: y1 },
            BezierPoint { x: x2, y: y2 },
            BezierPoint { x: 1.0, y: 1.0 },
        ];
        let start_parameter = cubic_parameter_for_x(from, x1, x2);
        let end_parameter = cubic_parameter_for_x(to, x1, x2);
        let segment = cubic_subsegment(points, start_parameter, end_parameter);
        let dx = segment[3].x - segment[0].x;
        let dy = segment[3].y - segment[0].y;
        if dx <= f32::EPSILON {
            return Err(ModelError::InvalidParam(
                "easing subsegment has no time span".into(),
            ));
        }
        if dy.abs() <= f32::EPSILON {
            let midpoint = self.apply(0.5 * (from + to));
            if (midpoint - segment[0].y).abs() <= f32::EPSILON {
                return Ok(Easing::Linear);
            }
            return Err(ModelError::InvalidParam(
                "easing subsegment with equal endpoints is not representable".into(),
            ));
        }

        let normalize_x = |x: f32| ((x - segment[0].x) / dx).clamp(0.0, 1.0);
        let normalize_y = |y: f32| (y - segment[0].y) / dy;
        Ok(Easing::Bezier {
            points: [
                normalize_x(segment[1].x),
                normalize_y(segment[1].y),
                normalize_x(segment[2].x),
                normalize_y(segment[2].y),
            ],
        })
    }

    /// `Ok` iff a bezier's x control points are within `0..=1` and every
    /// component is finite (an x outside the unit range makes the curve
    /// non-monotonic in time — not a function of t).
    pub fn validate(self) -> Result<(), ModelError> {
        match self {
            Easing::Bezier { points } => validate_bezier_points(points),
            Easing::Temporal {
                in_ease,
                out_ease,
                points,
                ..
            } => {
                if let Some(inp) = in_ease {
                    inp.validate()?;
                }
                if let Some(out) = out_ease {
                    out.validate()?;
                }
                validate_bezier_points(points)
            }
            Easing::Linear
            | Easing::EaseIn
            | Easing::EaseOut
            | Easing::EaseInOut
            | Easing::Hold => Ok(()),
        }
    }
}

/// Evaluate a CSS-style cubic bezier easing at progress `t`: solve the curve
/// parameter `s` where `x(s) = t` (Newton with bisection fallback), then
/// return `y(s)`. Endpoints are fixed at (0,0) and (1,1).
#[inline]
fn cubic_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let s = cubic_parameter_for_x(t, x1, x2);
    let (cy, by, ay) = poly_coefficients(y1, y2);
    ((ay * s + by) * s + cy) * s
}

/// Derivative `dy/dx` of the unit cubic Bézier at normalized time `t`.
#[inline]
fn cubic_bezier_derivative(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let s = cubic_parameter_for_x(t.clamp(0.0, 1.0), x1, x2);
    let (cx, bx, ax) = poly_coefficients(x1, x2);
    let (cy, by, ay) = poly_coefficients(y1, y2);
    let dx = (3.0 * ax * s + 2.0 * bx) * s + cx;
    let dy = (3.0 * ay * s + 2.0 * by) * s + cy;
    if dx.abs() > 1e-6 {
        dy / dx
    } else if s <= 0.5 {
        if x1 > 1e-6 { y1 / x1 } else { 0.0 }
    } else if (1.0 - x2) > 1e-6 {
        (1.0 - y2) / (1.0 - x2)
    } else {
        0.0
    }
}

fn validate_bezier_points(points: [f32; 4]) -> Result<(), ModelError> {
    if points.iter().any(|v| !v.is_finite()) {
        return Err(ModelError::InvalidParam(
            "bezier easing has non-finite control point".into(),
        ));
    }
    let [x1, _, x2, _] = points;
    if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
        return Err(ModelError::InvalidParam(
            "bezier easing x control points must be in 0..=1".into(),
        ));
    }
    Ok(())
}

/// Parameter `s` where a unit cubic Bézier with x controls `x1`/`x2`
/// intersects normalized time `x`. Newton converges quickly for ordinary
/// curves; bounded bisection handles flat derivatives deterministically.
#[inline]
fn cubic_parameter_for_x(x: f32, x1: f32, x2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (cx, bx, ax) = poly_coefficients(x1, x2);
    let eval = |s: f32| ((ax * s + bx) * s + cx) * s;
    let mut s = x;
    for _ in 0..8 {
        let error = eval(s) - x;
        if error.abs() < 1e-7 && (0.0..=1.0).contains(&s) {
            return s;
        }
        let dx = (3.0 * ax * s + 2.0 * bx) * s + cx;
        if dx.abs() < 1e-6 {
            break;
        }
        s -= error / dx;
        if !(0.0..=1.0).contains(&s) {
            break;
        }
    }

    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        s = 0.5 * (lo + hi);
        let value = eval(s);
        if (value - x).abs() < 1e-7 {
            return s;
        }
        if value < x {
            lo = s;
        } else {
            hi = s;
        }
    }
    0.5 * (lo + hi)
}

/// Coefficients `(c, b, a)` of `B(s) = a·s³ + b·s² + c·s` for a unit bezier
/// with inner control values `p1`, `p2`.
#[inline]
fn poly_coefficients(p1: f32, p2: f32) -> (f32, f32, f32) {
    let c = 3.0 * p1;
    let b = 3.0 * (p2 - p1) - c;
    let a = 1.0 - c - b;
    (c, b, a)
}

#[derive(Clone, Copy)]
struct BezierPoint {
    x: f32,
    y: f32,
}

impl BezierPoint {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

/// The exact cubic control polygon over parameter interval `from..=to`.
fn cubic_subsegment(points: [BezierPoint; 4], from: f32, to: f32) -> [BezierPoint; 4] {
    let (_, after_start) = split_cubic(points, from);
    let relative_end = ((to - from) / (1.0 - from)).clamp(0.0, 1.0);
    let (segment, _) = split_cubic(after_start, relative_end);
    segment
}

/// De Casteljau subdivision at parameter `t`.
fn split_cubic(points: [BezierPoint; 4], t: f32) -> ([BezierPoint; 4], [BezierPoint; 4]) {
    let p01 = points[0].lerp(points[1], t);
    let p12 = points[1].lerp(points[2], t);
    let p23 = points[2].lerp(points[3], t);
    let p012 = p01.lerp(p12, t);
    let p123 = p12.lerp(p23, t);
    let p = p012.lerp(p123, t);
    ([points[0], p01, p012, p], [p, p123, p23, points[3]])
}
