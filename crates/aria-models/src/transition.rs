//! Transitions (v1 roadmap M4): a timed blend at the junction between two
//! abutting clips on a track.
//!
//! A [`Transition`] is **data**, like effects: the model stores
//! `{left, right, transition_id, duration}` and the compositor owns the WGSL
//! that blends the two frames by progress. The [`transition_catalog`] is the
//! validation + UI source of truth (ids, display names); it is drift-checked
//! against the compositor's renderable set from `aria-engine`.
//!
//! A transition lives only while its pair of clips still abuts (left's end
//! tick equals right's start tick). Structural edits that break the abutment
//! prune the dead junction; undo restores it.

use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use crate::Map;
use crate::clip::ParamValue;
use crate::effects::{
    EffectCurvePoint, EffectGradientStop, EffectParamKind, EffectParamSpec, StoredParams,
    check_params, choice_check, curve_check, gradient_check, holds_scalar, holds_text, holds_vec2,
    int_range_check, linear_color_check, scalar_or_int_range_check, stored_slot,
    vector_range_check,
};
use crate::error::ModelError;
use crate::ids::ClipId;

/// A renderable transition: its stable id, a human label, and its ordered parameters.
///
/// Owned strings, not `&'static str`: the catalog is a file now, so the text is
/// read at startup rather than compiled in.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TransitionSpec {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<EffectParamSpec>,
}

impl TransitionSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            params: Vec::new(),
        }
    }

    pub fn with_param(mut self, param: EffectParamSpec) -> Self {
        self.params.push(param);
        self
    }

    pub fn with_params(mut self, params: Vec<EffectParamSpec>) -> Self {
        self.params = params;
        self
    }

    /// Look up a parameter specification by name.
    pub fn param(&self, name: &str) -> Option<&EffectParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Look up a parameter specification by positional slot index.
    pub fn param_at(&self, index: usize) -> Option<&EffectParamSpec> {
        self.params.get(index)
    }
}

/// The shipped transitions, read once. Shipped set at `data/transitions.toml`,
/// replaced by a file of that name under the host's catalog root.
static SHIPPED: LazyLock<Vec<Arc<TransitionSpec>>> = LazyLock::new(|| {
    crate::catalog::load(
        "transitions",
        include_str!("../data/transitions.toml"),
        |entries: &[TransitionSpec]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
            entries
                .iter()
                .try_for_each(|s| check_params(&s.id, &s.params))
        },
    )
    .into_iter()
    .map(Arc::new)
    .collect()
});

/// Default transition window length, in timeline ticks (centered on the cut).
pub const DEFAULT_TRANSITION_TICKS: i64 = 24;

/// The transitions that ship with the app, without registered ones.
pub fn shipped_transition_catalog() -> &'static [Arc<TransitionSpec>] {
    &SHIPPED
}

/// Every transition the model knows about now (validation + UI browsing): the
/// shipped set, then transitions registered by plugins.
pub fn transition_catalog() -> crate::Catalog<TransitionSpec> {
    crate::registered::transition_catalog()
}

/// The catalog entry for `id`, shipped or registered, or `None`.
pub fn transition_spec(id: &str) -> Option<Arc<TransitionSpec>> {
    crate::registered::transition_spec(id)
}

/// How a transition window sits relative to the cut between its two clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAlignment {
    /// Centered on the cut (`[cut - duration / 2, cut - duration / 2 + duration)`).
    #[default]
    Center,
    /// Starts at the cut (`[cut, cut + duration)`).
    Start,
    /// Ends at the cut (`[cut - duration, cut)`).
    End,
}

impl TransitionAlignment {
    pub const ALL: [Self; 3] = [Self::Center, Self::Start, Self::End];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Start => "start",
            Self::End => "end",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Center => "Center at Cut",
            Self::Start => "Start at Cut",
            Self::End => "End at Cut",
        }
    }

    pub const fn is_center(&self) -> bool {
        matches!(self, Self::Center)
    }
}

