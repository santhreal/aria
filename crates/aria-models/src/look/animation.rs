// --- Animations -----------------------------------------------------------------

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::error::ModelError;

/// Which animation slot a preset occupies (CapCut In / Out / Combo tabs).
/// A combo replaces both entrance and exit; setting one side clears a combo
/// and vice versa (enforced by `Project::set_clip_animation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationSlot {
    In,
    Out,
    Combo,
}

impl AnimationSlot {
    /// Stable wire/catalog id (the serde name).
    pub const fn id(self) -> &'static str {
        match self {
            AnimationSlot::In => "in",
            AnimationSlot::Out => "out",
            AnimationSlot::Combo => "combo",
        }
    }
}

/// Default value for [`AnimationRef`] tunable knobs (identity / catalog feel).
pub const ANIMATION_PARAM_DEFAULT: f32 = 1.0;
/// Inclusive range for [`AnimationRef::speed`].
pub const ANIMATION_SPEED_RANGE: (f32, f32) = (0.25, 4.0);
/// Inclusive range for [`AnimationRef::intensity`].
pub const ANIMATION_INTENSITY_RANGE: (f32, f32) = (0.0, 2.0);
/// Inclusive range for [`AnimationRef::stagger`].
pub const ANIMATION_STAGGER_RANGE: (f32, f32) = (0.0, 2.0);

fn default_anim_param() -> f32 {
    ANIMATION_PARAM_DEFAULT
}

fn is_default_anim_param(v: &f32) -> bool {
    (*v - ANIMATION_PARAM_DEFAULT).abs() < f32::EPSILON
}

/// Which user-tunable knobs a preset exposes in the inspector / AI wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AnimationKnobs {
    pub speed: bool,
    pub intensity: bool,
    pub stagger: bool,
}

impl Default for AnimationKnobs {
    /// Speed and intensity: what a whole-layer preset can be tuned by. A catalog
    /// entry that omits the key gets this, so only a per-character preset has to
    /// say anything.
    fn default() -> Self {
        Self::SPEED_INTENSITY
    }
}

impl AnimationKnobs {
    /// Speed + intensity (whole-layer and most text presets).
    pub const SPEED_INTENSITY: Self = Self {
        speed: true,
        intensity: true,
        stagger: false,
    };
    /// Speed + intensity + stagger (per-character presets).
    pub const SPEED_INTENSITY_STAGGER: Self = Self {
        speed: true,
        intensity: true,
        stagger: true,
    };

    /// Serde default for a catalog entry with no `knobs` key.
    fn speed_intensity() -> Self {
        Self::SPEED_INTENSITY
    }
}

/// A reference to a catalog animation, stored per slot on the clip.
///
/// `speed` / `intensity` / `stagger` default to [`ANIMATION_PARAM_DEFAULT`]
/// and serialize only when non-default (additive — old projects still load).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationRef {
    /// Catalog id (see [`animation_catalog`]).
    pub id: String,
    /// Playback rate of the entrance/exit window or combo period (`1` = catalog).
    #[serde(
        default = "default_anim_param",
        skip_serializing_if = "is_default_anim_param"
    )]
    pub speed: f32,
    /// Magnitude of motion / opacity swing (`1` = catalog, `0` = no motion).
    #[serde(
        default = "default_anim_param",
        skip_serializing_if = "is_default_anim_param"
    )]
    pub intensity: f32,
    /// Per-character stagger stretch (`1` = catalog; ignored when the preset
    /// has no stagger knob).
    #[serde(
        default = "default_anim_param",
        skip_serializing_if = "is_default_anim_param"
    )]
    pub stagger: f32,
}

/// The three animation slots a clip renders with, borrowed.
///
/// One owner for "which presets are in effect", so a preview can substitute a slot
/// without cloning the clip and without a second definition of what the slots are:
/// a combo replaces both sides, which is a rule the renderer must not restate.
#[derive(Debug, Clone, Copy, Default)]
pub struct LookAnimations<'a> {
    pub entrance: Option<&'a AnimationRef>,
    pub exit: Option<&'a AnimationRef>,
    pub combo: Option<&'a AnimationRef>,
}

