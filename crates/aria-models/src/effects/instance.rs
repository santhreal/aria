use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::catalog::{EffectSpec, effect_spec};
use super::params::{
    EffectCurvePoint, EffectGradientStop, EffectParamKind, EffectParamSpec, choice_check,
    curve_check, evaluate_effect_curve_lut, evaluate_effect_gradient_lut, gradient_check,
    holds_scalar, holds_text, holds_vec2, int_range_check, linear_color_check,
    scalar_or_int_range_check, stored_slot, vec2_range_check, vec3_range_check,
};
use crate::Map;
use crate::clip::ParamValue;
use crate::error::ModelError;
use crate::param::{Easing, Param};

/// Longest name an [`EffectInstance`] takes, in characters.
pub const EFFECT_NAME_MAX_CHARS: usize = 64;

/// Refuse an instance name that is blank, starts or ends with whitespace, is
/// longer than [`EFFECT_NAME_MAX_CHARS`], or holds a control character.
fn check_effect_name(name: &str) -> Result<(), ModelError> {
    let fault = if name.trim().is_empty() {
        "is blank"
    } else if name.trim() != name {
        "starts or ends with whitespace"
    } else if name.chars().count() > EFFECT_NAME_MAX_CHARS {
        "is longer than the limit"
    } else if name.chars().any(char::is_control) {
        "holds a control character"
    } else {
        return Ok(());
    };
    Err(ModelError::InvalidParam(format!(
        "the effect name {name:?} {fault}; a name is 1 to {EFFECT_NAME_MAX_CHARS} characters of \
         printable text"
    )))
}

/// One parameter of a held spec.
struct ParamRef {
    spec: Arc<EffectSpec>,
    index: usize,
}

impl Deref for ParamRef {
    type Target = EffectParamSpec;

    fn deref(&self) -> &EffectParamSpec {
        &self.spec.params[self.index]
    }
}

