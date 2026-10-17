//! Property drivers: code that replaces a property's keyframed value.
//!
//! A clip stores at most one driver per [`ClipParam`]: the id of the driver
//! language that compiles it and its source text. The model stores the code
//! and nothing else. Compiling, the input graph, and evaluation belong to the
//! renderer's driver registry, so a project whose language is not installed
//! still loads, saves, and shows the source.

use serde::{Deserialize, Serialize};

use crate::error::ModelError;

use super::param_catalog::{ParamRange, ParamRoute, ParamScope, effect_range};
use super::{Clip, ClipParam, ClipSource, ParamValue};

/// Longest driver source accepted, in bytes.
pub const MAX_DRIVER_SOURCE: usize = 64 * 1024;

/// Longest driver language id accepted, in bytes.
pub const MAX_DRIVER_LANGUAGE: usize = 128;

/// One driver's code: the language that compiles it and the text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DriverCode {
    /// The driver language id, such as `aria.expressions`.
    pub language: String,
    pub source: String,
}

impl DriverCode {
    pub fn new(language: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            source: source.into(),
        }
    }

    /// Shape check: a language id of ASCII letters, digits, `.`, `-` and `_`,
    /// and a source within [`MAX_DRIVER_SOURCE`] bytes.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.language.is_empty() {
            return Err(ModelError::InvalidParam("a driver needs a language".into()));
        }
        if self.language.len() > MAX_DRIVER_LANGUAGE
            || !self
                .language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(ModelError::InvalidParam(format!(
                "driver language {:?} is not an id of letters, digits, '.', '-' and '_' up to \
                 {MAX_DRIVER_LANGUAGE} bytes",
                self.language
            )));
        }
        if self.source.len() > MAX_DRIVER_SOURCE {
            return Err(ModelError::InvalidParam(format!(
                "driver source is {} bytes; the limit is {MAX_DRIVER_SOURCE}",
                self.source.len()
            )));
        }
        Ok(())
    }
}

/// A driver attached to one property of a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDriver {
    pub param: ClipParam,
    #[serde(flatten)]
    pub code: DriverCode,
}

impl Clip {
    /// The driver on `param`, if one is attached.
    pub fn driver(&self, param: ClipParam) -> Option<&DriverCode> {
        self.drivers
            .iter()
            .find(|driver| driver.param == param)
            .map(|driver| &driver.code)
    }

    /// Attach `code` to `param`, or detach with `None`. Returns the code that
    /// was attached before.
    pub(crate) fn put_driver(
        &mut self,
        param: ClipParam,
        code: Option<DriverCode>,
    ) -> Option<DriverCode> {
        let at = self.drivers.iter().position(|driver| driver.param == param);
        match (at, code) {
            (Some(index), Some(code)) => {
                Some(std::mem::replace(&mut self.drivers[index].code, code))
            }
            (Some(index), None) => Some(self.drivers.remove(index).code),
            (None, Some(code)) => {
                self.drivers.push(PropertyDriver { param, code });
                None
            }
            (None, None) => None,
        }
    }

    /// Re-address what the clip holds on effect parameters, its drivers and
    /// its line property, after the effect chain changed. `map` takes an old
    /// effect index to its new one, or `None` when that effect left the chain,
    /// which drops its drivers and returns the line to the lane's default.
    pub(crate) fn remap_effect_refs(&mut self, map: impl Fn(u32) -> Option<u32>) {
        self.drivers.retain_mut(|driver| match driver.param {
            ClipParam::Effect { effect, param } => match map(effect) {
                Some(effect) => {
                    driver.param = ClipParam::Effect { effect, param };
                    true
                }
                None => false,
            },
            _ => true,
        });
        if let Some(ClipParam::Effect { effect, param }) = self.line_param {
            self.line_param = map(effect).map(|effect| ClipParam::Effect { effect, param });
        }
    }

    /// Re-address what the clip holds on mask properties, its drivers and its
    /// line property, after the mask stack changed. `map` takes an old mask
    /// index to its new one, or `None` when that mask left the stack, which
    /// drops its drivers and returns the line to the lane's default.
    pub(crate) fn remap_mask_refs(&mut self, map: impl Fn(u32) -> Option<u32>) {
        self.drivers.retain_mut(|driver| match driver.param {
            ClipParam::Mask { mask, param } => match map(mask) {
                Some(mask) => {
                    driver.param = ClipParam::Mask { mask, param };
                    true
                }
                None => false,
            },
            _ => true,
        });
        if let Some(ClipParam::Mask { mask, param }) = self.line_param {
            self.line_param = map(mask).map(|mask| ClipParam::Mask { mask, param });
        }
    }
}

