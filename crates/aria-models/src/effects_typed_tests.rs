//! Typed parameters, checked kind by kind against every map, sampler, and
//! setter.
//!
//! WHY: [`EffectInstance`] and [`crate::Transition`] keep each
//! [`EffectParamKind`] in its own map and match on the kind by hand in every
//! sampler, every setter, the shader buffer, and `validate`. A kind matched to
//! the wrong map, a sampler that answers for a kind it does not read, a setter
//! that skips its range check, or a buffer slot that drifts from its sampler
//! is a one-token slip that a test of one effect does not reach. These tests
//! run every kind against every map, sampler, and setter, for both holders,
//! through [`Holder`] and [`holder_contract`].
//!
//! A new kind does not compile here until [`position`], [`Store::holds`],
//! [`Sampler::answers`], [`Value::accepts`], and [`buffer_sampler`] list it.
//! A new map on either holder does not compile in its `filled` until
//! [`Store`] lists it.
//!
//! Not caught: how the compositor reads the shader buffer, and the inspector
//! rows built from these values.

use serde::de::DeserializeOwned;

use super::EffectParamKind as K;
use super::*;
use crate::ClipId;

/// Slots of [`every_kind`].
pub(crate) const SCALAR: usize = 0;
pub(crate) const INT: usize = 1;
pub(crate) const TOGGLE: usize = 2;
pub(crate) const MODE: usize = 3;
pub(crate) const TINT: usize = 4;
pub(crate) const GLOW: usize = 5;
pub(crate) const OFFSET: usize = 6;
pub(crate) const PIVOT: usize = 7;
pub(crate) const EXTENT: usize = 8;
pub(crate) const SPIN: usize = 9;
pub(crate) const RESPONSE: usize = 10;
pub(crate) const RAMP: usize = 11;
pub(crate) const CAPTION: usize = 12;
pub(crate) const FACE: usize = 13;
pub(crate) const LUT: usize = 14;
pub(crate) const MATTE: usize = 15;

/// Every kind, in declaration order.
pub(crate) const KINDS: [EffectParamKind; 15] = [
    K::Scalar,
    K::Int,
    K::Bool,
    K::Choice,
    K::Color,
    K::Vec2,
    K::Point3,
    K::Size,
    K::Angle,
    K::Curve,
    K::Gradient,
    K::Text,
    K::Font,
    K::File,
    K::Layer,
];

/// Where `kind` sits in [`KINDS`]. No wildcard arm: a new kind does not
/// compile until it is listed.
const fn position(kind: EffectParamKind) -> usize {
    match kind {
        K::Scalar => 0,
        K::Int => 1,
        K::Bool => 2,
        K::Choice => 3,
        K::Color => 4,
        K::Vec2 => 5,
        K::Point3 => 6,
        K::Size => 7,
        K::Angle => 8,
        K::Curve => 9,
        K::Gradient => 10,
        K::Text => 11,
        K::Font => 12,
        K::File => 13,
        K::Layer => 14,
    }
}

pub(crate) fn pt(x: f32, y: f32) -> EffectCurvePoint {
    EffectCurvePoint::new(x, y)
}

pub(crate) fn stop(offset: f32, color: [f32; 4]) -> EffectGradientStop {
    EffectGradientStop::new(offset, color)
}

