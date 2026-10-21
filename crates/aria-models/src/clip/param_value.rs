//! The value a parameter command writes, and its conversions to typed
//! property values.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::ClipParam;
use super::transform::Scale2;
use crate::clip::CropRect;
use crate::error::ModelError;
use crate::param::{Keyframe, Lerp, SegmentSample};

/// A value for a [`ClipParam`]: scalar properties take `Scalar`, `position`
/// / per-axis `scale` take `Vec2` (scale also accepts `Scalar` → uniform),
/// color properties (shape fill/stroke) take `Color`, and crop (and any
/// future 4-float rect) take `Rect` as `[x, y, w, h]`. Commands carry this
/// so one command shape serves every param kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValue {
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Color([u8; 4]),
    /// Axis-aligned rect as `[x, y, w, h]` (e.g. [`ClipParam::Crop`]).
    Rect([f32; 4]),
}

impl ParamValue {
    pub(crate) fn scalar(self) -> Result<f32, ModelError> {
        match self {
            ParamValue::Scalar(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a scalar value".into())),
        }
    }

    pub(crate) fn vec2(self) -> Result<[f32; 2], ModelError> {
        match self {
            ParamValue::Vec2(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a vec2 value".into())),
        }
    }

    pub(crate) fn vec3(self) -> Result<[f32; 3], ModelError> {
        match self {
            ParamValue::Vec3(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a vec3 value".into())),
        }
    }

    pub(crate) fn color(self) -> Result<[u8; 4], ModelError> {
        match self {
            ParamValue::Color(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a color value".into())),
        }
    }

    pub(crate) fn rect(self) -> Result<[f32; 4], ModelError> {
        match self {
            ParamValue::Rect(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a rect value".into())),
        }
    }
}

impl Lerp for ParamValue {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (Self::Scalar(x), Self::Scalar(y)) => Self::Scalar(f32::lerp(x, y, t)),
            (Self::Vec2(x), Self::Vec2(y)) => Self::Vec2(<[f32; 2]>::lerp(x, y, t)),
            (Self::Vec3(x), Self::Vec3(y)) => Self::Vec3(<[f32; 3]>::lerp(x, y, t)),
            (Self::Color(x), Self::Color(y)) => Self::Color(<[u8; 4]>::lerp(x, y, t)),
            (Self::Rect(x), Self::Rect(y)) => Self::Rect([
                f32::lerp(x[0], y[0], t),
                f32::lerp(x[1], y[1], t),
                f32::lerp(x[2], y[2], t),
                f32::lerp(x[3], y[3], t),
            ]),
            (first, _) => first,
        }
    }
}

impl SegmentSample for ParamValue {
    fn segment_sample(a: &Keyframe<Self>, b: &Keyframe<Self>, eased_t: f32) -> Self {
        if let (Self::Vec2(va), Self::Vec2(vb)) = (a.value, b.value) {
            let ka = Keyframe {
                tick: a.tick,
                value: va,
                easing: a.easing,
                tangents: a.tangents,
            };
            let kb = Keyframe {
                tick: b.tick,
                value: vb,
                easing: b.easing,
                tangents: b.tangents,
            };
            return Self::Vec2(<[f32; 2]>::segment_sample(&ka, &kb, eased_t));
        }
        Lerp::lerp(a.value, b.value, eased_t)
    }

    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        match (a.value, b.value) {
            (Self::Scalar(va), Self::Scalar(vb)) => vb - va,
            (Self::Vec2(va), Self::Vec2(vb)) => {
                let ka = Keyframe {
                    tick: a.tick,
                    value: va,
                    easing: a.easing,
                    tangents: a.tangents,
                };
                let kb = Keyframe {
                    tick: b.tick,
                    value: vb,
                    easing: b.easing,
                    tangents: b.tangents,
                };
                <[f32; 2]>::segment_delta(&ka, &kb)
            }
            (Self::Vec3(va), Self::Vec3(vb)) => {
                let ka = Keyframe {
                    tick: a.tick,
                    value: va,
                    easing: a.easing,
                    tangents: a.tangents,
                };
                let kb = Keyframe {
                    tick: b.tick,
                    value: vb,
                    easing: b.easing,
                    tangents: b.tangents,
                };
                <[f32; 3]>::segment_delta(&ka, &kb)
            }
            (Self::Color(va), Self::Color(vb)) => {
                let ka = Keyframe::new(a.tick, va, a.easing);
                let kb = Keyframe::new(b.tick, vb, b.easing);
                <[u8; 4]>::segment_delta(&ka, &kb)
            }
            (Self::Rect(va), Self::Rect(vb)) => {
                let dx = vb[0] - va[0];
                let dy = vb[1] - va[1];
                let dw = vb[2] - va[2];
                let dh = vb[3] - va[3];
                (dx * dx + dy * dy + dw * dw + dh * dh).sqrt()
            }
            _ => 0.0,
        }
    }

    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        Self::segment_delta(a, b).abs()
    }
}

/// Accept scalar (uniform) or vec2 (per-axis) for [`ClipParam::Scale`].
pub(crate) fn scale_value(value: ParamValue) -> Result<Scale2, ModelError> {
    match value {
        ParamValue::Scalar(v) => Ok(Scale2::uniform(v)),
        ParamValue::Vec2(v) => Ok(Scale2::from(v)),
        ParamValue::Vec3(_) | ParamValue::Color(_) | ParamValue::Rect(_) => Err(
            ModelError::InvalidParam("expected a scalar or vec2 scale value".into()),
        ),
    }
}

/// Unwrap a rect [`ParamValue`] as a validated [`CropRect`].
pub(crate) fn crop_rect(value: ParamValue) -> Result<CropRect, ModelError> {
    let [x, y, w, h] = value.rect()?;
    let crop = CropRect { x, y, w, h };
    crop.validate()?;
    Ok(crop)
}