/// The index map for a stack that lost its entry at `removed`: later entries
/// move down one, and the removed entry maps to `None`.
pub(crate) fn index_after_remove(removed: u32) -> impl Fn(u32) -> Option<u32> {
    move |index| match index.cmp(&removed) {
        std::cmp::Ordering::Less => Some(index),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(index - 1),
    }
}

/// The index map for a stack whose entry at `from` moved to `to`; the entries
/// between them move one step toward `from`.
pub(crate) fn index_after_move(from: u32, to: u32) -> impl Fn(u32) -> Option<u32> {
    move |index| {
        Some(if index == from {
            to
        } else if from < to && (from + 1..=to).contains(&index) {
            index - 1
        } else if to < from && (to..from).contains(&index) {
            index + 1
        } else {
            index
        })
    }
}

impl ClipParam {
    /// This property's value on `clip` at a fractional parameter tick, the
    /// position [`Clip::param_tick_f`] returns. `None` when the property is
    /// not reachable on `clip`: an optional block that is not enabled, an
    /// effect slot that does not exist, or an audio envelope on a clip without
    /// sound.
    pub fn sample_at(self, clip: &Clip, tick: f64) -> Option<ParamValue> {
        if self.scope() == ParamScope::Audio && !matches!(clip.content, ClipSource::Media { .. }) {
            return None;
        }
        Some(match self.route(clip) {
            ParamRoute::Scalar(p) => ParamValue::Scalar(p.sample_at(tick)),
            ParamRoute::Vec2(p) => ParamValue::Vec2(p.sample_at(tick)),
            ParamRoute::Vec3(p) => ParamValue::Vec3(p.sample_at(tick)),
            ParamRoute::Scale(p) => {
                let scale = p.sample_at(tick);
                ParamValue::Vec2([scale.x, scale.y])
            }
            ParamRoute::Color(p) => ParamValue::Color(p.sample_at(tick)),
            ParamRoute::Rect(p) => {
                let r = p.sample_at(tick);
                ParamValue::Rect([r.x, r.y, r.w, r.h])
            }
            ParamRoute::Const(value) => value,
            ParamRoute::Absent => return None,
        })
    }

    /// The stored ticks of this property's keyframes on `clip`, in order,
    /// passed to `visit`. Nothing for a constant or unreachable property.
    pub fn visit_keyframe_ticks(self, clip: &Clip, mut visit: impl FnMut(i64)) {
        fn each<T>(p: &crate::param::Param<T>, visit: &mut impl FnMut(i64)) {
            for key in p.keyframes() {
                visit(key.tick);
            }
        }
        match self.route(clip) {
            ParamRoute::Scalar(p) => each(p, &mut visit),
            ParamRoute::Vec2(p) => each(p, &mut visit),
            ParamRoute::Vec3(p) => each(p, &mut visit),
            ParamRoute::Scale(p) => each(p, &mut visit),
            ParamRoute::Color(p) => each(p, &mut visit),
            ParamRoute::Rect(p) => each(p, &mut visit),
            ParamRoute::Const(_) | ParamRoute::Absent => {}
        }
    }

    /// The bounds a write to this property on `clip` is held to. An effect
    /// parameter's bounds come from its effect, which only the clip has.
    pub fn range_on(self, clip: &Clip) -> ParamRange {
        match self {
            ClipParam::Effect { effect, param } => effect_range(clip, effect, param),
            _ => self.range(),
        }
    }

    /// `value` held inside this property's bounds on `clip`, component by
    /// component. `None` when a component is not finite.
    pub fn clamp_on(self, clip: &Clip, value: ParamValue) -> Option<ParamValue> {
        let range = self.range_on(clip);
        let hold = |v: f32| v.is_finite().then(|| range.clamp(v));
        Some(match value {
            ParamValue::Scalar(v) => ParamValue::Scalar(hold(v)?),
            ParamValue::Vec2([x, y]) => ParamValue::Vec2([hold(x)?, hold(y)?]),
            ParamValue::Vec3([x, y, z]) => ParamValue::Vec3([hold(x)?, hold(y)?, hold(z)?]),
            ParamValue::Rect([x, y, w, h]) => {
                ParamValue::Rect([hold(x)?, hold(y)?, hold(w)?, hold(h)?])
            }
            ParamValue::Color(rgba) => ParamValue::Color(rgba),
        })
    }
}