/// One parameter of every kind, and two of color: `tint` defaults from bytes
/// and `glow` from linear floats.
pub(crate) fn every_kind() -> Vec<EffectParamSpec> {
    vec![
        EffectParamSpec::scalar("scalar", "Scalar", 0.25, 0.0, 1.0)
            .with_soft_range(0.1, 0.9)
            .with_unit("%")
            .with_display(EffectParamDisplay::Percentage)
            .with_tooltip("tip")
            .with_group("main"),
        EffectParamSpec::int("int", "Int", 3, -5, 10),
        EffectParamSpec::boolean("toggle", "Toggle", true),
        EffectParamSpec::choice(
            "mode",
            "Mode",
            vec![
                EffectChoiceOption::new("fast", "Fast"),
                EffectChoiceOption::new("best", "Best"),
            ],
            "best",
        ),
        EffectParamSpec::color("tint", "Tint", [255, 0, 0, 255]),
        EffectParamSpec::linear_color("glow", "Glow", [0.0, 0.5, 1.0, 1.0]),
        EffectParamSpec::vec2("offset", "Offset", [0.25, -0.25], -1.0, 1.0),
        EffectParamSpec::point3("pivot", "Pivot", [0.0, 0.5, 1.0], -2.0, 2.0),
        EffectParamSpec::size("extent", "Extent", [640.0, 360.0], 1.0, 8192.0),
        EffectParamSpec::angle("spin", "Spin", 45.0, -360.0, 360.0)
            .with_visible_when("toggle", "true"),
        EffectParamSpec::curve("response", "Response", Vec::new()),
        EffectParamSpec::gradient("ramp", "Ramp", Vec::new()),
        EffectParamSpec::text("caption", "Caption", "hello"),
        EffectParamSpec::font("face", "Face", "Inter"),
        EffectParamSpec::file("lut", "LUT", "grade.cube"),
        EffectParamSpec::layer("matte", "Matte"),
    ]
}

/// [`every_kind`], then rows shown only while a parameter of it holds a value.
fn visibility_params() -> Vec<EffectParamSpec> {
    let when = |name: &str, on: &str, equals: &str| {
        EffectParamSpec::scalar(name, name, 0.0, 0.0, 1.0).with_visible_when(on, equals)
    };
    let mut params = every_kind();
    params.extend([
        when("toggle_false", "toggle", "false"),
        when("toggle_zero", "toggle", "0"),
        when("toggle_word", "toggle", "maybe"),
        when("mode_fast", "mode", "fast"),
        when("int_seven", "int", "7"),
        when("int_word", "int", "seven"),
        when("scalar_half", "scalar", "0.5"),
        when("spin_back", "spin", "-90"),
        when("caption_bye", "caption", "bye"),
        when("offset_any", "offset", "anything"),
        when("ghost_any", "ghost", "anything"),
    ]);
    params
}

/// Index of slot `index`'s first float in the shader buffer.
pub(crate) fn offset(params: &[EffectParamSpec], index: usize) -> usize {
    params[..index].iter().map(|p| p.kind.shader_floats()).sum()
}

pub(crate) fn refused(message: impl Into<String>) -> Result<(), ModelError> {
    Err(ModelError::InvalidParam(message.into()))
}

/// One map of explicit values on a holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Store {
    Params,
    Color,
    LinearColor,
    Vec2,
    Vec3,
    Bool,
    Choice,
    Curve,
    Gradient,
    Text,
    Layer,
}

impl Store {
    pub(crate) const ALL: [Self; 11] = [
        Self::Params,
        Self::Color,
        Self::LinearColor,
        Self::Vec2,
        Self::Vec3,
        Self::Bool,
        Self::Choice,
        Self::Curve,
        Self::Gradient,
        Self::Text,
        Self::Layer,
    ];

