use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::param::{Easing, Keyframe, Lerp, Param, SegmentSample};

/// Per-axis scale. Serializes as a bare number when uniform (so old builds
/// and old saves stay interchangeable) and as `[x, y]` when split.
/// Deserializes from either form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale2 {
    pub x: f32,
    pub y: f32,
}

impl Scale2 {
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };

    #[inline]
    pub const fn uniform(s: f32) -> Self {
        Self { x: s, y: s }
    }

    /// Exact equality: values come from the same slider unless the unlink
    /// control (or a vec2 keyframe) has split the axes.
    #[inline]
    pub fn is_uniform(self) -> bool {
        self.x == self.y
    }

    /// Geometric mean of the absolute axes. Keeps stroke / blur / style
    /// reference-px widths stable under uniform scale and splits the
    /// difference under stretch (so a `[4, 1]` scale still doubles widths).
    #[inline]
    pub fn isotropic(self) -> f32 {
        (self.x.abs() * self.y.abs()).sqrt()
    }
}

impl Default for Scale2 {
    fn default() -> Self {
        Self::ONE
    }
}

impl From<f32> for Scale2 {
    fn from(s: f32) -> Self {
        Self::uniform(s)
    }
}

impl From<[f32; 2]> for Scale2 {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl From<Scale2> for [f32; 2] {
    fn from(s: Scale2) -> Self {
        [s.x, s.y]
    }
}

impl std::ops::Mul<f32> for Scale2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl std::ops::MulAssign<f32> for Scale2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Lerp for Scale2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            x: f32::lerp(a.x, b.x, t),
            y: f32::lerp(a.y, b.y, t),
        }
    }
}

impl crate::param::Extrapolate for Scale2 {}

impl SegmentSample for Scale2 {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let dx = b.value.x - a.value.x;
        let dy = b.value.y - a.value.y;
        if dx.abs() >= dy.abs() { dx } else { dy }
    }

    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let dx = b.value.x - a.value.x;
        let dy = b.value.y - a.value.y;
        (dx * dx + dy * dy).sqrt()
    }
}

impl Serialize for Scale2 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_uniform() {
            serializer.serialize_f32(self.x)
        } else {
            [self.x, self.y].serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Scale2 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Uniform(f32),
            Axes([f32; 2]),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Uniform(s) => Self::uniform(s),
            Wire::Axes([x, y]) => Self { x, y },
        })
    }
}

/// Spatial placement of a clip's content on the canvas (CapCut "Basic"
/// transform: position, anchor, scale, rotation, opacity).
///
/// Coordinates are normalized to the canvas so projects survive canvas-size
/// changes: `position` is the offset of the `anchor_point` from the canvas
/// center as a fraction of canvas width/height (+x right, +y down — screen
/// convention). With the default center anchor this matches the legacy
/// content-center semantics. `anchor_point` is the pivot within the content
/// bounds (0,0 = top-left, 0.5,0.5 = center). `scale` is per-axis with 1.0 =
/// aspect-fit inside the canvas (CapCut's 100%); uniform scales serialize as
/// a bare float for save/compat. `rotation` is degrees clockwise about the
/// anchor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipTransform {
    /// Anchor offset from canvas center, normalized to canvas dimensions.
    /// `[0.0, 0.0]` = anchor on the canvas center; `[0.5, 0.0]` = anchor on
    /// the right canvas edge.
    pub position: [f32; 2],
    /// Pivot within the content bounds, normalized to the placed size
    /// (+x right, +y down). `[0.5, 0.5]` = content center (default).
    #[serde(default = "default_anchor_point")]
    pub anchor_point: [f32; 2],
    /// Per-axis scale; `[1, 1]` aspect-fits the content inside the canvas.
    /// Uniform values serialize as a bare float.
    pub scale: Scale2,
    /// Clockwise rotation in degrees about the anchor.
    pub rotation: f32,
    /// Layer opacity, 0.0 (transparent) ..= 1.0 (opaque).
    pub opacity: f32,
}

fn default_anchor_point() -> [f32; 2] {
    [0.5, 0.5]
}

