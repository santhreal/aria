//! `Clip` parameter naming: the Python property names, the value coercion, and
//! the keyframe writes they drive.
//!
//! Declared from `clip.rs` with `#[path]`; `use super::*` reaches the parent's
//! imports, so the move changed no name resolution. Everything here reads the
//! model's `ClipParam` registry rather than restating which properties exist.

use super::*;

/// Resolve a Python property name to a [`ClipParam`].
///
/// The short aliases below are this binding's own naming surface and stay. What
/// does not stay is a second list of *which* properties exist: anything else is
/// looked up by its registry id, so a property added to the model is animatable
/// from Python without a row here.
pub(super) fn clip_param(name: &str) -> PyResult<ClipParam> {
    Ok(match name {
        "position" => ClipParam::Position,
        "anchor" | "anchor_point" => ClipParam::AnchorPoint,
        "scale" => ClipParam::Scale,
        "rotation" => ClipParam::Rotation,
        "opacity" => ClipParam::Opacity,
        "volume" => ClipParam::Volume,
        "width" => ClipParam::Shape {
            param: ShapeParam::Width,
        },
        "height" => ClipParam::Shape {
            param: ShapeParam::Height,
        },
        "corner_radius" => ClipParam::Shape {
            param: ShapeParam::CornerRadius,
        },
        "inner_ratio" => ClipParam::Shape {
            param: ShapeParam::InnerRatio,
        },
        "fill" | "color" => ClipParam::Shape {
            param: ShapeParam::Fill,
        },
        "stroke_color" => ClipParam::Shape {
            param: ShapeParam::StrokeColor,
        },
        "stroke_width" => ClipParam::Shape {
            param: ShapeParam::StrokeWidth,
        },
        other => {
            // Ids are hyphenated (`anchor-point`, `mask-0-feather`); Python
            // names are not, so try both spellings of the same id.
            let hyphenated = other.replace('_', "-");
            return aria_models::property_row(other)
                .or_else(|| aria_models::property_row(&hyphenated))
                .map(|spec| spec.param)
                .ok_or_else(|| {
                    PyValueError::new_err(format!("unknown animatable property {other:?}"))
                });
        }
    })
}

/// A `set_style` keyword on a shape clip → the shape parameter + constant value.
///
/// The name resolves through [`clip_param`], so the shape axis keeps no second
/// id table, and the value's kind comes off the parameter's own registry row
/// rather than a colour-or-scalar guess made here.
pub(super) fn shape_style_param(
    name: &str,
    value: &Bound<'_, PyAny>,
) -> PyResult<(ShapeParam, ParamValue)> {
    let Ok(ClipParam::Shape { param }) = clip_param(name) else {
        return Err(PyValueError::new_err(format!(
            "unknown shape style parameter {name:?}"
        )));
    };
    let value = param_value_for(ClipParam::Shape { param }, value)?;
    Ok((param, value))
}

