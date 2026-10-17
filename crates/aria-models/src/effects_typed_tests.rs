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
use crate::param::Keyframe;

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

/// `validate` takes a value only from the map its kind is stored in, and
/// prints the map when it refuses one.
pub(crate) fn stores_hold_exactly_their_kinds<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    for p in every_kind() {
        for store in Store::ALL {
            let mut holder = H::fresh(id);
            holder.poke(&p.name, store.probe());
            assert_eq!(holder.filled(), [store], "{} lands in {store:?}", p.name);
            let json = serde_json::to_value(&holder).expect("serializes");
            assert!(
                json.get(store.field()).is_some(),
                "{store:?} serializes as {}",
                store.field()
            );
            let want = if store.holds(p.kind) {
                Ok(())
            } else {
                refused(format!(
                    "{} parameter '{}' is {:?}, not stored in {}",
                    H::OWNER,
                    p.name,
                    p.kind,
                    store.field()
                ))
            };
            assert_eq!(
                holder.check(),
                want,
                "{} ({:?}) in {store:?}",
                p.name,
                p.kind
            );
        }
    }
    for store in Store::ALL {
        let mut holder = H::fresh(id);
        holder.poke("ghost", store.probe());
        assert_eq!(
            holder.check(),
            refused(format!("{} '{id}' has no parameter 'ghost'", H::OWNER)),
            "{store:?}"
        );
    }
}

/// Each sampler answers for the kinds it reads and for nothing else, and a
/// holder whose id is gone answers nothing.
pub(crate) fn samplers_answer_for_their_kinds_only<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let holder = H::fresh(id);
    for p in every_kind() {
        for sampler in Sampler::ALL {
            assert_eq!(
                holder.read(sampler, &p.name, 0.0).is_some(),
                sampler.answers(p.kind),
                "{sampler:?} on {} ({:?})",
                p.name,
                p.kind
            );
        }
    }
    let lost_id = format!("{id}.unregistered");
    let mut lost = H::fresh(&lost_id);
    for sampler in Sampler::ALL {
        assert_eq!(holder.read(sampler, "ghost", 0.0), None, "{sampler:?}");
        assert_eq!(lost.read(sampler, "scalar", 0.0), None, "{sampler:?}");
    }
    let unknown = format!("unknown {} '{lost_id}'", H::OWNER);
    assert_eq!(
        lost.flattened(0.0),
        Err(ModelError::InvalidParam(unknown.clone()))
    );
    assert_eq!(lost.check(), refused(unknown.clone()));
    assert_eq!(lost.set(SCALAR, Value::Scalar(0.5)), refused(unknown));
}

/// What the samplers read from a holder of [`every_kind`] with no writes.
fn default_reads() -> Vec<(usize, Sampler, Read)> {
    vec![
        (SCALAR, Sampler::Scalar, Read::Scalar(0.25)),
        (INT, Sampler::Scalar, Read::Scalar(3.0)),
        (INT, Sampler::Int, Read::Int(3)),
        (TOGGLE, Sampler::Bool, Read::Bool(true)),
        (MODE, Sampler::Choice, Read::Choice("best".into())),
        (MODE, Sampler::Index, Read::Index(1)),
        (TINT, Sampler::Color, Read::Color([255, 0, 0, 255])),
        (TINT, Sampler::Linear, Read::Linear([1.0, 0.0, 0.0, 1.0])),
        (GLOW, Sampler::Color, Read::Color([0, 128, 255, 255])),
        (GLOW, Sampler::Linear, Read::Linear([0.0, 0.5, 1.0, 1.0])),
        (OFFSET, Sampler::Vec2, Read::Vec2([0.25, -0.25])),
        (PIVOT, Sampler::Vec3, Read::Vec3([0.0, 0.5, 1.0])),
        (EXTENT, Sampler::Vec2, Read::Vec2([640.0, 360.0])),
        (SPIN, Sampler::Scalar, Read::Scalar(45.0)),
        (
            RESPONSE,
            Sampler::Curve,
            Read::Curve(vec![pt(0.0, 0.0), pt(1.0, 1.0)]),
        ),
        (
            RAMP,
            Sampler::Gradient,
            Read::Gradient(vec![stop(0.0, [0.0, 0.0, 0.0, 1.0]), stop(1.0, [1.0; 4])]),
        ),
        (CAPTION, Sampler::Text, Read::Text("hello".into())),
        (FACE, Sampler::Text, Read::Text("Inter".into())),
        (LUT, Sampler::Text, Read::Text("grade.cube".into())),
        (MATTE, Sampler::Layer, Read::Layer(None)),
    ]
}

/// A holder with no writes reads every catalog default, and its shader
/// buffer is the catalog's default buffer, slot for slot.
pub(crate) fn defaults_read_back_and_fill_the_buffer<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let params = every_kind();
    let holder = H::fresh(id);
    for (index, sampler, want) in default_reads() {
        assert_eq!(
            holder.read(sampler, &params[index].name, 0.0),
            Some(want),
            "{} by {sampler:?}",
            params[index].name
        );
    }
    let buffer = holder.flattened(0.0).expect("registered");
    assert_eq!(buffer, default_flattened_param_floats(&params));
    assert_eq!(buffer, buffer_from_samplers(&holder, &params, 0.0));
    let floats: usize = params.iter().map(|p| p.kind.shader_floats()).sum();
    assert_eq!(buffer.len(), floats);
    assert_eq!(holder.check(), Ok(()));
}

/// Every setter writes exactly the kinds its value fits, refuses the rest by
/// kind without writing, and leaves a holder that validates and whose buffer
/// is what the samplers read.
pub(crate) fn setters_take_exactly_their_kinds<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let params = every_kind();
    let probes: Vec<Value> = Store::ALL
        .into_iter()
        .map(Store::probe)
        .chain([Value::Int(2)])
        .collect();
    for (index, p) in params.iter().enumerate() {
        for value in &probes {
            let mut holder = H::fresh(id);
            let got = holder.set(index, value.clone());
            if value.accepts(p.kind) {
                assert_eq!(got, Ok(()), "{value:?} on {}", p.name);
                assert_eq!(holder.check(), Ok(()), "{value:?} on {}", p.name);
                assert_eq!(
                    holder.flattened(0.0).ok(),
                    Some(buffer_from_samplers(&holder, &params, 0.0)),
                    "{value:?} on {}",
                    p.name
                );
            } else {
                let message = format!(
                    "{} parameter '{}' is {:?}, expected {}",
                    H::OWNER,
                    p.name,
                    p.kind,
                    value.expects()
                );
                assert_eq!(got, refused(message), "{value:?} on {}", p.name);
                assert_eq!(holder, H::fresh(id), "a refused {value:?} writes nothing");
            }
        }
    }
    let past = params.len();
    for value in probes {
        let mut holder = H::fresh(id);
        assert_eq!(
            holder.set(past, value),
            refused(format!(
                "{} '{id}' has no parameter at index {past}",
                H::OWNER
            ))
        );
    }
}

