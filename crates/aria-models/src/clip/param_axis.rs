//! The [`ClipParam`] axis, as data.
//!
//! `ClipParam` names every animatable clip property a command can address.
//! Each variant used to be an arm in every place that dispatched on one: the
//! transform's three mutators, the project's keyframe / constant / erase /
//! easing-preset routes, the colour-param guard, the target precondition, and
//! the eight hand-written head rows of [`crate::clip_param_catalog`]. Most of
//! those matches ended in a wildcard, so a new variant fell into somebody
//! else's arm and shipped silently mis-routed.
//!
//! A row here carries the whole variant: what it addresses
//! ([`ClipParamClass`]), which lane it means anything on, its value kind, its
//! declared bounds, a read accessor, a write accessor, a validator, and the
//! four write verbs. Every dispatcher walks the row instead of restating the
//! variant list, so adding a property is one row.
//!
//! [`ClipParam::spec`] holds the only exhaustive match over `ClipParam` in the
//! crate. A new variant does not compile until it has a row, which is what
//! stops the next property from arriving unreachable.

use crate::effects::EffectInstance;
use crate::error::ModelError;
use crate::param::{Easing, Param, PiecewiseEasingPreset, TemporalEase};

use super::clip_param::{ClipParam, ClipParamClass, ParamExclusion};
use super::param_catalog::{ParamKind, ParamRange, ParamRoute, ParamScope, param_spec};
use super::param_value::{ParamValue, crop_rect, scale_value};
use super::transform::Scale2;
use super::{Clip, ClipSource, CropRect, Generator};

