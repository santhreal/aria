//! `aria.expressions` compiled and evaluated against a scripted scope.
//!
//! Closes the class "an expression computes something other than the After
//! Effects expression library documents": units at the model boundary, the
//! inputs a reference declares, the library's functions, loops over
//! keyframes, the limits that end a hostile program, and nesting that would
//! exhaust a render thread's stack. Rendering through a real project, faults,
//! and cycles are in `tests/expressions.rs`. Does not compare against After
//! Effects output: values are pinned against the formulas the reference
//! documents.

use std::cell::Cell;
use std::collections::HashMap;

use aria_models::{ClipParam, DriverCode, ParamValue};
use aria_render::{
    DriverClipInfo, DriverInput, DriverLanguage, DriverScope, InputClip, InputProperty,
};

use super::eval::{MAX_STEPS, MAX_STRING};
use super::{EXPRESSIONS_LANGUAGE, Expressions, ExpressionsPlugin};
use crate::PluginContext;

type Reader = fn(f64) -> ParamValue;

struct Scope {
    time: f64,
    property: ClipParam,
    canvas: [u32; 2],
    rate: f64,
    duration: f64,
    /// Keyframes of the driven property: linear between, held outside.
    keys: Vec<(f64, ParamValue)>,
    /// The driven property's value without keyframes.
    constant: ParamValue,
    table: HashMap<DriverInput, Reader>,
    declared: Vec<DriverInput>,
    clips: HashMap<InputClip, DriverClipInfo>,
    seed: u64,
    /// Clock reads left before the budget runs out; `None` never runs out.
    clock: Cell<Option<u32>>,
    clock_reads: Cell<u32>,
}

fn scope(property: ClipParam, constant: ParamValue) -> Scope {
    Scope {
        time: 0.0,
        property,
        canvas: [1920, 1080],
        rate: 24.0,
        duration: 10.0,
        keys: Vec::new(),
        constant,
        table: HashMap::new(),
        declared: Vec::new(),
        clips: HashMap::new(),
        seed: 7,
        clock: Cell::new(None),
        clock_reads: Cell::new(0),
    }
}

fn rotation() -> Scope {
    scope(ClipParam::Rotation, ParamValue::Scalar(0.0))
}

fn input(clip: InputClip, id: &str) -> DriverInput {
    DriverInput {
        clip,
        property: InputProperty::Id(id.into()),
    }
}

fn lerp(from: ParamValue, to: ParamValue, amount: f32) -> ParamValue {
    match (from, to) {
        (ParamValue::Scalar(a), ParamValue::Scalar(b)) => ParamValue::Scalar(a + (b - a) * amount),
        (ParamValue::Vec2(a), ParamValue::Vec2(b)) => {
            ParamValue::Vec2([a[0] + (b[0] - a[0]) * amount, a[1] + (b[1] - a[1]) * amount])
        }
        (from, _) => from,
    }
}

impl Scope {
    fn sample(&self, seconds: f64) -> ParamValue {
        let Some(&(first_time, first)) = self.keys.first() else {
            return self.constant;
        };
        if seconds <= first_time {
            return first;
        }
        for pair in self.keys.windows(2) {
            let ((start, from), (end, to)) = (pair[0], pair[1]);
            if seconds <= end {
                return lerp(from, to, ((seconds - start) / (end - start)) as f32);
            }
        }
        self.keys.last().map_or(self.constant, |&(_, last)| last)
    }

    fn read(&self, index: usize, seconds: f64) -> Result<ParamValue, String> {
        let wanted = &self.declared[index];
        let reader = self
            .table
            .get(wanted)
            .ok_or_else(|| format!("nothing answers {wanted:?}"))?;
        Ok(reader(seconds))
    }
}

