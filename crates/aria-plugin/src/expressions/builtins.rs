//! Built-in property methods, math, interpolation, colour, random, and noise
//! functions for `aria.expressions`.

use std::f64::consts;

use super::super::lex::Pos;
use super::super::parse::{Binary, PropRef};
use super::{Run, VELOCITY_SPAN, Value, binary, number_text};

/// Most octaves `wiggle` sums.
const MAX_OCTAVES: f64 = 16.0;
/// Seed of `noise`, which is the same function on every clip.
const NOISE_SEED: u64 = 0x6e6f_6973_6500;
/// The golden-ratio increment of the random sequence.
const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
/// Standard deviations from the mean to the edge of a `gaussRandom` range, so
/// nine draws in ten fall inside it.
const GAUSS_EDGE: f64 = 1.644_853_626_951_472_2;

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
pub(super) enum Method {
    ValueAt,
    VelocityAt,
    SpeedAt,
    Wiggle,
    LoopOut,
    LoopIn,
    Key,
}

impl Method {
    pub(super) fn of(name: &str) -> Option<Self> {
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

impl Run<'_, '_> {
    pub(super) fn method(
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

    pub(super) fn function(
        &mut self,
        name: &str,
        values: &[Value],
        pos: Pos,
    ) -> Result<Value, String> {
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

    pub(super) fn math(&mut self, name: &str, values: &[Value], pos: Pos) -> Result<Value, String> {
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

pub(super) fn magnitude(value: &Value, pos: Pos) -> Result<f64, String> {
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

pub(super) fn math_constant(name: &str, pos: Pos) -> Result<Value, String> {
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

/// SplitMix64's finaliser.
pub(super) fn mix(mut z: u64) -> u64 {
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
