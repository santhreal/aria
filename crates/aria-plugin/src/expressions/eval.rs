//! Evaluation of a bound program against a [`DriverScope`].
//!
//! Values are numbers, strings, booleans, `null`, arrays of numbers, and the
//! keyframes `key(i)` returns. An array is copied on assignment, so writing
//! one item changes that variable alone. A variable is visible from its first
//! assignment to the end of the program.

use std::collections::HashMap;
use std::f64::consts;

use aria_models::ParamValue;
use aria_render::DriverScope;

use super::Program;
use super::bind::Units;
use super::lex::Pos;
use super::parse::{Binary, Expr, PropRef, Stmt, Target, Unary};

/// Steps between two reads of the scope's clock.
const CLOCK_EVERY: u64 = 256;
/// Statements, loop turns, and calls one evaluation may take, so a program
/// ends under a scope whose clock never runs out.
pub(super) const MAX_STEPS: u64 = 20_000_000;
/// Longest string a program may build, in bytes.
pub(super) const MAX_STRING: usize = 64 * 1024;
/// Seconds either side of a time over which a velocity is measured.
const VELOCITY_SPAN: f64 = 1e-3;
/// Most octaves `wiggle` sums.
const MAX_OCTAVES: f64 = 16.0;
/// Seed of `noise`, which is the same function on every clip.
const NOISE_SEED: u64 = 0x6e6f_6973_6500;
/// The golden-ratio increment of the random sequence.
const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
/// Standard deviations from the mean to the edge of a `gaussRandom` range, so
/// nine draws in ten fall inside it.
const GAUSS_EDGE: f64 = 1.644_853_626_951_472_2;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    Array(Vec<f64>),
    Null,
    /// A keyframe of the driven property: its time, its value, and its index
    /// counted from 1.
    Key {
        time: f64,
        value: Box<Value>,
        index: usize,
    },
}

impl Value {
    fn describe(&self) -> String {
        match self {
            Self::Num(number) => format!("the number {}", number_text(*number)),
            Self::Bool(flag) => format!("the boolean {flag}"),
            Self::Str(_) => "a string".into(),
            Self::Array(items) => format!("an array of {} numbers", items.len()),
            Self::Null => "null".into(),
            Self::Key { .. } => "a keyframe".into(),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Num(number) => *number != 0.0 && !number.is_nan(),
            Self::Bool(flag) => *flag,
            Self::Str(text) => !text.is_empty(),
            Self::Array(_) | Self::Key { .. } => true,
            Self::Null => false,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Num(number) => number_text(*number),
            Self::Bool(flag) => flag.to_string(),
            Self::Str(text) => text.clone(),
            Self::Array(items) => items
                .iter()
                .map(|item| number_text(*item))
                .collect::<Vec<_>>()
                .join(","),
            Self::Null => "null".into(),
            Self::Key { value, .. } => value.text(),
        }
    }
}

/// A number as JavaScript prints it.
fn number_text(number: f64) -> String {
    if number.is_nan() {
        "NaN".into()
    } else if number.is_infinite() {
        String::from(if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        })
    } else if number == 0.0 {
        "0".into()
    } else {
        number.to_string()
    }
}

enum Flow {
    Next,
    Break(Pos),
    Continue(Pos),
    Return(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loop {
    Cycle,
    PingPong,
    Offset,
    Continue,
}

/// The methods a property answers, bare for the driven property or on a
/// property reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    ValueAt,
    VelocityAt,
    SpeedAt,
    Wiggle,
    LoopOut,
    LoopIn,
    Key,
}

impl Method {
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "valueAtTime" => Self::ValueAt,
            "velocityAtTime" => Self::VelocityAt,
            "speedAtTime" => Self::SpeedAt,
            "wiggle" => Self::Wiggle,
            "loopOut" => Self::LoopOut,
            "loopIn" => Self::LoopIn,
            "key" => Self::Key,
            _ => return None,
        })
    }
}

/// `program` evaluated against `scope`, as a value of the driven property's
/// kind.
pub(super) fn evaluate(
    program: &Program,
    scope: &mut dyn DriverScope,
) -> Result<ParamValue, String> {
    let own = scope.value();
    let [width, height] = scope.canvas();
    let random = mix(scope.seed() ^ mix(scope.time().to_bits()));
    let mut run = Run {
        program,
        own_units: Units::of_param(scope.property()),
        scope,
        vars: HashMap::new(),
        last: None,
        steps: 0,
        random,
        canvas: [f64::from(width), f64::from(height)],
    };
    let value = run.program()?;
    from_value(&value, own, run.own_units, run.canvas)
}

struct Run<'p, 's> {
    program: &'p Program,
    own_units: Units,
    scope: &'s mut dyn DriverScope,
    vars: HashMap<&'p str, Value>,
    /// The value of the last expression statement, which the program returns
    /// when it ends without `return`.
    last: Option<Value>,
    steps: u64,
    random: u64,
    canvas: [f64; 2],
}

