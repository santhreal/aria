//! Reading the parameter values stored on an effect instance or a transition.
//!
//! [`EffectInstance`](super::EffectInstance) and
//! [`Transition`](crate::Transition) hold one map per kind of value and read a
//! value that was never set as its catalog default. An effect keyframes its
//! scalar, colour, and vector values, so they are read at a clip-relative tick;
//! a transition stores constants and ignores the tick. [`StoredParams`] is the
//! single definition of how a stored value resolves against its
//! [`EffectParamSpec`], how a `visible_when` condition is evaluated, and the
//! layout of the shader storage buffer.

use super::params::{
    EffectCurvePoint, EffectGradientStop, EffectParamKind, EffectParamSpec,
    evaluate_effect_curve_lut, evaluate_effect_gradient_lut, holds_text, holds_vec2,
};
use crate::ids::ClipId;

/// The values stored on an effect instance or a transition, read by
/// parameter spec.
///
/// An implementor provides the raw lookups, each `None` when the value was
/// never set. The provided readers resolve a value against its spec: each
/// returns `None` when the spec is of another kind, and the catalog default
/// when no value is stored.
pub trait StoredParams {
    /// The stored scalar, angle, or integer value named `name` at `tick`.
    fn stored_scalar(&self, name: &str, tick: f64) -> Option<f32>;
    /// The stored 8-bit colour named `name` at `tick`.
    fn stored_color(&self, name: &str, tick: f64) -> Option<[u8; 4]>;
    /// The stored linear RGBA colour named `name` at `tick`.
    fn stored_linear_color(&self, name: &str, tick: f64) -> Option<[f32; 4]>;
    /// The stored vector or size named `name` at `tick`.
    fn stored_vec2(&self, name: &str, tick: f64) -> Option<[f32; 2]>;
    /// The stored 3D point named `name` at `tick`.
    fn stored_vec3(&self, name: &str, tick: f64) -> Option<[f32; 3]>;
    /// The stored boolean named `name`.
    fn stored_bool(&self, name: &str) -> Option<bool>;
    /// The stored choice option id named `name`, valid or not.
    fn stored_choice(&self, name: &str) -> Option<&str>;
    /// The stored curve control points named `name`.
    fn stored_curve(&self, name: &str) -> Option<&[EffectCurvePoint]>;
    /// The stored gradient stops named `name`.
    fn stored_gradient(&self, name: &str) -> Option<&[EffectGradientStop]>;
    /// The stored text, font, or file string named `name`.
    fn stored_text(&self, name: &str) -> Option<&str>;
    /// The stored raw layer `ClipId` named `name`.
    fn stored_layer(&self, name: &str) -> Option<u64>;