    /// The serialized field, as `validate` prints it.
    const fn field(self) -> &'static str {
        match self {
            Self::Params => "params",
            Self::Color => "color_params",
            Self::LinearColor => "linear_color_params",
            Self::Vec2 => "vec2_params",
            Self::Vec3 => "vec3_params",
            Self::Bool => "bool_params",
            Self::Choice => "choice_params",
            Self::Curve => "curve_params",
            Self::Gradient => "gradient_params",
            Self::Text => "text_params",
            Self::Layer => "layer_params",
        }
    }

    /// Whether this map holds values of `kind`.
    const fn holds(self, kind: EffectParamKind) -> bool {
        match kind {
            K::Scalar | K::Int | K::Angle => matches!(self, Self::Params),
            K::Bool => matches!(self, Self::Bool),
            K::Choice => matches!(self, Self::Choice),
            K::Color => matches!(self, Self::Color | Self::LinearColor),
            K::Vec2 | K::Size => matches!(self, Self::Vec2),
            K::Point3 => matches!(self, Self::Vec3),
            K::Curve => matches!(self, Self::Curve),
            K::Gradient => matches!(self, Self::Gradient),
            K::Text | K::Font | K::File => matches!(self, Self::Text),
            K::Layer => matches!(self, Self::Layer),
        }
    }

    /// A value of this map, in range for every slot of [`every_kind`] it holds.
    fn probe(self) -> Value {
        match self {
            Self::Params => Value::Scalar(1.0),
            Self::Color => Value::Color([1, 2, 3, 4]),
            Self::LinearColor => Value::Linear([0.1, 0.2, 0.3, 1.0]),
            Self::Vec2 => Value::Vec2([1.0, 1.0]),
            Self::Vec3 => Value::Vec3([0.5; 3]),
            Self::Bool => Value::Bool(false),
            Self::Choice => Value::Choice("fast"),
            Self::Curve => Value::Curve(Vec::new()),
            Self::Gradient => Value::Gradient(Vec::new()),
            Self::Text => Value::Text("x"),
            Self::Layer => Value::Layer(Some(ClipId::from_raw(7))),
        }
    }
}

/// A value written to one slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Scalar(f32),
    Int(i64),
    Bool(bool),
    Choice(&'static str),
    Color([u8; 4]),
    Linear([f32; 4]),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Curve(Vec<EffectCurvePoint>),
    Gradient(Vec<EffectGradientStop>),
    Text(&'static str),
    Layer(Option<ClipId>),
}

impl Value {
    /// The kind a refusal of this value names as expected.
    fn expects(&self) -> &'static str {
        match self {
            Self::Scalar(_) => "Scalar",
            Self::Int(_) => "Int",
            Self::Bool(_) => "Bool",
            Self::Choice(_) => "Choice",
            Self::Color(_) | Self::Linear(_) => "Color",
            Self::Vec2(_) => "Vec2",
            Self::Vec3(_) => "Point3",
            Self::Curve(_) => "Curve",
            Self::Gradient(_) => "Gradient",
            Self::Text(_) => "Text/Font/File",
            Self::Layer(_) => "Layer",
        }
    }

    /// Whether a slot of `kind` takes this value.
    fn accepts(&self, kind: EffectParamKind) -> bool {
        match self {
            Self::Scalar(_) => matches!(kind, K::Scalar | K::Int | K::Angle),
            Self::Int(_) => matches!(kind, K::Int),
            Self::Bool(_) => matches!(kind, K::Bool),
            Self::Choice(_) => matches!(kind, K::Choice),
            Self::Color(_) | Self::Linear(_) => matches!(kind, K::Color),
            Self::Vec2(_) => matches!(kind, K::Vec2 | K::Size),
            Self::Vec3(_) => matches!(kind, K::Point3),
            Self::Curve(_) => matches!(kind, K::Curve),
            Self::Gradient(_) => matches!(kind, K::Gradient),
            Self::Text(_) => matches!(kind, K::Text | K::Font | K::File),
            Self::Layer(_) => matches!(kind, K::Layer),
        }
    }
}

/// One typed read of a holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sampler {
    Scalar,
    Int,
    Bool,
    Choice,
    Index,
    Color,
    Linear,
    Vec2,
    Vec3,
    Curve,
    Gradient,
    Text,
    Layer,
}

impl Sampler {
    const ALL: [Self; 13] = [
        Self::Scalar,
        Self::Int,
        Self::Bool,
        Self::Choice,
        Self::Index,
        Self::Color,
        Self::Linear,
        Self::Vec2,
        Self::Vec3,
        Self::Curve,
        Self::Gradient,
        Self::Text,
        Self::Layer,
    ];