/// The [`Param`] a row addresses, borrowed mutably and typed by value kind.
///
/// Four kinds, not fourteen properties: once a row has handed over its slot the
/// write verbs are the same for every property that stores the same shape.
/// This is what collapses the transform's three parallel matches into three
/// one-liners.
pub(crate) enum ParamSlot<'a> {
    Scalar(&'a mut Param<f32>),
    Vec2(&'a mut Param<[f32; 2]>),
    Scale(&'a mut Param<Scale2>),
    Rect(&'a mut Param<CropRect>),
}

impl ParamSlot<'_> {
    fn set_keyframe(self, tick: i64, value: ParamValue, easing: Easing) -> Result<(), ModelError> {
        match self {
            Self::Scalar(p) => p.set_keyframe(tick, value.scalar()?, easing),
            Self::Vec2(p) => p.set_keyframe(tick, value.vec2()?, easing),
            Self::Scale(p) => p.set_keyframe(tick, scale_value(value)?, easing),
            Self::Rect(p) => p.set_keyframe(tick, crop_rect(value)?, easing),
        }
        Ok(())
    }

    fn set_constant(self, value: ParamValue) -> Result<(), ModelError> {
        match self {
            Self::Scalar(p) => p.set_constant(value.scalar()?),
            Self::Vec2(p) => p.set_constant(value.vec2()?),
            Self::Scale(p) => p.set_constant(scale_value(value)?),
            Self::Rect(p) => p.set_constant(crop_rect(value)?),
        }
        Ok(())
    }

    fn remove_keyframe(self, tick: i64) -> bool {
        match self {
            Self::Scalar(p) => p.remove_keyframe(tick),
            Self::Vec2(p) => p.remove_keyframe(tick),
            Self::Scale(p) => p.remove_keyframe(tick),
            Self::Rect(p) => p.remove_keyframe(tick),
        }
    }

    fn apply_easing_preset(
        self,
        tick: i64,
        preset: PiecewiseEasingPreset,
    ) -> Result<(), ModelError> {
        match self {
            Self::Scalar(p) => p.apply_easing_preset(tick, preset),
            Self::Vec2(p) => p.apply_easing_preset(tick, preset),
            Self::Scale(p) => p.apply_easing_preset(tick, preset),
            // A rect does not extrapolate, so a preset has no safe expansion.
            // `ClipParam::apply_easing_preset` refuses a rect row before it
            // reaches the slot; this arm keeps the slot honest on its own.
            Self::Rect(_) => Err(no_easing_preset_for_rect()),
        }
    }
}

/// The catalog facts a directly addressed property contributes to
/// [`crate::clip_param_catalog`].
///
/// A wrapper variant has none: its rows belong to the members of the axis it
/// delegates to, which declare their own labels and groups. An excluded
/// variant has none either — that is what excluded means.
pub(crate) struct Browse {
    pub label: &'static str,
    pub group: &'static str,
    pub kind: ParamKind,
    pub range: ParamRange,
}

/// One [`ClipParam`] variant, whole.
pub(crate) struct ClipParamSpec {
    /// What the variant addresses. The catalog id of a direct property lives
    /// in [`ClipParamClass::Direct`], so it is stated once.
    pub class: ClipParamClass,
    /// The lane the property means anything on, and so which target
    /// precondition a command checks.
    pub scope: ParamScope,
    /// The catalog row this variant contributes, when it is one property.
    pub browse: Option<Browse>,
    /// Value kind, given the payload. `None` for a parameter whose kind is a
    /// property of the clip rather than of the variant (an effect slot).
    pub kind: fn(ClipParam) -> Option<ParamKind>,
    /// Declared bounds, given the payload. A side left `None` claims the model
    /// accepts anything there.
    pub range: fn(ClipParam) -> ParamRange,
    /// Read accessor: where the value lives, borrowed.
    pub read: fn(&Clip, ClipParam) -> ParamRoute<'_>,
    /// Write accessor: the stored [`Param`], borrowed mutably.
    ///
    /// `None` for a variant whose value is not one `Param` reachable from the
    /// clip by itself — an effect slot, a generator property, a look or style
    /// block. Those write through their axis's own dispatcher, named by the
    /// verbs below.
    pub slot: Option<fn(&mut Clip) -> Result<ParamSlot<'_>, ModelError>>,
    /// Kind and range check for a candidate value, before anything is written.
    pub validate: fn(ClipParam, ParamValue) -> Result<(), ModelError>,
    pub keyframe: fn(&mut Clip, ClipParam, i64, ParamValue, Easing) -> Result<(), ModelError>,
    pub constant: fn(&mut Clip, ClipParam, ParamValue) -> Result<(), ModelError>,
    pub erase: fn(&mut Clip, ClipParam, i64) -> Result<(), ModelError>,
    /// The easing-preset verb, when the property carries one at all.
    ///
    /// `None` is a declaration, not an omission: a generator property and an
    /// effect slot are edited through their own axis and have no keyframe
    /// segment a preset can expand. `ClipParam::supports_easing_preset` is
    /// this column, so a surface that wants to fail closed before it reaches
    /// the model reads the row instead of restating the variant list.
    pub ease:
        Option<fn(&mut Clip, ClipParam, i64, PiecewiseEasingPreset) -> Result<(), ModelError>>,
    /// The value a reset returns the property to on `clip`: what the block
    /// holds when it is first made. `None` where nothing resets, which is a
    /// row with no write verb of its own (the speed ramp) or an effect slot
    /// whose kind no curve carries.
    pub default: fn(&Clip, ClipParam) -> Option<ParamValue>,
}

impl ClipParam {
    /// The registry row for this variant.
    ///
    /// The only exhaustive match over `ClipParam` in the crate. Every
    /// dispatcher reads its behaviour off the row this returns, so a variant
    /// added without a row fails to compile here rather than falling into
    /// somebody else's wildcard arm.
    pub(crate) fn spec(self) -> &'static ClipParamSpec {
        match self {
            Self::Position => &direct::POSITION,
            Self::AnchorPoint => &direct::ANCHOR_POINT,
            Self::Scale => &direct::SCALE,
            Self::Rotation => &direct::ROTATION,
            Self::Opacity => &direct::OPACITY,
            Self::Crop => &direct::CROP,
            Self::Speed => &direct::SPEED,
            Self::Volume => &direct::VOLUME,
            Self::Pan => &direct::PAN,
            Self::Effect { .. } => &delegated::EFFECT,
            Self::Mask { .. } => &delegated::MASK,
            Self::Shape { .. } => &delegated::SHAPE,
            Self::Text { .. } => &delegated::TEXT,
            Self::Look { .. } => &delegated::LOOK,
            Self::Style { .. } => &delegated::STYLE,
        }
    }

    /// Which kind of property this parameter is.
    ///
    /// Read off the registry row, so the classification and the behaviour that
    /// follows from it cannot disagree.
    pub fn class(self) -> ClipParamClass {
        self.spec().class
    }

    /// The lane this parameter means anything on. A visual property on an
    /// audio-lane clip and an audio envelope on a generated clip are both
    /// refused, and this is where a command learns which check to run.
    pub fn scope(self) -> ParamScope {
        self.spec().scope
    }

    /// The value kind this parameter carries, or `None` when the kind is a
    /// property of the clip (an effect slot) rather than of the parameter.
    pub fn kind(self) -> Option<ParamKind> {
        (self.spec().kind)(self)
    }

    /// The bounds this parameter's value is validated against.
    pub fn range(self) -> ParamRange {
        (self.spec().range)(self)
    }

    /// Where this parameter's value lives on `clip`, borrowed for reading.
    ///
    /// [`ParamRoute::Absent`] is the availability rule: a mask's feather has no
    /// route until that mask exists.
    pub(crate) fn route(self, clip: &Clip) -> ParamRoute<'_> {
        (self.spec().read)(clip, self)
    }

    /// This parameter's curve on `clip` when it is a keyframed point: the
    /// curve a motion path draws. `None` for any other value kind, and for a
    /// point with no keyframes.
    pub fn motion_path(self, clip: &Clip) -> Option<&Param<[f32; 2]>> {
        match self.route(clip) {
            ParamRoute::Vec2(curve) if !curve.keyframes().is_empty() => Some(curve),
            _ => None,
        }
    }

    /// Kind and range check for a candidate value, before anything is written.
    pub fn validate_value(self, value: ParamValue) -> Result<(), ModelError> {
        (self.spec().validate)(self, value)
    }

    /// Insert or replace a keyframe at a clip-relative `tick`.
    ///
    /// The value and the easing are checked here, once, for every row: a
    /// delegated axis validates its own range but not the value's *shape*, and
    /// the shape check belongs to the row that declares the kind. The caller
    /// still owns the preconditions the clip's *placement* decides — which lane
    /// it sits on, whether `tick` falls inside it.
    pub fn set_keyframe(
        self,
        clip: &mut Clip,
        tick: i64,
        value: ParamValue,
        easing: Easing,
    ) -> Result<(), ModelError> {
        easing.validate()?;
        self.validate_value(value)?;
        (self.spec().keyframe)(clip, self, tick, value, easing)
    }

    /// The value a reset returns this parameter to on `clip`: what its block
    /// holds when it is first made. `None` when nothing resets it.
    ///
    /// Not gated on the property being reachable on `clip`;
    /// [`crate::ParamSpec::default_value`] is.
    pub fn default_value(self, clip: &Clip) -> Option<ParamValue> {
        (self.spec().default)(clip, self)
    }

    /// Replace this parameter with a constant, dropping its keyframes.
    pub fn set_constant(self, clip: &mut Clip, value: ParamValue) -> Result<(), ModelError> {
        self.validate_value(value)?;
        (self.spec().constant)(clip, self, value)
    }

    /// Remove the keyframe at exactly `tick`. Errors when none sits there, so
    /// a no-op never lands in undo history.
    pub fn remove_keyframe(self, clip: &mut Clip, tick: i64) -> Result<(), ModelError> {
        (self.spec().erase)(clip, self, tick)
    }

    /// Whether an easing preset can be applied to this parameter at all.
    ///
    /// Two facts off the one row: the variant declares an `ease` verb, and its
    /// declared kind extrapolates. A colour has no meaning outside its
    /// endpoints and a rect has no safe expansion, so both are refused here
    /// rather than in each surface's own guard list.
    pub fn supports_easing_preset(self) -> bool {
        self.spec().ease.is_some()
            && !matches!(self.kind(), Some(ParamKind::Rect | ParamKind::Color))
    }

    /// Expand the outgoing keyframe segment at `tick` with a piecewise easing
    /// preset.
    pub fn apply_easing_preset(
        self,
        clip: &mut Clip,
        tick: i64,
        preset: PiecewiseEasingPreset,
    ) -> Result<(), ModelError> {
        match self.spec().ease {
            Some(ease) if self.supports_easing_preset() => ease(clip, self, tick, preset),
            _ => Err(no_easing_preset(self)),
        }
    }

    /// Set incoming/outgoing temporal ease and/or hold state on the keyframe
    /// at `tick`.
    pub fn set_keyframe_temporal_ease(
        self,
        clip: &mut Clip,
        tick: i64,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    ) -> Result<(), ModelError> {
        if let Some(inp) = in_ease {
            inp.validate()?;
        }
        if let Some(out) = out_ease {
            out.validate()?;
        }
        let (_, _, value, easing) = keyframe_entry_at(self, clip, tick)?;
        let next = easing.with_temporal_ease(in_ease, out_ease, hold);
        self.set_keyframe(clip, tick, value, next)
    }

    /// Set or clear roving on the keyframe at `tick` and return its solved
    /// clip-local tick. Roving is ignored on the first and last keyframe of a
    /// property.
    pub fn set_keyframe_roving(
        self,
        clip: &mut Clip,
        tick: i64,
        roving: bool,
    ) -> Result<i64, ModelError> {
        let (idx, len, value, easing) = keyframe_entry_at(self, clip, tick)?;
        let effective_roving = roving && idx > 0 && idx + 1 < len;
        let next = easing.with_roving(effective_roving);
        self.set_keyframe(clip, tick, value, next)?;
        Ok(keyframe_tick_at_index(self, clip, idx).unwrap_or(tick))
    }
}

