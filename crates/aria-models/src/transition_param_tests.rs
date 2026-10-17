//! Typed transition parameters.
//!
//! WHY: [`Transition`] keeps its own maps, samplers, and setters beside the
//! ones on [`crate::EffectInstance`], so a test of effect parameters does not
//! reach them. The shared contract in [`crate::effects::typed_tests`] runs
//! here against [`Transition`]; the tests below cover what only a transition
//! has: a duration, unkeyed `ParamValue` writes, and spec builders.
//!
//! Not caught: the compositor's reading of the transition buffer.

use super::*;
use crate::effects::typed_tests::{
    Holder, INT, Read, SCALAR, Sampler, Store, Value, every_kind, holder_contract, offset,
    param_value, refused, wrong_shape,
};

impl Holder for Transition {
    const OWNER: &'static str = "transition";

    fn register(id: &str, params: Vec<EffectParamSpec>) -> crate::Registered {
        let spec = TransitionSpec::new(id, id).with_params(params);
        crate::register_transition(spec)
            .expect("the test transition registers")
            .0
    }

    fn fresh(id: &str) -> Self {
        Self::new(ClipId::from_raw(1), ClipId::from_raw(2), id, 12)
    }

    fn poke(&mut self, name: &str, value: Value) {
        let name = name.to_owned();
        match value {
            Value::Scalar(v) => {
                self.params.insert(name, v);
            }
            Value::Int(v) => {
                self.params.insert(name, v as f32);
            }
            Value::Bool(v) => {
                self.bool_params.insert(name, v);
            }
            Value::Choice(v) => {
                self.choice_params.insert(name, v.to_owned());
            }
            Value::Color(v) => {
                self.color_params.insert(name, v);
            }
            Value::Linear(v) => {
                self.linear_color_params.insert(name, v);
            }
            Value::Vec2(v) => {
                self.vec2_params.insert(name, v);
            }
            Value::Vec3(v) => {
                self.vec3_params.insert(name, v);
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
            left: _,
            right: _,
            transition_id: _,
            duration: _,
            alignment: _,
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

    fn read(&self, sampler: Sampler, name: &str, _tick: f64) -> Option<Read> {
        Some(match sampler {
            Sampler::Scalar => Read::Scalar(self.sample_param(name)?),
            Sampler::Int => Read::Int(self.sample_int_param(name)?),
            Sampler::Bool => Read::Bool(self.sample_bool_param(name)?),
            Sampler::Choice => Read::Choice(self.sample_choice_param(name)?),
            Sampler::Index => Read::Index(self.sample_choice_index(name)?),
            Sampler::Color => Read::Color(self.sample_color_param(name)?),
            Sampler::Linear => Read::Linear(self.sample_linear_color_param(name)?),
            Sampler::Vec2 => Read::Vec2(self.sample_vec2_param(name)?),
            Sampler::Vec3 => Read::Vec3(self.sample_vec3_param(name)?),
            Sampler::Curve => Read::Curve(self.sample_curve_param(name)?),
            Sampler::Gradient => Read::Gradient(self.sample_gradient_param(name)?),
            Sampler::Text => Read::Text(self.sample_text_param(name)?),
            Sampler::Layer => Read::Layer(self.sample_layer_param(name)?),
        })
    }

    fn flattened(&self, tick: f64) -> Result<Vec<f32>, ModelError> {
        self.sample_flattened_floats(tick)
    }

    fn set(&mut self, index: usize, value: Value) -> Result<(), ModelError> {
        match value {
            Value::Scalar(v) => self.set_param_constant(index, v),
            Value::Int(v) => self.set_int_param(index, v),
            Value::Bool(v) => self.set_bool_param(index, v),
            Value::Choice(v) => self.set_choice_param(index, v),
            Value::Color(v) => self.set_color_param(index, v),
            Value::Linear(v) => self.set_linear_color_param(index, v),
            Value::Vec2(v) => self.set_vec2_param(index, v),
            Value::Vec3(v) => self.set_vec3_param(index, v),
            Value::Curve(v) => self.set_curve_param(index, v),
            Value::Gradient(v) => self.set_gradient_param(index, v),
            Value::Text(v) => self.set_text_param(index, v),
            Value::Layer(v) => self.set_layer_param(index, v),
        }
    }

    fn visible(&self, index: usize) -> bool {
        let spec = self.spec().expect("registered");
        self.is_param_visible(&spec.params[index], &spec)
    }
}

mod transition_contract {
    use super::*;

    holder_contract!(Transition, "test.typed.tr");
}

#[test]
fn a_transition_needs_a_positive_duration_before_anything_else() {
    let id = "test.typed.tr.duration";
    let _reg = Transition::register(id, every_kind());
    let left = ClipId::from_raw(1);
    let right = ClipId::from_raw(2);
    assert_eq!(Transition::new(left, right, id, -4).duration, 1);
    for duration in [0, -1, i64::MIN] {
        let mut t = Transition::fresh(id);
        t.duration = duration;
        assert_eq!(
            t.validate(),
            refused(format!("transition duration {duration} must be positive"))
        );
        t.transition_id = "test.typed.tr.unregistered".into();
        assert_eq!(
            t.validate(),
            refused(format!("transition duration {duration} must be positive")),
            "the duration is checked before the catalog"
        );
    }
    let mut t = Transition::fresh(id);
    t.duration = 1;
    assert_eq!(t.validate(), Ok(()));
}

#[test]
fn param_values_reach_exactly_the_shaped_kinds() {
    let id = "test.typed.tr.param_values";
    let _reg = Transition::register(id, every_kind());
    let params = every_kind();
    for (index, p) in params.iter().enumerate() {
        let mut t = Transition::fresh(id);
        let set = t.set_param_value(index, param_value(p.kind));
        if p.kind.is_keyframeable() {
            assert_eq!(set, Ok(()), "{}", p.name);
            assert_eq!(t.check(), Ok(()), "{}", p.name);
            let mut direct = Transition::fresh(id);
            let value = match param_value(p.kind) {
                ParamValue::Scalar(v) => Value::Scalar(v),
                ParamValue::Color(c) => Value::Color(c),
                ParamValue::Vec2(v) => Value::Vec2(v),
                ParamValue::Vec3(v) => Value::Vec3(v),
                other => panic!("{other:?} has no typed setter"),
            };
            direct.set(index, value).expect("in range");
            assert_eq!(
                t, direct,
                "{}: a ParamValue writes as its typed setter",
                p.name
            );
            let before = t.clone();
            assert_eq!(
                t.set_param_value(index, ParamValue::Rect([0.0; 4])),
                refused(wrong_shape(p.kind))
            );
            assert_eq!(t, before, "a refused shape writes nothing");
        } else {
            assert_eq!(
                set,
                refused(format!(
                    "transition parameter '{}' ({:?}) is not settable via ParamValue",
                    p.name, p.kind
                ))
            );
            assert_eq!(t, Transition::fresh(id), "{}", p.name);
        }
    }
    let past = params.len();
    assert_eq!(
        Transition::fresh(id).set_param_value(past, ParamValue::Scalar(0.5)),
        refused(format!(
            "transition '{id}' has no parameter at index {past}"
        ))
    );
}

#[test]
fn a_scalar_written_to_an_int_slot_is_stored_rounded() {
    let id = "test.typed.tr.rounding";
    let _reg = Transition::register(id, every_kind());
    let mut t = Transition::fresh(id);
    t.set(INT, Value::Scalar(2.6)).expect("in range");
    assert_eq!(t.params.get("int"), Some(&3.0));
    assert_eq!(t.read(Sampler::Int, "int", 0.0), Some(Read::Int(3)));
    t.poke("int", Value::Scalar(4.4));
    assert_eq!(t.read(Sampler::Int, "int", 0.0), Some(Read::Int(4)));
    assert_eq!(t.read(Sampler::Scalar, "int", 0.0), Some(Read::Scalar(4.0)));
    let buffer = t.flattened(0.0).expect("registered");
    assert_eq!(buffer[offset(&every_kind(), INT)], 4.0);
    t.set(SCALAR, Value::Scalar(0.6)).expect("in range");
    assert_eq!(
        t.params.get("scalar"),
        Some(&0.6),
        "a scalar slot is not rounded"
    );
}

#[test]
fn spec_builders_append_replace_and_look_up_parameters() {
    let a = EffectParamSpec::scalar("a", "A", 0.5, 0.0, 1.0);
    let b = EffectParamSpec::boolean("b", "B", false);
    let spec = TransitionSpec::new("test.typed.tr.builders", "Builders")
        .with_param(a.clone())
        .with_param(b.clone());
    assert_eq!(spec.id, "test.typed.tr.builders");
    assert_eq!(spec.label, "Builders");
    assert_eq!(spec.params, [a.clone(), b.clone()]);
    assert_eq!(spec.param("b"), Some(&b));
    assert_eq!(spec.param("c"), None);
    assert_eq!(spec.param_at(0), Some(&a));
    assert_eq!(spec.param_at(2), None);
    let replaced = spec.with_params(vec![b.clone()]);
    assert_eq!(replaced.params, [b.clone()]);
    assert_eq!(replaced.param("a"), None);
    assert_eq!(replaced.param_at(0), Some(&b));
}