/// A value for every slot of [`every_kind`], each away from its default.
fn written() -> Vec<(usize, Value)> {
    vec![
        (SCALAR, Value::Scalar(0.75)),
        (INT, Value::Int(7)),
        (TOGGLE, Value::Bool(false)),
        (MODE, Value::Choice("fast")),
        (TINT, Value::Color([10, 20, 30, 40])),
        (GLOW, Value::Linear([2.0, 0.25, 0.0, 0.5])),
        (OFFSET, Value::Vec2([-0.5, 0.5])),
        (PIVOT, Value::Vec3([1.0, -1.0, 2.0])),
        (EXTENT, Value::Vec2([1920.0, 1080.0])),
        (SPIN, Value::Scalar(-90.0)),
        (RESPONSE, Value::Curve(vec![pt(0.0, 1.0), pt(1.0, 0.0)])),
        (
            RAMP,
            Value::Gradient(vec![
                stop(0.0, [1.0, 0.0, 0.0, 1.0]),
                stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            ]),
        ),
        (CAPTION, Value::Text("bye")),
        (FACE, Value::Text("Mono")),
        (LUT, Value::Text("look.cube")),
        (MATTE, Value::Layer(Some(ClipId::from_raw(42)))),
    ]
}

/// What the samplers read after [`written`].
fn written_reads() -> Vec<(usize, Sampler, Read)> {
    vec![
        (SCALAR, Sampler::Scalar, Read::Scalar(0.75)),
        (INT, Sampler::Scalar, Read::Scalar(7.0)),
        (INT, Sampler::Int, Read::Int(7)),
        (TOGGLE, Sampler::Bool, Read::Bool(false)),
        (MODE, Sampler::Choice, Read::Choice("fast".into())),
        (MODE, Sampler::Index, Read::Index(0)),
        (TINT, Sampler::Color, Read::Color([10, 20, 30, 40])),
        (
            TINT,
            Sampler::Linear,
            Read::Linear([10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0]),
        ),
        (GLOW, Sampler::Linear, Read::Linear([2.0, 0.25, 0.0, 0.5])),
        (GLOW, Sampler::Color, Read::Color([255, 64, 0, 128])),
        (OFFSET, Sampler::Vec2, Read::Vec2([-0.5, 0.5])),
        (PIVOT, Sampler::Vec3, Read::Vec3([1.0, -1.0, 2.0])),
        (EXTENT, Sampler::Vec2, Read::Vec2([1920.0, 1080.0])),
        (SPIN, Sampler::Scalar, Read::Scalar(-90.0)),
        (
            RESPONSE,
            Sampler::Curve,
            Read::Curve(vec![pt(0.0, 1.0), pt(1.0, 0.0)]),
        ),
        (
            RAMP,
            Sampler::Gradient,
            Read::Gradient(vec![
                stop(0.0, [1.0, 0.0, 0.0, 1.0]),
                stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            ]),
        ),
        (CAPTION, Sampler::Text, Read::Text("bye".into())),
        (FACE, Sampler::Text, Read::Text("Mono".into())),
        (LUT, Sampler::Text, Read::Text("look.cube".into())),
        (
            MATTE,
            Sampler::Layer,
            Read::Layer(Some(ClipId::from_raw(42))),
        ),
    ]
}

/// A write to every slot reads back through the samplers, fills every map,
/// fills the shader buffer from the samplers, and survives a save and load.
pub(crate) fn writes_read_back_and_round_trip<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let params = every_kind();
    let mut holder = H::fresh(id);
    for (index, value) in written() {
        let name = &params[index].name;
        holder
            .set(index, value.clone())
            .unwrap_or_else(|e| panic!("{value:?} on {name}: {e}"));
    }
    for (index, sampler, want) in written_reads() {
        assert_eq!(
            holder.read(sampler, &params[index].name, 0.0),
            Some(want),
            "{} by {sampler:?}",
            params[index].name
        );
    }
    assert_eq!(holder.filled(), Store::ALL, "the writes fill every map");
    assert_eq!(holder.check(), Ok(()));
    assert_eq!(
        holder.flattened(0.0).ok(),
        Some(buffer_from_samplers(&holder, &params, 0.0))
    );
    let json = serde_json::to_string(&holder).expect("serializes");
    assert_eq!(serde_json::from_str::<H>(&json).expect("loads"), holder);
    holder.set(MATTE, Value::Layer(None)).expect("clears");
    assert_eq!(
        holder.read(Sampler::Layer, "matte", 0.0),
        Some(Read::Layer(None))
    );
    assert!(
        !holder.filled().contains(&Store::Layer),
        "a cleared layer leaves no entry"
    );
}

/// A byte write and a linear write to one color slot replace each other, and
/// each reads back through the other sampler.
pub(crate) fn color_writes_replace_each_other<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let mut holder = H::fresh(id);
    holder
        .set(TINT, Value::Linear([0.5, 0.5, 0.5, 1.0]))
        .expect("linear");
    assert_eq!(holder.filled(), [Store::LinearColor]);
    assert_eq!(
        holder.read(Sampler::Color, "tint", 0.0),
        Some(Read::Color([128, 128, 128, 255]))
    );
    holder
        .set(TINT, Value::Color([0, 51, 255, 255]))
        .expect("bytes");
    assert_eq!(holder.filled(), [Store::Color]);
    assert_eq!(
        holder.read(Sampler::Linear, "tint", 0.0),
        Some(Read::Linear([0.0, 0.2, 1.0, 1.0]))
    );
    holder
        .set(TINT, Value::Linear([0.5, 0.5, 0.5, 1.0]))
        .expect("linear");
    assert_eq!(holder.filled(), [Store::LinearColor]);
}