/// Timeline tick window `[start, end)` for a transition of `duration` ticks at `cut`.
pub fn transition_window(alignment: TransitionAlignment, duration: i64, cut: i64) -> (i64, i64) {
    match alignment {
        TransitionAlignment::Center => {
            let start = cut - duration / 2;
            (start, start + duration)
        }
        TransitionAlignment::Start => (cut, cut + duration),
        TransitionAlignment::End => (cut - duration, cut),
    }
}

/// A transition at the junction between `left` and the clip immediately to its
/// right on the same track. Centered on the cut; `duration` is the total
/// window length in timeline ticks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// The clip on the outgoing (left) side of the cut.
    pub left: ClipId,
    /// The clip on the incoming (right) side of the cut.
    pub right: ClipId,
    /// Catalog id of the blend to render.
    pub transition_id: String,
    /// Total window length in timeline ticks, centered on the junction.
    pub duration: i64,
    /// How the transition window sits relative to the junction.
    #[serde(default, skip_serializing_if = "TransitionAlignment::is_center")]
    pub alignment: TransitionAlignment,
    /// Explicitly-set scalar / int / angle parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub params: Map<String, f32>,
    /// Explicitly-set 8-bit color parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub color_params: Map<String, [u8; 4]>,
    /// Explicitly-set linear RGBA color parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub linear_color_params: Map<String, [f32; 4]>,
    /// Explicitly-set 2D vector / size parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec2_params: Map<String, [f32; 2]>,
    /// Explicitly-set 3D point parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec3_params: Map<String, [f32; 3]>,
    /// Explicitly-set boolean parameters, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub bool_params: Map<String, bool>,
    /// Explicitly-set choice option ids, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub choice_params: Map<String, String>,
    /// Explicitly-set 1D curve control points, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub curve_params: Map<String, Vec<EffectCurvePoint>>,
    /// Explicitly-set gradient stops, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub gradient_params: Map<String, Vec<EffectGradientStop>>,
    /// Explicitly-set text / font / file strings, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub text_params: Map<String, String>,
    /// Explicitly-set layer reference raw `ClipId`s, keyed by name.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub layer_params: Map<String, u64>,
}

impl Transition {
    pub fn new(
        left: ClipId,
        right: ClipId,
        transition_id: impl Into<String>,
        duration: i64,
    ) -> Self {
        Self {
            left,
            right,
            transition_id: transition_id.into(),
            duration: duration.max(1),
            alignment: TransitionAlignment::default(),
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
        }
    }

    /// Timeline tick window `[start, end)` when the cut sits at `cut`.
    pub fn window(&self, cut: i64) -> (i64, i64) {
        transition_window(self.alignment, self.duration, cut)
    }

    /// Look up the [`TransitionSpec`] for this transition.
    pub fn spec(&self) -> Result<Arc<TransitionSpec>, ModelError> {
        transition_spec(&self.transition_id).ok_or_else(|| {
            ModelError::InvalidParam(format!("unknown transition '{}'", self.transition_id))
        })
    }

    fn param_spec(&self, index: usize) -> Result<EffectParamSpec, ModelError> {
        let spec = self.spec()?;
        spec.params.get(index).cloned().ok_or_else(|| {
            ModelError::InvalidParam(format!(
                "transition '{}' has no parameter at index {index}",
                self.transition_id
            ))
        })
    }

    /// Check whether `pspec` is visible given the current values on `self`.
    pub fn is_param_visible(&self, pspec: &EffectParamSpec, spec: &TransitionSpec) -> bool {
        self.visible(pspec, &spec.params, 0.0)
    }

    /// Read the parameter named `param` with `read`, or `None` when the
    /// transition or the parameter is unknown.
    fn read_named<T>(
        &self,
        param: &str,
        read: impl FnOnce(&EffectParamSpec) -> Option<T>,
    ) -> Option<T> {
        read(self.spec().ok()?.param(param)?)
    }

    /// Sample a scalar (`f32`), angle, or integer parameter on this transition.
    pub fn sample_param(&self, param: &str) -> Option<f32> {
        self.read_named(param, |p| self.scalar(p, 0.0))
    }

    /// Sample an integer (`i64`) parameter on this transition.
    pub fn sample_int_param(&self, param: &str) -> Option<i64> {
        self.read_named(param, |p| self.int(p, 0.0))
    }

