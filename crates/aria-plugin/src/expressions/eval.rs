//! Evaluation of a bound program against a [`DriverScope`].
//!
//! Values are numbers, strings, booleans, `null`, arrays of numbers, and the
//! keyframes `key(i)` returns. An array is copied on assignment, so writing
//! one item changes that variable alone. A variable is visible from its first
//! assignment to the end of the program.

use std::collections::HashMap;

use aria_models::ParamValue;
use aria_render::DriverScope;

use super::Program;
use super::bind::Units;
use super::lex::Pos;
use super::parse::{Binary, Expr, PropRef, Stmt, Target, Unary};

#[path = "builtins.rs"]
mod builtins;

use self::builtins::{Method, magnitude, math_constant, mix};

/// Steps between two reads of the scope's clock.
const CLOCK_EVERY: u64 = 256;
/// Statements, loop turns, and calls one evaluation may take, so a program
/// ends under a scope whose clock never runs out.
pub(super) const MAX_STEPS: u64 = 20_000_000;
/// Longest string a program may build, in bytes.
pub(super) const MAX_STRING: usize = 64 * 1024;
/// Seconds either side of a time over which a velocity is measured.
const VELOCITY_SPAN: f64 = 1e-3;

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
    pub(self) fn describe(&self) -> String {
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
    pub(self) own_units: Units,
    pub(self) scope: &'s mut dyn DriverScope,
    vars: HashMap<&'p str, Value>,
    /// The value of the last expression statement, which the program returns
    /// when it ends without `return`.
    last: Option<Value>,
    steps: u64,
    pub(self) random: u64,
    pub(self) canvas: [f64; 2],
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
    pub(self) fn read(
        &mut self,
        property: PropRef,
        at: Option<f64>,
        pos: Pos,
    ) -> Result<Value, String> {
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

    pub(self) fn velocity(
        &mut self,
        property: PropRef,
        at: f64,
        pos: Pos,
    ) -> Result<Value, String> {
        let before = self.read(property, Some(at - VELOCITY_SPAN), pos)?;
        let after = self.read(property, Some(at + VELOCITY_SPAN), pos)?;
        let change = binary(Binary::Sub, after, before, pos)?;
        binary(Binary::Div, change, Value::Num(2.0 * VELOCITY_SPAN), pos)
    }

    pub(self) fn own_only(&self, property: PropRef, what: &str, pos: Pos) -> Result<(), String> {
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