impl<'p> Run<'p, '_> {
    fn program(&mut self) -> Result<Value, String> {
        let program = self.program;
        match self.block(&program.body)? {
            Flow::Return(value) => Ok(value),
            Flow::Next => self.last.take().ok_or_else(|| {
                "the driver ends without a value: end it with an expression, such as value"
                    .to_string()
            }),
            Flow::Break(pos) => Err(format!("{pos}: break is outside a loop")),
            Flow::Continue(pos) => Err(format!("{pos}: continue is outside a loop")),
        }
    }

    fn step(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err(format!("the driver took more than {MAX_STEPS} steps"));
        }
        if self.steps.is_multiple_of(CLOCK_EVERY) && self.scope.expired() {
            return Err("the driver ran past its time budget".into());
        }
        Ok(())
    }

    fn block(&mut self, body: &'p [Stmt]) -> Result<Flow, String> {
        for stmt in body {
            let flow = self.stmt(stmt)?;
            if !matches!(flow, Flow::Next) {
                return Ok(flow);
            }
        }
        Ok(Flow::Next)
    }

    fn stmt(&mut self, stmt: &'p Stmt) -> Result<Flow, String> {
        self.step()?;
        match stmt {
            Stmt::Let(name, value) => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => Value::Null,
                };
                self.vars.insert(name.as_str(), value);
            }
            Stmt::Expr(expr) => {
                let value = self.expr(expr)?;
                self.last = Some(value);
            }
            Stmt::If(cond, then, otherwise) => {
                let branch = if self.expr(cond)?.truthy() {
                    then
                } else {
                    otherwise
                };
                return self.block(branch);
            }
            Stmt::While(cond, body) => return self.looping(Some(cond), None, body),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(init) = init.as_deref() {
                    self.stmt(init)?;
                }
                return self.looping(cond.as_ref(), step.as_ref(), body);
            }
            Stmt::Return(value, _) => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => Value::Null,
                };
                return Ok(Flow::Return(value));
            }
            Stmt::Break(pos) => return Ok(Flow::Break(*pos)),
            Stmt::Continue(pos) => return Ok(Flow::Continue(*pos)),
            Stmt::Block(body) => return self.block(body),
        }
        Ok(Flow::Next)
    }

    fn looping(
        &mut self,
        cond: Option<&'p Expr>,
        step: Option<&'p Expr>,
        body: &'p [Stmt],
    ) -> Result<Flow, String> {
        loop {
            self.step()?;
            if let Some(cond) = cond
                && !self.expr(cond)?.truthy()
            {
                return Ok(Flow::Next);
            }
            match self.block(body)? {
                Flow::Break(_) => return Ok(Flow::Next),
                Flow::Return(value) => return Ok(Flow::Return(value)),
                Flow::Next | Flow::Continue(_) => {}
            }
            if let Some(step) = step {
                self.expr(step)?;
            }
        }
    }

    fn expr(&mut self, expr: &'p Expr) -> Result<Value, String> {
        match expr {
            Expr::Num(number) => Ok(Value::Num(*number)),
            Expr::Str(text) => Ok(Value::Str(text.clone())),
            Expr::Bool(flag) => Ok(Value::Bool(*flag)),
            Expr::Null => Ok(Value::Null),
            Expr::Array(items, pos) => {
                let mut numbers = Vec::with_capacity(items.len());
                for item in items {
                    match self.expr(item)? {
                        Value::Num(number) => numbers.push(number),
                        other => {
                            return Err(format!(
                                "{pos}: an array holds numbers, not {}",
                                other.describe()
                            ));
                        }
                    }
                }
                Ok(Value::Array(numbers))
            }
            Expr::Name(name, pos) => self.name(name, *pos),
            Expr::Member(base, name, pos) => self.member(base, name, *pos),
            Expr::Index(base, index, pos) => {
                let base = self.expr(base)?;
                let index = array_index(&self.expr(index)?, *pos)?;
                item(&base, index, *pos)
            }
            Expr::Call(callee, args, pos) => self.call(callee, args, *pos),
            Expr::Unary(op, operand, pos) => {
                let value = self.expr(operand)?;
                unary(*op, value, *pos)
            }
            Expr::Binary(op, left, right, pos) => {
                let left = self.expr(left)?;
                let right = self.expr(right)?;
                binary(*op, left, right, *pos)
            }
            Expr::And(left, right) => {
                let left = self.expr(left)?;
                if left.truthy() {
                    self.expr(right)
                } else {
                    Ok(left)
                }
            }
            Expr::Or(left, right) => {
                let left = self.expr(left)?;
                if left.truthy() {
                    Ok(left)
                } else {
                    self.expr(right)
                }
            }
            Expr::Cond(cond, then, otherwise) => {
                if self.expr(cond)?.truthy() {
                    self.expr(then)
                } else {
                    self.expr(otherwise)
                }
            }
            Expr::Assign(target, op, value, pos) => {
                let place = self.place(target, *pos)?;
                let value = self.expr(value)?;
                let value = match op {
                    Some(op) => binary(*op, self.get(place, *pos)?, value, *pos)?,
                    None => value,
                };
                self.set(place, value.clone(), *pos)?;
                Ok(value)
            }
            Expr::Step {
                target,
                delta,
                prefix,
                pos,
            } => {
                let place = self.place(target, *pos)?;
                let Value::Num(number) = self.get(place, *pos)? else {
                    return Err(format!("{pos}: ++ and -- change a number"));
                };
                self.set(place, Value::Num(number + delta), *pos)?;
                Ok(Value::Num(if *prefix { number + delta } else { number }))
            }
            Expr::Property(property, pos) => self.read(*property, None, *pos),
            Expr::Layer(index, attribute, pos) => self.layer(*index, attribute, *pos),
        }
    }

    fn place(&mut self, target: &'p Target, pos: Pos) -> Result<(&'p str, Option<usize>), String> {
        match target {
            Target::Name(name) => Ok((name.as_str(), None)),
            Target::Index(name, index) => {
                let index = array_index(&self.expr(index)?, pos)?;
                Ok((name.as_str(), Some(index)))
            }
        }
    }

    fn get(&self, (name, index): (&str, Option<usize>), pos: Pos) -> Result<Value, String> {
        let Some(value) = self.vars.get(name) else {
            return Err(format!("{pos}: {name} is not defined; declare it with let"));
        };
        match index {
            None => Ok(value.clone()),
            Some(index) => item(value, index, pos),
        }
    }

    fn set(
        &mut self,
        (name, index): (&'p str, Option<usize>),
        value: Value,
        pos: Pos,
    ) -> Result<(), String> {
        let Some(index) = index else {
            self.vars.insert(name, value);
            return Ok(());
        };
        let Value::Num(number) = value else {
            return Err(format!(
                "{pos}: an array holds numbers, not {}",
                value.describe()
            ));
        };
        match self.vars.get_mut(name) {
            Some(Value::Array(items)) => {
                let len = items.len();
                let slot = items.get_mut(index).ok_or_else(|| {
                    format!("{pos}: index {index} is past the end of {name}, which holds {len}")
                })?;
                *slot = number;
                Ok(())
            }
            Some(other) => Err(format!(
                "{pos}: {name} is {}, not an array",
                other.describe()
            )),
            None => Err(format!("{pos}: {name} is not defined; declare it with let")),
        }
    }

    fn is_global(&self, name: &str) -> bool {
        !self.vars.contains_key(name)
    }

    fn name(&mut self, name: &'p str, pos: Pos) -> Result<Value, String> {
        if let Some(value) = self.vars.get(name) {
            return Ok(value.clone());
        }
        match name {
            "time" => Ok(Value::Num(self.scope.time())),
            "value" => self.read(PropRef::Own, None, pos),
            "numKeys" => Ok(Value::Num(self.scope.key_count() as f64)),
            "NaN" => Ok(Value::Num(f64::NAN)),
            "Infinity" => Ok(Value::Num(f64::INFINITY)),
            "thisComp" | "thisLayer" | "transform" | "audio" | "Math" => Err(format!(
                "{pos}: {name} is not a value; read one of its properties"
            )),
            _ => Err(format!("{pos}: {name} is not defined")),
        }
    }

    fn member(&mut self, base: &'p Expr, name: &str, pos: Pos) -> Result<Value, String> {
        if let Expr::Name(object, _) = base
            && self.is_global(object)
        {
            match object.as_str() {
                "thisComp" => return self.comp(name, pos),
                "Math" => return math_constant(name, pos),
                "thisLayer" => {
                    return Err(format!(
                        "{pos}: thisLayer has no property {name}; it has name, inPoint, \
                         outPoint, the groups transform and audio, effect(\"Name\"), and \
                         property(\"id\")"
                    ));
                }
                _ => {}
            }
        }
        if let Expr::Property(property, _) = base {
            return match name {
                "value" => self.read(*property, None, pos),
                "numKeys" => self
                    .own_only(*property, "numKeys", pos)
                    .map(|()| Value::Num(self.scope.key_count() as f64)),
                "velocity" => {
                    let now = self.scope.time();
                    self.velocity(*property, now, pos)
                }
                "speed" => {
                    let now = self.scope.time();
                    Ok(Value::Num(magnitude(
                        &self.velocity(*property, now, pos)?,
                        pos,
                    )?))
                }
                _ => Err(format!(
                    "{pos}: a property has value, numKeys, velocity, and speed, not {name}"
                )),
            };
        }
        let value = self.expr(base)?;
        match (&value, name) {
            (Value::Array(items), "length") => Ok(Value::Num(items.len() as f64)),
            (Value::Str(text), "length") => Ok(Value::Num(text.chars().count() as f64)),
            (Value::Key { time, .. }, "time") => Ok(Value::Num(*time)),
            (Value::Key { value, .. }, "value") => Ok(value.as_ref().clone()),
            (Value::Key { index, .. }, "index") => Ok(Value::Num(*index as f64)),
            _ => Err(format!(
                "{pos}: {} has no property {name}",
                value.describe()
            )),
        }
    }

    fn comp(&self, name: &str, pos: Pos) -> Result<Value, String> {
        Ok(Value::Num(match name {
            "width" => self.canvas[0],
            "height" => self.canvas[1],
            "frameDuration" => 1.0 / self.scope.frame_rate(),
            "duration" => self.scope.duration(),
            _ => {
                return Err(format!(
                    "{pos}: thisComp has no property {name}; it has width, height, \
                     frameDuration, duration, and layer(\"Name\")"
                ));
            }
        }))
    }

    fn layer(&mut self, index: usize, attribute: &str, pos: Pos) -> Result<Value, String> {
        let program = self.program;
        let Some(clip) = program.clips.get(index) else {
            return Err(format!("{pos}: layer {index} is not bound"));
        };
        let info = self
            .scope
            .clip_info(clip)
            .map_err(|error| format!("{pos}: {error}"))?;
        match attribute {
            "name" => Ok(Value::Str(info.name)),
            "inPoint" => Ok(Value::Num(info.in_point)),
            "outPoint" => Ok(Value::Num(info.out_point)),
            _ => Err(format!("{pos}: a layer has no property {attribute}")),
        }
    }

    /// A property's value at `at`, or at the scope's time, in the units the
    /// expression reads.
    fn read(&mut self, property: PropRef, at: Option<f64>, pos: Pos) -> Result<Value, String> {
        let (value, units) = match property {
            PropRef::Own => {
                let value = match at {
                    Some(seconds) => self.scope.value_at(seconds),
                    None => self.scope.value(),
                };
                (value, self.own_units)
            }
            PropRef::Input(index) => {
                let value = match at {
                    Some(seconds) => self.scope.input_at(index, seconds),
                    None => self.scope.input(index),
                }
                .map_err(|error| format!("{pos}: {error}"))?;
                let units = self.program.units.get(index).copied();
                (value, units.unwrap_or(Units::Raw))
            }
        };
        Ok(to_value(value, units, self.canvas))
    }

    fn velocity(&mut self, property: PropRef, at: f64, pos: Pos) -> Result<Value, String> {
        let before = self.read(property, Some(at - VELOCITY_SPAN), pos)?;
        let after = self.read(property, Some(at + VELOCITY_SPAN), pos)?;
        let change = binary(Binary::Sub, after, before, pos)?;
        binary(Binary::Div, change, Value::Num(2.0 * VELOCITY_SPAN), pos)
    }

    fn own_only(&self, property: PropRef, what: &str, pos: Pos) -> Result<(), String> {
        if property == PropRef::Own {
            return Ok(());
        }
        Err(format!(
            "{pos}: {what} reads the keyframes of the property the driver is on"
        ))
    }

    fn args(&mut self, args: &'p [Expr]) -> Result<Vec<Value>, String> {
        args.iter().map(|arg| self.expr(arg)).collect()
    }

    fn call(&mut self, callee: &'p Expr, args: &'p [Expr], pos: Pos) -> Result<Value, String> {
        self.step()?;
        match callee {
            Expr::Name(name, _) if self.is_global(name) => {
                if let Some(method) = Method::of(name) {
                    let values = self.args(args)?;
                    return self.method(PropRef::Own, method, name, &values, pos);
                }
                let values = self.args(args)?;
                self.function(name, &values, pos)
            }
            Expr::Member(base, name, _) => match base.as_ref() {
                Expr::Name(object, _) if object == "Math" && self.is_global(object) => {
                    let values = self.args(args)?;
                    self.math(name, &values, pos)
                }
                Expr::Name(object, _)
                    if object == "thisComp" && name == "layer" && self.is_global(object) =>
                {
                    Err(format!(
                        "{pos}: thisComp.layer(\"Name\") is a layer; read one of its \
                         properties, such as thisComp.layer(\"Name\").transform.position"
                    ))
                }
                Expr::Property(property, _) => {
                    let Some(method) = Method::of(name) else {
                        return Err(format!(
                            "{pos}: a property has no method {name}; it has valueAtTime, \
                             velocityAtTime, speedAtTime, wiggle, loopIn, loopOut, and key"
                        ));
                    };
                    let values = self.args(args)?;
                    self.method(*property, method, name, &values, pos)
                }
                _ => Err(format!("{pos}: {name} is not a function")),
            },
            _ => Err(format!("{pos}: only a named function can be called")),
        }
    }

    fn method(
        &mut self,
        property: PropRef,
        method: Method,
        name: &str,
        values: &[Value],
        pos: Pos,
    ) -> Result<Value, String> {
        match method {
            Method::ValueAt => {
                arity(name, values, 1..=1, pos)?;
                let at = number(name, &values[0], pos)?;
                self.read(property, Some(at), pos)
            }
            Method::VelocityAt => {
                arity(name, values, 1..=1, pos)?;
                let at = number(name, &values[0], pos)?;
                self.velocity(property, at, pos)
            }
            Method::SpeedAt => {
                arity(name, values, 1..=1, pos)?;
                let at = number(name, &values[0], pos)?;
                Ok(Value::Num(magnitude(
                    &self.velocity(property, at, pos)?,
                    pos,
                )?))
            }
            Method::Wiggle => self.wiggle(property, values, pos),
            Method::LoopOut => self.looped(property, true, values, pos),
            Method::LoopIn => self.looped(property, false, values, pos),
            Method::Key => self.key(property, values, pos),
        }
    }

    fn wiggle(&mut self, property: PropRef, values: &[Value], pos: Pos) -> Result<Value, String> {
        arity("wiggle", values, 2..=5, pos)?;
        let frequency = number("wiggle", &values[0], pos)?;
        let amplitude = number("wiggle", &values[1], pos)?;
        // `f64::max` drops a NaN, so NaN octaves are one.
        let octaves = optional_number("wiggle", values, 2, 1.0, pos)?
            .max(1.0)
            .min(MAX_OCTAVES) as u32;
        let falloff = optional_number("wiggle", values, 3, 0.5, pos)?;
        let at = values
            .get(4)
            .map(|value| number("wiggle", value, pos))
            .transpose()?;
        let seconds = at.unwrap_or_else(|| self.scope.time());
        let base = self.read(property, at, pos)?;
        let salt = match property {
            PropRef::Own => 0,
            PropRef::Input(index) => index as u64 + 1,
        };
        let seed = mix(self.scope.seed() ^ mix(salt));
        let shake = |axis: u64| {
            amplitude * fractal(mix(seed ^ axis), seconds * frequency, octaves, falloff)
        };
        match base {
            Value::Num(number) => Ok(Value::Num(number + shake(0))),
            Value::Array(items) => Ok(Value::Array(
                items
                    .iter()
                    .zip(0u64..)
                    .map(|(item, axis)| item + shake(axis))
                    .collect(),
            )),
            other => Err(format!(
                "{pos}: wiggle moves a number or an array, not {}",
                other.describe()
            )),
        }
    }

    fn looped(
        &mut self,
        property: PropRef,
        out: bool,
        values: &[Value],
        pos: Pos,
    ) -> Result<Value, String> {
        let name = if out { "loopOut" } else { "loopIn" };
        self.own_only(property, name, pos)?;
        arity(name, values, 0..=2, pos)?;
        let kind = match values.first() {
            None => Loop::Cycle,
            Some(Value::Str(kind)) => match kind.as_str() {
                "cycle" => Loop::Cycle,
                "pingpong" => Loop::PingPong,
                "offset" => Loop::Offset,
                "continue" => Loop::Continue,
                _ => {
                    return Err(format!(
                        "{pos}: {name} has no type {kind:?}; use \"cycle\", \"pingpong\", \
                         \"offset\", or \"continue\""
                    ));
                }
            },
            Some(other) => {
                return Err(format!(
                    "{pos}: the type of {name} is a string, not {}",
                    other.describe()
                ));
            }
        };
        let count = optional_number(name, values, 1, 0.0, pos)?;
        let keys = self.scope.key_count();
        let now = self.scope.time();
        if keys < 2 {
            return self.read(property, None, pos);
        }
        let span = if count >= 1.0 && count < keys as f64 {
            count as usize
        } else {
            keys - 1
        };
        let (first, last) = if out {
            (keys - 1 - span, keys - 1)
        } else {
            (0, span)
        };
        let (Some(start), Some(end)) = (self.scope.key_time(first), self.scope.key_time(last))
        else {
            return self.read(property, None, pos);
        };
        let length = end - start;
        let inside = if out { now <= end } else { now >= start };
        if inside || length <= 0.0 {
            return self.read(property, None, pos);
        }
        if kind == Loop::Continue {
            let edge = if out { end } else { start };
            let near = if out {
                edge - VELOCITY_SPAN
            } else {
                edge + VELOCITY_SPAN
            };
            let at_edge = self.read(property, Some(edge), pos)?;
            let at_near = self.read(property, Some(near), pos)?;
            let change = binary(Binary::Sub, at_edge.clone(), at_near, pos)?;
            let slope = binary(Binary::Div, change, Value::Num(edge - near), pos)?;
            let travel = binary(Binary::Mul, slope, Value::Num(now - edge), pos)?;
            return binary(Binary::Add, at_edge, travel, pos);
        }
        let cycles = ((now - start) / length).floor();
        let local = ((now - start) - cycles * length).max(0.0).min(length);
        let mirrored = kind == Loop::PingPong && cycles.rem_euclid(2.0) == 1.0;
        let sample = if mirrored { end - local } else { start + local };
        let value = self.read(property, Some(sample), pos)?;
        if kind != Loop::Offset {
            return Ok(value);
        }
        let first_value = self.read(property, Some(start), pos)?;
        let last_value = self.read(property, Some(end), pos)?;
        let change = binary(Binary::Sub, last_value, first_value, pos)?;
        let shift = binary(Binary::Mul, change, Value::Num(cycles), pos)?;
        binary(Binary::Add, value, shift, pos)
    }

    fn key(&mut self, property: PropRef, values: &[Value], pos: Pos) -> Result<Value, String> {
        self.own_only(property, "key", pos)?;
        arity("key", values, 1..=1, pos)?;
        let wanted = number("key", &values[0], pos)?;
        let keys = self.scope.key_count();
        if !(wanted >= 1.0 && wanted.fract() == 0.0 && wanted <= keys as f64) {
            return Err(format!(
                "{pos}: key({}) is out of range; the property has {keys} keyframes, \
                 counted from 1",
                number_text(wanted)
            ));
        }
        let index = wanted as usize;
        let Some(time) = self.scope.key_time(index - 1) else {
            return Err(format!("{pos}: key({index}) has no time"));
        };
        let value = self.read(property, Some(time), pos)?;
        Ok(Value::Key {
            time,
            value: Box::new(value),
            index,
        })
    }

    fn uniform(&mut self) -> f64 {
        self.random = self.random.wrapping_add(GOLDEN);
        unit(mix(self.random))
    }

    fn gaussian(&mut self) -> f64 {
        let radius = (-2.0 * (1.0 - self.uniform()).ln()).sqrt();
        let angle = consts::TAU * self.uniform();
        0.5 + radius * angle.cos() / (2.0 * GAUSS_EDGE)
    }

    /// `random` and `gaussRandom`: a draw between 0 and 1, 0 and a maximum,
    /// or a minimum and a maximum, per item for arrays.
    fn draw(
        &mut self,
        name: &str,
        values: &[Value],
        gauss: bool,
        pos: Pos,
    ) -> Result<Value, String> {
        arity(name, values, 0..=2, pos)?;
        let (low, high) = match values {
            [] => (Value::Num(0.0), Value::Num(1.0)),
            [high] => (zero_like(high), high.clone()),
            [low, high, ..] => (low.clone(), high.clone()),
        };
        let sample = |run: &mut Self, low: f64, high: f64| {
            let unit = if gauss { run.gaussian() } else { run.uniform() };
            low + unit * (high - low)
        };
        match (&low, &high) {
            (Value::Num(low), Value::Num(high)) => Ok(Value::Num(sample(self, *low, *high))),
            (Value::Array(low), Value::Array(high)) if low.len() == high.len() => Ok(Value::Array(
                low.iter()
                    .zip(high)
                    .map(|(low, high)| sample(self, *low, *high))
                    .collect(),
            )),
            _ => Err(format!(
                "{pos}: {name} takes numbers, or arrays of one length, not {} and {}",
                low.describe(),
                high.describe()
            )),
        }
    }

    fn seed_random(&mut self, values: &[Value], pos: Pos) -> Result<Value, String> {
        arity("seedRandom", values, 1..=2, pos)?;
        let seed = number("seedRandom", &values[0], pos)?;
        let timeless = values.get(1).is_some_and(Value::truthy);
        let time = if timeless {
            0
        } else {
            mix(self.scope.time().to_bits())
        };
        self.random = mix(self.scope.seed() ^ mix(seed.to_bits()) ^ time);
        Ok(Value::Null)
    }

    fn function(&mut self, name: &str, values: &[Value], pos: Pos) -> Result<Value, String> {
        match name {
            "linear" => interpolate(name, values, |u| u, pos),
            "ease" => interpolate(name, values, |u| u * u * (3.0 - 2.0 * u), pos),
            "easeIn" => interpolate(name, values, |u| u * u * (2.0 - u), pos),
            "easeOut" => interpolate(name, values, |u| u * (1.0 + u - u * u), pos),
            "clamp" => clamp(values, pos),
            "length" => match values {
                [vector] => Ok(Value::Num(magnitude(vector, pos)?)),
                [from, to] => {
                    let between = binary(Binary::Sub, from.clone(), to.clone(), pos)?;
                    Ok(Value::Num(magnitude(&between, pos)?))
                }
                _ => arity(name, values, 1..=2, pos).map(|()| Value::Null),
            },
            "normalize" => {
                arity(name, values, 1..=1, pos)?;
                let length = magnitude(&values[0], pos)?;
                if length == 0.0 {
                    return Ok(values[0].clone());
                }
                binary(Binary::Div, values[0].clone(), Value::Num(length), pos)
            }
            "dot" => {
                arity(name, values, 2..=2, pos)?;
                let (left, right) = (
                    vector(name, &values[0], pos)?,
                    vector(name, &values[1], pos)?,
                );
                if left.len() != right.len() {
                    return Err(format!("{pos}: dot takes two arrays of one length"));
                }
                Ok(Value::Num(left.iter().zip(right).map(|(a, b)| a * b).sum()))
            }
            "cross" => {
                arity(name, values, 2..=2, pos)?;
                let a = space(name, &values[0], pos)?;
                let b = space(name, &values[1], pos)?;
                Ok(Value::Array(vec![
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ]))
            }
            "add" | "sub" | "mul" | "div" => {
                arity(name, values, 2..=2, pos)?;
                let op = match name {
                    "add" => Binary::Add,
                    "sub" => Binary::Sub,
                    "mul" => Binary::Mul,
                    _ => Binary::Div,
                };
                binary(op, values[0].clone(), values[1].clone(), pos)
            }
            "random" => self.draw(name, values, false, pos),
            "gaussRandom" => self.draw(name, values, true, pos),
            "seedRandom" => self.seed_random(values, pos),
            "noise" => {
                arity(name, values, 1..=1, pos)?;
                noise_of(&values[0], pos)
            }
            "degreesToRadians" => map_number(name, values, f64::to_radians, pos),
            "radiansToDegrees" => map_number(name, values, f64::to_degrees, pos),
            "timeToFrames" => {
                arity(name, values, 0..=3, pos)?;
                let seconds = optional_number(name, values, 0, self.scope.time(), pos)?;
                let rate = optional_number(name, values, 1, self.scope.frame_rate(), pos)?;
                Ok(Value::Num((seconds * rate + 1e-6).floor()))
            }
            "framesToTime" => {
                arity(name, values, 1..=2, pos)?;
                let frames = number(name, &values[0], pos)?;
                let rate = optional_number(name, values, 1, self.scope.frame_rate(), pos)?;
                Ok(Value::Num(frames / rate))
            }
            "hslToRgb" => colour(name, values, hsl_to_rgb, pos),
            "rgbToHsl" => colour(name, values, rgb_to_hsl, pos),
            "effect" => Err(format!(
                "{pos}: effect(\"Name\") reads one parameter: effect(\"Name\")(\"Parameter\")"
            )),
            _ => Err(format!("{pos}: {name} is not a function")),
        }
    }

    fn math(&mut self, name: &str, values: &[Value], pos: Pos) -> Result<Value, String> {
        let one = |apply: fn(f64) -> f64| map_number(&format!("Math.{name}"), values, apply, pos);
        let two = |apply: fn(f64, f64) -> f64| {
            let label = format!("Math.{name}");
            arity(&label, values, 2..=2, pos)?;
            let left = number(&label, &values[0], pos)?;
            let right = number(&label, &values[1], pos)?;
            Ok(Value::Num(apply(left, right)))
        };
        let fold = |start: f64, apply: fn(f64, f64) -> f64| {
            let label = format!("Math.{name}");
            values
                .iter()
                .try_fold(start, |total, value| {
                    Ok::<f64, String>(apply(total, number(&label, value, pos)?))
                })
                .map(Value::Num)
        };
        match name {
            "abs" => one(f64::abs),
            "acos" => one(f64::acos),
            "asin" => one(f64::asin),
            "atan" => one(f64::atan),
            "cbrt" => one(f64::cbrt),
            "ceil" => one(f64::ceil),
            "cos" => one(f64::cos),
            "cosh" => one(f64::cosh),
            "exp" => one(f64::exp),
            "floor" => one(f64::floor),
            "log" => one(f64::ln),
            "log10" => one(f64::log10),
            "log2" => one(f64::log2),
            "round" => one(|x| (x + 0.5).floor()),
            "sign" => one(|x| {
                if x == 0.0 || x.is_nan() {
                    x
                } else {
                    x.signum()
                }
            }),
            "sin" => one(f64::sin),
            "sinh" => one(f64::sinh),
            "sqrt" => one(f64::sqrt),
            "tan" => one(f64::tan),
            "tanh" => one(f64::tanh),
            "trunc" => one(f64::trunc),
            "atan2" => two(f64::atan2),
            "pow" => two(f64::powf),
            "max" => fold(f64::NEG_INFINITY, f64::max),
            "min" => fold(f64::INFINITY, f64::min),
            "hypot" => fold(0.0, f64::hypot),
            "random" => {
                arity("Math.random", values, 0..=0, pos)?;
                Ok(Value::Num(self.uniform()))
            }
            _ => Err(format!("{pos}: Math has no function {name}")),
        }
    }
}

