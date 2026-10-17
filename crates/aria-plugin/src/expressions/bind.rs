//! Binding: property references in the source become the program's declared
//! inputs, so the host knows what a driver reads before it runs.
//!
//! A reference is a fixed chain: `thisComp.layer("Logo").transform.position`,
//! `thisLayer.opacity`, bare `scale`, `effect("Blur")("Radius")`,
//! `audio.volume`, or `thisLayer.property("text-size")` for any catalog id.
//! Names inside a chain are string literals; a chain that computes a name is
//! refused, since its input could not be declared.

use std::collections::HashSet;

use aria_models::ClipParam;
use aria_render::{DriverInput, InputClip, InputProperty};

use super::lex::Pos;
use super::parse::{Expr, PropRef, Stmt};

/// How many distinct properties and clips one driver may read.
const MAX_INPUTS: usize = 256;

/// How a property's model value is shown to the expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Units {
    /// Canvas pixels from the top-left corner.
    Pixels,
    /// Percent, where the model stores a fraction.
    Percent,
    /// The model's own units.
    Raw,
}

impl Units {
    pub(super) fn of_id(id: &str) -> Self {
        match id {
            "position" => Self::Pixels,
            "scale" | "opacity" => Self::Percent,
            _ => Self::Raw,
        }
    }

    /// The units of the property a driver replaces.
    pub(super) fn of_param(param: ClipParam) -> Self {
        match param {
            ClipParam::Position => Self::Pixels,
            ClipParam::Scale | ClipParam::Opacity => Self::Percent,
            _ => Self::Raw,
        }
    }
}

/// The transform names an expression reads, and their catalog ids.
pub(super) const TRANSFORM: [(&str, &str); 6] = [
    ("position", "position"),
    ("anchorPoint", "anchor-point"),
    ("scale", "scale"),
    ("rotation", "rotation"),
    ("zRotation", "rotation"),
    ("opacity", "opacity"),
];

/// Layer attributes read through [`aria_render::DriverScope::clip_info`].
const LAYER_ATTRIBUTES: [&str; 3] = ["name", "inPoint", "outPoint"];

fn transform_id(name: &str) -> Option<&'static str> {
    TRANSFORM
        .iter()
        .find(|(word, _)| *word == name)
        .map(|(_, id)| *id)
}

pub(super) struct Bound {
    pub(super) body: Vec<Stmt>,
    pub(super) inputs: Vec<DriverInput>,
    /// The units of each input, by index.
    pub(super) units: Vec<Units>,
    /// The clips whose attributes the expression reads.
    pub(super) clips: Vec<InputClip>,
}

pub(super) fn bind(body: Vec<Stmt>) -> Result<Bound, String> {
    let mut declared = HashSet::new();
    declared_names(&body, &mut declared);
    let mut binder = Binder {
        declared,
        inputs: Vec::new(),
        units: Vec::new(),
        clips: Vec::new(),
    };
    let body = binder.block(body)?;
    Ok(Bound {
        body,
        inputs: binder.inputs,
        units: binder.units,
        clips: binder.clips,
    })
}

/// Every name the source declares or assigns at statement level, which then
/// shadows a property of the same name.
fn declared_names(body: &[Stmt], out: &mut HashSet<String>) {
    for stmt in body {
        match stmt {
            Stmt::Let(name, _) => {
                out.insert(name.clone());
            }
            Stmt::Expr(Expr::Assign(super::parse::Target::Name(name), ..)) => {
                out.insert(name.clone());
            }
            Stmt::If(_, then, otherwise) => {
                declared_names(then, out);
                declared_names(otherwise, out);
            }
            Stmt::While(_, body) | Stmt::Block(body) => declared_names(body, out),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    declared_names(std::slice::from_ref(init.as_ref()), out);
                }
                declared_names(body, out);
            }
            Stmt::Expr(_) | Stmt::Return(..) | Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }
}

struct Binder {
    declared: HashSet<String>,
    inputs: Vec<DriverInput>,
    units: Vec<Units>,
    clips: Vec<InputClip>,
}

impl Binder {
    fn free(&self, name: &str) -> bool {
        !self.declared.contains(name)
    }

    fn block(&mut self, body: Vec<Stmt>) -> Result<Vec<Stmt>, String> {
        body.into_iter().map(|stmt| self.stmt(stmt)).collect()
    }

    fn stmt(&mut self, stmt: Stmt) -> Result<Stmt, String> {
        let optional =
            |binder: &mut Self, expr: Option<Expr>| expr.map(|e| binder.expr(e)).transpose();
        Ok(match stmt {
            Stmt::Let(name, value) => Stmt::Let(name, optional(self, value)?),
            Stmt::Expr(expr) => Stmt::Expr(self.expr(expr)?),
            Stmt::If(cond, then, otherwise) => {
                Stmt::If(self.expr(cond)?, self.block(then)?, self.block(otherwise)?)
            }
            Stmt::While(cond, body) => Stmt::While(self.expr(cond)?, self.block(body)?),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => Stmt::For {
                init: init
                    .map(|init| self.stmt(*init).map(Box::new))
                    .transpose()?,
                cond: optional(self, cond)?,
                step: optional(self, step)?,
                body: self.block(body)?,
            },
            Stmt::Return(value, pos) => Stmt::Return(optional(self, value)?, pos),
            Stmt::Block(body) => Stmt::Block(self.block(body)?),
            stmt @ (Stmt::Break(_) | Stmt::Continue(_)) => stmt,
        })
    }

