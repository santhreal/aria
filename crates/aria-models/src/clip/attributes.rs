//! The groups of a clip's properties that Remove Attributes returns to the
//! values a new clip starts with.
//!
//! A group covers the stored value, its keyframes, and any driver on its
//! properties: a driver left behind would keep overriding the value that was
//! just reset.

use serde::{Deserialize, Serialize};

use super::{AnimatedTransform, Clip, ClipParam, LookParam};

/// One group of clip properties, as Edit > Remove Attributes lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipAttribute {
    /// Position, anchor point, scale, and rotation.
    Motion,
    /// Opacity and blend mode.
    Opacity,
    /// The crop window and both mirrors.
    Crop,
    /// The speed ramp. Clearing it re-derives the clip's duration.
    TimeRemapping,
    /// The gain level and its envelope. Fades are kept.
    Volume,
    /// The stereo pan and its envelope.
    Pan,
    /// The video effect chain.
    Effects,
    /// The audio effect chain.
    AudioEffects,
    /// The colour adjustments, the filter, and the LUT.
    Grade,
    /// Every mask in the stack.
    Mask,
    /// The chroma key.
    ChromaKey,
}

impl ClipAttribute {
    /// Every group, in the order the dialog lists them.
    pub const ALL: [Self; 11] = [
        Self::Motion,
        Self::Opacity,
        Self::Crop,
        Self::TimeRemapping,
        Self::Volume,
        Self::Pan,
        Self::Effects,
        Self::AudioEffects,
        Self::Grade,
        Self::Mask,
        Self::ChromaKey,
    ];

    /// The dialog's name for the group.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Motion => "Motion",
            Self::Opacity => "Opacity",
            Self::Crop => "Crop",
            Self::TimeRemapping => "Time Remapping",
            Self::Volume => "Volume",
            Self::Pan => "Pan",
            Self::Effects => "Effects",
            Self::AudioEffects => "Audio Effects",
            Self::Grade => "Grade",
            Self::Mask => "Mask",
            Self::ChromaKey => "Chroma Key",
        }
    }

    /// Whether the group is a property every clip has rather than something
    /// applied to the clip. Motion, Opacity, Crop, Time Remapping, Volume, and
    /// Pan are intrinsic; an effect chain, a grade, a mask, and a key are
    /// applied. The fx badge on a timeline clip bar draws the two in different
    /// colours.
    pub const fn is_intrinsic(self) -> bool {
        match self {
            Self::Motion
            | Self::Opacity
            | Self::Crop
            | Self::TimeRemapping
            | Self::Volume
            | Self::Pan => true,
            Self::Effects | Self::AudioEffects | Self::Grade | Self::Mask | Self::ChromaKey => {
                false
            }
        }
    }

    /// Whether a driver on `param` belongs to this group.
    pub fn drives(self, param: &ClipParam) -> bool {
        match self {
            Self::Motion => matches!(
                param,
                ClipParam::Position
                    | ClipParam::AnchorPoint
                    | ClipParam::Scale
                    | ClipParam::Rotation
            ),
            Self::Opacity => matches!(param, ClipParam::Opacity),
            Self::Crop => matches!(param, ClipParam::Crop),
            Self::TimeRemapping => matches!(param, ClipParam::Speed),
            Self::Volume => matches!(param, ClipParam::Volume),
            Self::Pan => matches!(param, ClipParam::Pan),
            Self::Effects => matches!(param, ClipParam::Effect { .. }),
            Self::AudioEffects => false,
            Self::Grade => matches!(param, ClipParam::Look { param } if param.is_grade()),
            Self::Mask => matches!(param, ClipParam::Mask { .. }),
            Self::ChromaKey => matches!(param, ClipParam::Look { param } if param.is_chroma()),
        }
    }

    /// Whether `clip` holds anything in this group that a new clip does not.
    pub fn is_set(self, clip: &Clip) -> bool {
        let fresh = AnimatedTransform::identity();
        let t = &clip.transform;
        let held = match self {
            Self::Motion => {
                t.position != fresh.position
                    || t.anchor_point != fresh.anchor_point
                    || t.scale != fresh.scale
                    || t.rotation != fresh.rotation
            }
            Self::Opacity => t.opacity != fresh.opacity || !clip.blend_mode.is_normal(),
            Self::Crop => clip.crop != super::default_crop() || clip.flip_h || clip.flip_v,
            Self::TimeRemapping => clip.has_speed_curve(),
            Self::Volume => clip.volume != super::default_volume(),
            Self::Pan => clip.pan != super::default_pan(),
            Self::Effects => !clip.effects.is_empty(),
            Self::AudioEffects => !clip.audio_effects.is_empty(),
            Self::Grade => {
                clip.filter.is_some()
                    || clip.lut.is_some()
                    || clip.adjust != crate::look::ColorAdjustments::default()
            }
            Self::Mask => !clip.masks.is_empty(),
            Self::ChromaKey => clip.chroma_key.is_some(),
        };
        held || clip.drivers.iter().any(|driver| self.drives(&driver.param))
    }

    /// Return the group to a new clip's values on `clip`, drivers included.
    ///
    /// The speed ramp is left alone: clearing it re-derives the clip's
    /// duration, which needs the track, so
    /// [`crate::Project::remove_clip_attributes`] clears it first.
    pub(crate) fn clear(self, clip: &mut Clip) {
        let fresh = AnimatedTransform::identity();
        match self {
            Self::Motion => {
                clip.transform.position = fresh.position;
                clip.transform.anchor_point = fresh.anchor_point;
                clip.transform.scale = fresh.scale;
                clip.transform.rotation = fresh.rotation;
            }
            Self::Opacity => {
                clip.transform.opacity = fresh.opacity;
                clip.blend_mode = crate::look::BlendMode::default();
            }
            Self::Crop => {
                clip.crop = super::default_crop();
                clip.flip_h = false;
                clip.flip_v = false;
            }
            Self::TimeRemapping => {}
            Self::Volume => clip.volume = super::default_volume(),
            Self::Pan => clip.pan = super::default_pan(),
            Self::Effects => clip.effects.clear(),
            Self::AudioEffects => clip.audio_effects.clear(),
            Self::Grade => {
                clip.filter = None;
                clip.lut = None;
                clip.adjust = crate::look::ColorAdjustments::default();
            }
            Self::Mask => clip.masks.clear(),
            Self::ChromaKey => clip.chroma_key = None,
        }
        clip.drivers.retain(|driver| !self.drives(&driver.param));
        // A line on an effect or a mask has nothing left to draw once the
        // chain or stack is gone; any other property survives at its default.
        if clip.line_param.is_some_and(|param| {
            matches!(param, ClipParam::Effect { .. } | ClipParam::Mask { .. })
                && self.drives(&param)
        }) {
            clip.line_param = None;
        }
    }
}

impl LookParam {
    /// A colour adjustment, filter, or LUT parameter.
    pub fn is_grade(self) -> bool {
        !self.is_chroma() && !matches!(self, Self::MatteSoftness)
    }

    /// A chroma key parameter.
    pub fn is_chroma(self) -> bool {
        matches!(self, Self::ChromaStrength | Self::ChromaShadow)
    }
}