/// A value read from the model, in the units the expression reads.
fn to_value(value: ParamValue, units: Units, [width, height]: [f64; 2]) -> Value {
    match value {
        ParamValue::Scalar(number) => Value::Num(match units {
            Units::Percent => f64::from(number) * 100.0,
            Units::Pixels | Units::Raw => f64::from(number),
        }),
        ParamValue::Vec2([x, y]) => {
            let (x, y) = (f64::from(x), f64::from(y));
            Value::Array(match units {
                Units::Pixels => vec![(x + 0.5) * width, (y + 0.5) * height],
                Units::Percent => vec![x * 100.0, y * 100.0],
                Units::Raw => vec![x, y],
            })
        }
        ParamValue::Vec3([x, y, z]) => Value::Array(vec![f64::from(x), f64::from(y), f64::from(z)]),
        ParamValue::Color(bytes) => {
            Value::Array(bytes.iter().map(|byte| f64::from(*byte) / 255.0).collect())
        }
        ParamValue::Rect(rect) => Value::Array(rect.iter().map(|item| f64::from(*item)).collect()),
    }
}

/// The program's result as a value of `like`'s kind, in model units.
fn from_value(
    value: &Value,
    like: ParamValue,
    units: Units,
    [width, height]: [f64; 2],
) -> Result<ParamValue, String> {
    let finite = |numbers: &[f64]| -> Result<(), String> {
        match numbers.iter().find(|number| !number.is_finite()) {
            Some(number) => Err(format!(
                "the driver returned {}, which is not a finite number",
                number_text(*number)
            )),
            None => Ok(()),
        }
    };
    let narrow = |number: f64| -> Result<f32, String> {
        let narrowed = number as f32;
        if narrowed.is_finite() {
            Ok(narrowed)
        } else {
            Err(format!(
                "the driver returned {}, which is out of range",
                number_text(number)
            ))
        }
    };
    match (like, value) {
        (ParamValue::Scalar(_), Value::Num(number)) => {
            finite(&[*number])?;
            let number = match units {
                Units::Percent => number / 100.0,
                Units::Pixels | Units::Raw => *number,
            };
            Ok(ParamValue::Scalar(narrow(number)?))
        }
        (ParamValue::Vec2(_), Value::Array(items)) if items.len() == 2 => {
            finite(items)?;
            let (x, y) = match units {
                Units::Pixels => (items[0] / width - 0.5, items[1] / height - 0.5),
                Units::Percent => (items[0] / 100.0, items[1] / 100.0),
                Units::Raw => (items[0], items[1]),
            };
            Ok(ParamValue::Vec2([narrow(x)?, narrow(y)?]))
        }
        (ParamValue::Vec3(_), Value::Array(items)) if items.len() == 3 => {
            finite(items)?;
            Ok(ParamValue::Vec3([
                narrow(items[0])?,
                narrow(items[1])?,
                narrow(items[2])?,
            ]))
        }
        (ParamValue::Color(_), Value::Array(items)) if matches!(items.len(), 3 | 4) => {
            finite(items)?;
            let byte = |index: usize| {
                let channel = items.get(index).copied().unwrap_or(1.0);
                (channel.clamp(0.0, 1.0) * 255.0).round() as u8
            };
            Ok(ParamValue::Color([byte(0), byte(1), byte(2), byte(3)]))
        }
        (ParamValue::Rect(_), Value::Array(items)) if items.len() == 4 => {
            finite(items)?;
            Ok(ParamValue::Rect([
                narrow(items[0])?,
                narrow(items[1])?,
                narrow(items[2])?,
                narrow(items[3])?,
            ]))
        }
        (like, value) => Err(format!(
            "the driver returned {}; the property takes {}",
            value.describe(),
            match like {
                ParamValue::Scalar(_) => "a number",
                ParamValue::Vec2(_) => "an array of 2 numbers",
                ParamValue::Vec3(_) => "an array of 3 numbers",
                ParamValue::Color(_) => "an array of 3 or 4 numbers from 0 to 1",
                ParamValue::Rect(_) => "an array of 4 numbers",
            }
        )),
    }
}