/// Every row, paired with the variant it belongs to, in declaration order.
///
/// Read from [`ClipParam::CENSUS`], which the enum macro emits from the same
/// variant list as the enum, so this cannot go short while a variant ships.
pub(crate) fn clip_param_axis() -> impl Iterator<Item = (ClipParam, &'static ClipParamSpec)> {
    ClipParam::CENSUS.iter().map(|&param| (param, param.spec()))
}

/// Every directly addressed property, as `(param, catalog id, browse facts)`,
/// in `ClipParam` declaration order.
///
/// [`crate::clip_param_catalog`] builds its head rows from this, so the id,
/// label, group, kind, range and route of a direct property are declared once,
/// on the row.
pub(crate) fn direct_rows() -> impl Iterator<Item = (ClipParam, &'static str, &'static Browse)> {
    clip_param_axis().filter_map(|(param, spec)| match (spec.class, spec.browse.as_ref()) {
        (ClipParamClass::Direct(id), Some(browse)) => Some((param, id, browse)),
        _ => None,
    })
}

fn no_easing_preset_for_rect() -> ModelError {
    ModelError::InvalidParam(
        "easing presets are not supported on crop (no safe extrapolation)".into(),
    )
}

fn keyframe_entry_at(
    param: ClipParam,
    clip: &Clip,
    tick: i64,
) -> Result<(usize, usize, ParamValue, Easing), ModelError> {
    fn find<T: Copy>(
        p: &Param<T>,
        tick: i64,
        wrap: impl FnOnce(T) -> ParamValue,
    ) -> Option<(usize, usize, ParamValue, Easing)> {
        let kfs = p.keyframes();
        let i = kfs.binary_search_by_key(&tick, |kf| kf.tick).ok()?;
        Some((i, kfs.len(), wrap(kfs[i].value), kfs[i].easing))
    }
    let found = match param.route(clip) {
        ParamRoute::Scalar(p) => find(p, tick, ParamValue::Scalar),
        ParamRoute::Vec2(p) => find(p, tick, ParamValue::Vec2),
        ParamRoute::Vec3(p) => find(p, tick, ParamValue::Vec3),
        ParamRoute::Scale(p) => find(p, tick, |s| ParamValue::Vec2([s.x, s.y])),
        ParamRoute::Color(p) => find(p, tick, ParamValue::Color),
        ParamRoute::Rect(p) => find(p, tick, |r| ParamValue::Rect([r.x, r.y, r.w, r.h])),
        ParamRoute::Const(_) | ParamRoute::Absent => None,
    };
    found.ok_or_else(|| ModelError::InvalidParam(format!("no {param:?} keyframe at tick {tick}")))
}