/// Writes past a range or with a non-finite part, refused with the message
/// each slot prints.
fn out_of_range() -> Vec<(usize, Value, &'static str)> {
    vec![
        (
            SCALAR,
            Value::Scalar(1.5),
            "scalar = 1.5 out of range [0, 1]",
        ),
        (
            SCALAR,
            Value::Scalar(-0.5),
            "scalar = -0.5 out of range [0, 1]",
        ),
        (
            SCALAR,
            Value::Scalar(f32::NAN),
            "scalar = NaN out of range [0, 1]",
        ),
        (INT, Value::Int(11), "int = 11 out of range [-5, 10]"),
        (INT, Value::Int(-6), "int = -6 out of range [-5, 10]"),
        (INT, Value::Scalar(10.4), "int = 10.4 out of range [-5, 10]"),
        (
            INT,
            Value::Scalar(f32::INFINITY),
            "int = inf out of range [-5, 10]",
        ),
        (
            SPIN,
            Value::Scalar(400.0),
            "spin = 400 out of range [-360, 360]",
        ),
        (
            MODE,
            Value::Choice("slow"),
            "mode = 'slow' is not a valid choice",
        ),
        (
            GLOW,
            Value::Linear([0.0, f32::NAN, 0.0, 1.0]),
            "glow has non-finite linear color component",
        ),
        (
            OFFSET,
            Value::Vec2([0.0, 1.5]),
            "offset[1] = 1.5 out of range [-1, 1]",
        ),
        (
            EXTENT,
            Value::Vec2([0.5, 10.0]),
            "extent[0] = 0.5 out of range [1, 8192]",
        ),
        (
            PIVOT,
            Value::Vec3([0.0, 0.0, -3.0]),
            "pivot[2] = -3 out of range [-2, 2]",
        ),
        (
            PIVOT,
            Value::Vec3([f32::NAN, 0.0, 0.0]),
            "pivot[0] = NaN out of range [-2, 2]",
        ),
        (
            RESPONSE,
            Value::Curve(vec![pt(0.0, 0.0), pt(1.0, f32::NAN)]),
            "response has non-finite curve point",
        ),
        (
            RESPONSE,
            Value::Curve(vec![pt(f32::NEG_INFINITY, 0.0)]),
            "response has non-finite curve point",
        ),
        (
            RAMP,
            Value::Gradient(vec![stop(f32::INFINITY, [0.0; 4])]),
            "ramp has non-finite gradient stop",
        ),
        (
            RAMP,
            Value::Gradient(vec![stop(0.5, [0.0, f32::NAN, 0.0, 1.0])]),
            "ramp has non-finite gradient stop",
        ),
    ]
}

/// Writes on the edge of a range, all accepted.
fn at_the_bounds() -> Vec<(usize, Value)> {
    vec![
        (SCALAR, Value::Scalar(0.0)),
        (SCALAR, Value::Scalar(1.0)),
        (INT, Value::Int(-5)),
        (INT, Value::Int(10)),
        (INT, Value::Scalar(-5.0)),
        (SPIN, Value::Scalar(-360.0)),
        (SPIN, Value::Scalar(360.0)),
        (OFFSET, Value::Vec2([-1.0, 1.0])),
        (EXTENT, Value::Vec2([1.0, 8192.0])),
        (PIVOT, Value::Vec3([-2.0, 2.0, 0.0])),
        (GLOW, Value::Linear([-4.0, 0.0, 16.0, 1.0])),
    ]
}

/// A setter refuses a value out of range without writing it, `validate`
/// refuses the same value stored without a setter with the same message, and
/// values on the edge of a range pass both.
pub(crate) fn out_of_range_writes_fail_and_change_nothing<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let params = every_kind();
    for (index, value, message) in out_of_range() {
        let mut holder = H::fresh(id);
        assert_eq!(
            holder.set(index, value.clone()),
            refused(message),
            "set {value:?}"
        );
        assert_eq!(holder, H::fresh(id), "a refused {value:?} writes nothing");
        holder.poke(&params[index].name, value.clone());
        assert_eq!(holder.check(), refused(message), "stored {value:?}");
    }
    for (index, value) in at_the_bounds() {
        let mut holder = H::fresh(id);
        assert_eq!(holder.set(index, value.clone()), Ok(()), "set {value:?}");
        assert_eq!(holder.check(), Ok(()), "stored {value:?}");
    }
}

/// A stored option the catalog no longer lists reads as the default option
/// in the sampled id, the sampled index, and the shader buffer alike, and
/// `validate` reports it.
pub(crate) fn stale_choice_reads_as_the_default<H: Holder>(id: &str) {
    let _reg = H::register(id, every_kind());
    let params = every_kind();
    let mut holder = H::fresh(id);
    holder.poke("mode", Value::Choice("retired"));
    assert_eq!(
        holder.read(Sampler::Choice, "mode", 0.0),
        Some(Read::Choice("best".into()))
    );
    assert_eq!(
        holder.read(Sampler::Index, "mode", 0.0),
        Some(Read::Index(1))
    );
    let buffer = holder.flattened(0.0).expect("registered");
    assert_eq!(buffer[offset(&params, MODE)], 1.0);
    assert_eq!(buffer, default_flattened_param_floats(&params));
    assert_eq!(
        holder.check(),
        refused("mode = 'retired' is not a valid choice")
    );
}

/// The names of the rows of `params` that `holder` hides.
fn hidden<'p, H: Holder>(holder: &H, params: &'p [EffectParamSpec]) -> Vec<&'p str> {
    params
        .iter()
        .enumerate()
        .filter(|&(index, _)| !holder.visible(index))
        .map(|(_, p)| p.name.as_str())
        .collect()
}