fn arity(
    name: &str,
    values: &[Value],
    range: std::ops::RangeInclusive<usize>,
    pos: Pos,
) -> Result<(), String> {
    if range.contains(&values.len()) {
        return Ok(());
    }
    let (low, high) = (*range.start(), *range.end());
    let wanted = if low == high {
        low.to_string()
    } else {
        format!("{low} to {high}")
    };
    Err(format!(
        "{pos}: {name} takes {wanted} arguments, not {}",
        values.len()
    ))
}

fn number(name: &str, value: &Value, pos: Pos) -> Result<f64, String> {
    match value {
        Value::Num(number) => Ok(*number),
        other => Err(format!(
            "{pos}: {name} takes a number here, not {}",
            other.describe()
        )),
    }
}

fn optional_number(
    name: &str,
    values: &[Value],
    index: usize,
    default: f64,
    pos: Pos,
) -> Result<f64, String> {
    values
        .get(index)
        .map_or(Ok(default), |value| number(name, value, pos))
}

fn vector<'v>(name: &str, value: &'v Value, pos: Pos) -> Result<&'v [f64], String> {
    match value {
        Value::Array(items) => Ok(items),
        other => Err(format!(
            "{pos}: {name} takes arrays, not {}",
            other.describe()
        )),
    }
}