fn keyframe_tick_at_index(param: ClipParam, clip: &Clip, idx: usize) -> Option<i64> {
    match param.route(clip) {
        ParamRoute::Scalar(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Vec2(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Vec3(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Scale(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Color(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Rect(p) => p.keyframes().get(idx).map(|kf| kf.tick),
        ParamRoute::Const(_) | ParamRoute::Absent => None,
    }
}

/// A parameter whose value is one [`Param`] on the clip: the write verbs are
/// the slot's, so the row states the field once and nothing else repeats it.
fn slot_keyframe(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    value: ParamValue,
    easing: Easing,
) -> Result<(), ModelError> {
    slot_of(clip, param)?.set_keyframe(tick, value, easing)
}

fn slot_constant(clip: &mut Clip, param: ClipParam, value: ParamValue) -> Result<(), ModelError> {
    slot_of(clip, param)?.set_constant(value)
}

fn slot_erase(clip: &mut Clip, param: ClipParam, tick: i64) -> Result<(), ModelError> {
    if slot_of(clip, param)?.remove_keyframe(tick) {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "no {param:?} keyframe at tick {tick}"
        )))
    }
}

fn slot_ease(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    preset: PiecewiseEasingPreset,
) -> Result<(), ModelError> {
    slot_of(clip, param)?.apply_easing_preset(tick, preset)
}

fn slot_of(clip: &mut Clip, param: ClipParam) -> Result<ParamSlot<'_>, ModelError> {
    let slot = param
        .spec()
        .slot
        .ok_or_else(|| ModelError::InvalidParam(format!("{param:?} has no stored parameter")))?;
    slot(clip)
}

/// A parameter the registry holds a row for but no write route: it is edited
/// through its own command, and a misrouted param command must fail loudly
/// rather than land somewhere plausible.
fn excluded_keyframe(
    _: &mut Clip,
    param: ClipParam,
    _: i64,
    _: ParamValue,
    _: Easing,
) -> Result<(), ModelError> {
    Err(not_writable(param))
}

fn excluded_constant(_: &mut Clip, param: ClipParam, _: ParamValue) -> Result<(), ModelError> {
    Err(not_writable(param))
}

fn excluded_erase(_: &mut Clip, param: ClipParam, _: i64) -> Result<(), ModelError> {
    Err(not_writable(param))
}

/// Why an easing preset cannot be applied, named from the row rather than
/// generically, so a rejected command says which of the three reasons it hit.
fn no_easing_preset(param: ClipParam) -> ModelError {
    match param.kind() {
        Some(ParamKind::Rect) => no_easing_preset_for_rect(),
        Some(ParamKind::Color) => ModelError::InvalidParam(format!(
            "easing presets are not supported on {param:?}: a color does not extrapolate"
        )),
        _ => ModelError::InvalidParam(format!("easing presets are not supported on {param:?}")),
    }
}

/// Why a param command cannot write this parameter, and what does write it.
///
/// Read off [`ClipParamClass`], exhaustively: a new exclusion states its own
/// redirection here instead of falling into a generic refusal that tells an
/// operator nothing about where the property is actually edited.
fn not_writable(param: ClipParam) -> ModelError {
    let why = match param.class() {
        ClipParamClass::Excluded(ParamExclusion::SpeedCurve) => {
            "a speed ramp is keyframed through set_clip_speed_curve, which re-derives the clip's \
             timeline duration"
        }
        ClipParamClass::Excluded(ParamExclusion::EffectSlot) => {
            "an effect slot is addressed through the clip's effect chain"
        }
        ClipParamClass::Excluded(ParamExclusion::MaskSlot) => {
            "a mask property is addressed through the clip's mask stack"
        }
        ClipParamClass::Direct(_) | ClipParamClass::Delegated(_) => "the row declares no slot",
    };
    ModelError::InvalidParam(format!(
        "{param:?} is not written through a param command: {why}"
    ))
}

/// The kind and bounds a delegated wrapper's payload declares, read from the
/// per-property catalog rather than restated per axis.
fn delegated_kind(param: ClipParam) -> Option<ParamKind> {
    param_spec(param).map(|spec| spec.kind)
}

fn delegated_range(param: ClipParam) -> ParamRange {
    param_spec(param).map_or(ParamRange::ANY, |spec| spec.range)
}

/// The route a delegated wrapper's payload names, read from the per-property
/// catalog, which is where a sub-axis declares its own fields.
fn delegated_route(clip: &Clip, param: ClipParam) -> ParamRoute<'_> {
    match param_spec(param) {
        Some(spec) => spec.resolve(clip),
        None => ParamRoute::Absent,
    }
}