    /// Whether this read answers for a slot of `kind`.
    const fn answers(self, kind: EffectParamKind) -> bool {
        match self {
            Self::Scalar => matches!(kind, K::Scalar | K::Int | K::Angle),
            Self::Int => matches!(kind, K::Int),
            Self::Bool => matches!(kind, K::Bool),
            Self::Choice | Self::Index => matches!(kind, K::Choice),
            Self::Color | Self::Linear => matches!(kind, K::Color),
            Self::Vec2 => matches!(kind, K::Vec2 | K::Size),
            Self::Vec3 => matches!(kind, K::Point3),
            Self::Curve => matches!(kind, K::Curve),
            Self::Gradient => matches!(kind, K::Gradient),
            Self::Text => matches!(kind, K::Text | K::Font | K::File),
            Self::Layer => matches!(kind, K::Layer),
        }
    }
}

/// What a [`Sampler`] read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Read {
    Scalar(f32),
    Int(i64),
    Bool(bool),
    Choice(String),
    Index(usize),
    Color([u8; 4]),
    Linear([f32; 4]),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Curve(Vec<EffectCurvePoint>),
    Gradient(Vec<EffectGradientStop>),
    Text(String),
    Layer(Option<ClipId>),
}

impl Read {
    /// This read laid out as the shader buffer lays out its slot.
    fn floats(&self) -> Vec<f32> {
        match self {
            Self::Scalar(v) => vec![*v],
            Self::Int(v) => vec![*v as f32],
            Self::Bool(b) => vec![if *b { 1.0 } else { 0.0 }],
            Self::Index(i) => vec![*i as f32],
            Self::Linear(c) => c.to_vec(),
            Self::Vec2(v) => v.to_vec(),
            Self::Vec3(v) => v.to_vec(),
            Self::Curve(points) => evaluate_effect_curve_lut(points).to_vec(),
            Self::Gradient(stops) => evaluate_effect_gradient_lut(stops).concat(),
            Self::Text(_) => Vec::new(),
            Self::Layer(clip) => vec![clip.map_or(0.0, |c| c.raw() as f32)],
            Self::Choice(_) | Self::Color(_) => panic!("{self:?} fills no buffer slot"),
        }
    }
}

/// The read that fills a slot of `kind` in the shader buffer.
const fn buffer_sampler(kind: EffectParamKind) -> Sampler {
    match kind {
        K::Scalar | K::Int | K::Angle => Sampler::Scalar,
        K::Bool => Sampler::Bool,
        K::Choice => Sampler::Index,
        K::Color => Sampler::Linear,
        K::Vec2 | K::Size => Sampler::Vec2,
        K::Point3 => Sampler::Vec3,
        K::Curve => Sampler::Curve,
        K::Gradient => Sampler::Gradient,
        K::Text | K::Font | K::File => Sampler::Text,
        K::Layer => Sampler::Layer,
    }
}

/// The shader buffer assembled from the samplers, slot by slot.
fn buffer_from_samplers<H: Holder>(holder: &H, params: &[EffectParamSpec], tick: f64) -> Vec<f32> {
    params
        .iter()
        .flat_map(|p| {
            holder
                .read(buffer_sampler(p.kind), &p.name, tick)
                .unwrap_or_else(|| panic!("{} reads as {:?}", p.name, p.kind))
                .floats()
        })
        .collect()
}