/// A row with a condition shows while its controlling parameter holds the
/// value, compared as that parameter's kind. A condition on a kind with no
/// comparison, or on a parameter the spec lacks, never hides the row.
pub(crate) fn visibility_follows_the_controlling_value<H: Holder>(id: &str) {
    let _reg = H::register(id, visibility_params());
    let params = visibility_params();
    let mut holder = H::fresh(id);
    assert_eq!(
        hidden(&holder, &params),
        [
            "toggle_false",
            "toggle_zero",
            "toggle_word",
            "mode_fast",
            "int_seven",
            "int_word",
            "scalar_half",
            "spin_back",
            "caption_bye",
        ]
    );
    for (index, value) in [
        (TOGGLE, Value::Bool(false)),
        (MODE, Value::Choice("fast")),
        (INT, Value::Int(7)),
        (SCALAR, Value::Scalar(0.5)),
        (SPIN, Value::Scalar(-90.0)),
        (CAPTION, Value::Text("bye")),
    ] {
        holder.set(index, value).expect("in range");
    }
    assert_eq!(
        hidden(&holder, &params),
        ["spin", "toggle_word", "int_word"]
    );
}

/// One `#[test]` per holder contract above, for `$holder`, registering ids
/// that start with `$prefix`.
macro_rules! holder_contract {
    ($holder:ty, $prefix:literal) => {
        #[test]
        fn stores_hold_exactly_their_kinds() {
            $crate::effects::typed_tests::stores_hold_exactly_their_kinds::<$holder>(concat!(
                $prefix, ".stores"
            ));
        }

        #[test]
        fn samplers_answer_for_their_kinds_only() {
            $crate::effects::typed_tests::samplers_answer_for_their_kinds_only::<$holder>(concat!(
                $prefix,
                ".samplers"
            ));
        }

        #[test]
        fn defaults_read_back_and_fill_the_buffer() {
            $crate::effects::typed_tests::defaults_read_back_and_fill_the_buffer::<$holder>(
                concat!($prefix, ".defaults"),
            );
        }

        #[test]
        fn setters_take_exactly_their_kinds() {
            $crate::effects::typed_tests::setters_take_exactly_their_kinds::<$holder>(concat!(
                $prefix, ".setters"
            ));
        }

        #[test]
        fn writes_read_back_and_round_trip() {
            $crate::effects::typed_tests::writes_read_back_and_round_trip::<$holder>(concat!(
                $prefix, ".writes"
            ));
        }

        #[test]
        fn color_writes_replace_each_other() {
            $crate::effects::typed_tests::color_writes_replace_each_other::<$holder>(concat!(
                $prefix, ".colors"
            ));
        }

        #[test]
        fn out_of_range_writes_fail_and_change_nothing() {
            $crate::effects::typed_tests::out_of_range_writes_fail_and_change_nothing::<$holder>(
                concat!($prefix, ".ranges"),
            );
        }

        #[test]
        fn stale_choice_reads_as_the_default() {
            $crate::effects::typed_tests::stale_choice_reads_as_the_default::<$holder>(concat!(
                $prefix, ".stale"
            ));
        }

        #[test]
        fn visibility_follows_the_controlling_value() {
            $crate::effects::typed_tests::visibility_follows_the_controlling_value::<$holder>(
                concat!($prefix, ".visibility"),
            );
        }
    };
}

pub(crate) use holder_contract;

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

/// The keyframe setter for `value`'s map.
fn keyframe(
    fx: &mut EffectInstance,
    index: usize,
    tick: i64,
    value: &Value,
    easing: Easing,
) -> Result<(), ModelError> {
    match value {
        Value::Scalar(v) => fx.set_param_keyframe(index, tick, *v, easing),
        Value::Color(v) => fx.set_color_param_keyframe(index, tick, *v, easing),
        Value::Linear(v) => fx.set_linear_color_param_keyframe(index, tick, *v, easing),
        Value::Vec2(v) => fx.set_vec2_param_keyframe(index, tick, *v, easing),
        Value::Vec3(v) => fx.set_vec3_param_keyframe(index, tick, *v, easing),
        other => panic!("{other:?} has no keyframe setter"),
    }
}

/// One in-range value per keyframe setter.
fn keyframe_probes() -> [Value; 5] {
    [
        Value::Scalar(1.0),
        Value::Color([1, 2, 3, 4]),
        Value::Linear([0.1, 0.2, 0.3, 1.0]),
        Value::Vec2([1.0, 1.0]),
        Value::Vec3([0.5; 3]),
    ]
}

const BEZIER_OUT_OF_RANGE: &str = "bezier easing x control points must be in 0..=1";

#[test]
fn kinds_lists_every_kind_and_the_fixture_holds_each() {
    let params = every_kind();
    for (at, kind) in KINDS.into_iter().enumerate() {
        assert_eq!(position(kind), at, "{kind:?}");
        assert!(
            params.iter().any(|p| p.kind == kind),
            "every_kind has no {kind:?}"
        );
    }
}

#[test]
fn kind_names_round_trip_and_aliases_resolve() {
    for kind in KINDS {
        let name = kind.as_str();
        assert_eq!(EffectParamKind::from_name(name), Some(kind));
        let json = format!("\"{name}\"");
        assert_eq!(serde_json::to_string(&kind).expect("serializes"), json);
        assert_eq!(
            serde_json::from_str::<EffectParamKind>(&json).ok(),
            Some(kind)
        );
    }
    for (alias, kind) in [
        ("float", K::Scalar),
        ("integer", K::Int),
        ("boolean", K::Bool),
        ("enum", K::Choice),
        ("colour", K::Color),
        ("point2d", K::Vec2),
        ("point_2d", K::Vec2),
        ("vec3", K::Point3),
        ("point3d", K::Point3),
        ("point_3d", K::Point3),
        ("string", K::Text),
        ("path", K::File),
        ("layer_ref", K::Layer),
    ] {
        assert_eq!(EffectParamKind::from_name(alias), Some(kind), "{alias}");
    }
    for alias in [
        "float",
        "point2d",
        "point_2d",
        "vec3",
        "point3d",
        "point_3d",
        "layer_ref",
    ] {
        let parsed: EffectParamKind =
            serde_json::from_str(&format!("\"{alias}\"")).expect("a manifest alias parses");
        assert_eq!(Some(parsed), EffectParamKind::from_name(alias), "{alias}");
    }
    assert_eq!(EffectParamKind::from_name("Scalar"), None);
    assert_eq!(EffectParamKind::from_name(""), None);
}

