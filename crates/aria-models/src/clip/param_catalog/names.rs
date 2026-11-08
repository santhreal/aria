//! The words a message and a surface state a property by: its row's id and
//! label.
//!
//! On a clip, a property reads as the row the clip offers for it, which states
//! an effect slot by its parameter name. Without a clip, and for a property the
//! clip does not offer, a property reads as its static row, as the row its mask
//! would give it, or by position: `effect-<index>-opacity`,
//! `effect-<index>-<slot>` and `speed`.

use std::borrow::Cow;

use crate::clip::{Clip, ClipParam};
use crate::error::ModelError;

use super::{ParamKind, ParamSpec, mask_rows, param_spec, visit_clip_params};

impl ClipParam {
    /// The id messages state this property by, read without a clip.
    pub fn id(self) -> Cow<'static, str> {
        self.words().0
    }

    /// The label a surface states this property by, read without a clip.
    pub fn label(self) -> Cow<'static, str> {
        self.words().1
    }

    /// The id of the row `clip` offers for this property, else [`Self::id`].
    pub fn id_on(self, clip: &Clip) -> Cow<'static, str> {
        self.offered(clip, |row| row.id.clone())
            .unwrap_or_else(|| self.id())
    }

    /// The label of the row `clip` offers for this property, else
    /// [`Self::label`].
    pub fn label_on(self, clip: &Clip) -> Cow<'static, str> {
        self.offered(clip, |row| row.label.clone())
            .unwrap_or_else(|| self.label())
    }

    /// No keyframe sits at the clip-local `tick` of this property.
    pub(crate) fn no_keyframe(self, tick: i64) -> ModelError {
        no_keyframe(&self.id(), tick)
    }

    /// No keyframe sits at the clip-local `tick` of this property on `clip`.
    pub(crate) fn no_keyframe_on(self, clip: &Clip, tick: i64) -> ModelError {
        no_keyframe(&self.id_on(clip), tick)
    }

    /// `value` falls outside the range this property declares.
    pub(crate) fn out_of_range(self, value: f32) -> ModelError {
        let id = self.id();
        let range = self.range();
        ModelError::InvalidParam(match (range.min, range.max) {
            _ if !value.is_finite() => format!("{id} {value} is not a finite number"),
            (Some(min), Some(max)) => format!("{id} {value} is outside {min} to {max}"),
            (Some(min), None) => format!("{id} {value} is below {min}"),
            (None, Some(max)) => format!("{id} {value} is above {max}"),
            (None, None) => format!("{id} {value} is not a finite number"),
        })
    }

    /// `got` is not the value kind this property takes.
    pub(crate) fn wrong_kind(self, want: ParamKind, got: ParamKind) -> ModelError {
        ModelError::InvalidParam(format!(
            "{} takes a {} value, got a {} value",
            self.id(),
            want.name(),
            got.name()
        ))
    }

    /// What `take` reads off the row `clip` offers for this property. Only an
    /// effect's rows read differently on a clip: every other row states the
    /// same words with or without one.
    fn offered(
        self,
        clip: &Clip,
        take: impl Fn(&ParamSpec) -> Cow<'static, str>,
    ) -> Option<Cow<'static, str>> {
        if !matches!(self, Self::Effect { .. } | Self::EffectOpacity { .. }) {
            return None;
        }
        let mut found = None;
        visit_clip_params(clip, |row| {
            if found.is_none() && row.param == self {
                found = Some(take(row));
            }
        });
        found
    }

    fn words(self) -> (Cow<'static, str>, Cow<'static, str>) {
        if let Some(row) = param_spec(self) {
            return (row.id.clone(), row.label.clone());
        }
        match self {
            Self::Mask { mask, param } => {
                let row = mask_rows::row(mask, param);
                (row.id, row.label)
            }
            Self::EffectOpacity { effect } => (
                format!("effect-{effect}-opacity").into(),
                format!("Effect {} Opacity", u64::from(effect) + 1).into(),
            ),
            Self::Effect { effect, param } => (
                format!("effect-{effect}-{param}").into(),
                format!(
                    "Effect {} Parameter {}",
                    u64::from(effect) + 1,
                    u64::from(param) + 1
                )
                .into(),
            ),
            // The speed ramp is the one other property without a static row;
            // `every_property_reads_as_its_own_id` holds the rest to theirs.
            _ => (Cow::Borrowed("speed"), Cow::Borrowed("Speed")),
        }
    }
}

fn no_keyframe(id: &str, tick: i64) -> ModelError {
    ModelError::InvalidParam(format!("{id} has no keyframe at tick {tick}"))
}

impl ParamKind {
    /// The kind as messages state it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Color => "color",
            Self::Rect => "rect",
        }
    }
}
