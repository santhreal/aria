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

use aria_models::{ClipParam, ParamValue};
use aria_render::{
    DriverClipInfo, DriverInput, DriverLanguage, DriverScope, InputClip, InputProperty,
};

use super::{Expressions, bind};

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

#[path = "tests_builtins.rs"]
mod builtins;
#[path = "tests_language.rs"]
mod language;
#[path = "tests_limits.rs"]
mod limits;
