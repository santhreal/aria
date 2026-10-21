//! Value checks against an [`EffectParamSpec`], shared by video, transition
//! and audio effects.

use super::curve::{EffectCurvePoint, EffectGradientStop};
use super::params::{EffectParamKind, EffectParamSpec};
use crate::error::ModelError;

/// The parameter rules shared by video, transition, and audio effects, for `id`.
pub(crate) fn check_params(id: &str, params: &[EffectParamSpec]) -> Result<(), String> {
    for (index, param) in params.iter().enumerate() {
        if param.name.is_empty() {
            return Err(format!("{id} parameter {index} has no name"));
        }
        if params[..index].iter().any(|p| p.name == param.name) {
            return Err(format!("{id} repeats parameter {}", param.name));
        }
        if param.kind.has_float_range() && param.min > param.max {
            return Err(format!("{id}.{} has an inverted range", param.name));
        }
        if param.kind == EffectParamKind::Int && param.int_min > param.int_max {
            return Err(format!("{id}.{} has an inverted range", param.name));
        }
        if let (Some(smin), Some(smax)) = (param.soft_min, param.soft_max)
            && smin > smax
        {
            return Err(format!("{id}.{} has an inverted soft range", param.name));
        }
        if param.kind == EffectParamKind::Choice && param.choices.is_empty() {
            return Err(format!(
                "{id}.{} choice parameter has no choices",
                param.name
            ));
        }
    }
    Ok(())
}

/// Kinds stored in the scalar map: scalar, angle, and integer.
pub(crate) fn holds_scalar(kind: EffectParamKind) -> bool {
    matches!(
        kind,
        EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
    )
}

/// Kinds stored in the two-component map: vector and size.
pub(crate) fn holds_vec2(kind: EffectParamKind) -> bool {
    matches!(kind, EffectParamKind::Vec2 | EffectParamKind::Size)
}

/// Kinds stored in the string map: text, font, and file.
pub(crate) fn holds_text(kind: EffectParamKind) -> bool {
    matches!(
        kind,
        EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
    )
}

/// The catalog spec of the stored parameter `name`.
///
/// Fails when `params` has no parameter of that name, or when its kind is not
/// one that `store`, the field the value was read from, holds. `owner` is
/// `"effect"` or `"transition"` and `id` its catalog id, for the message.
pub(crate) fn stored_slot<'s>(
    params: &'s [EffectParamSpec],
    owner: &str,
    id: &str,
    name: &str,
    store: &str,
    holds: fn(EffectParamKind) -> bool,
) -> Result<&'s EffectParamSpec, ModelError> {
    let pspec = params.iter().find(|p| p.name == name).ok_or_else(|| {
        ModelError::InvalidParam(format!("{owner} '{id}' has no parameter '{name}'"))
    })?;
    if holds(pspec.kind) {
        Ok(pspec)
    } else {
        Err(ModelError::InvalidParam(format!(
            "{owner} parameter '{name}' is {:?}, not stored in {store}",
            pspec.kind
        )))
    }
}

pub(crate) fn choice_check(pspec: &EffectParamSpec, choice_id: &str) -> Result<(), ModelError> {
    if pspec.choice_index(choice_id).is_none() {
        return Err(ModelError::InvalidParam(format!(
            "{} = '{choice_id}' is not a valid choice",
            pspec.name
        )));
    }
    Ok(())
}

pub(crate) fn range_check(pspec: &EffectParamSpec, value: f32) -> Result<(), ModelError> {
    if !value.is_finite() || value < pspec.min || value > pspec.max {
        return Err(ModelError::InvalidParam(format!(
            "{} = {value} out of range [{}, {}]",
            pspec.name, pspec.min, pspec.max
        )));
    }
    Ok(())
}

pub(crate) fn scalar_or_int_range_check(
    pspec: &EffectParamSpec,
    value: f32,
) -> Result<(), ModelError> {
    if pspec.kind == EffectParamKind::Int {
        let min = pspec.int_min as f32;
        let max = pspec.int_max as f32;
        if !value.is_finite() || value < min || value > max {
            return Err(ModelError::InvalidParam(format!(
                "{} = {value} out of range [{}, {}]",
                pspec.name, pspec.int_min, pspec.int_max
            )));
        }
        Ok(())
    } else {
        range_check(pspec, value)
    }
}

pub(crate) fn int_range_check(pspec: &EffectParamSpec, value: i64) -> Result<(), ModelError> {
    if value < pspec.int_min || value > pspec.int_max {
        return Err(ModelError::InvalidParam(format!(
            "{} = {value} out of range [{}, {}]",
            pspec.name, pspec.int_min, pspec.int_max
        )));
    }
    Ok(())
}

pub(crate) fn linear_color_check(
    pspec: &EffectParamSpec,
    value: [f32; 4],
) -> Result<(), ModelError> {
    if value.iter().any(|c| !c.is_finite()) {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite linear color component",
            pspec.name
        )));
    }
    Ok(())
}

/// Every component of a `Vec2` or `Point3` value finite and inside the
/// parameter's `[min, max]`.
pub(crate) fn vector_range_check(pspec: &EffectParamSpec, value: &[f32]) -> Result<(), ModelError> {
    for (i, &component) in value.iter().enumerate() {
        if !component.is_finite() || component < pspec.min || component > pspec.max {
            return Err(ModelError::InvalidParam(format!(
                "{}[{i}] = {component} out of range [{}, {}]",
                pspec.name, pspec.min, pspec.max
            )));
        }
    }
    Ok(())
}

pub(crate) fn curve_check(
    pspec: &EffectParamSpec,
    points: &[EffectCurvePoint],
) -> Result<(), ModelError> {
    if points
        .iter()
        .any(|pt| !pt.x.is_finite() || !pt.y.is_finite())
    {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite curve point",
            pspec.name
        )));
    }
    Ok(())
}

pub(crate) fn gradient_check(
    pspec: &EffectParamSpec,
    stops: &[EffectGradientStop],
) -> Result<(), ModelError> {
    if stops
        .iter()
        .any(|s| !s.offset.is_finite() || s.color.iter().any(|c| !c.is_finite()))
    {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite gradient stop",
            pspec.name
        )));
    }
    Ok(())
}