impl ClipTransform {
    pub const IDENTITY: Self = Self {
        position: [0.0, 0.0],
        anchor_point: [0.5, 0.5],
        scale: Scale2::ONE,
        rotation: 0.0,
        opacity: 1.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// `Ok` iff every component is finite, both scale axes are positive, and
    /// opacity is within `0..=1` — the invariant
    /// [`crate::Project::set_transform`] enforces before storing.
    pub fn validate(&self) -> Result<(), ModelError> {
        let finite = self.position.iter().all(|v| v.is_finite())
            && self.anchor_point.iter().all(|v| v.is_finite())
            && self.scale.x.is_finite()
            && self.scale.y.is_finite()
            && self.rotation.is_finite()
            && self.opacity.is_finite();
        if !finite {
            return Err(ModelError::InvalidTransform("non-finite component".into()));
        }
        validate_scale(self.scale)?;
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(ModelError::InvalidTransform(
                "opacity must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The animatable spatial placement stored on a clip: each [`ClipTransform`]
/// property as a [`Param`] (M2 keystone). Constant params serialize as bare
/// values, so a never-animated transform is byte-identical to the pre-M2
/// `ClipTransform` JSON and old projects load unchanged.
///
/// Keyframe ticks are clip-relative (offset from the clip's timeline start)
/// at the timeline rate — animation rides along when a clip moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatedTransform {
    /// Anchor offset from canvas center (see [`ClipTransform::position`]).
    #[serde(default = "default_position_param")]
    pub position: Param<[f32; 2]>,
    /// Pivot within the content bounds (see [`ClipTransform::anchor_point`]).
    #[serde(
        default = "default_anchor_point_param",
        skip_serializing_if = "is_default_anchor_param"
    )]
    pub anchor_point: Param<[f32; 2]>,
    /// Per-axis scale (see [`ClipTransform::scale`]). Uniform constants
    /// serialize as a bare float for pre-M2 / pre-Scale2 save compat.
    #[serde(default = "default_scale_param")]
    pub scale: Param<Scale2>,
    /// Clockwise rotation in degrees (see [`ClipTransform::rotation`]).
    #[serde(default = "default_rotation_param")]
    pub rotation: Param<f32>,
    /// Layer opacity 0..=1 (see [`ClipTransform::opacity`]).
    #[serde(default = "default_opacity_param")]
    pub opacity: Param<f32>,
}

fn default_position_param() -> Param<[f32; 2]> {
    Param::Constant([0.0, 0.0])
}
fn default_anchor_point_param() -> Param<[f32; 2]> {
    Param::Constant([0.5, 0.5])
}
fn is_default_anchor_param(p: &Param<[f32; 2]>) -> bool {
    p.constant() == Some([0.5, 0.5])
}
fn default_scale_param() -> Param<Scale2> {
    Param::Constant(Scale2::ONE)
}
fn default_rotation_param() -> Param<f32> {
    Param::Constant(0.0)
}
fn default_opacity_param() -> Param<f32> {
    Param::Constant(1.0)
}

impl AnimatedTransform {
    /// All-constant identity (centered, aspect-fit, opaque).
    pub fn identity() -> Self {
        Self::from(ClipTransform::IDENTITY)
    }

    /// True iff no property is animated and every constant is the identity.
    pub fn is_identity(&self) -> bool {
        !self.is_animated() && self.sample(0).is_identity()
    }

    /// True iff any property has keyframes.
    pub fn is_animated(&self) -> bool {
        self.position.is_animated()
            || self.anchor_point.is_animated()
            || self.scale.is_animated()
            || self.rotation.is_animated()
            || self.opacity.is_animated()
    }

    /// The transform value at a clip-relative `tick` — the per-frame hot
    /// path (pure, allocation-free).
    pub fn sample(&self, tick: i64) -> ClipTransform {
        self.sample_at(tick as f64)
    }

    /// [`sample`](Self::sample) at a fractional clip-relative tick:
    /// sub-frame animation sampling for export at rates above the timeline
    /// rate (see [`Param::sample_at`]).
    pub fn sample_at(&self, tick: f64) -> ClipTransform {
        ClipTransform {
            position: self.position.sample_at(tick),
            anchor_point: self.anchor_point.sample_at(tick),
            scale: self.scale.sample_at(tick),
            rotation: self.rotation.sample_at(tick),
            opacity: self.opacity.sample_at(tick),
        }
    }

    /// Set every property to a constant, dropping any keyframes.
    pub fn set_constant(&mut self, transform: ClipTransform) {
        self.position.set_constant(transform.position);
        self.anchor_point.set_constant(transform.anchor_point);
        self.scale.set_constant(transform.scale);
        self.rotation.set_constant(transform.rotation);
        self.opacity.set_constant(transform.opacity);
    }

    /// Apply a full-transform edit composing with animation CapCut-style:
    /// animated properties get a keyframe at `tick` (linear easing),
    /// constant properties stay constant. A gesture on a never-animated
    /// clip behaves exactly like the pre-M2 `set_constant`.
    pub fn compose_at(&mut self, transform: ClipTransform, tick: i64) {
        if self.position.is_animated() {
            self.position
                .set_keyframe(tick, transform.position, Easing::Linear);
        } else {
            self.position.set_constant(transform.position);
        }
        if self.anchor_point.is_animated() {
            self.anchor_point
                .set_keyframe(tick, transform.anchor_point, Easing::Linear);
        } else {
            self.anchor_point.set_constant(transform.anchor_point);
        }
        if self.scale.is_animated() {
            self.scale
                .set_keyframe(tick, transform.scale, Easing::Linear);
        } else {
            self.scale.set_constant(transform.scale);
        }
        if self.rotation.is_animated() {
            self.rotation
                .set_keyframe(tick, transform.rotation, Easing::Linear);
        } else {
            self.rotation.set_constant(transform.rotation);
        }
        if self.opacity.is_animated() {
            self.opacity
                .set_keyframe(tick, transform.opacity, Easing::Linear);
        } else {
            self.opacity.set_constant(transform.opacity);
        }
    }

    /// `Ok` iff every stored value (constants and keyframes) passes the
    /// per-property rules [`ClipTransform::validate`] enforces, and every
    /// keyframed param is structurally sound (sorted, non-empty, valid
    /// easings). Used on load and by model mutators.
    pub fn validate(&self) -> Result<(), ModelError> {
        self.position.validate_shape()?;
        self.anchor_point.validate_shape()?;
        self.scale.validate_shape()?;
        self.rotation.validate_shape()?;
        self.opacity.validate_shape()?;
        self.position.for_each_value(validate_position)?;
        self.anchor_point.for_each_value(validate_anchor_point)?;
        self.scale.for_each_value(|v| validate_scale(*v))?;
        self.rotation.for_each_value(|v| validate_rotation(*v))?;
        self.opacity.for_each_value(|v| validate_opacity(*v))?;
        Ok(())
    }
}

pub(crate) fn validate_position(v: &[f32; 2]) -> Result<(), ModelError> {
    if v.iter().all(|c| c.is_finite()) {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite component".into()))
    }
}

pub(crate) fn validate_anchor_point(v: &[f32; 2]) -> Result<(), ModelError> {
    if v.iter().all(|c| c.is_finite()) {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite anchor".into()))
    }
}

pub(crate) fn validate_scale(v: Scale2) -> Result<(), ModelError> {
    if !v.x.is_finite() || !v.y.is_finite() {
        return Err(ModelError::InvalidTransform("non-finite component".into()));
    }
    if v.x <= 0.0 || v.y <= 0.0 {
        return Err(ModelError::InvalidTransform(
            "scale must be positive".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_rotation(v: f32) -> Result<(), ModelError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite component".into()))
    }
}

pub(crate) fn validate_opacity(v: f32) -> Result<(), ModelError> {
    if !v.is_finite() {
        return Err(ModelError::InvalidTransform("non-finite component".into()));
    }
    if !(0.0..=1.0).contains(&v) {
        return Err(ModelError::InvalidTransform(
            "opacity must be in 0..=1".into(),
        ));
    }
    Ok(())
}

impl Default for AnimatedTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl From<ClipTransform> for AnimatedTransform {
    fn from(t: ClipTransform) -> Self {
        Self {
            position: Param::Constant(t.position),
            anchor_point: Param::Constant(t.anchor_point),
            scale: Param::Constant(t.scale),
            rotation: Param::Constant(t.rotation),
            opacity: Param::Constant(t.opacity),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_transform_validate_rejects_non_positive_and_invalid() {
        assert!(ClipTransform::IDENTITY.validate().is_ok());

        let mut t = ClipTransform::IDENTITY;
        t.scale = Scale2 { x: 0.0, y: 1.0 };
        assert!(t.validate().is_err());

        t.scale = Scale2 { x: 1.0, y: -0.5 };
        assert!(t.validate().is_err());

        t.scale = Scale2 {
            x: f32::NAN,
            y: 1.0,
        };
        assert!(t.validate().is_err());

        t.scale = Scale2::ONE;
        t.opacity = 1.5;
        assert!(t.validate().is_err());

        t.opacity = -0.1;
        assert!(t.validate().is_err());
    }
}