    /// The scalar, angle, or integer value of `p` at `tick`. An integer is
    /// rounded.
    fn scalar(&self, p: &EffectParamSpec, tick: f64) -> Option<f32> {
        match p.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => {
                Some(self.stored_scalar(&p.name, tick).unwrap_or(p.default))
            }
            EffectParamKind::Int => Some(
                self.stored_scalar(&p.name, tick)
                    .map_or(p.default_int as f32, f32::round),
            ),
            _ => None,
        }
    }

    /// The integer value of `p` at `tick`.
    fn int(&self, p: &EffectParamSpec, tick: f64) -> Option<i64> {
        (p.kind == EffectParamKind::Int).then(|| {
            self.stored_scalar(&p.name, tick)
                .map_or(p.default_int, |v| v.round() as i64)
        })
    }

    /// The boolean value of `p`.
    fn boolean(&self, p: &EffectParamSpec) -> Option<bool> {
        (p.kind == EffectParamKind::Bool)
            .then(|| self.stored_bool(&p.name).unwrap_or(p.default_bool))
    }

    /// The option id `p` selects: the stored id when the catalog lists it,
    /// else the default.
    fn choice<'s>(&'s self, p: &'s EffectParamSpec) -> Option<&'s str> {
        if p.kind != EffectParamKind::Choice {
            return None;
        }
        p.choice_or_default(self.stored_choice(&p.name))
    }

    /// The index in `p.choices` of the option `p` selects.
    fn choice_index(&self, p: &EffectParamSpec) -> Option<usize> {
        let id = self.choice(p)?;
        Some(p.choice_index(id).unwrap_or(0))
    }

    /// The 8-bit colour of `p` at `tick`. A linear colour is clamped and
    /// rounded to 8 bits.
    fn color(&self, p: &EffectParamSpec, tick: f64) -> Option<[u8; 4]> {
        if p.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(c) = self.stored_color(&p.name, tick) {
            return Some(c);
        }
        if let Some(c) = self.stored_linear_color(&p.name, tick) {
            return Some(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        Some(p.default_color_u8())
    }

    /// The linear RGBA colour of `p` at `tick`. An 8-bit colour is divided
    /// by 255.
    fn linear_color(&self, p: &EffectParamSpec, tick: f64) -> Option<[f32; 4]> {
        if p.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(c) = self.stored_linear_color(&p.name, tick) {
            return Some(c);
        }
        if let Some(c) = self.stored_color(&p.name, tick) {
            return Some(c.map(|v| f32::from(v) / 255.0));
        }
        Some(p.default_color_linear())
    }

    /// The vector or size value of `p` at `tick`.
    fn vec2(&self, p: &EffectParamSpec, tick: f64) -> Option<[f32; 2]> {
        holds_vec2(p.kind).then(|| self.stored_vec2(&p.name, tick).unwrap_or(p.default_vec2))
    }

    /// The 3D point value of `p` at `tick`.
    fn vec3(&self, p: &EffectParamSpec, tick: f64) -> Option<[f32; 3]> {
        (p.kind == EffectParamKind::Point3)
            .then(|| self.stored_vec3(&p.name, tick).unwrap_or(p.default_vec3))
    }

    /// The curve control points of `p`.
    fn curve(&self, p: &EffectParamSpec) -> Option<Vec<EffectCurvePoint>> {
        (p.kind == EffectParamKind::Curve).then(|| {
            self.stored_curve(&p.name)
                .map_or_else(|| p.default_curve_points(), <[_]>::to_vec)
        })
    }

    /// The gradient stops of `p`.
    fn gradient(&self, p: &EffectParamSpec) -> Option<Vec<EffectGradientStop>> {
        (p.kind == EffectParamKind::Gradient).then(|| {
            self.stored_gradient(&p.name)
                .map_or_else(|| p.default_gradient_stops(), <[_]>::to_vec)
        })
    }

    /// The text, font, or file string of `p`.
    fn text(&self, p: &EffectParamSpec) -> Option<String> {
        holds_text(p.kind).then(|| {
            self.stored_text(&p.name)
                .map_or_else(|| p.default_text.clone(), str::to_owned)
        })
    }

    /// The clip `p` refers to, or `Some(None)` when it refers to none.
    fn layer(&self, p: &EffectParamSpec) -> Option<Option<ClipId>> {
        (p.kind == EffectParamKind::Layer).then(|| {
            let raw = self.stored_layer(&p.name).unwrap_or(0);
            (raw != 0).then(|| ClipId::from_raw(raw))
        })
    }

    /// Whether `p` is shown at `tick`.
    ///
    /// True when `p` has no `visible_when` condition, when `params` has no
    /// parameter of the name the condition reads, or when that parameter is
    /// of a kind a condition cannot compare. Otherwise true when the
    /// parameter's value equals the condition's value: a scalar within
    /// `1e-4`, a boolean written `true`, `false`, `1`, or `0`.
    fn visible(&self, p: &EffectParamSpec, params: &[EffectParamSpec], tick: f64) -> bool {
        let Some(cond) = &p.visible_when else {
            return true;
        };
        let Some(other) = params.iter().find(|q| q.name == cond.param) else {
            return true;
        };
        match other.kind {
            EffectParamKind::Choice => self.choice(other).is_some_and(|v| v == cond.equals),
            EffectParamKind::Bool => {
                let target = match cond.equals.as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                };
                target.is_some_and(|b| self.boolean(other) == Some(b))
            }
            EffectParamKind::Int => cond
                .equals
                .parse::<i64>()
                .ok()
                .is_some_and(|n| self.int(other, tick) == Some(n)),
            EffectParamKind::Scalar | EffectParamKind::Angle => {
                cond.equals.parse::<f32>().ok().is_some_and(|v| {
                    self.scalar(other, tick)
                        .is_some_and(|cur| (cur - v).abs() <= 1e-4)
                })
            }
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {
                self.text(other).is_some_and(|s| s == cond.equals)
            }
            _ => true,
        }
    }

    /// The shader storage-buffer floats of `params` at `tick`, in
    /// declaration order: one float per scalar, integer, boolean, choice
    /// index, and layer id, four per colour, two per vector, three per
    /// point, and a lookup table per curve and gradient. Text, font, and
    /// file parameters take no floats. `point` replaces the value of the
    /// vector at its slot index.
    fn flatten_floats(
        &self,
        params: &[EffectParamSpec],
        tick: f64,
        point: Option<(usize, [f32; 2])>,
    ) -> Vec<f32> {
        let mut out = Vec::new();
        self.flatten_floats_into(params, tick, point, &mut out);
        out
    }

    /// [`Self::flatten_floats`] written into `out`, which is cleared first.
    fn flatten_floats_into(
        &self,
        params: &[EffectParamSpec],
        tick: f64,
        point: Option<(usize, [f32; 2])>,
        out: &mut Vec<f32>,
    ) {
        let total: usize = params.iter().map(|p| p.kind.shader_floats()).sum();
        out.clear();
        out.reserve(total);
        for (slot, p) in params.iter().enumerate() {
            match p.kind {
                EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                    out.extend(self.scalar(p, tick));
                }
                EffectParamKind::Bool => out.extend(self.boolean(p).map(f32::from)),
                EffectParamKind::Choice => out.push(self.choice_index(p).unwrap_or(0) as f32),
                EffectParamKind::Color => {
                    if let Some(rgba) = self.linear_color(p, tick) {
                        out.extend_from_slice(&rgba);
                    }
                }
                EffectParamKind::Vec2 | EffectParamKind::Size => {
                    let v = match point {
                        Some((at, v)) if at == slot => Some(v),
                        _ => self.vec2(p, tick),
                    };
                    if let Some(v) = v {
                        out.extend_from_slice(&v);
                    }
                }
                EffectParamKind::Point3 => {
                    if let Some(v) = self.vec3(p, tick) {
                        out.extend_from_slice(&v);
                    }
                }
                EffectParamKind::Curve => {
                    let points = self.stored_curve(&p.name).unwrap_or(&p.default_curve);
                    out.extend_from_slice(&evaluate_effect_curve_lut(points));
                }
                EffectParamKind::Gradient => {
                    let stops = self.stored_gradient(&p.name).unwrap_or(&p.default_gradient);
                    for rgba in evaluate_effect_gradient_lut(stops) {
                        out.extend_from_slice(&rgba);
                    }
                }
                EffectParamKind::Layer => {
                    out.push(self.stored_layer(&p.name).unwrap_or(0) as f32);
                }
                EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {}
            }
        }
    }
}

