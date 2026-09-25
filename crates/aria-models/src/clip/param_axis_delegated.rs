//! The [`ClipParam`] rows that delegate to a property axis or to an effect
//! instance.
//!
//! A wrapper variant addresses a group, not a property: `Look { param }` names
//! one of twenty-one look properties, `Effect { effect, param }` names a slot
//! on one of the clip's effects. So a row here carries no label, no group and
//! no `slot` of its own — the members declare those in
//! [`crate::clip_param_catalog`], which is also where the row reads its kind,
//! its range and its read route from. What the row does own is the four write
//! verbs: which sub-axis dispatcher a command reaches, and under which
//! precondition.
//!
//! Each row unwraps its own payload with an `if let` rather than a match over
//! `ClipParam`. A row is reached only through [`super::ClipParam::spec`], so
//! the variant always matches; the other branch states the impossible case as
//! an error instead of a panic.

use super::*;

use crate::clip::{LookParam, ParamAxis, ShapeParam, StyleParam, TextParam};
use crate::look::mask::{
    is_mask_param, mask_scalar_mut, mask_vec2_mut, remove_mask_param_keyframe,
    set_mask_param_constant, set_mask_param_keyframe,
};
use crate::look::styles::{
    remove_style_param_keyframe, set_style_param_constant, set_style_param_keyframe,
    style_scalar_mut, style_vec2_mut,
};

pub(super) static LOOK: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Delegated(ParamAxis::Look),
    scope: ParamScope::Visual,
    browse: None,
    kind: delegated_kind,
    range: delegated_range,
    read: delegated_route,
    slot: None,
    validate: delegated_validate,
    keyframe: look_keyframe,
    constant: look_constant,
    erase: look_erase,
    ease: Some(look_ease),
};

pub(super) static STYLE: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Delegated(ParamAxis::Style),
    scope: ParamScope::Visual,
    browse: None,
    kind: delegated_kind,
    range: delegated_range,
    read: delegated_route,
    slot: None,
    validate: delegated_validate,
    keyframe: |clip, param, tick, value, easing| {
        set_style_param_keyframe(&mut clip.styles, style_of(param)?, tick, value, easing)
    },
    constant: |clip, param, value| {
        set_style_param_constant(&mut clip.styles, style_of(param)?, value)
    },
    erase: |clip, param, tick| {
        remove_style_param_keyframe(&mut clip.styles, style_of(param)?, tick)
    },
    ease: Some(style_ease),
};

pub(super) static TEXT: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Delegated(ParamAxis::Text),
    scope: ParamScope::Visual,
    browse: None,
    kind: delegated_kind,
    range: delegated_range,
    read: delegated_route,
    slot: None,
    validate: delegated_validate,
    keyframe: |clip, param, tick, value, easing| {
        let param = text_of(param)?;
        generator_mut(clip)?.set_text_param_keyframe(param, tick, value, easing)
    },
    constant: |clip, param, value| {
        let param = text_of(param)?;
        generator_mut(clip)?.set_text_param_constant(param, value)
    },
    erase: |clip, param, tick| {
        let param = text_of(param)?;
        generator_mut(clip)?.remove_text_param_keyframe(param, tick)
    },
    ease: None,
};

pub(super) static SHAPE: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Delegated(ParamAxis::Shape),
    scope: ParamScope::Visual,
    browse: None,
    kind: delegated_kind,
    range: delegated_range,
    read: delegated_route,
    slot: None,
    validate: delegated_validate,
    keyframe: |clip, param, tick, value, easing| {
        let param = shape_of(param)?;
        generator_mut(clip)?.set_shape_param_keyframe(param, tick, value, easing)
    },
    constant: |clip, param, value| {
        let param = shape_of(param)?;
        generator_mut(clip)?.set_shape_param_constant(param, value)
    },
    erase: |clip, param, tick| {
        let param = shape_of(param)?;
        generator_mut(clip)?.remove_shape_param_keyframe(param, tick)
    },
    ease: None,
};