impl<'a> LookAnimations<'a> {
    /// What the clip itself carries.
    pub fn of(clip: &'a crate::Clip) -> Self {
        Self {
            entrance: clip.animation_in.as_ref(),
            exit: clip.animation_out.as_ref(),
            combo: clip.animation_combo.as_ref(),
        }
    }

    /// The same slots with one replaced, as a hover preview needs.
    ///
    /// Setting a side clears a combo and setting a combo clears both sides, matching
    /// `Project::set_clip_animation`: a preview that showed a combination the commit
    /// would never produce is a lie about what the click will do.
    pub fn with_slot(self, slot: AnimationSlot, animation: Option<&'a AnimationRef>) -> Self {
        match slot {
            AnimationSlot::In => Self {
                entrance: animation,
                exit: self.exit,
                combo: None,
            },
            AnimationSlot::Out => Self {
                entrance: self.entrance,
                exit: animation,
                combo: None,
            },
            AnimationSlot::Combo => Self {
                entrance: None,
                exit: None,
                combo: animation,
            },
        }
    }

    /// Whether any slot is filled.
    pub fn is_empty(&self) -> bool {
        self.entrance.is_none() && self.exit.is_none() && self.combo.is_none()
    }
}

impl AnimationRef {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            speed: ANIMATION_PARAM_DEFAULT,
            intensity: ANIMATION_PARAM_DEFAULT,
            stagger: ANIMATION_PARAM_DEFAULT,
        }
    }

    /// Clamp / zero unsupported knobs against `spec`, rejecting out-of-range
    /// values on knobs the preset exposes.
    pub fn normalized_for(self, spec: &AnimationSpec) -> Result<Self, ModelError> {
        let speed = normalize_knob("speed", self.speed, spec.knobs.speed, ANIMATION_SPEED_RANGE)?;
        let intensity = normalize_knob(
            "intensity",
            self.intensity,
            spec.knobs.intensity,
            ANIMATION_INTENSITY_RANGE,
        )?;
        let stagger = normalize_knob(
            "stagger",
            self.stagger,
            spec.knobs.stagger,
            ANIMATION_STAGGER_RANGE,
        )?;
        Ok(Self {
            id: self.id,
            speed,
            intensity,
            stagger,
        })
    }
}

fn normalize_knob(
    name: &str,
    value: f32,
    supported: bool,
    (lo, hi): (f32, f32),
) -> Result<f32, ModelError> {
    if !supported {
        return Ok(ANIMATION_PARAM_DEFAULT);
    }
    if !value.is_finite() || value < lo || value > hi {
        return Err(ModelError::InvalidParam(format!(
            "animation {name} {value} outside [{lo}, {hi}]"
        )));
    }
    Ok(value)
}

/// One animation catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnimationSpec {
    pub id: String,
    pub label: String,
    pub slot: AnimationSlot,
    /// Presets designed for text clips only (the text panel's animation
    /// chips); rejected on other content.
    #[serde(default)]
    pub text_only: bool,
    /// Which of speed / intensity / stagger the UI and AI may expose. Speed and
    /// intensity by default: stagger is per character, so a whole-layer preset
    /// has nothing to stagger.
    #[serde(default = "AnimationKnobs::speed_intensity")]
    pub knobs: AnimationKnobs,
}

/// Every animation, read once. Shipped set at `data/animations.toml`.
static ANIMATIONS: LazyLock<Vec<AnimationSpec>> = LazyLock::new(|| {
    crate::catalog::load(
        "animations",
        include_str!("../../data/animations.toml"),
        |entries| crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label),
    )
});

/// Every animation preset (UI browsing order; filter by slot / text_only).
pub fn animation_catalog() -> &'static [AnimationSpec] {
    &ANIMATIONS
}

/// The catalog entry for `id`, or `None`.
pub fn animation_spec(id: &str) -> Option<&'static AnimationSpec> {
    ANIMATIONS.iter().find(|s| s.id == id)
}