/// No stored values: every reader returns the catalog default.
struct Defaults;

impl StoredParams for Defaults {
    fn stored_scalar(&self, _: &str, _: f64) -> Option<f32> {
        None
    }
    fn stored_color(&self, _: &str, _: f64) -> Option<[u8; 4]> {
        None
    }
    fn stored_linear_color(&self, _: &str, _: f64) -> Option<[f32; 4]> {
        None
    }
    fn stored_vec2(&self, _: &str, _: f64) -> Option<[f32; 2]> {
        None
    }
    fn stored_vec3(&self, _: &str, _: f64) -> Option<[f32; 3]> {
        None
    }
    fn stored_bool(&self, _: &str) -> Option<bool> {
        None
    }
    fn stored_choice(&self, _: &str) -> Option<&str> {
        None
    }
    fn stored_curve(&self, _: &str) -> Option<&[EffectCurvePoint]> {
        None
    }
    fn stored_gradient(&self, _: &str) -> Option<&[EffectGradientStop]> {
        None
    }
    fn stored_text(&self, _: &str) -> Option<&str> {
        None
    }
    fn stored_layer(&self, _: &str) -> Option<u64> {
        None
    }
}

/// The shader storage-buffer floats of `params` with every parameter at its
/// catalog default, laid out as [`StoredParams::flatten_floats`] lays them out.
pub fn default_flattened_param_floats(params: &[EffectParamSpec]) -> Vec<f32> {
    Defaults.flatten_floats(params, 0.0, None)
}

/// Float slot ranges in [`StoredParams::flatten_floats`] for parameters in
/// `params` measured in canvas pixels ([`EffectParamSpec::is_canvas_px`]).
pub fn canvas_px_float_ranges(
    params: &[EffectParamSpec],
) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    params
        .iter()
        .scan(0usize, |offset, param| {
            let start = *offset;
            let width = param.kind.shader_floats();
            *offset = start + width;
            Some((param.is_canvas_px() && width > 0).then_some(start..start + width))
        })
        .flatten()
}