impl DriverScope for Scope {
    fn time(&self) -> f64 {
        self.time
    }
    fn property(&self) -> ClipParam {
        self.property
    }
    fn clip_time(&self) -> f64 {
        self.time
    }
    fn frame_rate(&self) -> f64 {
        self.rate
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn canvas(&self) -> [u32; 2] {
        self.canvas
    }
    fn value(&self) -> ParamValue {
        self.sample(self.time)
    }
    fn value_at(&self, seconds: f64) -> ParamValue {
        self.sample(seconds)
    }
    fn key_count(&self) -> usize {
        self.keys.len()
    }
    fn key_time(&self, index: usize) -> Option<f64> {
        self.keys.get(index).map(|&(time, _)| time)
    }
    fn input(&mut self, index: usize) -> Result<ParamValue, String> {
        self.read(index, self.time)
    }
    fn input_at(&mut self, index: usize, seconds: f64) -> Result<ParamValue, String> {
        self.read(index, seconds)
    }
    fn clip_info(&mut self, clip: &InputClip) -> Result<DriverClipInfo, String> {
        self.clips
            .get(clip)
            .cloned()
            .ok_or_else(|| format!("no clip {clip:?}"))
    }
    fn seed(&self) -> u64 {
        self.seed
    }
    fn expired(&self) -> bool {
        self.clock_reads.set(self.clock_reads.get() + 1);
        match self.clock.get() {
            None => false,
            Some(0) => true,
            Some(left) => {
                self.clock.set(Some(left - 1));
                false
            }
        }
    }
}

fn compile(source: &str) -> Result<std::sync::Arc<dyn aria_render::DriverProgram>, String> {
    Expressions.compile(source)
}

fn inputs_of(source: &str) -> Vec<DriverInput> {
    compile(source)
        .unwrap_or_else(|err| panic!("{source}: {err}"))
        .inputs()
        .to_vec()
}

fn run(source: &str, scope: &mut Scope) -> Result<ParamValue, String> {
    let program = compile(source)?;
    scope.declared = program.inputs().to_vec();
    program.evaluate(scope)
}

fn number(source: &str, scope: &mut Scope) -> f64 {
    match run(source, scope) {
        Ok(ParamValue::Scalar(value)) => f64::from(value),
        other => panic!("{source}: {other:?}"),
    }
}

fn error(source: &str, scope: &mut Scope) -> String {
    match run(source, scope) {
        Err(message) => message,
        Ok(value) => panic!("{source}: evaluated to {value:?}"),
    }
}

#[track_caller]
fn close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-4 * expected.abs().max(1.0),
        "{what}: {actual} != {expected}"
    );
}