#[test]
fn check_params_refuses_unnamed_repeated_inverted_and_empty_parameters() {
    let cases = [
        (
            EffectParamSpec::scalar("", "Unnamed", 0.0, 0.0, 1.0),
            "fx parameter 0 has no name",
        ),
        (
            EffectParamSpec::scalar("p", "P", 0.0, 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::vec2("p", "P", [0.0; 2], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::point3("p", "P", [0.0; 3], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::size("p", "P", [0.0; 2], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::angle("p", "P", 0.0, 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::int("p", "P", 0, 5, 1),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::scalar("p", "P", 0.5, 0.0, 1.0).with_soft_range(0.9, 0.1),
            "fx.p has an inverted soft range",
        ),
        (
            EffectParamSpec::choice("p", "P", Vec::new(), "x"),
            "fx.p choice parameter has no choices",
        ),
    ];
    for (param, message) in cases {
        assert_eq!(check_params("fx", &[param]), Err(message.to_owned()));
    }
    let twice = [
        EffectParamSpec::scalar("a", "A", 0.0, 0.0, 1.0),
        EffectParamSpec::boolean("a", "A", false),
    ];
    assert_eq!(
        check_params("fx", &twice),
        Err("fx repeats parameter a".to_owned())
    );
    for kind in KINDS {
        let inverted = EffectParamSpec {
            name: "p".into(),
            kind,
            min: 1.0,
            max: 0.0,
            choices: vec![EffectChoiceOption::new("a", "A")],
            ..EffectParamSpec::default()
        };
        let want = if kind.has_float_range() {
            Err("fx.p has an inverted range".to_owned())
        } else {
            Ok(())
        };
        assert_eq!(check_params("fx", &[inverted]), want, "{kind:?}");
    }
    assert_eq!(check_params("fx", &every_kind()), Ok(()));
}

#[test]
fn visibility_conditions_load_from_every_spelling() {
    for (json, equals) in [
        (r#"{"param":"p","equals":"x"}"#, "x"),
        (r#"{"param":"p","equals_choice":"fast"}"#, "fast"),
        (r#"{"param":"p","equals_bool":true}"#, "true"),
        (r#"{"param":"p","equals_int":-3}"#, "-3"),
        (r#"{"param":"p"}"#, ""),
        (
            r#"{"param":"p","equals":"x","equals_choice":"y","equals_bool":false,"equals_int":1}"#,
            "x",
        ),
        (
            r#"{"param":"p","equals_choice":"y","equals_bool":false}"#,
            "y",
        ),
        (
            r#"{"param":"p","equals_bool":false,"equals_int":1}"#,
            "false",
        ),
    ] {
        let condition: EffectVisibilityCondition = serde_json::from_str(json).expect(json);
        assert_eq!(
            condition,
            EffectVisibilityCondition::new("p", equals),
            "{json}"
        );
    }
    let saved = serde_json::to_string(&EffectVisibilityCondition::new("p", "x")).expect("saves");
    assert_eq!(saved, r#"{"param":"p","equals":"x"}"#);
}

#[test]
fn color_defaults_convert_between_bytes_and_linear() {
    let linear_only = EffectParamSpec {
        name: "c".into(),
        kind: K::Color,
        default_linear_color: Some([1.0, 0.5, 0.0, 1.0]),
        ..EffectParamSpec::default()
    };
    assert_eq!(linear_only.default_color_u8(), [255, 128, 0, 255]);
    let bytes_only = EffectParamSpec {
        name: "c".into(),
        kind: K::Color,
        default_color: [255, 0, 51, 255],
        ..EffectParamSpec::default()
    };
    assert_eq!(bytes_only.default_color_linear(), [1.0, 0.0, 0.2, 1.0]);
    let bright = EffectParamSpec::linear_color("c", "C", [2.0, -1.0, 0.5, 1.0]);
    assert_eq!(bright.default_color_u8(), [255, 0, 128, 255]);
    assert_eq!(bright.default_color_linear(), [2.0, -1.0, 0.5, 1.0]);
}

#[test]
fn a_choice_falls_back_to_its_default_then_its_first_option() {
    let options = vec![
        EffectChoiceOption::new("a", "A"),
        EffectChoiceOption::new("b", "B"),
        EffectChoiceOption::new("c", "C"),
    ];
    let no_default = EffectParamSpec {
        name: "m".into(),
        kind: K::Choice,
        choices: options.clone(),
        ..EffectParamSpec::default()
    };
    assert_eq!(no_default.default_choice_id(), Some("a"));
    assert_eq!(no_default.choice_or_default(None), Some("a"));
    assert_eq!(no_default.choice_or_default(Some("b")), Some("b"));
    assert_eq!(no_default.choice_or_default(Some("gone")), Some("a"));
    let empty = EffectParamSpec {
        kind: K::Choice,
        ..EffectParamSpec::default()
    };
    assert_eq!(empty.choice_or_default(Some("x")), None);
    let built = EffectParamSpec::choice("m", "M", options, "c");
    assert_eq!((built.default, built.min, built.max), (2.0, 0.0, 2.0));
    assert_eq!(built.choice_or_default(Some("gone")), Some("c"));
}

#[test]
fn numeric_constructors_mirror_ranges_and_set_units() {
    let int = EffectParamSpec::int("i", "I", 3, -5, 10);
    assert_eq!((int.default, int.min, int.max), (3.0, -5.0, 10.0));
    assert_eq!(EffectParamSpec::boolean("b", "B", true).default, 1.0);
    assert_eq!(EffectParamSpec::boolean("b", "B", false).default, 0.0);
    let size = EffectParamSpec::size("s", "S", [1.0; 2], 0.0, 2.0);
    assert_eq!(size.unit.as_deref(), Some("px"));
    let angle = EffectParamSpec::angle("a", "A", 0.0, -1.0, 1.0);
    assert_eq!(angle.unit.as_deref(), Some("deg"));
    assert_eq!(angle.display, Some(EffectParamDisplay::AngleDial));
}

#[test]
fn curves_and_gradients_hold_their_ends_and_interpolate_between() {
    assert_eq!(
        sample_effect_curve(&[], 0.25),
        0.25,
        "no points read as identity"
    );
    assert_eq!(sample_effect_curve(&[], 1.5), 1.0, "t is clamped");
    assert_eq!(sample_effect_curve(&[pt(0.5, 0.2)], 0.9), 0.2);
    let bend = [pt(0.25, 0.0), pt(0.75, 1.0), pt(1.0, 0.5)];
    assert_eq!(
        sample_effect_curve(&bend, 0.125),
        0.0,
        "before the first point"
    );
    assert_eq!(sample_effect_curve(&bend, 0.5), 0.5);
    assert_eq!(sample_effect_curve(&bend, 0.875), 0.75);
    assert_eq!(
        sample_effect_curve(&bend[..2], 0.875),
        1.0,
        "after the last point"
    );
    let step = [pt(0.0, 0.0), pt(0.5, 0.2), pt(0.5 + f32::EPSILON, 0.8)];
    assert_eq!(
        sample_effect_curve(&step, 0.5 + f32::EPSILON / 2.0),
        0.8,
        "a step narrower than 1e-6 jumps to its far value"
    );
    let lut = evaluate_effect_curve_lut(&[pt(0.0, 1.0), pt(1.0, 0.0)]);
    assert_eq!((lut[0], lut[EFFECT_CURVE_LUT_SAMPLES - 1]), (1.0, 0.0));

    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    assert_eq!(sample_effect_gradient(&[], 0.25), [0.25, 0.25, 0.25, 1.0]);
    assert_eq!(sample_effect_gradient(&[stop(0.5, red)], 0.0), red);
    let ramp = [stop(0.25, red), stop(0.75, blue)];
    assert_eq!(
        sample_effect_gradient(&ramp, 0.0),
        red,
        "before the first stop"
    );
    assert_eq!(sample_effect_gradient(&ramp, 0.5), [0.5, 0.0, 0.5, 1.0]);
    assert_eq!(
        sample_effect_gradient(&ramp, 1.0),
        blue,
        "after the last stop"
    );
    let hard = [
        stop(0.0, red),
        stop(0.5, red),
        stop(0.5 + f32::EPSILON, blue),
    ];
    assert_eq!(
        sample_effect_gradient(&hard, 0.5 + f32::EPSILON / 2.0),
        blue
    );
    let lut = evaluate_effect_gradient_lut(&[stop(0.0, red), stop(1.0, blue)]);
    assert_eq!((lut[0], lut[EFFECT_GRADIENT_LUT_SAMPLES - 1]), (red, blue));
}

#[test]
fn bounds_combine_and_grow_the_quad_toward_each_side() {
    assert_eq!(
        EffectBounds::new(1.0, 2.0, 3.0, 4.0).combine(EffectBounds::uniform(1.0)),
        EffectBounds::new(2.0, 3.0, 4.0, 5.0)
    );
    assert!(EffectBounds::ZERO.is_zero());
    assert!(!EffectBounds::new(0.0, 0.0, 0.0, 0.5).is_zero());
    let (center, size) = ([100.0, 100.0], [50.0, 20.0]);
    assert_eq!(
        EffectBounds::ZERO.apply_to_quad(center, size),
        (center, size)
    );
    assert_eq!(
        EffectBounds::new(10.0, 0.0, 30.0, 4.0).apply_to_quad(center, size),
        ([110.0, 102.0], [90.0, 24.0])
    );
    assert_eq!(
        EffectBounds::uniform(-100.0).apply_to_quad(center, size),
        (center, [0.0, 0.0]),
        "a shrink past zero leaves an empty quad"
    );
}

#[test]
fn a_spec_built_in_code_matches_its_catalog_entry() {
    let parsed: EffectSpec = toml::from_str(
        r#"
id = "test.bounds"
label = "Bounds"
reads-outside = true
time-dependent = true

[bounds]
left = 4.0
top = 4.0
right = 4.0
bottom = 4.0

[[params]]
name = "radius"
label = "Radius"
default = 4.0
max = 64.0
unit = "px"

[[groups]]
id = "adv"
label = "Advanced"
collapsed = true
"#,
    )
    .expect("parses");
    let built = EffectSpec {
        id: "test.bounds".into(),
        label: "Bounds".into(),
        params: vec![EffectParamSpec::scalar("radius", "Radius", 4.0, 0.0, 64.0).with_unit("px")],
        groups: vec![EffectGroupSpec::new("adv", "Advanced").with_collapsed(true)],
        ..EffectSpec::default()
    }
    .with_bounds(EffectBounds::uniform(4.0))
    .with_reads_outside(true)
    .with_time_dependent(true);
    assert_eq!(parsed, built);
    assert_eq!(built.param("radius"), built.param_at(0));
    assert!(built.param("radius").is_some());
    assert_eq!((built.param("ghost"), built.param_at(1)), (None, None));
    assert_eq!(built.group("adv").map(|g| g.collapsed), Some(true));
    assert_eq!(built.group("ghost"), None);
}

#[test]
fn a_point_override_replaces_one_vector_slot_of_the_buffer() {
    let id = "test.typed.fx.point";
    let _reg = EffectInstance::register(id, every_kind());
    let params = every_kind();
    let fx = EffectInstance::new(id);
    let base = fx.sample_flattened_floats(0.0, None).expect("registered");
    let moved = fx
        .sample_flattened_floats(0.0, Some((OFFSET, [0.9, -0.9])))
        .expect("registered");
    let mut want = base.clone();
    let at = offset(&params, OFFSET);
    want[at..at + 2].copy_from_slice(&[0.9, -0.9]);
    assert_eq!(moved, want);
    assert_eq!(
        fx.sample_flattened_floats(0.0, Some((SCALAR, [0.9, -0.9])))
            .ok(),
        Some(base),
        "a slot that is not a vector ignores the override"
    );
}

#[test]
fn keyframe_setters_take_exactly_their_kinds() {
    let id = "test.typed.fx.keyframe_kinds";
    let _reg = EffectInstance::register(id, every_kind());
    let params = every_kind();
    for (index, p) in params.iter().enumerate() {
        for value in keyframe_probes() {
            let mut fx = EffectInstance::new(id);
            let got = keyframe(&mut fx, index, 0, &value, Easing::Linear);
            if value.accepts(p.kind) {
                assert_eq!(got, Ok(()), "{value:?} on {}", p.name);
                assert_eq!(fx.validate(), Ok(()), "{value:?} on {}", p.name);
            } else {
                let message = format!(
                    "effect parameter '{}' is {:?}, expected {}",
                    p.name,
                    p.kind,
                    value.expects()
                );
                assert_eq!(got, refused(message), "{value:?} on {}", p.name);
                assert_eq!(
                    fx,
                    EffectInstance::new(id),
                    "a refused {value:?} writes nothing"
                );
            }
        }
        let mut fx = EffectInstance::new(id);
        assert_eq!(
            fx.remove_param_keyframe(index, 0),
            refused(format!("no keyframe at tick 0 on {id}.{}", p.name))
        );
    }
    let past = params.len();
    let missing = format!("effect '{id}' has no parameter at index {past}");
    let mut fx = EffectInstance::new(id);
    for value in keyframe_probes() {
        assert_eq!(
            keyframe(&mut fx, past, 0, &value, Easing::Linear),
            refused(missing.clone())
        );
    }
    assert_eq!(fx.remove_param_keyframe(past, 0), refused(missing));
}

#[test]
fn keyframes_interpolate_and_the_buffer_follows() {
    let id = "test.typed.fx.keyframes";
    let _reg = EffectInstance::register(id, every_kind());
    let params = every_kind();
    let cases = [
        (
            SCALAR,
            Value::Scalar(0.0),
            Value::Scalar(1.0),
            Sampler::Scalar,
            Read::Scalar(0.5),
        ),
        (
            INT,
            Value::Scalar(0.0),
            Value::Scalar(10.0),
            Sampler::Int,
            Read::Int(5),
        ),
        (
            SPIN,
            Value::Scalar(-90.0),
            Value::Scalar(90.0),
            Sampler::Scalar,
            Read::Scalar(0.0),
        ),
        (
            TINT,
            Value::Color([0; 4]),
            Value::Color([200, 100, 50, 250]),
            Sampler::Color,
            Read::Color([100, 50, 25, 125]),
        ),
        (
            GLOW,
            Value::Linear([0.0; 4]),
            Value::Linear([1.0; 4]),
            Sampler::Linear,
            Read::Linear([0.5; 4]),
        ),
        (
            OFFSET,
            Value::Vec2([-1.0, -1.0]),
            Value::Vec2([1.0, 1.0]),
            Sampler::Vec2,
            Read::Vec2([0.0, 0.0]),
        ),
        (
            EXTENT,
            Value::Vec2([100.0, 100.0]),
            Value::Vec2([300.0, 300.0]),
            Sampler::Vec2,
            Read::Vec2([200.0, 200.0]),
        ),
        (
            PIVOT,
            Value::Vec3([0.0; 3]),
            Value::Vec3([2.0; 3]),
            Sampler::Vec3,
            Read::Vec3([1.0; 3]),
        ),
    ];
    for (index, from, to, sampler, halfway) in cases {
        let name = &params[index].name;
        let mut fx = EffectInstance::new(id);
        keyframe(&mut fx, index, 0, &from, Easing::Linear).expect("first keyframe");
        keyframe(&mut fx, index, 10, &to, Easing::Linear).expect("second keyframe");
        assert_eq!(fx.read(sampler, name, 5.0), Some(halfway), "{name} halfway");
        assert_eq!(fx.validate(), Ok(()), "{name}");
        assert_eq!(
            fx.flattened(5.0).ok(),
            Some(buffer_from_samplers(&fx, &params, 5.0)),
            "{name} buffer halfway"
        );
        assert_eq!(fx.remove_param_keyframe(index, 10), Ok(()), "{name}");
        assert_eq!(
            fx.remove_param_keyframe(index, 10),
            refused(format!("no keyframe at tick 10 on {id}.{name}"))
        );
    }
    let mut fx = EffectInstance::new(id);
    fx.set_linear_color_param_keyframe(TINT, 0, [0.0; 4], Easing::Linear)
        .expect("linear");
    fx.set_color_param_keyframe(TINT, 0, [1, 2, 3, 4], Easing::Linear)
        .expect("bytes");
    assert_eq!(
        fx.filled(),
        [Store::Color],
        "a byte keyframe replaces linear keys"
    );
    fx.set_linear_color_param_keyframe(TINT, 0, [0.0; 4], Easing::Linear)
        .expect("linear");
    assert_eq!(
        fx.filled(),
        [Store::LinearColor],
        "a linear keyframe replaces byte keys"
    );
}

#[test]
fn refused_keyframes_write_nothing() {
    let id = "test.typed.fx.keyframe_refusals";
    let _reg = EffectInstance::register(id, every_kind());
    let bad = Easing::Bezier {
        points: [1.5, 0.0, 0.5, 1.0],
    };
    let cases = [
        (
            SCALAR,
            Value::Scalar(2.0),
            Easing::Linear,
            "scalar = 2 out of range [0, 1]",
        ),
        (
            INT,
            Value::Scalar(11.0),
            Easing::Linear,
            "int = 11 out of range [-5, 10]",
        ),
        (
            GLOW,
            Value::Linear([f32::NAN, 0.0, 0.0, 1.0]),
            Easing::Linear,
            "glow has non-finite linear color component",
        ),
        (
            OFFSET,
            Value::Vec2([0.0, -2.0]),
            Easing::Linear,
            "offset[1] = -2 out of range [-1, 1]",
        ),
        (
            PIVOT,
            Value::Vec3([3.0, 0.0, 0.0]),
            Easing::Linear,
            "pivot[0] = 3 out of range [-2, 2]",
        ),
        (SCALAR, Value::Scalar(0.5), bad, BEZIER_OUT_OF_RANGE),
        (TINT, Value::Color([1, 2, 3, 4]), bad, BEZIER_OUT_OF_RANGE),
        (GLOW, Value::Linear([0.0; 4]), bad, BEZIER_OUT_OF_RANGE),
        (OFFSET, Value::Vec2([0.0; 2]), bad, BEZIER_OUT_OF_RANGE),
        (PIVOT, Value::Vec3([0.0; 3]), bad, BEZIER_OUT_OF_RANGE),
    ];
    for (index, value, easing, message) in cases {
        let mut fx = EffectInstance::new(id);
        assert_eq!(
            keyframe(&mut fx, index, 0, &value, easing),
            refused(message),
            "{value:?}"
        );
        assert_eq!(
            fx,
            EffectInstance::new(id),
            "a refused {value:?} writes nothing"
        );
    }
    let mut fx = EffectInstance::new(id);
    fx.set_param_keyframe(INT, 0, 2.6, Easing::Linear)
        .expect("in range");
    assert_eq!(
        fx.sample_int_param("int", 0.0),
        Some(3),
        "an int keyframe is rounded"
    );
}

#[test]
fn param_values_reach_exactly_the_keyframeable_kinds() {
    let id = "test.typed.fx.param_values";
    let _reg = EffectInstance::register(id, every_kind());
    for (index, p) in every_kind().iter().enumerate() {
        let mut constant = EffectInstance::new(id);
        let set = constant.set_param_value_constant(index, param_value(p.kind));
        let mut keyed = EffectInstance::new(id);
        let key = keyed.set_param_value_keyframe(index, 4, param_value(p.kind), Easing::Linear);
        if p.kind.is_keyframeable() {
            assert_eq!((&set, &key), (&Ok(()), &Ok(())), "{}", p.name);
            assert_eq!(
                constant.flattened(4.0),
                keyed.flattened(4.0),
                "{}: a constant and a keyframe of one value read alike",
                p.name
            );
            let rect = ParamValue::Rect([0.0; 4]);
            assert_eq!(
                constant.set_param_value_constant(index, rect),
                refused(wrong_shape(p.kind))
            );
            assert_eq!(
                keyed.set_param_value_keyframe(index, 8, rect, Easing::Linear),
                refused(wrong_shape(p.kind))
            );
        } else {
            assert_eq!(
                set,
                refused(format!(
                    "effect parameter '{}' ({:?}) is not settable via ParamValue",
                    p.name, p.kind
                ))
            );
            assert_eq!(
                key,
                refused(format!(
                    "effect parameter '{}' ({:?}) does not support keyframes",
                    p.name, p.kind
                ))
            );
        }
    }
}

/// Keyframes of `(tick, value)` pairs, each eased linearly.
fn keyed<T>(keys: Vec<(i64, T)>) -> Param<T> {
    Param::Keyframed {
        keyframes: keys
            .into_iter()
            .map(|(tick, value)| Keyframe::new(tick, value, Easing::Linear))
            .collect(),
    }
}

#[test]
fn validate_refuses_malformed_and_out_of_range_keyframes_in_every_map() {
    let id = "test.typed.fx.shapes";
    let _reg = EffectInstance::register(id, every_kind());
    const EMPTY: &str = "keyframed param with no keyframes";
    const UNSORTED: &str = "keyframes must be strictly sorted by tick";
    let cases: [(fn(&mut EffectInstance), &str); 12] = [
        (
            |fx| {
                fx.params.insert("scalar".into(), keyed(Vec::new()));
            },
            EMPTY,
        ),
        (
            |fx| {
                fx.params
                    .insert("int".into(), keyed(vec![(5, 1.0), (5, 2.0)]));
            },
            UNSORTED,
        ),
        (
            |fx| {
                fx.color_params.insert("tint".into(), keyed(Vec::new()));
            },
            EMPTY,
        ),
        (
            |fx| {
                let keys = keyed(vec![(9, [0; 4]), (3, [1; 4])]);
                fx.color_params.insert("tint".into(), keys);
            },
            UNSORTED,
        ),
        (
            |fx| {
                fx.linear_color_params
                    .insert("glow".into(), keyed(Vec::new()));
            },
            EMPTY,
        ),
        (
            |fx| {
                fx.vec2_params.insert("offset".into(), keyed(Vec::new()));
            },
            EMPTY,
        ),
        (
            |fx| {
                fx.vec3_params.insert("pivot".into(), keyed(Vec::new()));
            },
            EMPTY,
        ),
        (
            |fx| {
                fx.params
                    .insert("scalar".into(), keyed(vec![(0, 0.5), (10, 2.0)]));
            },
            "scalar = 2 out of range [0, 1]",
        ),
        (
            |fx| {
                let keys = keyed(vec![(0, [0.0; 4]), (10, [f32::INFINITY, 0.0, 0.0, 1.0])]);
                fx.linear_color_params.insert("glow".into(), keys);
            },
            "glow has non-finite linear color component",
        ),
        (
            |fx| {
                let keys = keyed(vec![(0, [10.0; 2]), (10, [10.0, 9000.0])]);
                fx.vec2_params.insert("extent".into(), keys);
            },
            "extent[1] = 9000 out of range [1, 8192]",
        ),
        (
            |fx| {
                let keys = keyed(vec![(0, [0.0; 3]), (10, [0.0, 0.0, 5.0])]);
                fx.vec3_params.insert("pivot".into(), keys);
            },
            "pivot[2] = 5 out of range [-2, 2]",
        ),
        (
            |fx| {
                let eased = Param::Keyframed {
                    keyframes: vec![Keyframe::new(
                        0,
                        0.5,
                        Easing::Bezier {
                            points: [2.0, 0.0, 0.5, 1.0],
                        },
                    )],
                };
                fx.params.insert("scalar".into(), eased);
            },
            BEZIER_OUT_OF_RANGE,
        ),
    ];
    for (store, message) in cases {
        let mut fx = EffectInstance::new(id);
        store(&mut fx);
        assert_eq!(fx.validate(), refused(message));
    }
}

#[test]
fn a_generator_whose_catalog_entry_leaves_reads_as_missing() {
    let id = "test.typed.fx.generator";
    let spec = |generator| EffectSpec {
        id: id.into(),
        label: "Generator".into(),
        generator,
        ..EffectSpec::default()
    };
    let (registered, _) = crate::register_effect(spec(true)).expect("registers");
    let mut fx = EffectInstance::new(id);
    assert!(fx.generator && !fx.is_missing_generator());
    drop(registered);
    assert!(fx.is_missing_generator());
    assert_eq!(
        fx.spec().err(),
        Some(ModelError::InvalidParam(format!("unknown effect '{id}'")))
    );
    fx.note_catalog();
    assert!(
        fx.generator,
        "an id the catalog lacks keeps the placed flag"
    );
    let (_reg, _) = crate::register_effect(spec(false)).expect("registers");
    fx.mark_from_catalog();
    assert!(!fx.generator && !fx.is_missing_generator());
}
