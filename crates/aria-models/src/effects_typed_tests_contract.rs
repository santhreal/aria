use super::*;

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