/// A 2 or 3 item array as a point in space, with z 0 for 2 items.
fn space(name: &str, value: &Value, pos: Pos) -> Result<[f64; 3], String> {
    match vector(name, value, pos)? {
        [x, y] => Ok([*x, *y, 0.0]),
        [x, y, z] => Ok([*x, *y, *z]),
        other => Err(format!(
            "{pos}: {name} takes arrays of 2 or 3 numbers, not {}",
            other.len()
        )),
    }
}

fn map_number(
    name: &str,
    values: &[Value],
    apply: fn(f64) -> f64,
    pos: Pos,
) -> Result<Value, String> {
    arity(name, values, 1..=1, pos)?;
    Ok(Value::Num(apply(number(name, &values[0], pos)?)))
}

fn magnitude(value: &Value, pos: Pos) -> Result<f64, String> {
    match value {
        Value::Num(number) => Ok(number.abs()),
        Value::Array(items) => Ok(items.iter().map(|item| item * item).sum::<f64>().sqrt()),
        other => Err(format!("{pos}: {} has no length", other.describe())),
    }
}

fn zero_like(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(vec![0.0; items.len()]),
        _ => Value::Num(0.0),
    }
}

fn array_index(index: &Value, pos: Pos) -> Result<usize, String> {
    match index {
        Value::Num(number) if *number >= 0.0 && number.fract() == 0.0 && *number < 1e15 => {
            Ok(*number as usize)
        }
        other => Err(format!(
            "{pos}: an index is a whole number from 0, not {}",
            other.describe()
        )),
    }
}