/// A holder of typed parameter values: an effect instance or a transition.
pub(crate) trait Holder:
    Sized + PartialEq + std::fmt::Debug + Serialize + DeserializeOwned
{
    /// The holder as messages print it.
    const OWNER: &'static str;
    /// Register `params` under `id` until the guard drops.
    fn register(id: &str, params: Vec<EffectParamSpec>) -> crate::Registered;
    /// A holder of `id` with every parameter at its default.
    fn fresh(id: &str) -> Self;
    /// Store `value` under `name` in the map its variant belongs to, unchecked.
    fn poke(&mut self, name: &str, value: Value);
    /// The maps that hold at least one value, in [`Store::ALL`] order.
    fn filled(&self) -> Vec<Store>;
    fn check(&self) -> Result<(), ModelError>;
    fn read(&self, sampler: Sampler, name: &str, tick: f64) -> Option<Read>;
    fn flattened(&self, tick: f64) -> Result<Vec<f32>, ModelError>;
    /// Write `value` to slot `index` through the setter for its variant.
    fn set(&mut self, index: usize, value: Value) -> Result<(), ModelError>;
    fn visible(&self, index: usize) -> bool;
}

impl Holder for EffectInstance {
    const OWNER: &'static str = "effect";

    fn register(id: &str, params: Vec<EffectParamSpec>) -> crate::Registered {
        let spec = EffectSpec {
            id: id.into(),
            label: id.into(),
            category: "Test".into(),
            params,
            ..EffectSpec::default()
        };
        crate::register_effect(spec)
            .expect("the test effect registers")
            .0
    }

    fn fresh(id: &str) -> Self {
        Self::new(id)
    }

    fn poke(&mut self, name: &str, value: Value) {
        let name = name.to_owned();
        match value {
            Value::Scalar(v) => {
                self.params.insert(name, Param::Constant(v));
            }
            Value::Int(v) => {
                self.params.insert(name, Param::Constant(v as f32));
            }
            Value::Bool(v) => {
                self.bool_params.insert(name, v);
            }
            Value::Choice(v) => {
                self.choice_params.insert(name, v.to_owned());
            }
            Value::Color(v) => {
                self.color_params.insert(name, Param::Constant(v));
            }
            Value::Linear(v) => {
                self.linear_color_params.insert(name, Param::Constant(v));
            }
            Value::Vec2(v) => {
                self.vec2_params.insert(name, Param::Constant(v));
            }
            Value::Vec3(v) => {
                self.vec3_params.insert(name, Param::Constant(v));
            }
            Value::Curve(v) => {
                self.curve_params.insert(name, v);
            }
            Value::Gradient(v) => {
                self.gradient_params.insert(name, v);
            }
            Value::Text(v) => {
                self.text_params.insert(name, v.to_owned());
            }
            Value::Layer(v) => {
                self.layer_params.insert(name, v.map_or(0, |c| c.raw()));
            }
        }
    }

    fn filled(&self) -> Vec<Store> {
        let Self {
            effect_id: _,
            params,
            color_params,
            linear_color_params,
            vec2_params,
            vec3_params,
            bool_params,
            choice_params,
            curve_params,
            gradient_params,
            text_params,
            layer_params,
            generator: _,
            bypass: _,
            name: _,
        } = self;
        [
            (Store::Params, params.is_empty()),
            (Store::Color, color_params.is_empty()),
            (Store::LinearColor, linear_color_params.is_empty()),
            (Store::Vec2, vec2_params.is_empty()),
            (Store::Vec3, vec3_params.is_empty()),
            (Store::Bool, bool_params.is_empty()),
            (Store::Choice, choice_params.is_empty()),
            (Store::Curve, curve_params.is_empty()),
            (Store::Gradient, gradient_params.is_empty()),
            (Store::Text, text_params.is_empty()),
            (Store::Layer, layer_params.is_empty()),
        ]
        .into_iter()
        .filter(|&(_, empty)| !empty)
        .map(|(store, _)| store)
        .collect()
    }

    fn check(&self) -> Result<(), ModelError> {
        self.validate()
    }

