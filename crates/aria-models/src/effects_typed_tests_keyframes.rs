use super::*;
use crate::param::Keyframe;

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