/// Kind check for a delegated wrapper: the axis's own dispatcher owns the
/// range rule, so the registry checks only that the value shape can reach it.
fn delegated_validate(param: ClipParam, value: ParamValue) -> Result<(), ModelError> {
    let Some(want) = delegated_kind(param) else {
        return Ok(());
    };
    let got = match value {
        ParamValue::Scalar(_) => ParamKind::Scalar,
        ParamValue::Vec2(_) => ParamKind::Vec2,
        ParamValue::Vec3(_) => ParamKind::Vec3,
        ParamValue::Color(_) => ParamKind::Color,
        ParamValue::Rect(_) => ParamKind::Rect,
    };
    if got == want {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "{param:?} takes a {want:?} value, got {got:?}"
        )))
    }
}

/// The effect at `index` on a clip's chain, or an out-of-range error.
pub(crate) fn effect_mut(clip: &mut Clip, index: u32) -> Result<&mut EffectInstance, ModelError> {
    clip.effects
        .get_mut(index as usize)
        .ok_or_else(|| ModelError::InvalidParam(format!("effect index {index} out of range")))
}

/// A generated clip's generator, or an error for media-backed clips (generator
/// params route here; the generator itself rejects the wrong kind).
pub(crate) fn generator_mut(clip: &mut Clip) -> Result<&mut Generator, ModelError> {
    match &mut clip.content {
        ClipSource::Generated(generator) => Ok(generator),
        ClipSource::Media { .. } | ClipSource::Multicam { .. } | ClipSource::Sequence { .. } => {
            Err(ModelError::InvalidParam(
                "shape parameters apply only to generated clips".into(),
            ))
        }
    }
}

#[path = "param_axis_direct.rs"]
mod direct;

#[path = "param_axis_delegated.rs"]
mod delegated;

#[cfg(test)]
#[path = "param_axis_tests.rs"]
mod tests;