fn item(value: &Value, index: usize, pos: Pos) -> Result<Value, String> {
    match value {
        Value::Array(items) => items
            .get(index)
            .map(|item| Value::Num(*item))
            .ok_or_else(|| {
                format!(
                    "{pos}: index {index} is past the end of an array of {}",
                    items.len()
                )
            }),
        Value::Str(text) => text
            .chars()
            .nth(index)
            .map(|item| Value::Str(item.to_string()))
            .ok_or_else(|| format!("{pos}: index {index} is past the end of the string")),
        other => Err(format!("{pos}: {} has no items", other.describe())),
    }
}

fn math_constant(name: &str, pos: Pos) -> Result<Value, String> {
    Ok(Value::Num(match name {
        "PI" => consts::PI,
        "E" => consts::E,
        "LN2" => consts::LN_2,
        "LN10" => consts::LN_10,
        "LOG2E" => consts::LOG2_E,
        "LOG10E" => consts::LOG10_E,
        "SQRT2" => consts::SQRT_2,
        "SQRT1_2" => consts::FRAC_1_SQRT_2,
        _ => return Err(format!("{pos}: Math has no constant {name}")),
    }))
}

fn interpolate(
    name: &str,
    values: &[Value],
    curve: fn(f64) -> f64,
    pos: Pos,
) -> Result<Value, String> {
    let (at, low, high, from, to) = match values {
        [at, from, to] => (number(name, at, pos)?, 0.0, 1.0, from, to),
        [at, low, high, from, to] => (
            number(name, at, pos)?,
            number(name, low, pos)?,
            number(name, high, pos)?,
            from,
            to,
        ),
        _ => {
            return Err(format!(
                "{pos}: {name} takes (t, value1, value2) or (t, tMin, tMax, value1, value2)"
            ));
        }
    };
    let progress = if high == low {
        if at >= high { 1.0 } else { 0.0 }
    } else {
        ((at - low) / (high - low)).clamp(0.0, 1.0)
    };
    let amount = curve(progress);
    match (from, to) {
        (Value::Num(from), Value::Num(to)) => Ok(Value::Num(from + (to - from) * amount)),
        (Value::Array(from), Value::Array(to)) if from.len() == to.len() => Ok(Value::Array(
            from.iter()
                .zip(to)
                .map(|(from, to)| from + (to - from) * amount)
                .collect(),
        )),
        _ => Err(format!(
            "{pos}: {name} blends two numbers or two arrays of one length, not {} and {}",
            from.describe(),
            to.describe()
        )),
    }
}

