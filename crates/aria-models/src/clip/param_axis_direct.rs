//! The [`ClipParam`] rows that address one property of the clip directly.
//!
//! Each row states its field twice and nothing else states it at all: once as
//! `read` (borrowed, for sampling and the keyframe strip) and once as `slot`
//! (borrowed mutably, for the four write verbs). The verbs themselves are the
//! shared `slot_*` walkers in the parent module, so a row here is facts only.
//!
//! [`super::ClipParam::spec`] is the exhaustive map that reaches these, so a
//! new direct property is one `const` here plus one arm there.

use super::*;

use crate::clip::transform::{
    ClipTransform, validate_anchor_point, validate_opacity, validate_position, validate_rotation,
    validate_scale,
};
use crate::clip::{MAX_CLIP_PAN, MAX_CLIP_VOLUME, MAX_SPEED, MIN_CLIP_PAN, MIN_SPEED};

const UNIT: ParamRange = ParamRange::between(0.0, 1.0);

pub(super) static POSITION: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("position"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Position",
        group: "transform",
        kind: ParamKind::Vec2,
        range: ParamRange::ANY,
    }),
    kind: |_| Some(ParamKind::Vec2),
    range: |_| ParamRange::ANY,
    read: |c, _| ParamRoute::Vec2(&c.transform.position),
    slot: Some(|c| Ok(ParamSlot::Vec2(&mut c.transform.position))),
    validate: |_, value| validate_position(&value.vec2()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Vec2(ClipTransform::IDENTITY.position)),
};

pub(super) static ANCHOR_POINT: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("anchor-point"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Anchor point",
        group: "transform",
        kind: ParamKind::Vec2,
        range: ParamRange::ANY,
    }),
    kind: |_| Some(ParamKind::Vec2),
    range: |_| ParamRange::ANY,
    read: |c, _| ParamRoute::Vec2(&c.transform.anchor_point),
    slot: Some(|c| Ok(ParamSlot::Vec2(&mut c.transform.anchor_point))),
    validate: |_, value| validate_anchor_point(&value.vec2()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Vec2(ClipTransform::IDENTITY.anchor_point)),
};

pub(super) static SCALE: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("scale"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Scale",
        group: "transform",
        kind: ParamKind::Vec2,
        range: ParamRange::above(f32::EPSILON),
    }),
    kind: |_| Some(ParamKind::Vec2),
    range: |_| ParamRange::above(f32::EPSILON),
    read: |c, _| ParamRoute::Scale(&c.transform.scale),
    slot: Some(|c| Ok(ParamSlot::Scale(&mut c.transform.scale))),
    validate: |_, value| validate_scale(scale_value(value)?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Vec2(ClipTransform::IDENTITY.scale.into())),
};

pub(super) static ROTATION: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("rotation"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Rotation",
        group: "transform",
        kind: ParamKind::Scalar,
        range: ParamRange::ANY,
    }),
    kind: |_| Some(ParamKind::Scalar),
    range: |_| ParamRange::ANY,
    read: |c, _| ParamRoute::Scalar(&c.transform.rotation),
    slot: Some(|c| Ok(ParamSlot::Scalar(&mut c.transform.rotation))),
    validate: |_, value| validate_rotation(value.scalar()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Scalar(ClipTransform::IDENTITY.rotation)),
};

pub(super) static OPACITY: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("opacity"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Opacity",
        group: "transform",
        kind: ParamKind::Scalar,
        range: UNIT,
    }),
    kind: |_| Some(ParamKind::Scalar),
    range: |_| UNIT,
    read: |c, _| ParamRoute::Scalar(&c.transform.opacity),
    slot: Some(|c| Ok(ParamSlot::Scalar(&mut c.transform.opacity))),
    validate: |_, value| validate_opacity(value.scalar()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Scalar(ClipTransform::IDENTITY.opacity)),
};

pub(super) static CROP: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("crop"),
    scope: ParamScope::Visual,
    browse: Some(Browse {
        label: "Crop",
        group: "framing",
        kind: ParamKind::Rect,
        range: ParamRange::ANY,
    }),
    kind: |_| Some(ParamKind::Rect),
    range: |_| ParamRange::ANY,
    read: |c, _| ParamRoute::Rect(&c.crop),
    slot: Some(|c| Ok(ParamSlot::Rect(&mut c.crop))),
    validate: |_, value| crop_rect(value).map(|_| ()),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| {
        let CropRect { x, y, w, h } = CropRect::FULL;
        Some(ParamValue::Rect([x, y, w, h]))
    },
};

pub(super) static VOLUME: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("volume"),
    scope: ParamScope::Audio,
    browse: Some(Browse {
        label: "Volume",
        group: "audio",
        kind: ParamKind::Scalar,
        range: ParamRange::between(0.0, MAX_CLIP_VOLUME),
    }),
    kind: |_| Some(ParamKind::Scalar),
    range: |_| ParamRange::between(0.0, MAX_CLIP_VOLUME),
    read: |c, _| ParamRoute::Scalar(&c.volume),
    slot: Some(|c| Ok(ParamSlot::Scalar(&mut c.volume))),
    validate: |_, value| crate::clip::validate_volume(value.scalar()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Scalar(crate::clip::default_volume().sample(0))),
};

pub(super) static PAN: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Direct("pan"),
    scope: ParamScope::Audio,
    browse: Some(Browse {
        label: "Pan",
        group: "audio",
        kind: ParamKind::Scalar,
        range: ParamRange::between(MIN_CLIP_PAN, MAX_CLIP_PAN),
    }),
    kind: |_| Some(ParamKind::Scalar),
    range: |_| ParamRange::between(MIN_CLIP_PAN, MAX_CLIP_PAN),
    read: |c, _| ParamRoute::Scalar(&c.pan),
    slot: Some(|c| Ok(ParamSlot::Scalar(&mut c.pan))),
    validate: |_, value| crate::clip::validate_pan(value.scalar()?),
    keyframe: slot_keyframe,
    constant: slot_constant,
    erase: slot_erase,
    ease: Some(slot_ease),
    default: |_, _| Some(ParamValue::Scalar(crate::clip::default_pan().sample(0))),
};

/// The speed ramp reads like any other property and writes like none of them:
/// its keyframe ticks live on a normalized scale and editing it re-derives the
/// clip's timeline duration, so it is edited through `set_clip_speed_curve`.
/// The row still carries the read accessor, because a surface drawing the ramp
/// wants it, and `browse: None` keeps it out of the catalog — which is what
/// [`crate::ParamExclusion::SpeedCurve`] means.
pub(super) static SPEED: ClipParamSpec = ClipParamSpec {
    class: ClipParamClass::Excluded(ParamExclusion::SpeedCurve),
    scope: ParamScope::Visual,
    browse: None,
    kind: |_| Some(ParamKind::Scalar),
    range: |_| ParamRange::between(MIN_SPEED, MAX_SPEED),
    read: |c, _| ParamRoute::Scalar(&c.speed_curve),
    slot: None,
    validate: |_, value| value.scalar().map(|_| ()),
    keyframe: excluded_keyframe,
    constant: excluded_constant,
    erase: excluded_erase,
    ease: None,
    default: |_, _| None,
};