    /// Sample a boolean parameter on this transition.
    pub fn sample_bool_param(&self, param: &str) -> Option<bool> {
        self.read_named(param, |p| self.boolean(p))
    }

    /// Sample a choice parameter's selected option id on this transition.
    pub fn sample_choice_param(&self, param: &str) -> Option<String> {
        self.read_named(param, |p| self.choice(p).map(str::to_owned))
    }

    /// Sample a choice parameter's selected option index on this transition.
    pub fn sample_choice_index(&self, param: &str) -> Option<usize> {
        self.read_named(param, |p| self.choice_index(p))
    }

    /// Sample an 8-bit RGBA color parameter on this transition.
    pub fn sample_color_param(&self, param: &str) -> Option<[u8; 4]> {
        self.read_named(param, |p| self.color(p, 0.0))
    }

    /// Sample a linear RGBA (`[f32; 4]`) color parameter on this transition.
    pub fn sample_linear_color_param(&self, param: &str) -> Option<[f32; 4]> {
        self.read_named(param, |p| self.linear_color(p, 0.0))
    }

    /// Sample a 2D vector (`[f32; 2]`) or size parameter on this transition.
    pub fn sample_vec2_param(&self, param: &str) -> Option<[f32; 2]> {
        self.read_named(param, |p| self.vec2(p, 0.0))
    }

    /// Sample a 3D point (`[f32; 3]`) parameter on this transition.
    pub fn sample_vec3_param(&self, param: &str) -> Option<[f32; 3]> {
        self.read_named(param, |p| self.vec3(p, 0.0))
    }

    /// Sample a 1D curve parameter's control points on this transition.
    pub fn sample_curve_param(&self, param: &str) -> Option<Vec<EffectCurvePoint>> {
        self.read_named(param, |p| self.curve(p))
    }

    /// Sample a gradient parameter's stops on this transition.
    pub fn sample_gradient_param(&self, param: &str) -> Option<Vec<EffectGradientStop>> {
        self.read_named(param, |p| self.gradient(p))
    }

    /// Sample a text, font, or file parameter on this transition.
    pub fn sample_text_param(&self, param: &str) -> Option<String> {
        self.read_named(param, |p| self.text(p))
    }

    /// Sample a layer reference (`ClipId`) parameter on this transition.
    pub fn sample_layer_param(&self, param: &str) -> Option<Option<ClipId>> {
        self.read_named(param, |p| self.layer(p))
    }

    /// Sample all shader-visible transition parameters in declaration order into a flat `Vec<f32>`.
    pub fn sample_flattened_floats(&self) -> Result<Vec<f32>, ModelError> {
        Ok(self.flatten_floats(&self.spec()?.params, 0.0, None))
    }