fn clamp(values: &[Value], pos: Pos) -> Result<Value, String> {
    let [value, low, high] = values else {
        return arity("clamp", values, 3..=3, pos).map(|()| Value::Null);
    };
    let bound = |bound: &Value, index: usize| -> Result<f64, String> {
        match bound {
            Value::Num(number) => Ok(*number),
            Value::Array(items) => items
                .get(index)
                .copied()
                .ok_or_else(|| format!("{pos}: a clamp bound has fewer items than the value")),
            other => Err(format!(
                "{pos}: clamp takes number bounds, not {}",
                other.describe()
            )),
        }
    };
    match value {
        Value::Num(number) => Ok(Value::Num(number.max(bound(low, 0)?).min(bound(high, 0)?))),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| Ok(item.max(bound(low, index)?).min(bound(high, index)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(Value::Array),
        other => Err(format!(
            "{pos}: clamp takes a number or an array, not {}",
            other.describe()
        )),
    }
}

fn noise_of(value: &Value, pos: Pos) -> Result<Value, String> {
    match value {
        Value::Num(number) => Ok(Value::Num(noise(NOISE_SEED, *number))),
        Value::Array(items) if items.is_empty() => Ok(Value::Num(0.0)),
        Value::Array(items) => {
            let sum: f64 = items
                .iter()
                .zip(0u64..)
                .map(|(item, axis)| noise(mix(NOISE_SEED ^ axis), *item))
                .sum();
            Ok(Value::Num(sum / items.len() as f64))
        }
        other => Err(format!(
            "{pos}: noise takes a number or an array, not {}",
            other.describe()
        )),
    }
}

fn colour(
    name: &str,
    values: &[Value],
    convert: fn([f64; 3]) -> [f64; 3],
    pos: Pos,
) -> Result<Value, String> {
    arity(name, values, 1..=1, pos)?;
    let items = vector(name, &values[0], pos)?;
    let [a, b, c, rest @ ..] = items else {
        return Err(format!("{pos}: {name} takes an array of 3 or 4 numbers"));
    };
    if rest.len() > 1 {
        return Err(format!("{pos}: {name} takes an array of 3 or 4 numbers"));
    }
    let mut converted = convert([*a, *b, *c]).to_vec();
    converted.extend_from_slice(rest);
    Ok(Value::Array(converted))
}

fn hsl_to_rgb([hue, saturation, lightness]: [f64; 3]) -> [f64; 3] {
    if saturation == 0.0 {
        return [lightness; 3];
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    let channel = |offset: f64| {
        let t = (hue + offset).rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(1.0 / 3.0), channel(0.0), channel(-1.0 / 3.0)]
}

fn rgb_to_hsl([red, green, blue]: [f64; 3]) -> [f64; 3] {
    let high = red.max(green).max(blue);
    let low = red.min(green).min(blue);
    let lightness = (high + low) / 2.0;
    let range = high - low;
    if range == 0.0 {
        return [0.0, 0.0, lightness];
    }
    let saturation = if lightness > 0.5 {
        range / (2.0 - high - low)
    } else {
        range / (high + low)
    };
    let hue = if high == red {
        (green - blue) / range + if green < blue { 6.0 } else { 0.0 }
    } else if high == green {
        (blue - red) / range + 2.0
    } else {
        (red - green) / range + 4.0
    };
    [hue / 6.0, saturation, lightness]
}

fn unary(op: Unary, value: Value, pos: Pos) -> Result<Value, String> {
    match (op, value) {
        (Unary::Not, value) => Ok(Value::Bool(!value.truthy())),
        (Unary::Neg, Value::Num(number)) => Ok(Value::Num(-number)),
        (Unary::Neg, Value::Array(items)) => {
            Ok(Value::Array(items.into_iter().map(|item| -item).collect()))
        }
        (Unary::Plus, Value::Num(number)) => Ok(Value::Num(number)),
        (Unary::Plus, Value::Bool(flag)) => Ok(Value::Num(f64::from(u8::from(flag)))),
        (Unary::Plus, Value::Str(text)) => {
            let text = text.trim();
            Ok(Value::Num(if text.is_empty() {
                0.0
            } else {
                text.parse().unwrap_or(f64::NAN)
            }))
        }
        (op, value) => Err(format!(
            "{pos}: {}{} is not defined",
            if op == Unary::Neg { "-" } else { "+" },
            value.describe()
        )),
    }
}

fn symbol(op: Binary) -> &'static str {
    match op {
        Binary::Add => "+",
        Binary::Sub => "-",
        Binary::Mul => "*",
        Binary::Div => "/",
        Binary::Rem => "%",
        Binary::Less => "<",
        Binary::LessEq => "<=",
        Binary::Greater => ">",
        Binary::GreaterEq => ">=",
        Binary::Eq => "==",
        Binary::NotEq => "!=",
    }
}

fn binary(op: Binary, left: Value, right: Value, pos: Pos) -> Result<Value, String> {
    match op {
        Binary::Eq => return Ok(Value::Bool(left == right)),
        Binary::NotEq => return Ok(Value::Bool(left != right)),
        Binary::Less | Binary::LessEq | Binary::Greater | Binary::GreaterEq => {
            let ordering = match (&left, &right) {
                (Value::Num(left), Value::Num(right)) => left.partial_cmp(right),
                (Value::Str(left), Value::Str(right)) => Some(left.cmp(right)),
                _ => {
                    return Err(format!(
                        "{pos}: {} {} {} is not defined",
                        left.describe(),
                        symbol(op),
                        right.describe()
                    ));
                }
            };
            return Ok(Value::Bool(ordering.is_some_and(|ordering| match op {
                Binary::Less => ordering.is_lt(),
                Binary::LessEq => ordering.is_le(),
                Binary::Greater => ordering.is_gt(),
                _ => ordering.is_ge(),
            })));
        }
        Binary::Add | Binary::Sub | Binary::Mul | Binary::Div | Binary::Rem => {}
    }
    if op == Binary::Add && (matches!(left, Value::Str(_)) || matches!(right, Value::Str(_))) {
        let text = left.text() + &right.text();
        if text.len() > MAX_STRING {
            return Err(format!("{pos}: a string grew past {MAX_STRING} bytes"));
        }
        return Ok(Value::Str(text));
    }
    let apply = |a: f64, b: f64| match op {
        Binary::Add => a + b,
        Binary::Sub => a - b,
        Binary::Mul => a * b,
        Binary::Div => a / b,
        _ => a % b,
    };
    match (left, right) {
        (Value::Num(a), Value::Num(b)) => Ok(Value::Num(apply(a, b))),
        (Value::Array(a), Value::Array(b)) if matches!(op, Binary::Add | Binary::Sub) => {
            let len = a.len().max(b.len());
            Ok(Value::Array(
                (0..len)
                    .map(|index| {
                        let a = a.get(index).copied().unwrap_or(0.0);
                        let b = b.get(index).copied().unwrap_or(0.0);
                        apply(a, b)
                    })
                    .collect(),
            ))
        }
        (Value::Array(a), Value::Num(b)) if matches!(op, Binary::Mul | Binary::Div) => {
            Ok(Value::Array(a.into_iter().map(|a| apply(a, b)).collect()))
        }
        (Value::Num(a), Value::Array(b)) if op == Binary::Mul => {
            Ok(Value::Array(b.into_iter().map(|b| a * b).collect()))
        }
        (left, right) => Err(format!(
            "{pos}: {} {} {} is not defined",
            left.describe(),
            symbol(op),
            right.describe()
        )),
    }
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The top 53 bits of `bits` as a number in `0..1`.
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// Gradient noise in `-1..=1`, smooth in `x`.
fn noise(seed: u64, x: f64) -> f64 {
    let cell = x.floor();
    let offset = x - cell;
    let lattice = cell as i64;
    let gradient = |point: i64| unit(mix(seed ^ point as u64)) * 2.0 - 1.0;
    let left = gradient(lattice) * offset;
    let right = gradient(lattice.wrapping_add(1)) * (offset - 1.0);
    let fade = offset * offset * offset * (offset * (offset * 6.0 - 15.0) + 10.0);
    2.0 * (left + fade * (right - left))
}

/// `octaves` layers of [`noise`], each twice the frequency of the last and
/// `falloff` times its amplitude.
fn fractal(seed: u64, x: f64, octaves: u32, falloff: f64) -> f64 {
    let (mut sum, mut amplitude, mut frequency) = (0.0, 1.0, 1.0);
    for octave in 0..octaves {
        sum += amplitude * noise(seed.wrapping_add(u64::from(octave)), x * frequency);
        amplitude *= falloff;
        frequency *= 2.0;
    }
    sum
}