/// An effect placed on a clip. Only parameters that differ from their catalog
/// default are stored (others fall back to the default), so a freshly-added
/// effect serializes to just its id and old files that predate a new
/// parameter keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub effect_id: String,
    /// Explicitly-set scalar / int / angle parameters, keyed by name; constant or keyframed.
    /// Keyframe ticks are clip-relative, like the transform params.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub params: Map<String, Param<f32>>,
    /// Explicitly-set 8-bit color parameters (`EffectParamKind::Color`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub color_params: Map<String, Param<[u8; 4]>>,
    /// Explicitly-set linear RGBA (`[f32; 4]`) color parameters (`EffectParamKind::Color`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub linear_color_params: Map<String, Param<[f32; 4]>>,
    /// Explicitly-set 2-d vector / size parameters (`EffectParamKind::Vec2` | `Size`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec2_params: Map<String, Param<[f32; 2]>>,
    /// Explicitly-set 3-d point parameters (`EffectParamKind::Point3`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec3_params: Map<String, Param<[f32; 3]>>,
    /// Explicitly-set boolean parameters (`EffectParamKind::Bool`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub bool_params: Map<String, bool>,
    /// Explicitly-set choice option ids (`EffectParamKind::Choice`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub choice_params: Map<String, String>,
    /// Explicitly-set 1D curve control points (`EffectParamKind::Curve`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub curve_params: Map<String, Vec<EffectCurvePoint>>,
    /// Explicitly-set gradient stops (`EffectParamKind::Gradient`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub gradient_params: Map<String, Vec<EffectGradientStop>>,
    /// Explicitly-set text / font / file strings (`EffectParamKind::Text` | `Font` | `File`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub text_params: Map<String, String>,
    /// Explicitly-set layer reference raw `ClipId`s (`EffectParamKind::Layer`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub layer_params: Map<String, u64>,
    /// Whether [`Self::effect_id`] was a generator when this instance was placed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub generator: bool,
    /// Whether the effect is switched off. A bypassed effect keeps its
    /// parameters and its place in the chain and renders nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
    /// The name given to this instance, drawn in place of the catalog label.
    /// `None` draws the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EffectInstance {
    /// A new instance of `effect_id` with every parameter at its default.
    pub fn new(effect_id: impl Into<String>) -> Self {
        let effect_id = effect_id.into();
        let generator = effect_spec(&effect_id).is_some_and(|s| s.generator);
        Self {
            effect_id,
            params: Map::default(),
            color_params: Map::default(),
            linear_color_params: Map::default(),
            vec2_params: Map::default(),
            vec3_params: Map::default(),
            bool_params: Map::default(),
            choice_params: Map::default(),
            curve_params: Map::default(),
            gradient_params: Map::default(),
            text_params: Map::default(),
            layer_params: Map::default(),
            generator,
            bypass: false,
            name: None,
        }
    }

    /// The name drawn for this instance: its own name, else the catalog
    /// label, else the id when the catalog no longer has the effect.
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        effect_spec(&self.effect_id)
            .map_or_else(|| self.effect_id.clone(), |spec| spec.label.clone())
    }

    /// This instance with every parameter at its catalog default and no
    /// keyframes. The name, the switch and the generator flag are kept.
    pub fn reset(&self) -> Self {
        Self {
            generator: self.generator,
            bypass: self.bypass,
            name: self.name.clone(),
            ..Self::new(self.effect_id.clone())
        }
    }

    /// Refresh [`Self::generator`] from the catalog when the effect is loaded.
    pub fn note_catalog(&mut self) {
        if let Some(spec) = effect_spec(&self.effect_id) {
            self.generator = spec.generator;
        }
    }

    /// Refresh [`Self::generator`] from the catalog.
    pub fn mark_from_catalog(&mut self) {
        self.note_catalog();
    }

    /// Whether this instance was placed as a generator whose catalog entry is now missing.
    pub fn is_missing_generator(&self) -> bool {
        self.generator && effect_spec(&self.effect_id).is_none()
    }

    /// The catalog entry for this instance, or an error if the id is unknown.
    pub fn spec(&self) -> Result<Arc<EffectSpec>, ModelError> {
        effect_spec(&self.effect_id)
            .ok_or_else(|| ModelError::InvalidParam(format!("unknown effect '{}'", self.effect_id)))
    }

    fn param_spec(&self, index: usize) -> Result<ParamRef, ModelError> {
        let spec = self.spec()?;
        if index >= spec.params.len() {
            return Err(ModelError::InvalidParam(format!(
                "effect '{}' has no parameter at index {index}",
                self.effect_id
            )));
        }
        Ok(ParamRef { spec, index })
    }

    fn expect_kind(
        &self,
        pspec: &EffectParamSpec,
        kind: EffectParamKind,
    ) -> Result<(), ModelError> {
        if pspec.kind == kind {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected {:?}",
                pspec.name, pspec.kind, kind
            )))
        }
    }

    /// Check whether `pspec` is visible given the current values on `self` at `tick`.
    pub fn is_param_visible(&self, pspec: &EffectParamSpec, spec: &EffectSpec, tick: f64) -> bool {
        let Some(cond) = &pspec.visible_when else {
            return true;
        };
        let Some(other) = spec.param(&cond.param) else {
            return true;
        };
        match other.kind {
            EffectParamKind::Choice => self
                .sample_choice_param(&cond.param)
                .is_some_and(|v| v == cond.equals),
            EffectParamKind::Bool => {
                let target = match cond.equals.as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                };
                target.is_some_and(|b| self.sample_bool_param(&cond.param) == Some(b))
            }
            EffectParamKind::Int => cond
                .equals
                .parse::<i64>()
                .ok()
                .is_some_and(|n| self.sample_int_param(&cond.param, tick) == Some(n)),
            EffectParamKind::Scalar | EffectParamKind::Angle => {
                cond.equals.parse::<f32>().ok().is_some_and(|v| {
                    self.sample_param(&cond.param, tick)
                        .is_some_and(|cur| (cur - v).abs() <= 1e-4)
                })
            }
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => self
                .sample_text_param(&cond.param)
                .is_some_and(|s| s == cond.equals),
            _ => true,
        }
    }

    /// Sampled scalar (`f32`), angle, or integer value of `param` at clip-relative
    /// fractional `tick`, falling back to the catalog default when never set.
    pub fn sample_param(&self, param: &str, tick: f64) -> Option<f32> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => Some(
                self.params
                    .get(param)
                    .map(|p| p.sample_at(tick))
                    .unwrap_or(pspec.default),
            ),
            EffectParamKind::Int => Some(
                self.params
                    .get(param)
                    .map(|p| p.sample_at(tick).round())
                    .unwrap_or(pspec.default_int as f32),
            ),
            _ => None,
        }
    }

    /// Sampled integer (`i64`) value of `param` at clip-relative fractional `tick`.
    pub fn sample_int_param(&self, param: &str, tick: f64) -> Option<i64> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Int {
            return None;
        }
        Some(
            self.params
                .get(param)
                .map(|p| p.sample_at(tick).round() as i64)
                .unwrap_or(pspec.default_int),
        )
    }

    /// Sampled boolean value of `param`, falling back to the catalog default.
    pub fn sample_bool_param(&self, param: &str) -> Option<bool> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Bool {
            return None;
        }
        Some(
            self.bool_params
                .get(param)
                .copied()
                .unwrap_or(pspec.default_bool),
        )
    }

    /// Sampled choice option id of `param`, falling back to the catalog default.
    pub fn sample_choice_param(&self, param: &str) -> Option<String> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Choice {
            return None;
        }
        pspec
            .choice_or_default(self.choice_params.get(param).map(String::as_str))
            .map(str::to_owned)
    }

    /// Sampled choice option index of `param` in `pspec.choices`.
    pub fn sample_choice_index(&self, param: &str) -> Option<usize> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Choice {
            return None;
        }
        let id = pspec.choice_or_default(self.choice_params.get(param).map(String::as_str))?;
        Some(pspec.choice_index(id).unwrap_or(0))
    }

    /// Sampled 8-bit color value of `param` at clip-relative fractional `tick`.
    pub fn sample_color_param(&self, param: &str, tick: f64) -> Option<[u8; 4]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(p) = self.color_params.get(param) {
            return Some(p.sample_at(tick));
        }
        if let Some(p) = self.linear_color_params.get(param) {
            let rgba = p.sample_at(tick);
            return Some(rgba.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        Some(pspec.default_color_u8())
    }

    /// Sampled linear RGBA (`[f32; 4]`) color value of `param` at clip-relative fractional `tick`.
    pub fn sample_linear_color_param(&self, param: &str, tick: f64) -> Option<[f32; 4]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(p) = self.linear_color_params.get(param) {
            return Some(p.sample_at(tick));
        }
        if let Some(p) = self.color_params.get(param) {
            let rgba = p.sample_at(tick);
            return Some(rgba.map(|c| f32::from(c) / 255.0));
        }
        Some(pspec.default_color_linear())
    }

    /// Sampled vec2 or size value of `param` at clip-relative fractional `tick`.
    pub fn sample_vec2_param(&self, param: &str, tick: f64) -> Option<[f32; 2]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return None;
        }
        Some(match self.vec2_params.get(param) {
            Some(p) => p.sample_at(tick),
            None => pspec.default_vec2,
        })
    }

    /// Sampled 3D point (`[f32; 3]`) value of `param` at clip-relative fractional `tick`.
    pub fn sample_vec3_param(&self, param: &str, tick: f64) -> Option<[f32; 3]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Point3 {
            return None;
        }
        Some(match self.vec3_params.get(param) {
            Some(p) => p.sample_at(tick),
            None => pspec.default_vec3,
        })
    }

    /// Sampled 1D curve control points of `param`.
    pub fn sample_curve_param(&self, param: &str) -> Option<Vec<EffectCurvePoint>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Curve {
            return None;
        }
        Some(
            self.curve_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_curve_points()),
        )
    }

    /// Sampled gradient color stops of `param`.
    pub fn sample_gradient_param(&self, param: &str) -> Option<Vec<EffectGradientStop>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Gradient {
            return None;
        }
        Some(
            self.gradient_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_gradient_stops()),
        )
    }

    /// Sampled text, font, or file string of `param`.
    pub fn sample_text_param(&self, param: &str) -> Option<String> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return None;
        }
        Some(
            self.text_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_text.clone()),
        )
    }

    /// Sampled layer reference (`ClipId`) of `param` (`None` when unset / `0`).
    pub fn sample_layer_param(&self, param: &str) -> Option<Option<crate::ClipId>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Layer {
            return None;
        }
        let raw = self.layer_params.get(param).copied().unwrap_or(0);
        Some((raw != 0).then(|| crate::ClipId::from_raw(raw)))
    }

    /// Sample all shader-visible parameters in declaration order into a flat `Vec<f32>`.
    pub fn sample_flattened_floats(
        &self,
        tick: f64,
        point: Option<(usize, [f32; 2])>,
    ) -> Result<Vec<f32>, ModelError> {
        let spec = self.spec()?;
        let total: usize = spec.params.iter().map(|p| p.kind.shader_floats()).sum();
        let mut out = Vec::with_capacity(total);
        for (slot, pspec) in spec.params.iter().enumerate() {
            match pspec.kind {
                EffectParamKind::Scalar | EffectParamKind::Angle => {
                    let v = self
                        .params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick))
                        .unwrap_or(pspec.default);
                    out.push(v);
                }
                EffectParamKind::Int => {
                    let v = self
                        .params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick).round())
                        .unwrap_or(pspec.default_int as f32);
                    out.push(v);
                }
                EffectParamKind::Bool => {
                    let b = self
                        .bool_params
                        .get(&pspec.name)
                        .copied()
                        .unwrap_or(pspec.default_bool);
                    out.push(if b { 1.0 } else { 0.0 });
                }
                EffectParamKind::Choice => {
                    let idx = pspec
                        .choice_or_default(self.choice_params.get(&pspec.name).map(String::as_str))
                        .and_then(|s| pspec.choice_index(s))
                        .unwrap_or(0);
                    out.push(idx as f32);
                }
                EffectParamKind::Color => {
                    let rgba = if let Some(p) = self.linear_color_params.get(&pspec.name) {
                        p.sample_at(tick)
                    } else if let Some(p) = self.color_params.get(&pspec.name) {
                        p.sample_at(tick).map(|c| f32::from(c) / 255.0)
                    } else {
                        pspec.default_color_linear()
                    };
                    out.extend_from_slice(&rgba);
                }
                EffectParamKind::Vec2 | EffectParamKind::Size => {
                    let v = match point {
                        Some((override_slot, override_val)) if override_slot == slot => {
                            override_val
                        }
                        _ => self
                            .vec2_params
                            .get(&pspec.name)
                            .map(|p| p.sample_at(tick))
                            .unwrap_or(pspec.default_vec2),
                    };
                    out.extend_from_slice(&v);
                }
                EffectParamKind::Point3 => {
                    let v = self
                        .vec3_params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick))
                        .unwrap_or(pspec.default_vec3);
                    out.extend_from_slice(&v);
                }
                EffectParamKind::Curve => {
                    let pts = self
                        .curve_params
                        .get(&pspec.name)
                        .map(Vec::as_slice)
                        .unwrap_or(&pspec.default_curve);
                    let lut = evaluate_effect_curve_lut(pts);
                    out.extend_from_slice(&lut);
                }
                EffectParamKind::Gradient => {
                    let stops = self
                        .gradient_params
                        .get(&pspec.name)
                        .map(Vec::as_slice)
                        .unwrap_or(&pspec.default_gradient);
                    let lut = evaluate_effect_gradient_lut(stops);
                    for rgba in lut {
                        out.extend_from_slice(&rgba);
                    }
                }
                EffectParamKind::Layer => {
                    let raw = self.layer_params.get(&pspec.name).copied().unwrap_or(0);
                    out.push(raw as f32);
                }
                EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {}
            }
        }
        Ok(out)
    }

    /// Insert or replace a keyframe on a scalar, angle, or integer parameter slot.
    pub fn set_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: f32,
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Scalar",
                pspec.name, pspec.kind
            )));
        }
        scalar_or_int_range_check(&pspec, value)?;
        easing.validate()?;
        let stored = if pspec.kind == EffectParamKind::Int {
            value.round()
        } else {
            value
        };
        let default = if pspec.kind == EffectParamKind::Int {
            pspec.default_int as f32
        } else {
            pspec.default
        };
        let name = pspec.name.clone();
        self.params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, stored, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a color parameter slot.
    pub fn set_color_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [u8; 4],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_color_u8());
        self.linear_color_params.remove(&name);
        self.color_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a linear RGBA (`[f32; 4]`) color parameter slot.
    pub fn set_linear_color_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 4],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        linear_color_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_color_linear());
        self.color_params.remove(&name);
        self.linear_color_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a vec2 or size parameter slot.
    pub fn set_vec2_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 2],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Vec2",
                pspec.name, pspec.kind
            )));
        }
        vec2_range_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_vec2);
        self.vec2_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a 3D point (`[f32; 3]`) parameter slot.
    pub fn set_vec3_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 3],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Point3)?;
        vec3_range_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_vec3);
        self.vec3_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Remove the keyframe at exactly `tick` on parameter slot `index`.
    pub fn remove_param_keyframe(&mut self, index: usize, tick: i64) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        let name = pspec.name.as_str();
        let removed = match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => self
                .params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            EffectParamKind::Color => {
                let lin = self
                    .linear_color_params
                    .get_mut(name)
                    .is_some_and(|p| p.remove_keyframe(tick));
                let u8_rem = self
                    .color_params
                    .get_mut(name)
                    .is_some_and(|p| p.remove_keyframe(tick));
                lin || u8_rem
            }
            EffectParamKind::Vec2 | EffectParamKind::Size => self
                .vec2_params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            EffectParamKind::Point3 => self
                .vec3_params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            _ => false,
        };
        if removed {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "no keyframe at tick {tick} on {}.{name}",
                self.effect_id
            )))
        }
    }

    /// Replace a scalar, angle, or integer parameter slot with a constant, dropping keyframes.
    pub fn set_param_constant(&mut self, index: usize, value: f32) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Scalar",
                pspec.name, pspec.kind
            )));
        }
        scalar_or_int_range_check(&pspec, value)?;
        let stored = if pspec.kind == EffectParamKind::Int {
            value.round()
        } else {
            value
        };
        self.params
            .insert(pspec.name.to_string(), Param::Constant(stored));
        Ok(())
    }

    /// Replace an integer parameter slot with a constant `i64`, dropping keyframes.
    pub fn set_int_param_constant(&mut self, index: usize, value: i64) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Int)?;
        int_range_check(&pspec, value)?;
        self.params
            .insert(pspec.name.to_string(), Param::Constant(value as f32));
        Ok(())
    }

    /// Set a boolean parameter slot.
    pub fn set_bool_param(&mut self, index: usize, value: bool) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Bool)?;
        self.bool_params.insert(pspec.name.to_string(), value);
        Ok(())
    }

    /// Set a choice parameter slot to `choice_id`.
    pub fn set_choice_param(
        &mut self,
        index: usize,
        choice_id: impl Into<String>,
    ) -> Result<(), ModelError> {
        let choice_id = choice_id.into();
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Choice)?;
        choice_check(&pspec, &choice_id)?;
        self.choice_params.insert(pspec.name.to_string(), choice_id);
        Ok(())
    }

    /// Replace a color parameter slot with a constant, dropping keyframes.
    pub fn set_color_param_constant(
        &mut self,
        index: usize,
        value: [u8; 4],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        self.linear_color_params.remove(&pspec.name);
        self.color_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a color parameter slot with a constant linear RGBA (`[f32; 4]`), dropping keyframes.
    pub fn set_linear_color_param_constant(
        &mut self,
        index: usize,
        value: [f32; 4],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        linear_color_check(&pspec, value)?;
        self.color_params.remove(&pspec.name);
        self.linear_color_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a vec2 or size parameter slot with a constant, dropping keyframes.
    pub fn set_vec2_param_constant(
        &mut self,
        index: usize,
        value: [f32; 2],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Vec2",
                pspec.name, pspec.kind
            )));
        }
        vec2_range_check(&pspec, value)?;
        self.vec2_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a 3D point parameter slot with a constant `[f32; 3]`, dropping keyframes.
    pub fn set_vec3_param_constant(
        &mut self,
        index: usize,
        value: [f32; 3],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Point3)?;
        vec3_range_check(&pspec, value)?;
        self.vec3_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Set a 1D curve parameter slot's control points.
    pub fn set_curve_param(
        &mut self,
        index: usize,
        points: Vec<EffectCurvePoint>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Curve)?;
        curve_check(&pspec, &points)?;
        self.curve_params.insert(pspec.name.to_string(), points);
        Ok(())
    }

    /// Set a gradient parameter slot's color stops.
    pub fn set_gradient_param(
        &mut self,
        index: usize,
        stops: Vec<EffectGradientStop>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Gradient)?;
        gradient_check(&pspec, &stops)?;
        self.gradient_params.insert(pspec.name.to_string(), stops);
        Ok(())
    }

    /// Set a text, font, or file parameter slot's string value.
    pub fn set_text_param(
        &mut self,
        index: usize,
        value: impl Into<String>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Text/Font/File",
                pspec.name, pspec.kind
            )));
        }
        self.text_params
            .insert(pspec.name.to_string(), value.into());
        Ok(())
    }

    /// Set a layer reference parameter slot (`None` clears the reference).
    pub fn set_layer_param(
        &mut self,
        index: usize,
        layer: Option<crate::ClipId>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Layer)?;
        match layer {
            Some(id) => {
                self.layer_params.insert(pspec.name.to_string(), id.raw());
            }
            None => {
                self.layer_params.remove(&pspec.name);
            }
        }
        Ok(())
    }

    /// Set parameter slot `index` from a [`ParamValue`], dispatching on the
    /// catalog kind.
    pub fn set_param_value_constant(
        &mut self,
        index: usize,
        value: ParamValue,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                self.set_param_constant(index, expect_scalar(value)?)
            }
            EffectParamKind::Color => self.set_color_param_constant(index, expect_color(value)?),
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                self.set_vec2_param_constant(index, expect_vec2(value)?)
            }
            EffectParamKind::Point3 => self.set_vec3_param_constant(index, expect_vec3(value)?),
            _ => Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' ({:?}) is not settable via ParamValue",
                pspec.name, pspec.kind
            ))),
        }
    }

    /// Insert or replace a keyframe from a [`ParamValue`], dispatching on kind.
    pub fn set_param_value_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: ParamValue,
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                self.set_param_keyframe(index, tick, expect_scalar(value)?, easing)
            }
            EffectParamKind::Color => {
                self.set_color_param_keyframe(index, tick, expect_color(value)?, easing)
            }
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                self.set_vec2_param_keyframe(index, tick, expect_vec2(value)?, easing)
            }
            EffectParamKind::Point3 => {
                self.set_vec3_param_keyframe(index, tick, expect_vec3(value)?, easing)
            }
            _ => Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' ({:?}) does not support keyframes",
                pspec.name, pspec.kind
            ))),
        }
    }

    /// `Ok` iff the id is known, every set parameter names a real slot of the
    /// matching kind, every curve is structurally sound, and every value lies
    /// in range.
    pub fn validate(&self) -> Result<(), ModelError> {
        let spec = self.spec()?;
        if let Some(name) = &self.name {
            check_effect_name(name)?;
        }
        let slot = |name: &str, store: &str, holds: fn(EffectParamKind) -> bool| {
            stored_slot(&spec.params, "effect", &self.effect_id, name, store, holds)
        };
        for (name, param) in &self.params {
            let pspec = slot(name, "params", holds_scalar)?;
            param.validate_shape()?;
            param.for_each_value(|v| scalar_or_int_range_check(pspec, *v))?;
        }
        for (name, param) in &self.color_params {
            slot(name, "color_params", |k| k == EffectParamKind::Color)?;
            param.validate_shape()?;
        }
        for (name, param) in &self.linear_color_params {
            let pspec = slot(name, "linear_color_params", |k| k == EffectParamKind::Color)?;
            param.validate_shape()?;
            param.for_each_value(|v| linear_color_check(pspec, *v))?;
        }
        for (name, param) in &self.vec2_params {
            let pspec = slot(name, "vec2_params", holds_vec2)?;
            param.validate_shape()?;
            param.for_each_value(|v| vec2_range_check(pspec, *v))?;
        }
        for (name, param) in &self.vec3_params {
            let pspec = slot(name, "vec3_params", |k| k == EffectParamKind::Point3)?;
            param.validate_shape()?;
            param.for_each_value(|v| vec3_range_check(pspec, *v))?;
        }
        for name in self.bool_params.keys() {
            slot(name, "bool_params", |k| k == EffectParamKind::Bool)?;
        }
        for (name, choice_id) in &self.choice_params {
            choice_check(
                slot(name, "choice_params", |k| k == EffectParamKind::Choice)?,
                choice_id,
            )?;
        }
        for (name, points) in &self.curve_params {
            curve_check(
                slot(name, "curve_params", |k| k == EffectParamKind::Curve)?,
                points,
            )?;
        }
        for (name, stops) in &self.gradient_params {
            gradient_check(
                slot(name, "gradient_params", |k| k == EffectParamKind::Gradient)?,
                stops,
            )?;
        }
        for name in self.text_params.keys() {
            slot(name, "text_params", holds_text)?;
        }
        for name in self.layer_params.keys() {
            slot(name, "layer_params", |k| k == EffectParamKind::Layer)?;
        }
        Ok(())
    }
}

fn expect_scalar(value: ParamValue) -> Result<f32, ModelError> {
    match value {
        ParamValue::Scalar(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a scalar value".into())),
    }
}

fn expect_vec2(value: ParamValue) -> Result<[f32; 2], ModelError> {
    match value {
        ParamValue::Vec2(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a vec2 value".into())),
    }
}

fn expect_vec3(value: ParamValue) -> Result<[f32; 3], ModelError> {
    match value {
        ParamValue::Vec3(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a vec3 value".into())),
    }
}

fn expect_color(value: ParamValue) -> Result<[u8; 4], ModelError> {
    match value {
        ParamValue::Color(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a color value".into())),
    }
}