    fn exprs(&mut self, exprs: Vec<Expr>) -> Result<Vec<Expr>, String> {
        exprs.into_iter().map(|expr| self.expr(expr)).collect()
    }

    fn expr(&mut self, expr: Expr) -> Result<Expr, String> {
        if let Some(bound) = self.reference(&expr)? {
            return Ok(bound);
        }
        let boxed = |binder: &mut Self, expr: Box<Expr>| binder.expr(*expr).map(Box::new);
        Ok(match expr {
            Expr::Array(items, pos) => Expr::Array(self.exprs(items)?, pos),
            Expr::Member(base, name, pos) => Expr::Member(boxed(self, base)?, name, pos),
            Expr::Index(base, index, pos) => {
                Expr::Index(boxed(self, base)?, boxed(self, index)?, pos)
            }
            Expr::Call(callee, args, pos) => {
                Expr::Call(boxed(self, callee)?, self.exprs(args)?, pos)
            }
            Expr::Unary(op, operand, pos) => Expr::Unary(op, boxed(self, operand)?, pos),
            Expr::Binary(op, left, right, pos) => {
                Expr::Binary(op, boxed(self, left)?, boxed(self, right)?, pos)
            }
            Expr::And(left, right) => Expr::And(boxed(self, left)?, boxed(self, right)?),
            Expr::Or(left, right) => Expr::Or(boxed(self, left)?, boxed(self, right)?),
            Expr::Cond(cond, then, otherwise) => Expr::Cond(
                boxed(self, cond)?,
                boxed(self, then)?,
                boxed(self, otherwise)?,
            ),
            Expr::Assign(target, op, value, pos) => {
                Expr::Assign(self.target(target)?, op, boxed(self, value)?, pos)
            }
            Expr::Step {
                target,
                delta,
                prefix,
                pos,
            } => Expr::Step {
                target: self.target(target)?,
                delta,
                prefix,
                pos,
            },
            leaf @ (Expr::Num(_)
            | Expr::Str(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Name(..)
            | Expr::Property(..)
            | Expr::Layer(..)) => leaf,
        })
    }

    fn target(&mut self, target: super::parse::Target) -> Result<super::parse::Target, String> {
        use super::parse::Target;
        Ok(match target {
            Target::Index(name, index) => Target::Index(name, Box::new(self.expr(*index)?)),
            name @ Target::Name(_) => name,
        })
    }

    /// `expr` as a property reference or a layer attribute, if it is one.
    fn reference(&mut self, expr: &Expr) -> Result<Option<Expr>, String> {
        match expr {
            Expr::Name(name, pos) if self.free(name) => {
                if name == "thisProperty" {
                    return Ok(Some(Expr::Property(PropRef::Own, *pos)));
                }
                if let Some(id) = transform_id(name) {
                    return self.input(InputClip::This, InputProperty::Id(id.into()), *pos);
                }
                if LAYER_ATTRIBUTES.contains(&name.as_str()) && name != "name" {
                    return self.attribute(InputClip::This, name, *pos);
                }
                Ok(None)
            }
            Expr::Member(base, name, pos) => {
                if let Some(layer) = self.group(base, "transform")? {
                    let Some(id) = transform_id(name) else {
                        return Err(format!(
                            "{pos}: transform has no property {name}; it has position, \
                             anchorPoint, scale, rotation and opacity"
                        ));
                    };
                    return self.input(layer, InputProperty::Id(id.into()), *pos);
                }
                if let Some(layer) = self.group(base, "audio")? {
                    if name != "volume" && name != "pan" {
                        return Err(format!(
                            "{pos}: audio has no property {name}; it has volume and pan"
                        ));
                    }
                    return self.input(layer, InputProperty::Id(name.clone()), *pos);
                }
                let Some(layer) = self.layer(base)? else {
                    return Ok(None);
                };
                if let Some(id) = transform_id(name) {
                    return self.input(layer, InputProperty::Id(id.into()), *pos);
                }
                if LAYER_ATTRIBUTES.contains(&name.as_str()) {
                    return self.attribute(layer, name, *pos);
                }
                Ok(None)
            }
            Expr::Call(callee, args, pos) => {
                if let Some((layer, effect)) = self.effect(callee)? {
                    let param = literal(args, *pos, "an effect parameter is named in quotes")?;
                    return self.input(layer, InputProperty::Effect { effect, param }, *pos);
                }
                let Expr::Member(inner, method, _) = callee.as_ref() else {
                    return Ok(None);
                };
                if method == "param"
                    && let Some((layer, effect)) = self.effect(inner)?
                {
                    let param = literal(args, *pos, "an effect parameter is named in quotes")?;
                    return self.input(layer, InputProperty::Effect { effect, param }, *pos);
                }
                if (method == "property" || method == "prop")
                    && let Some(layer) = self.layer(inner)?
                {
                    let id = literal(args, *pos, "a property is named by its id in quotes")?;
                    return self.input(layer, InputProperty::Id(id), *pos);
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// The clip `expr` names: `thisLayer` or `thisComp.layer("Name")`.
    fn layer(&self, expr: &Expr) -> Result<Option<InputClip>, String> {
        match expr {
            Expr::Name(name, _) if name == "thisLayer" && self.free(name) => {
                Ok(Some(InputClip::This))
            }
            Expr::Call(callee, args, pos) => {
                let Expr::Member(base, method, _) = callee.as_ref() else {
                    return Ok(None);
                };
                let is_comp = matches!(base.as_ref(), Expr::Name(name, _) if name == "thisComp" && self.free(name));
                if !is_comp || method != "layer" {
                    return Ok(None);
                }
                let name = literal(
                    args,
                    *pos,
                    "thisComp.layer takes the layer's name in quotes",
                )?;
                Ok(Some(InputClip::Named(name)))
            }
            _ => Ok(None),
        }
    }

    /// The clip whose `group` (`transform` or `audio`) `expr` is: on a layer,
    /// or bare for this clip.
    fn group(&self, expr: &Expr, group: &str) -> Result<Option<InputClip>, String> {
        match expr {
            Expr::Name(name, _) if name == group && self.free(name) => Ok(Some(InputClip::This)),
            Expr::Member(base, name, _) if name == group => self.layer(base),
            _ => Ok(None),
        }
    }

    /// The clip and effect name of `effect("Name")` on a layer, or bare.
    fn effect(&self, expr: &Expr) -> Result<Option<(InputClip, String)>, String> {
        let Expr::Call(callee, args, pos) = expr else {
            return Ok(None);
        };
        let layer = match callee.as_ref() {
            Expr::Name(name, _) if name == "effect" && self.free(name) => InputClip::This,
            Expr::Member(base, method, _) if method == "effect" => match self.layer(base)? {
                Some(layer) => layer,
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        let name = literal(args, *pos, "effect takes the effect's name in quotes")?;
        Ok(Some((layer, name)))
    }

    fn input(
        &mut self,
        clip: InputClip,
        property: InputProperty,
        pos: Pos,
    ) -> Result<Option<Expr>, String> {
        let units = match &property {
            InputProperty::Id(id) => Units::of_id(id),
            InputProperty::Effect { .. } => Units::Raw,
        };
        let input = DriverInput { clip, property };
        let index = match self.inputs.iter().position(|held| *held == input) {
            Some(index) => index,
            None => {
                if self.inputs.len() >= MAX_INPUTS {
                    return Err(format!(
                        "{pos}: a driver reads at most {MAX_INPUTS} properties"
                    ));
                }
                self.inputs.push(input);
                self.units.push(units);
                self.inputs.len() - 1
            }
        };
        Ok(Some(Expr::Property(PropRef::Input(index), pos)))
    }

    fn attribute(&mut self, clip: InputClip, name: &str, pos: Pos) -> Result<Option<Expr>, String> {
        let index = match self.clips.iter().position(|held| *held == clip) {
            Some(index) => index,
            None => {
                if self.clips.len() >= MAX_INPUTS {
                    return Err(format!("{pos}: a driver reads at most {MAX_INPUTS} layers"));
                }
                self.clips.push(clip);
                self.clips.len() - 1
            }
        };
        Ok(Some(Expr::Layer(index, name.to_string(), pos)))
    }
}

/// The single string literal in `args`.
fn literal(args: &[Expr], pos: Pos, wanted: &str) -> Result<String, String> {
    match args {
        [Expr::Str(text)] => Ok(text.clone()),
        _ => Err(format!("{pos}: {wanted}")),
    }
}

/// Source that reads `input`: the inverse of binding.
pub(super) fn reference(input: &DriverInput) -> String {
    let layer = match &input.clip {
        InputClip::This => "thisLayer".to_owned(),
        InputClip::Named(name) => format!("thisComp.layer({})", quote(name)),
    };
    match &input.property {
        InputProperty::Id(id) => {
            if let Some((word, _)) = TRANSFORM.iter().find(|(_, held)| held == id) {
                format!("{layer}.transform.{word}")
            } else if id == "volume" || id == "pan" {
                format!("{layer}.audio.{id}")
            } else {
                format!("{layer}.property({})", quote(id))
            }
        }
        InputProperty::Effect { effect, param } => {
            format!("{layer}.effect({})({})", quote(effect), quote(param))
        }
    }
}

/// `text` as a double-quoted string literal the lexer reads back unchanged.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