    /// Set a scalar, angle, or integer parameter slot on this transition.
    pub fn set_param(&mut self, index: usize, value: f32) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !holds_scalar(pspec.kind) {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Scalar",
                pspec.name, pspec.kind
            )));
        }
        scalar_or_int_range_check(&pspec, value)?;
        let stored = if pspec.kind == EffectParamKind::Int {
            value.round()
        } else {
            value
        };
        self.params.insert(pspec.name, stored);
        Ok(())
    }

    /// Alias of [`Self::set_param`] matching [`crate::EffectInstance::set_param_constant`].
    pub fn set_param_constant(&mut self, index: usize, value: f32) -> Result<(), ModelError> {
        self.set_param(index, value)
    }

    /// Set an integer parameter slot on this transition.
    pub fn set_int_param(&mut self, index: usize, value: i64) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Int {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Int",
                pspec.name, pspec.kind
            )));
        }
        int_range_check(&pspec, value)?;
        self.params.insert(pspec.name, value as f32);
        Ok(())
    }

    /// Set a boolean parameter slot on this transition.
    pub fn set_bool_param(&mut self, index: usize, value: bool) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Bool {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Bool",
                pspec.name, pspec.kind
            )));
        }
        self.bool_params.insert(pspec.name, value);
        Ok(())
    }

    /// Set a choice parameter slot on this transition.
    pub fn set_choice_param(
        &mut self,
        index: usize,
        choice_id: impl Into<String>,
    ) -> Result<(), ModelError> {
        let choice_id = choice_id.into();
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Choice {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Choice",
                pspec.name, pspec.kind
            )));
        }
        choice_check(&pspec, &choice_id)?;
        self.choice_params.insert(pspec.name, choice_id);
        Ok(())
    }

    /// Set an 8-bit color parameter slot on this transition.
    pub fn set_color_param(&mut self, index: usize, value: [u8; 4]) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Color {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Color",
                pspec.name, pspec.kind
            )));
        }
        self.linear_color_params.remove(&pspec.name);
        self.color_params.insert(pspec.name, value);
        Ok(())
    }

    /// Set a linear RGBA (`[f32; 4]`) color parameter slot on this transition.
    pub fn set_linear_color_param(
        &mut self,
        index: usize,
        value: [f32; 4],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Color {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Color",
                pspec.name, pspec.kind
            )));
        }
        linear_color_check(&pspec, value)?;
        self.color_params.remove(&pspec.name);
        self.linear_color_params.insert(pspec.name, value);
        Ok(())
    }

    /// Set a 2D vector or size parameter slot on this transition.
    pub fn set_vec2_param(&mut self, index: usize, value: [f32; 2]) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !holds_vec2(pspec.kind) {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Vec2",
                pspec.name, pspec.kind
            )));
        }
        vector_range_check(&pspec, &value)?;
        self.vec2_params.insert(pspec.name, value);
        Ok(())
    }

    /// Set a 3D point parameter slot on this transition.
    pub fn set_vec3_param(&mut self, index: usize, value: [f32; 3]) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Point3 {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Point3",
                pspec.name, pspec.kind
            )));
        }
        vector_range_check(&pspec, &value)?;
        self.vec3_params.insert(pspec.name, value);
        Ok(())
    }

    /// Set a 1D curve parameter slot on this transition.
    pub fn set_curve_param(
        &mut self,
        index: usize,
        points: Vec<EffectCurvePoint>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Curve {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Curve",
                pspec.name, pspec.kind
            )));
        }
        curve_check(&pspec, &points)?;
        self.curve_params.insert(pspec.name, points);
        Ok(())
    }

    /// Set a gradient parameter slot on this transition.
    pub fn set_gradient_param(
        &mut self,
        index: usize,
        stops: Vec<EffectGradientStop>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Gradient {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Gradient",
                pspec.name, pspec.kind
            )));
        }
        gradient_check(&pspec, &stops)?;
        self.gradient_params.insert(pspec.name, stops);
        Ok(())
    }

    /// Set a text, font, or file parameter slot on this transition.
    pub fn set_text_param(
        &mut self,
        index: usize,
        value: impl Into<String>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !holds_text(pspec.kind) {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Text/Font/File",
                pspec.name, pspec.kind
            )));
        }
        self.text_params.insert(pspec.name, value.into());
        Ok(())
    }

    /// Set a layer reference parameter slot on this transition.
    pub fn set_layer_param(
        &mut self,
        index: usize,
        layer: Option<ClipId>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if pspec.kind != EffectParamKind::Layer {
            return Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' is {:?}, expected Layer",
                pspec.name, pspec.kind
            )));
        }
        match layer {
            Some(id) => {
                self.layer_params.insert(pspec.name, id.raw());
            }
            None => {
                self.layer_params.remove(&pspec.name);
            }
        }
        Ok(())
    }

    /// Set transition parameter slot `index` from a [`ParamValue`].
    pub fn set_param_value(&mut self, index: usize, value: ParamValue) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                let ParamValue::Scalar(v) = value else {
                    return Err(ModelError::InvalidParam("expected a scalar value".into()));
                };
                self.set_param(index, v)
            }
            EffectParamKind::Color => {
                let ParamValue::Color(c) = value else {
                    return Err(ModelError::InvalidParam("expected a color value".into()));
                };
                self.set_color_param(index, c)
            }
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                let ParamValue::Vec2(v) = value else {
                    return Err(ModelError::InvalidParam("expected a vec2 value".into()));
                };
                self.set_vec2_param(index, v)
            }
            EffectParamKind::Point3 => {
                let ParamValue::Vec3(v) = value else {
                    return Err(ModelError::InvalidParam("expected a vec3 value".into()));
                };
                self.set_vec3_param(index, v)
            }
            _ => Err(ModelError::InvalidParam(format!(
                "transition parameter '{}' ({:?}) is not settable via ParamValue",
                pspec.name, pspec.kind
            ))),
        }
    }

    /// `Ok` iff the transition id is known and every set parameter names a
    /// real slot of the matching kind with values in range.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.duration <= 0 {
            return Err(ModelError::InvalidParam(format!(
                "transition duration {} must be positive",
                self.duration
            )));
        }
        let spec = self.spec()?;
        let slot = |name: &str, store: &str, holds: fn(EffectParamKind) -> bool| {
            stored_slot(
                &spec.params,
                "transition",
                &self.transition_id,
                name,
                store,
                holds,
            )
        };
        for (name, &value) in &self.params {
            scalar_or_int_range_check(slot(name, "params", holds_scalar)?, value)?;
        }
        for name in self.color_params.keys() {
            slot(name, "color_params", |k| k == EffectParamKind::Color)?;
        }
        for (name, &rgba) in &self.linear_color_params {
            linear_color_check(
                slot(name, "linear_color_params", |k| k == EffectParamKind::Color)?,
                rgba,
            )?;
        }
        for (name, &v) in &self.vec2_params {
            vector_range_check(slot(name, "vec2_params", holds_vec2)?, &v)?;
        }
        for (name, &v) in &self.vec3_params {
            vector_range_check(
                slot(name, "vec3_params", |k| k == EffectParamKind::Point3)?,
                &v,
            )?;
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

impl StoredParams for Transition {
    fn stored_scalar(&self, name: &str, _tick: f64) -> Option<f32> {
        self.params.get(name).copied()
    }
    fn stored_color(&self, name: &str, _tick: f64) -> Option<[u8; 4]> {
        self.color_params.get(name).copied()
    }
    fn stored_linear_color(&self, name: &str, _tick: f64) -> Option<[f32; 4]> {
        self.linear_color_params.get(name).copied()
    }
    fn stored_vec2(&self, name: &str, _tick: f64) -> Option<[f32; 2]> {
        self.vec2_params.get(name).copied()
    }
    fn stored_vec3(&self, name: &str, _tick: f64) -> Option<[f32; 3]> {
        self.vec3_params.get(name).copied()
    }
    fn stored_bool(&self, name: &str) -> Option<bool> {
        self.bool_params.get(name).copied()
    }
    fn stored_choice(&self, name: &str) -> Option<&str> {
        self.choice_params.get(name).map(String::as_str)
    }
    fn stored_curve(&self, name: &str) -> Option<&[EffectCurvePoint]> {
        self.curve_params.get(name).map(Vec::as_slice)
    }
    fn stored_gradient(&self, name: &str) -> Option<&[EffectGradientStop]> {
        self.gradient_params.get(name).map(Vec::as_slice)
    }
    fn stored_text(&self, name: &str) -> Option<&str> {
        self.text_params.get(name).map(String::as_str)
    }
    fn stored_layer(&self, name: &str) -> Option<u64> {
        self.layer_params.get(name).copied()
    }
}

#[cfg(test)]
#[path = "transition_param_tests.rs"]
mod param_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped file is what loads when nothing overrides it, and its ids are
    /// unique — a duplicate would shadow a transition the compositor can render.
    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<&str> = SHIPPED.iter().map(|s| s.id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "transition ids are unique");
        assert_eq!(count, 8, "the M4 starter set is eight transitions");
        assert!(SHIPPED.iter().all(|spec| !spec.label.is_empty()));
    }

    #[test]
    fn spec_lookup_resolves_known_and_rejects_unknown() {
        assert_eq!(transition_spec("crossfade").unwrap().label, "Cross Fade");
        assert!(transition_spec("nope").is_none());
    }

    #[test]
    fn new_clamps_duration_to_at_least_one() {
        let t = Transition::new(ClipId::from_raw(1), ClipId::from_raw(2), "crossfade", 0);
        assert_eq!(t.duration, 1);
    }
}