#[test]
fn statements_and_operators_follow_javascript() {
    let cases = [
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("10 % 4 - -2", 4.0),
        ("let x = 2; x += 3; x * 2", 10.0),
        ("let a = [1, 2]; a[1] = 5; a[0] + a[1]", 6.0),
        ("let b = [1, 2]; let c = b; c[0] = 9; b[0]", 1.0),
        (
            "let s = 0; for (let i = 0; i < 4; i++) { if (i == 2) continue; s += i } s",
            4.0,
        ),
        ("let n = 0; while (true) { n++; if (n >= 3) break } n", 3.0),
        ("let n = 5; let m = n--; m * 10 + n", 54.0),
        ("let n = 5; let m = --n; m * 10 + n", 44.0),
        ("true ? 4 : 5", 4.0),
        ("0 || 6", 6.0),
        ("2 && 8", 8.0),
        ("return 3; 4", 3.0),
        ("if (1 > 2) 7; else 8", 8.0),
        ("1 == 1 && 2 === 2 && 1 != 2 && \"a\" < \"b\" ? 1 : 0", 1.0),
        ("[1, 2] == [1, 2] ? 1 : 0", 1.0),
        (
            "Math.max(1, 5, 3) + Math.min(2) + Math.round(2.5) + Math.round(-2.5)",
            8.0,
        ),
        ("\"ab\".length + [1, 2, 3].length", 5.0),
        ("(\"x\" + 1 + 2).length", 3.0),
        ("+\"3\" + +true", 4.0),
        ("Math.PI", std::f64::consts::PI),
        ("Math.atan2(1, 1) * 4", std::f64::consts::PI),
        ("Math.pow(2, 10) + Math.sqrt(16) + Math.hypot(3, 4)", 1033.0),
        ("let v = 1\nv + 1", 2.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
}

#[test]
fn values_convert_to_pixels_and_percent_at_the_model_boundary() {
    // Position: the anchor's offset from the canvas centre as a fraction,
    // read as pixels from the top-left corner.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.25, -0.5]));
    assert_eq!(
        run(
            "value[0] == 1440 && value[1] == 0 ? [960, 540] : [1, 1]",
            &mut position
        ),
        Ok(ParamValue::Vec2([0.0, 0.0]))
    );
    assert_eq!(
        run("value", &mut position),
        Ok(ParamValue::Vec2([0.25, -0.5]))
    );
    assert_eq!(
        run("[value[0] / 2, value[1]]", &mut position),
        Ok(ParamValue::Vec2([-0.125, -0.5]))
    );

    // Opacity and scale: fractions, read as percent.
    let mut opacity = scope(ClipParam::Opacity, ParamValue::Scalar(0.5));
    assert_eq!(
        run("value == 50 ? 25 : 0", &mut opacity),
        Ok(ParamValue::Scalar(0.25))
    );
    let mut scale = scope(ClipParam::Scale, ParamValue::Vec2([1.0, 2.0]));
    assert_eq!(
        run("value[1] == 200 ? [50, 50] : [0, 0]", &mut scale),
        Ok(ParamValue::Vec2([0.5, 0.5]))
    );

    // Everything else is read in the model's own units.
    let mut rotation = scope(ClipParam::Rotation, ParamValue::Scalar(45.0));
    assert_eq!(
        run("value + 1", &mut rotation),
        Ok(ParamValue::Scalar(46.0))
    );
    let mut anchor = scope(ClipParam::AnchorPoint, ParamValue::Vec2([0.5, 0.5]));
    assert_eq!(
        run("[value[0] + 0.25, value[1]]", &mut anchor),
        Ok(ParamValue::Vec2([0.75, 0.5]))
    );

    // Colours are four channels from 0 to 1; out-of-range channels clamp and
    // a missing alpha is opaque.
    let mut colour = scope(ClipParam::Rotation, ParamValue::Color([255, 0, 51, 255]));
    assert_eq!(
        run("value[2] == 0.2 ? value : [0, 0, 0, 0]", &mut colour),
        Ok(ParamValue::Color([255, 0, 51, 255]))
    );
    assert_eq!(
        run("[0, 1, 0]", &mut colour),
        Ok(ParamValue::Color([0, 255, 0, 255]))
    );
    assert_eq!(
        run("[2, -1, 0.5, 0.5]", &mut colour),
        Ok(ParamValue::Color([255, 0, 128, 128]))
    );
    let mut crop = scope(ClipParam::Crop, ParamValue::Rect([0.0, 0.0, 1.0, 1.0]));
    assert_eq!(
        run("[0.25, 0, value[2] / 2, 1]", &mut crop),
        Ok(ParamValue::Rect([0.25, 0.0, 0.5, 1.0]))
    );
}

#[test]
fn a_result_of_the_wrong_kind_or_out_of_range_is_an_error() {
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    let mut opacity = scope(ClipParam::Opacity, ParamValue::Scalar(1.0));
    let cases: [(&str, &mut Scope, &str); 9] = [
        (
            "5",
            &mut position,
            "the property takes an array of 2 numbers",
        ),
        ("[1, 2]", &mut opacity, "the property takes a number"),
        ("0 / 0", &mut rotation(), "NaN"),
        ("1 / 0", &mut rotation(), "Infinity"),
        ("1e300", &mut rotation(), "out of range"),
        ("let x = 1", &mut rotation(), "ends without a value"),
        ("seedRandom(1)", &mut rotation(), "returned null"),
        ("break", &mut rotation(), "break is outside a loop"),
        ("\"text\"", &mut rotation(), "returned a string"),
    ];
    for (source, scope, wanted) in cases {
        let message = error(source, scope);
        assert!(message.contains(wanted), "{source}: {message}");
    }
    let empty = compile("  // nothing\n").err().unwrap_or_default();
    assert!(empty.contains("empty"), "{empty}");
}

#[test]
fn references_declare_inputs_once_in_first_use_order() {
    let named = || InputClip::Named("Logo".into());
    let effect = |clip: InputClip, effect: &str, param: &str| DriverInput {
        clip,
        property: InputProperty::Effect {
            effect: effect.into(),
            param: param.into(),
        },
    };
    let source = "thisComp.layer(\"Logo\").transform.position[0] \
        + effect(\"Blur\")(\"Radius\") + thisLayer.opacity + scale[1] \
        + thisLayer.property(\"text-size\") + audio.volume \
        + thisComp.layer(\"Logo\").transform.position[1] \
        + thisComp.layer(\"Logo\").effect(\"Glow\").param(\"Intensity\") \
        + thisLayer.audio.pan + thisComp.layer(\"Logo\").prop(\"opacity\")";
    assert_eq!(
        inputs_of(source),
        vec![
            input(named(), "position"),
            effect(InputClip::This, "Blur", "Radius"),
            input(InputClip::This, "opacity"),
            input(InputClip::This, "scale"),
            input(InputClip::This, "text-size"),
            input(InputClip::This, "volume"),
            effect(named(), "Glow", "Intensity"),
            input(InputClip::This, "pan"),
            input(named(), "opacity"),
        ]
    );
    // Every transform name, bare, in the group, and on the layer; zRotation is
    // rotation.
    assert_eq!(
        inputs_of(
            "rotation + transform.rotation + thisLayer.zRotation \
             + thisLayer.transform.anchorPoint[0] + anchorPoint[1] + opacity"
        ),
        vec![
            input(InputClip::This, "rotation"),
            input(InputClip::This, "anchor-point"),
            input(InputClip::This, "opacity"),
        ]
    );
    // The driven property and time read no input.
    assert_eq!(
        inputs_of("value + thisProperty.valueAtTime(time - 1)"),
        Vec::new()
    );
}

#[test]
fn a_declared_name_shadows_the_property_of_that_name() {
    assert_eq!(inputs_of("let scale = 2; scale * 3"), Vec::new());
    assert_eq!(inputs_of("position = 4; position"), Vec::new());
    assert_eq!(number("let scale = 2; scale * 3", &mut rotation()), 6.0);
}

#[test]
fn a_reference_that_cannot_be_declared_is_refused_at_compile_time() {
    let cases = [
        (
            "let n = \"Logo\"; thisComp.layer(n).transform.position[0]",
            "line 1, column 31: thisComp.layer takes the layer's name in quotes",
        ),
        (
            "effect(1)(\"Radius\")",
            "effect takes the effect's name in quotes",
        ),
        (
            "effect(\"Blur\")(2)",
            "an effect parameter is named in quotes",
        ),
        (
            "thisLayer.property(\"a\", \"b\")",
            "a property is named by its id",
        ),
        ("transform.skew", "transform has no property skew"),
        ("audio.gain", "audio has no property gain"),
        (
            "function f() { return 1 }",
            "function is not part of aria.expressions",
        ),
    ];
    for (source, wanted) in cases {
        let message = compile(source).err().unwrap_or_default();
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn inputs_and_layers_read_through_the_scope_in_expression_units() {
    let mut scope = rotation();
    scope.time = 1.0;
    scope
        .table
        .insert(input(InputClip::Named("Logo".into()), "position"), |_| {
            ParamValue::Vec2([0.0, 0.0])
        });
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    scope.table.insert(
        DriverInput {
            clip: InputClip::This,
            property: InputProperty::Effect {
                effect: "Blur".into(),
                param: "Radius".into(),
            },
        },
        |seconds| ParamValue::Scalar(seconds as f32 * 10.0),
    );
    close(
        number(
            "thisComp.layer(\"Logo\").transform.position[0] + thisLayer.opacity \
             + effect(\"Blur\")(\"Radius\").valueAtTime(2) + effect(\"Blur\")(\"Radius\")",
            &mut scope,
        ),
        960.0 + 40.0 + 20.0 + 10.0,
        "inputs",
    );
    close(
        number(
            "effect(\"Blur\")(\"Radius\").velocityAtTime(0.5)",
            &mut scope,
        ),
        10.0,
        "input velocity",
    );
    // An input the scope cannot answer fails with the reference's position.
    let message = error("1 + scale[0]", &mut scope);
    assert!(
        message.starts_with("line 1, column 5: nothing answers"),
        "{message}"
    );

    scope.clips.insert(
        InputClip::This,
        DriverClipInfo {
            name: "Me".into(),
            in_point: 1.0,
            out_point: 3.0,
        },
    );
    scope.clips.insert(
        InputClip::Named("Logo".into()),
        DriverClipInfo {
            name: "Logo".into(),
            in_point: 0.5,
            out_point: 2.5,
        },
    );
    close(
        number(
            "thisLayer.outPoint - thisLayer.inPoint + thisComp.layer(\"Logo\").inPoint + inPoint",
            &mut scope,
        ),
        3.5,
        "extents",
    );
    close(
        number(
            "(thisLayer.name + thisComp.layer(\"Logo\").name).length",
            &mut scope,
        ),
        6.0,
        "names",
    );
}

#[test]
fn time_and_composition_values() {
    let mut scope = rotation();
    scope.time = 1.5;
    let cases = [
        ("time * 2", 3.0),
        ("thisComp.frameDuration * 24", 1.0),
        ("thisComp.width + thisComp.height", 3000.0),
        ("thisComp.duration", 10.0),
        ("timeToFrames()", 36.0),
        ("timeToFrames(0.1, 30)", 3.0),
        ("timeToFrames(0.7, 10)", 7.0),
        ("framesToTime(48)", 2.0),
        ("framesToTime(15, 30)", 0.5),
    ];
    for (source, expected) in cases {
        close(number(source, &mut scope), expected, source);
    }
}

#[test]
fn interpolation_follows_its_curves_and_clamps() {
    let cases = [
        ("linear(0.5, 0, 2, 0, 100)", 25.0),
        ("linear(0.5, 0, 2, 100, 0)", 75.0),
        ("linear(5, 0, 2, 0, 100)", 100.0),
        ("linear(-1, 0, 2, 0, 100)", 0.0),
        ("linear(0.25, 10, 20)", 12.5),
        ("linear(1, 1, 1, 0, 10)", 10.0),
        ("linear(0.5, 2, 0, 0, 100)", 75.0),
        ("ease(1, 0, 2, 0, 100)", 50.0),
        ("ease(0.5, 0, 2, 0, 100)", 15.625),
        ("easeIn(0.5, 0, 2, 0, 100)", 10.9375),
        ("easeOut(0.5, 0, 2, 0, 100)", 29.6875),
        ("easeIn(2, 0, 2, 0, 100) + easeOut(0, 0, 2, 0, 100)", 100.0),
        ("linear(0.5, [0, 10], [10, 30])[1]", 20.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    assert_eq!(
        run("linear(0.5, [0, 0], [960, 1080])", &mut position),
        Ok(ParamValue::Vec2([-0.25, 0.0]))
    );
    let message = error("linear(0.5, [0], 1)", &mut rotation());
    assert!(message.contains("two numbers or two arrays"), "{message}");
}

#[test]
fn vector_and_colour_functions() {
    let cases = [
        ("length([3, 4])", 5.0),
        ("length([1, 1], [4, 5])", 5.0),
        ("length(-3)", 3.0),
        ("dot([1, 2], [3, 4])", 11.0),
        ("cross([1, 0, 0], [0, 1, 0])[2]", 1.0),
        ("cross([1, 0], [0, 1])[2]", 1.0),
        ("normalize([3, 4])[0]", 0.6),
        ("normalize([0, 0])[1]", 0.0),
        ("add([1, 2], [3, 4])[1]", 6.0),
        ("([1, 2] + [3, 4])[0]", 4.0),
        ("mul([1, 2], 3)[1]", 6.0),
        ("(3 * [1, 2])[1]", 6.0),
        ("div([2, 4], 2)[1]", 2.0),
        ("sub([5], [1, 1])[1]", -1.0),
        ("(-[1, 2])[1]", -2.0),
        ("clamp(150, 0, 100)", 100.0),
        ("clamp(-1, 0, 100)", 0.0),
        ("clamp([-5, 50], 0, [10, 20])[1]", 20.0),
        ("degreesToRadians(180)", std::f64::consts::PI),
        ("radiansToDegrees(Math.PI)", 180.0),
        ("hslToRgb([0, 1, 0.5, 1])[0]", 1.0),
        ("hslToRgb([1 / 3, 1, 0.5])[1]", 1.0),
        ("hslToRgb([0, 0, 0.3])[2]", 0.3),
        ("rgbToHsl([0, 0, 1])[0]", 2.0 / 3.0),
        ("rgbToHsl([0.2, 0.4, 0.6, 0.5])[3]", 0.5),
        ("hslToRgb(rgbToHsl([0.2, 0.4, 0.6]))[2]", 0.6),
        ("noise(3)", 0.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
    for (source, wanted) in [
        (
            "[1, 2] * [3, 4]",
            "an array of 2 numbers * an array of 2 numbers is not defined",
        ),
        ("[1, 2] + 1", "is not defined"),
        ("dot([1], [1, 2])", "two arrays of one length"),
        ("cross([1], [1, 2])", "arrays of 2 or 3 numbers"),
        ("clamp(1, 0)", "clamp takes 3 arguments, not 2"),
        ("[\"a\"]", "an array holds numbers"),
    ] {
        let message = error(source, &mut rotation());
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn wiggle_is_seeded_smooth_and_bounded() {
    let at = |source: &str, time: f64, seed: u64| {
        let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(10.0));
        scope.time = time;
        scope.seed = seed;
        number(source, &mut scope)
    };
    let first = at("wiggle(2, 5)", 1.3, 7);
    assert_eq!(first, at("wiggle(2, 5)", 1.3, 7), "repeatable");
    assert_ne!(
        first,
        at("wiggle(2, 5)", 1.3, 8),
        "seeded per clip and property"
    );
    assert!(
        (at("wiggle(2, 5)", 1.3001, 7) - first).abs() < 0.05,
        "smooth over a tenth of a millisecond"
    );
    assert_eq!(
        at("wiggle(2, 5, 1, 0.5, 3)", 1.3, 7),
        at("wiggle(2, 5)", 3.0, 7),
        "the time argument"
    );
    assert_eq!(at("wiggle(2, 0)", 1.3, 7), 10.0);

    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for step in 0..400 {
        let shaken = at("wiggle(3, 5, 3, 0.5)", f64::from(step) * 0.025, 7);
        assert!((shaken - 10.0).abs() <= 5.0 * 1.75 + 1e-4, "{shaken}");
        low = low.min(shaken);
        high = high.max(shaken);
    }
    assert!(high - low > 2.0, "wiggle moves: {low}..{high}");

    // Each axis moves on its own.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    position.time = 0.7;
    let Ok(ParamValue::Vec2([x, y])) = run("wiggle(1, 100)", &mut position) else {
        panic!("wiggle on a position is a position");
    };
    assert_ne!(x * 1920.0, y * 1080.0);

    // An input wiggles from its own value.
    let mut scope = rotation();
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    close(
        number("thisLayer.opacity.wiggle(1, 0)", &mut scope),
        40.0,
        "input wiggle",
    );
}

#[test]
fn random_repeats_per_frame_and_reseeds() {
    let at = |source: &str, time: f64| {
        let mut scope = rotation();
        scope.time = time;
        number(source, &mut scope)
    };
    assert_eq!(at("random()", 1.0), at("random()", 1.0));
    assert_ne!(at("random()", 1.0), at("random()", 2.0));
    assert_ne!(at("random() - random()", 1.0), 0.0, "each call draws anew");
    assert_eq!(
        at("seedRandom(4, true); random()", 1.0),
        at("seedRandom(4, true); random()", 2.0),
        "timeless"
    );
    assert_ne!(
        at("seedRandom(4, true); random()", 1.0),
        at("seedRandom(5, true); random()", 1.0)
    );
    let mut inside = 0;
    let mut sum = 0.0;
    for step in 0..400 {
        let time = f64::from(step) * 0.01;
        let uniform = at("random(10, 20)", time);
        assert!((10.0..20.0).contains(&uniform), "{uniform}");
        let item = at("random([5, 5])[1]", time);
        assert!((0.0..5.0).contains(&item), "{item}");
        assert!((0.0..1.0).contains(&at("Math.random()", time)));
        let gauss = at("gaussRandom(0, 10)", time);
        sum += gauss;
        inside += usize::from((0.0..=10.0).contains(&gauss));
    }
    // Four standard errors of the mean of 400 draws.
    assert!(
        (sum / 400.0 - 5.0).abs() < 0.6,
        "gaussian mean {}",
        sum / 400.0
    );
    assert!(inside >= 340, "{inside} of 400 within range");
    let message = error("random(1, 2, 3)", &mut rotation());
    assert!(
        message.contains("random takes 0 to 2 arguments, not 3"),
        "{message}"
    );
}

#[test]
fn noise_is_continuous_and_bounded() {
    for step in 0..500 {
        let x = f64::from(step) * 0.037 - 5.0;
        let here = number(&format!("noise({x})"), &mut rotation());
        let next = number(&format!("noise({})", x + 1e-5), &mut rotation());
        assert!((-1.0..=1.0).contains(&here), "{x}: {here}");
        assert!((next - here).abs() < 2e-4, "{x}: {here} -> {next}");
        let pair = number(&format!("noise([{x}, {}])", x * 2.0), &mut rotation());
        assert!((-1.0..=1.0).contains(&pair), "{x}: {pair}");
    }
}

fn keyed(keys: &[(f64, f32)], time: f64) -> Scope {
    let mut scope = rotation();
    scope.keys = keys
        .iter()
        .map(|&(time, value)| (time, ParamValue::Scalar(value)))
        .collect();
    scope.time = time;
    scope
}

#[test]
fn loops_repeat_the_keyframes_before_and_after() {
    let ramp = [(0.0, 0.0), (1.0, 10.0)];
    let cases = [
        ("loopOut()", 1.25, 2.5),
        ("loopOut(\"cycle\")", 2.25, 2.5),
        ("loopOut(\"pingpong\")", 1.25, 7.5),
        ("loopOut(\"pingpong\")", 2.25, 2.5),
        ("loopOut(\"offset\")", 1.25, 12.5),
        ("loopOut(\"offset\")", 2.25, 22.5),
        ("loopOut(\"continue\")", 1.25, 12.5),
        ("loopOut(\"continue\")", 3.0, 30.0),
        ("loopOut()", 0.5, 5.0),
        ("loopIn()", -0.25, 7.5),
        ("loopIn(\"pingpong\")", -0.25, 2.5),
        ("loopIn(\"pingpong\")", -1.25, 7.5),
        ("loopIn(\"offset\")", -0.25, -2.5),
        ("loopIn(\"continue\")", -0.25, -2.5),
        ("loopIn()", 0.5, 5.0),
        ("thisProperty.loopOut(\"offset\")", 1.25, 12.5),
    ];
    for (source, time, expected) in cases {
        close(
            number(source, &mut keyed(&ramp, time)),
            expected,
            &format!("{source} at {time}"),
        );
    }
    // Counting keyframes back from the last, or on from the first.
    let peak = [(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
    close(
        number("loopOut(\"cycle\", 1)", &mut keyed(&peak, 2.25)),
        7.5,
        "the last segment",
    );
    close(
        number("loopOut(\"cycle\", 0)", &mut keyed(&peak, 2.25)),
        2.5,
        "every segment",
    );
    close(
        number("loopIn(\"cycle\", 1)", &mut keyed(&peak, -0.25)),
        7.5,
        "the first segment",
    );
    // Fewer than two keyframes leave the value.
    close(
        number("loopOut()", &mut keyed(&[(0.0, 3.0)], 5.0)),
        3.0,
        "one key",
    );

    let mut scope = keyed(&ramp, 2.0);
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    for (source, wanted) in [
        (
            "thisLayer.opacity.loopOut()",
            "loopOut reads the keyframes of the property the driver is on",
        ),
        ("loopOut(\"bounce\")", "loopOut has no type \"bounce\""),
        ("loopIn(3)", "the type of loopIn is a string"),
    ] {
        let message = error(source, &mut scope);
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn keyframes_and_rates_of_change() {
    let peak = [(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
    let cases = [
        ("numKeys", 3.0),
        ("thisProperty.numKeys", 3.0),
        ("key(2).time", 1.0),
        ("key(2).value", 10.0),
        ("key(3).index", 3.0),
        ("valueAtTime(1.5)", 5.0),
        ("velocityAtTime(0.5)", 10.0),
        ("speedAtTime(1.5)", 10.0),
        ("thisProperty.velocity", 10.0),
        ("thisProperty.speed", 10.0),
        ("value", 5.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut keyed(&peak, 0.5)), expected, source);
    }
    for source in ["key(0)", "key(4)", "key(1.5)"] {
        let message = error(source, &mut keyed(&peak, 0.5));
        assert!(message.contains("is out of range"), "{source}: {message}");
    }
    // A position's velocity is in pixels per second.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    position.keys = vec![
        (0.0, ParamValue::Vec2([0.0, 0.0])),
        (1.0, ParamValue::Vec2([0.5, 0.0])),
    ];
    position.time = 0.5;
    let Ok(ParamValue::Vec2([x, y])) = run("velocityAtTime(0.5)", &mut position) else {
        panic!("a velocity of a position is a position");
    };
    close(f64::from(x), 960.0 / 1920.0 - 0.5, "x");
    close(f64::from(y), -0.5, "y");
}

#[test]
fn a_runaway_program_ends_with_an_error() {
    // The clock runs out: the program stops at the next read.
    let mut scope = rotation();
    scope.clock.set(Some(2));
    let message = error("while (true) {}", &mut scope);
    assert_eq!(message, "the driver ran past its time budget");
    assert_eq!(scope.clock_reads.get(), 3);

    // A clock that never runs out still ends.
    let message = error("for (;;) {}", &mut rotation());
    assert_eq!(
        message,
        format!("the driver took more than {MAX_STEPS} steps")
    );

    // A string cannot outgrow its limit, and an array does not grow.
    let message = error(
        "let s = \"ab\"; while (true) { s = s + s }",
        &mut rotation(),
    );
    assert_eq!(
        message,
        format!("line 1, column 36: a string grew past {MAX_STRING} bytes")
    );
    let message = error("let a = [1]; a[5] = 2", &mut rotation());
    assert!(
        message.contains("index 5 is past the end of a, which holds 1"),
        "{message}"
    );
}

#[test]
fn the_deepest_accepted_nesting_evaluates_on_a_default_thread_stack() {
    let shapes: [(&str, fn(usize) -> String); 12] = [
        ("parentheses", |n| {
            format!("{}1{}", "(".repeat(n), ")".repeat(n))
        }),
        ("negation", |n| format!("{}1", "- ".repeat(n))),
        ("not", |n| format!("{}1", "!".repeat(n))),
        ("calls", |n| {
            format!("{}1{}", "Math.abs(".repeat(n), ")".repeat(n))
        }),
        ("arrays", |n| format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("ternaries", |n| {
            format!("{}1{}", "1 ? ".repeat(n), " : 0".repeat(n))
        }),
        ("blocks", |n| format!("{}1{}", "{".repeat(n), "}".repeat(n))),
        ("ifs", |n| format!("{}1", "if (1) ".repeat(n))),
        ("indexes", |n| format!("[1]{}", "[0]".repeat(n))),
        ("sums", |n| format!("1{}", " + 1".repeat(n))),
        ("assignments", |n| {
            format!("let a = 0; {}1", "a = ".repeat(n))
        }),
        ("members", |n| format!("thisProperty{}", ".value".repeat(n))),
    ];
    for (shape, build) in shapes {
        let deepest = (1..2000)
            .take_while(|&depth| compile(&build(depth)).is_ok())
            .last()
            .unwrap_or_else(|| panic!("{shape}: even depth 1 fails"));
        let refusal = compile(&build(deepest + 1)).err().unwrap_or_default();
        assert!(
            refusal.contains("nests more than"),
            "{shape} at {}: {refusal}",
            deepest + 1
        );
        let source = build(deepest);
        // The default stack of a spawned thread, which render threads use.
        let evaluated = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut scope = rotation();
                run(&source, &mut scope).map(|_| ()).err()
            })
            .unwrap_or_else(|err| panic!("{err}"))
            .join();
        assert!(evaluated.is_ok(), "{shape} at depth {deepest} panicked");
    }
}

#[test]
fn errors_state_the_line_and_column() {
    for (source, wanted) in [
        ("let a = 1\nfoo + a", "line 2, column 1: foo is not defined"),
        (
            "thisComp.layer(\"X\")",
            "line 1, column 15: thisComp.layer(\"Name\") is a layer",
        ),
        (
            "Math.nope(1)",
            "line 1, column 10: Math has no function nope",
        ),
        (
            "thisComp.depth",
            "line 1, column 9: thisComp has no property depth",
        ),
        (
            "wiggle(1)",
            "line 1, column 7: wiggle takes 2 to 5 arguments, not 1",
        ),
        ("thisLayer.scale.foo(1)", "a property has no method foo"),
        ("thisComp", "line 1, column 1: thisComp is not a value"),
        ("1 < \"a\"", "the number 1 < a string is not defined"),
        ("effect(\"Blur\")", "effect(\"Name\")(\"Parameter\")"),
    ] {
        let message = error(source, &mut rotation());
        assert!(message.contains(wanted), "{source}: {message}");
    }
    let message = compile("1 +").err().unwrap_or_default();
    assert!(message.starts_with("line 1, column 4:"), "{message}");
}

/// The pick whip inserts what [`super::bind::reference`] writes. Every
/// property the catalog and the transform table name, on this clip and on a
/// named clip whose name needs escaping, binds back to the input it was
/// written for. A reference the binder reads as another property, or a name
/// the lexer reads back changed, fails here.
#[test]
fn every_pick_whip_reference_binds_back_to_its_input() {
    let clips = [
        InputClip::This,
        InputClip::Named("Logo".into()),
        InputClip::Named("say \"hi\" \\ back\nslash\t\u{e9}".into()),
    ];
    let mut properties: Vec<InputProperty> = aria_models::clip_param_catalog()
        .iter()
        .map(|spec| InputProperty::Id(spec.id.to_string()))
        .chain(
            super::bind::TRANSFORM
                .iter()
                .map(|(_, id)| InputProperty::Id((*id).into())),
        )
        .collect();
    properties.push(InputProperty::Id("volume".into()));
    properties.push(InputProperty::Id("pan".into()));
    properties.push(InputProperty::Effect {
        effect: "Gaussian \"Blur\"".into(),
        param: "Radius".into(),
    });
    for clip in &clips {
        for property in &properties {
            let wanted = DriverInput {
                clip: clip.clone(),
                property: property.clone(),
            };
            let source = Expressions
                .reference(&wanted)
                .unwrap_or_else(|| panic!("no reference for {wanted:?}"));
            let program = compile(&source).unwrap_or_else(|err| panic!("{source}: {err}"));
            assert_eq!(program.inputs(), [wanted.clone()], "{source}");
        }
    }
    // A new driver starts at the property's own value.
    let template = Expressions.template();
    let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(12.0));
    assert_eq!(run(&template, &mut scope), Ok(ParamValue::Scalar(12.0)));
}

#[test]
fn the_plugin_registers_the_language_for_every_render_path() {
    let ctx = PluginContext::new();
    ctx.mount(&ExpressionsPlugin)
        .unwrap_or_else(|err| panic!("{err}"));
    let languages = ctx.driver_languages();
    assert_eq!(languages.len(), 1);
    assert_eq!(languages[0].spec.id, EXPRESSIONS_LANGUAGE);
    assert_eq!(&*languages[0].plugin_id, EXPRESSIONS_LANGUAGE);
    assert!(aria_render::driver_languages().contains(&EXPRESSIONS_LANGUAGE.to_string()));
    let program = aria_render::compile_driver(&DriverCode::new(EXPRESSIONS_LANGUAGE, "value * 2"))
        .unwrap_or_else(|err| panic!("{err}"));
    let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(3.0));
    assert_eq!(program.evaluate(&mut scope), Ok(ParamValue::Scalar(6.0)));
    let refused = aria_render::compile_driver(&DriverCode::new(EXPRESSIONS_LANGUAGE, "1 +"));
    assert!(refused.is_err());
    // The registry forwards the pick whip and the template through the
    // plugin's timing wrapper.
    let logo = input(InputClip::Named("Logo".into()), "position");
    assert_eq!(
        aria_render::driver_reference(EXPRESSIONS_LANGUAGE, &logo).as_deref(),
        Some("thisComp.layer(\"Logo\").transform.position")
    );
    assert_eq!(
        aria_render::driver_template(EXPRESSIONS_LANGUAGE).as_deref(),
        Some("value")
    );
    assert_eq!(aria_render::driver_template("nobody.language"), None);
}