/// Coerce a Python value to the kind the parameter's registry row declares.
///
/// Kind and writability come off the model's `ClipParam` registry. This used to
/// be a second variant list, and it had already gone short: five of the model's
/// fourteen parameters had no arm at all, and per-axis scale could not be
/// expressed because the list mapped `scale` to a scalar.
pub(super) fn param_value_for(param: ClipParam, value: &Bound<'_, PyAny>) -> PyResult<ParamValue> {
    let refusal = match param.class() {
        aria_models::ClipParamClass::Excluded(aria_models::ParamExclusion::EffectSlot) => {
            Some("use Effect.animate for effect parameters")
        }
        aria_models::ClipParamClass::Excluded(aria_models::ParamExclusion::SpeedCurve) => {
            Some("speed curves are not exposed yet")
        }
        // A mask property is addressed by name (`mask_0_feather`) and its kind
        // comes off the parameter like any other.
        aria_models::ClipParamClass::Excluded(aria_models::ParamExclusion::MaskSlot)
        | aria_models::ClipParamClass::Direct(_)
        | aria_models::ClipParamClass::Delegated(_) => None,
    };
    if let Some(refusal) = refusal {
        return Err(PyValueError::new_err(refusal));
    }
    let Some(kind) = param.kind() else {
        return Err(PyValueError::new_err(format!(
            "{param:?} declares no value kind"
        )));
    };
    Ok(match kind {
        aria_models::ParamKind::Scalar => {
            let v = value.extract::<f32>()?;
            if !v.is_finite() {
                return Err(PyValueError::new_err("parameter value must be finite"));
            }
            ParamValue::Scalar(v)
        }
        aria_models::ParamKind::Vec2 => match value.extract::<(f32, f32)>() {
            Ok((x, y)) => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(PyValueError::new_err("parameter value must be finite"));
                }
                ParamValue::Vec2([x, y])
            }
            // A bare number where the model widens one (uniform scale). Where it
            // does not, the model names the kind it wanted, so the tuple error is
            // only surfaced when the value is neither.
            Err(pair) => match value.extract::<f32>() {
                Ok(v) => {
                    if !v.is_finite() {
                        return Err(PyValueError::new_err("parameter value must be finite"));
                    }
                    ParamValue::Scalar(v)
                }
                Err(_) => return Err(pair),
            },
        },
        aria_models::ParamKind::Vec3 => {
            let (x, y, z) = value.extract::<(f32, f32, f32)>()?;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(PyValueError::new_err("parameter value must be finite"));
            }
            ParamValue::Vec3([x, y, z])
        }
        aria_models::ParamKind::Color => ParamValue::Color(parse_color(value)?),
        aria_models::ParamKind::Rect => {
            let (x, y, w, h) = value.extract::<(f32, f32, f32, f32)>()?;
            if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
                return Err(PyValueError::new_err("parameter value must be finite"));
            }
            ParamValue::Rect([x, y, w, h])
        }
    })
}

/// The parameter's current value on the clip, read through its registry row.
///
/// The row owns where the value lives and whether it is reachable at all — a
/// mask's feather has no route until that mask exists, a stroke width none until
/// the shape has a stroke. The clip's own rows are walked because a mask
/// property has no static row.
pub(super) fn sample_param_at_clip_start(
    project: &Project,
    id: ClipId,
    param: &ClipParam,
) -> PyResult<ParamValue> {
    let clip = Clip::require(project, id)?;
    let mut held = None;
    aria_models::visit_clip_params(clip, |spec| {
        if held.is_none() && spec.param == *param {
            held = spec.sample(clip, 0);
        }
    });
    held.ok_or_else(|| {
        PyValueError::new_err(format!("{param:?} has no value on this clip to hold"))
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_clip_animation(
    project: &mut Project,
    clip_id: ClipId,
    clip_span: (i64, i64),
    rate: aria_models::Rational,
    name: &str,
    value: &Bound<'_, PyAny>,
    at: Option<f64>,
    default_easing: Easing,
) -> PyResult<()> {
    let (clip_start, clip_dur) = clip_span;
    let param = clip_param(name)?;
    if value.is_instance_of::<PyList>() {
        let list = value.cast::<PyList>()?;
        let pairs = parse_keyframe_pairs(value.py(), list, default_easing, |v| {
            param_value_for(param, v)
        })?;
        for (rel, val, easing) in pairs {
            let abs = clip_key_time(clip_start, clip_dur, rel, rate);
            project
                .model_mut()
                .set_param_keyframe(clip_id, param, abs, val, easing, None)
                .map_err(model_err)?;
        }
        return Ok(());
    }
    let at = at
        .ok_or_else(|| PyValueError::new_err(format!("single keyframe for {name} requires at=")))?;
    let val = param_value_for(param, value)?;
    let abs = clip_key_time(clip_start, clip_dur, at, rate);
    project
        .model_mut()
        .set_param_keyframe(clip_id, param, abs, val, default_easing, None)
        .map_err(model_err)
}

pub(super) fn parse_animation_slot(slot: &str) -> PyResult<AnimationSlot> {
    match slot {
        "in" => Ok(AnimationSlot::In),
        "out" => Ok(AnimationSlot::Out),
        "combo" => Ok(AnimationSlot::Combo),
        _ => Err(PyValueError::new_err(
            "slot must be one of: 'in', 'out', 'combo'",
        )),
    }
}