    fn read(&self, sampler: Sampler, name: &str, tick: f64) -> Option<Read> {
        Some(match sampler {
            Sampler::Scalar => Read::Scalar(self.sample_param(name, tick)?),
            Sampler::Int => Read::Int(self.sample_int_param(name, tick)?),
            Sampler::Bool => Read::Bool(self.sample_bool_param(name)?),
            Sampler::Choice => Read::Choice(self.sample_choice_param(name)?),
            Sampler::Index => Read::Index(self.sample_choice_index(name)?),
            Sampler::Color => Read::Color(self.sample_color_param(name, tick)?),
            Sampler::Linear => Read::Linear(self.sample_linear_color_param(name, tick)?),
            Sampler::Vec2 => Read::Vec2(self.sample_vec2_param(name, tick)?),
            Sampler::Vec3 => Read::Vec3(self.sample_vec3_param(name, tick)?),
            Sampler::Curve => Read::Curve(self.sample_curve_param(name)?),
            Sampler::Gradient => Read::Gradient(self.sample_gradient_param(name)?),
            Sampler::Text => Read::Text(self.sample_text_param(name)?),
            Sampler::Layer => Read::Layer(self.sample_layer_param(name)?),
        })
    }

    fn flattened(&self, tick: f64) -> Result<Vec<f32>, ModelError> {
        self.sample_flattened_floats(tick, None)
    }

    fn set(&mut self, index: usize, value: Value) -> Result<(), ModelError> {
        match value {
            Value::Scalar(v) => self.set_param_constant(index, v),
            Value::Int(v) => self.set_int_param_constant(index, v),
            Value::Bool(v) => self.set_bool_param(index, v),
            Value::Choice(v) => self.set_choice_param(index, v),
            Value::Color(v) => self.set_color_param_constant(index, v),
            Value::Linear(v) => self.set_linear_color_param_constant(index, v),
            Value::Vec2(v) => self.set_vec2_param_constant(index, v),
            Value::Vec3(v) => self.set_vec3_param_constant(index, v),
            Value::Curve(v) => self.set_curve_param(index, v),
            Value::Gradient(v) => self.set_gradient_param(index, v),
            Value::Text(v) => self.set_text_param(index, v),
            Value::Layer(v) => self.set_layer_param(index, v),
        }
    }

    fn visible(&self, index: usize) -> bool {
        let spec = self.spec().expect("registered");
        self.is_param_visible(&spec.params[index], &spec, 0.0)
    }
}

mod effect_contract {
    use super::*;

    holder_contract!(EffectInstance, "test.typed.fx");
}

/// A [`ParamValue`] of the shape a slot of `kind` takes; kinds without one
/// get a scalar.
pub(crate) fn param_value(kind: EffectParamKind) -> ParamValue {
    match kind {
        K::Scalar | K::Int | K::Angle => ParamValue::Scalar(1.0),
        K::Color => ParamValue::Color([1, 2, 3, 4]),
        K::Vec2 | K::Size => ParamValue::Vec2([1.0, 1.0]),
        K::Point3 => ParamValue::Vec3([0.5; 3]),
        _ => ParamValue::Scalar(0.0),
    }
}

/// The refusal of a [`ParamValue`] of the wrong shape for a slot of `kind`.
pub(crate) fn wrong_shape(kind: EffectParamKind) -> &'static str {
    match kind {
        K::Scalar | K::Int | K::Angle => "expected a scalar value",
        K::Color => "expected a color value",
        K::Vec2 | K::Size => "expected a vec2 value",
        K::Point3 => "expected a vec3 value",
        _ => panic!("{kind:?} takes no ParamValue"),
    }
}

#[path = "effects_typed_tests_contract.rs"]
mod contract;
#[path = "effects_typed_tests_keyframes.rs"]
mod keyframes;
#[path = "effects_typed_tests_specs.rs"]
mod specs;

pub(crate) use contract::{
    color_writes_replace_each_other, defaults_read_back_and_fill_the_buffer, holder_contract,
    out_of_range_writes_fail_and_change_nothing, samplers_answer_for_their_kinds_only,
    setters_take_exactly_their_kinds, stale_choice_reads_as_the_default,
    stores_hold_exactly_their_kinds, visibility_follows_the_controlling_value,
    writes_read_back_and_round_trip,
};