/// An effect slot exists once per effect on one clip, so the static catalog
/// holds no row for it: its kind, range and label come from
/// [`crate::effect_catalog`] through `clip_params_for`. The row declares no
/// kind for that reason, which is also what refuses an easing preset on one.
pub(super) static EFFECT: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Excluded(ParamExclusion::EffectSlot),
    scope: ParamScope::Visual,
    browse: None,
    kind: |_| None,
    range: |_| ParamRange::ANY,
    read: |clip, param| match effect_of(param) {
        Ok((effect, slot)) => crate::clip::param_catalog::effect_route(clip, effect, slot),
        Err(_) => ParamRoute::Absent,
    },
    slot: None,
    validate: delegated_validate,
    keyframe: |clip, param, tick, value, easing| {
        let (effect, slot) = effect_of(param)?;
        effect_mut(clip, effect)?.set_param_value_keyframe(slot as usize, tick, value, easing)
    },
    constant: |clip, param, value| {
        let (effect, slot) = effect_of(param)?;
        effect_mut(clip, effect)?.set_param_value_constant(slot as usize, value)
    },
    erase: |clip, param, tick| {
        let (effect, slot) = effect_of(param)?;
        effect_mut(clip, effect)?.remove_param_keyframe(slot as usize, tick)
    },
    ease: None,
};

/// The look properties on [`crate::Mask`] route through the mask helpers; the
/// rest are one scalar `Param` each, reached by name.
fn look_keyframe(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    value: ParamValue,
    easing: Easing,
) -> Result<(), ModelError> {
    let look = look_of(param)?;
    if is_mask_param(look) {
        return set_mask_param_keyframe(&mut clip.mask, look, tick, value, easing);
    }
    let v = value.scalar()?;
    validate_look_value(look, v)?;
    look_param_mut(clip, look)?.set_keyframe(tick, v, easing);
    Ok(())
}

fn look_constant(clip: &mut Clip, param: ClipParam, value: ParamValue) -> Result<(), ModelError> {
    let look = look_of(param)?;
    if is_mask_param(look) {
        return set_mask_param_constant(&mut clip.mask, look, value);
    }
    let v = value.scalar()?;
    validate_look_value(look, v)?;
    look_param_mut(clip, look)?.set_constant(v);
    Ok(())
}

fn look_erase(clip: &mut Clip, param: ClipParam, tick: i64) -> Result<(), ModelError> {
    let look = look_of(param)?;
    if is_mask_param(look) {
        return remove_mask_param_keyframe(&mut clip.mask, look, tick);
    }
    if look_param_mut(clip, look)?.remove_keyframe(tick) {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "no {look:?} keyframe at tick {tick} to remove"
        )))
    }
}

fn look_ease(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    preset: PiecewiseEasingPreset,
) -> Result<(), ModelError> {
    let look = look_of(param)?;
    if !is_mask_param(look) {
        return look_param_mut(clip, look)?.apply_easing_preset(tick, preset);
    }
    // Mask centre and size are vec2, the rest scalar. The kind comes off the
    // catalog row, so a new mask property picks the right helper by declaring
    // its kind rather than by being added to a list here.
    if param.kind() == Some(ParamKind::Vec2) {
        mask_vec2_mut(&mut clip.mask, look)?.apply_easing_preset(tick, preset)
    } else {
        mask_scalar_mut(&mut clip.mask, look)?.apply_easing_preset(tick, preset)
    }
}

fn style_ease(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    preset: PiecewiseEasingPreset,
) -> Result<(), ModelError> {
    let style = style_of(param)?;
    // Colour style properties never reach here: `ClipParam::apply_easing_preset`
    // refuses a colour row from its declared kind.
    if param.kind() == Some(ParamKind::Vec2) {
        style_vec2_mut(&mut clip.styles, style)?.apply_easing_preset(tick, preset)
    } else {
        style_scalar_mut(&mut clip.styles, style)?.apply_easing_preset(tick, preset)
    }
}

fn look_of(param: ClipParam) -> Result<LookParam, ModelError> {
    if let ClipParam::Look { param } = param {
        Ok(param)
    } else {
        Err(wrong_row(param))
    }
}

fn style_of(param: ClipParam) -> Result<StyleParam, ModelError> {
    if let ClipParam::Style { param } = param {
        Ok(param)
    } else {
        Err(wrong_row(param))
    }
}

fn text_of(param: ClipParam) -> Result<TextParam, ModelError> {
    if let ClipParam::Text { param } = param {
        Ok(param)
    } else {
        Err(wrong_row(param))
    }
}

