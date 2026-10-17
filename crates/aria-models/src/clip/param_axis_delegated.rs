//! The [`ClipParam`] rows that delegate to a property axis or to an effect
//! instance.
//!
//! A wrapper variant addresses a group, not a property: `Look { param }` names
//! one of the look properties, `Effect { effect, param }` names a slot on one
//! of the clip's effects, `Mask { mask, param }` names a property of one of its
//! masks. So a row here carries no label, no group and
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

use crate::clip::{
    LookParam, MaskParam, ParamAxis, SHAPE_DROP_HEIGHT, SHAPE_DROP_WIDTH, ShapeParam, ShapeStroke,
    StyleParam, TextBackground, TextParam, TextShadow, TextStroke, TextStyle,
};
use crate::look::mask::{mask_mut, validate_mask_value};
use crate::look::styles::{
    remove_style_param_keyframe, set_style_param_constant, set_style_param_keyframe,
    style_scalar_mut, style_vec2_mut,
};
use crate::look::{
    ChromaKey, ColorAdjustments, Filter, LayerBackground, LayerGlow, LayerOutline, LayerShadow,
    Lut, Mask, MaskKind, TrackMatte,
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
    default: |_, param| {
        look_of(param)
            .ok()
            .map(|param| ParamValue::Scalar(look_default(param)))
    },
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
    default: |_, param| style_of(param).ok().map(style_default),
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
    default: |_, param| text_of(param).ok().map(text_default),
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
    default: |_, param| shape_of(param).ok().map(shape_default),
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
    default: |clip, param| {
        let (effect, slot) = effect_of(param).ok()?;
        crate::clip::param_catalog::effect_default(clip, effect, slot)
    },
};

/// A mask property exists once per mask on one clip, so the static catalog
/// holds no row for it: [`crate::clip_params_for`] builds one per property of
/// each mask. The kind and range come off [`MaskParam`], and the stored
/// `Param` is reached through the mask's index.
pub(super) static MASK: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Excluded(ParamExclusion::MaskSlot),
    scope: ParamScope::Visual,
    browse: None,
    kind: |param| mask_of(param).ok().map(|(_, param)| param.kind()),
    range: |param| mask_of(param).map_or(ParamRange::ANY, |(_, param)| param.range()),
    read: |clip, param| match mask_of(param) {
        Ok((mask, param)) => crate::clip::param_catalog::mask_route(clip, mask, param),
        Err(_) => ParamRoute::Absent,
    },
    slot: None,
    validate: |param, value| validate_mask_value(mask_of(param)?.1, value),
    keyframe: |clip, param, tick, value, easing| {
        mask_slot(clip, param)?.set_keyframe(tick, value, easing)
    },
    constant: |clip, param, value| mask_slot(clip, param)?.set_constant(value),
    erase: |clip, param, tick| {
        if mask_slot(clip, param)?.remove_keyframe(tick) {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "no {param:?} keyframe at tick {tick}"
            )))
        }
    },
    ease: Some(|clip, param, tick, preset| {
        mask_slot(clip, param)?.apply_easing_preset(tick, preset)
    }),
    default: |_, param| mask_of(param).ok().map(|(_, param)| mask_default(param)),
};

/// The stored `Param` a mask property names.
fn mask_slot(clip: &mut Clip, param: ClipParam) -> Result<ParamSlot<'_>, ModelError> {
    let (index, param) = mask_of(param)?;
    let mask = mask_mut(&mut clip.masks, index)?;
    Ok(match param {
        MaskParam::Feather => ParamSlot::Scalar(&mut mask.feather),
        MaskParam::Center => ParamSlot::Vec2(&mut mask.center),
        MaskParam::Size => ParamSlot::Vec2(&mut mask.size),
        MaskParam::Rotation => ParamSlot::Scalar(&mut mask.rotation),
        MaskParam::Roundness => ParamSlot::Scalar(&mut mask.roundness),
        MaskParam::Opacity => ParamSlot::Scalar(&mut mask.opacity),
        MaskParam::Expansion => ParamSlot::Scalar(&mut mask.expansion),
    })
}

/// Each look property is one scalar `Param`, reached by name.
fn look_keyframe(
    clip: &mut Clip,
    param: ClipParam,
    tick: i64,
    value: ParamValue,
    easing: Easing,
) -> Result<(), ModelError> {
    let look = look_of(param)?;
    let v = value.scalar()?;
    validate_look_value(look, v)?;
    look_param_mut(clip, look)?.set_keyframe(tick, v, easing);
    Ok(())
}

fn look_constant(clip: &mut Clip, param: ClipParam, value: ParamValue) -> Result<(), ModelError> {
    let look = look_of(param)?;
    let v = value.scalar()?;
    validate_look_value(look, v)?;
    look_param_mut(clip, look)?.set_constant(v);
    Ok(())
}