fn shape_of(param: ClipParam) -> Result<ShapeParam, ModelError> {
    if let ClipParam::Shape { param } = param {
        Ok(param)
    } else {
        Err(wrong_row(param))
    }
}

fn effect_of(param: ClipParam) -> Result<(u32, u32), ModelError> {
    if let ClipParam::Effect {
        effect,
        param: slot,
    } = param
    {
        Ok((effect, slot))
    } else {
        Err(wrong_row(param))
    }
}

/// A row reached with a payload it does not describe. Unreachable through
/// [`super::ClipParam::spec`]; stated as an error so no path panics.
fn wrong_row(param: ClipParam) -> ModelError {
    ModelError::InvalidParam(format!("{param:?} does not belong to this parameter axis"))
}

/// Resolve a non-mask look [`LookParam`] to the scalar [`Param`] it names.
///
/// The grade arms come from the adjustment row list, so an axis a row does not
/// declare — and a row no axis declares — fails to compile here rather than
/// routing to the wrong field.
///
/// The mask geometry properties are excluded on purpose: they live on an
/// optional [`crate::Mask`] and two of them are vec2, so they route through the
/// mask helpers instead.
macro_rules! define_look_param_mut {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        fn look_param_mut(
            clip: &mut Clip,
            param: LookParam,
        ) -> Result<&mut crate::param::Param<f32>, ModelError> {
            let missing =
                |name: &str| ModelError::InvalidParam(format!("{name} is not enabled on this clip"));
            match param {
                LookParam::FilterIntensity => clip
                    .filter
                    .as_mut()
                    .map(|filter| &mut filter.intensity)
                    .ok_or_else(|| missing("filter")),
                LookParam::LutIntensity => clip
                    .lut
                    .as_mut()
                    .map(|lut| &mut lut.intensity)
                    .ok_or_else(|| missing("LUT")),
                $(LookParam::$variant => Ok(&mut clip.adjust.$field),)*
                LookParam::ChromaStrength => clip
                    .chroma_key
                    .as_mut()
                    .map(|chroma| &mut chroma.strength)
                    .ok_or_else(|| missing("chroma key")),
                LookParam::ChromaShadow => clip
                    .chroma_key
                    .as_mut()
                    .map(|chroma| &mut chroma.shadow)
                    .ok_or_else(|| missing("chroma key")),
                LookParam::MatteSoftness => clip
                    .matte
                    .as_mut()
                    .map(|matte| &mut matte.softness)
                    .ok_or_else(|| missing("track matte")),
                LookParam::MaskFeather
                | LookParam::MaskCenter
                | LookParam::MaskSize
                | LookParam::MaskRotation
                | LookParam::MaskRoundness => Err(ModelError::InvalidParam(format!(
                    "look parameter {param:?} is routed through the mask param helpers"
                ))),
            }
        }
    };
}

crate::adjust_slider_rows!(define_look_param_mut);

/// Range-check a scalar look value (matches the block's own `validate`).
///
/// A grade axis takes its range from the slider its row declares, so the axis
/// and the grade's own `validate` cannot disagree about what is in range.
macro_rules! define_validate_look_value {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        fn validate_look_value(param: LookParam, value: f32) -> Result<(), ModelError> {
            let valid = match param {
                LookParam::FilterIntensity
                | LookParam::LutIntensity
                | LookParam::ChromaStrength
                | LookParam::ChromaShadow
                | LookParam::MatteSoftness => (0.0..=1.0).contains(&value),
                $(LookParam::$variant => crate::ColorAdjustments::slider_of(param)
                    .is_some_and(|slider| (slider.min..=slider.max).contains(&value)),)*
                LookParam::MaskFeather
                | LookParam::MaskCenter
                | LookParam::MaskSize
                | LookParam::MaskRotation
                | LookParam::MaskRoundness => {
                    return Err(ModelError::InvalidParam(format!(
                        "look parameter {param:?} is routed through the mask param helpers"
                    )));
                }
            };
            if value.is_finite() && valid {
                Ok(())
            } else {
                Err(ModelError::InvalidParam(format!(
                    "look parameter {param:?} = {value} is out of range"
                )))
            }
        }
    };
}

crate::adjust_slider_rows!(define_validate_look_value);