fn look_erase(clip: &mut Clip, param: ClipParam, tick: i64) -> Result<(), ModelError> {
    let look = look_of(param)?;
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
    look_param_mut(clip, look)?.apply_easing_preset(tick, preset)
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

fn mask_of(param: ClipParam) -> Result<(u32, MaskParam), ModelError> {
    if let ClipParam::Mask { mask, param } = param {
        Ok((mask, param))
    } else {
        Err(wrong_row(param))
    }
}

/// A row reached with a payload it does not describe. Unreachable through
/// [`super::ClipParam::spec`]; stated as an error so no path panics.
fn wrong_row(param: ClipParam) -> ModelError {
    ModelError::InvalidParam(format!("{param:?} does not belong to this parameter axis"))
}

/// What a reset puts a style property back to: the block as it is first
/// made.
fn style_default(param: StyleParam) -> ParamValue {
    match param {
        StyleParam::ShadowColor => ParamValue::Color(LayerShadow::default().rgba.sample(0)),
        StyleParam::ShadowOffset => ParamValue::Vec2(LayerShadow::default().offset.sample(0)),
        StyleParam::ShadowBlur => ParamValue::Scalar(LayerShadow::default().blur.sample(0)),
        StyleParam::GlowColor => ParamValue::Color(LayerGlow::default().rgba.sample(0)),
        StyleParam::GlowRadius => ParamValue::Scalar(LayerGlow::default().radius.sample(0)),
        StyleParam::GlowIntensity => ParamValue::Scalar(LayerGlow::default().intensity.sample(0)),
        StyleParam::OutlineColor => ParamValue::Color(LayerOutline::default().rgba.sample(0)),
        StyleParam::OutlineWidth => ParamValue::Scalar(LayerOutline::default().width.sample(0)),
        StyleParam::BackgroundColor => ParamValue::Color(LayerBackground::default().rgba.sample(0)),
        StyleParam::BackgroundPadding => {
            ParamValue::Scalar(LayerBackground::default().padding.sample(0))
        }
        StyleParam::BackgroundRadius => {
            ParamValue::Scalar(LayerBackground::default().radius.sample(0))
        }
    }
}

/// What a reset puts a title's property back to: a new title's style, and a
/// stroke, shadow or card as each is first added.
fn text_default(param: TextParam) -> ParamValue {
    match param {
        TextParam::Size => ParamValue::Scalar(TextStyle::default().size.sample(0)),
        TextParam::Fill => ParamValue::Color(TextStyle::default().fill.sample(0)),
        TextParam::LetterSpacing => {
            ParamValue::Scalar(TextStyle::default().letter_spacing.sample(0))
        }
        TextParam::LineSpacing => ParamValue::Scalar(TextStyle::default().line_spacing.sample(0)),
        TextParam::StrokeWidth => ParamValue::Scalar(TextStroke::default().width.sample(0)),
        TextParam::StrokeColor => ParamValue::Color(TextStroke::default().rgba.sample(0)),
        TextParam::ShadowBlur => ParamValue::Scalar(TextShadow::default().blur.sample(0)),
        TextParam::ShadowDistance => ParamValue::Scalar(TextShadow::default().distance.sample(0)),
        TextParam::ShadowColor => ParamValue::Color(TextShadow::default().rgba.sample(0)),
        TextParam::BackgroundColor => ParamValue::Color(TextBackground::default().rgba.sample(0)),
        TextParam::BackgroundRadius => {
            ParamValue::Scalar(TextBackground::default().radius.sample(0))
        }
    }
}

/// What a reset puts a shape's property back to: a freshly dropped shape,
/// and a stroke as it is first added.
fn shape_default(param: ShapeParam) -> ParamValue {
    match param {
        ShapeParam::Width => ParamValue::Scalar(SHAPE_DROP_WIDTH),
        ShapeParam::Height => ParamValue::Scalar(SHAPE_DROP_HEIGHT),
        ShapeParam::CornerRadius => {
            ParamValue::Scalar(crate::clip::generator::zero_param().sample(0))
        }
        ShapeParam::InnerRatio => {
            ParamValue::Scalar(crate::clip::generator::default_star_inner().sample(0))
        }
        ShapeParam::Fill => {
            ParamValue::Color(crate::clip::generator::default_shape_rgba().sample(0))
        }
        ShapeParam::StrokeColor => ParamValue::Color(ShapeStroke::default().rgba.sample(0)),
        ShapeParam::StrokeWidth => ParamValue::Scalar(ShapeStroke::default().width.sample(0)),
    }
}

/// What a reset puts a mask's property back to: a mask as it is first drawn.
fn mask_default(param: MaskParam) -> ParamValue {
    let fresh = Mask::new(MaskKind::Rectangle);
    match param {
        MaskParam::Feather => ParamValue::Scalar(fresh.feather.sample(0)),
        MaskParam::Center => ParamValue::Vec2(fresh.center.sample(0)),
        MaskParam::Size => ParamValue::Vec2(fresh.size.sample(0)),
        MaskParam::Rotation => ParamValue::Scalar(fresh.rotation.sample(0)),
        MaskParam::Roundness => ParamValue::Scalar(fresh.roundness.sample(0)),
        MaskParam::Opacity => ParamValue::Scalar(fresh.opacity.sample(0)),
        MaskParam::Expansion => ParamValue::Scalar(fresh.expansion.sample(0)),
    }
}

/// What a reset puts a look property back to: a grade slider at its neutral,
/// a filter or LUT at the strength it is applied at, and a key or matte as it
/// is first made.
///
/// The grade arms come from the adjustment row list, so a slider cannot be
/// added without a value to reset to.
macro_rules! define_look_default {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        fn look_default(param: LookParam) -> f32 {
            match param {
                LookParam::FilterIntensity => Filter::new(String::new()).intensity.sample(0),
                LookParam::LutIntensity => Lut::new(String::new()).intensity.sample(0),
                $(LookParam::$variant => ColorAdjustments::default().$field.sample(0),)*
                LookParam::ChromaStrength => ChromaKey::new([0, 0, 0]).strength.sample(0),
                LookParam::ChromaShadow => ChromaKey::new([0, 0, 0]).shadow.sample(0),
                LookParam::MatteSoftness => TrackMatte::alpha().softness.sample(0),
            }
        }
    };
}

crate::adjust_slider_rows!(define_look_default);

/// Resolve a look [`LookParam`] to the scalar [`Param`] it names.
///
/// The grade arms come from the adjustment row list, so an axis a row does not
/// declare — and a row no axis declares — fails to compile here rather than
/// routing to the wrong field.
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
