//! How much motion the app is allowed: a persisted setting.
//!
//! A choice is a [`MotionPolicy`]: a duration scale that multiplies every
//! motion duration, and whether motion is reduced. Adding a speed is one
//! variant here, with its label, its multiplier, and whether it counts as
//! reduced motion.

use gpui::App;
use gpui::motion::MotionPolicy;
use serde::{Deserialize, Serialize};

/// The motion speeds offered in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MotionPreference {
    /// The catalog as designed.
    #[default]
    Full,
    /// The same curves over a shorter span, for a setup where a transition
    /// reads as latency.
    Brisk,
    /// Longer transitions. Easier to follow, and what a demo wants.
    Relaxed,
    /// No transitions. Distinct from a very small multiplier: every primitive
    /// checks reduced motion and takes its end state directly, so nothing is
    /// drawn mid-flight and no animation frames are requested.
    Off,
}

impl MotionPreference {
    /// Every choice, in the order Settings lists them.
    pub const ALL: [Self; 4] = [Self::Full, Self::Brisk, Self::Relaxed, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full motion",
            Self::Brisk => "Brisk motion",
            Self::Relaxed => "Relaxed motion",
            Self::Off => "No motion",
        }
    }

    /// What the choice does, for the row beside the label. The multiplier is
    /// named rather than described: it is the whole difference between two of
    /// these rows.
    pub fn detail(self) -> &'static str {
        match self {
            Self::Full => "every transition at its designed speed",
            Self::Brisk => "0.6× duration, same curves",
            Self::Relaxed => "1.4× duration, same curves",
            Self::Off => "state changes land immediately",
        }
    }

    /// The multiplier applied to every catalog duration.
    pub fn scale(self) -> f32 {
        match self {
            Self::Full | Self::Off => 1.0,
            Self::Brisk => 0.6,
            Self::Relaxed => 1.4,
        }
    }

    /// Whether this choice means reduced motion: no transition runs, and the
    /// end state is drawn on the frame the change happened.
    pub fn reduced(self) -> bool {
        matches!(self, Self::Off)
    }

    /// The motion policy this choice installs.
    pub fn policy(self) -> MotionPolicy {
        MotionPolicy::DEFAULT
            .with_reduced(self.reduced())
            .with_duration_scale(self.scale())
    }

    /// Installs the choice as the app's [`MotionPolicy`], which redraws every
    /// window. The scale and the reduced flag are set together, so a scale left
    /// over from the previous choice never applies to the next transition.
    pub fn apply(self, cx: &mut App) {
        cx.set_motion_policy(self.policy());
    }
}

/// The environment variable that sets the duration scale at startup.
pub const SCALE_ENV: &str = "ARIA_MOTION_SCALE";

/// Sets the duration scale of the app's [`MotionPolicy`] from [`SCALE_ENV`],
/// clamped by [`MotionPolicy::parse_duration_scale`]. An unset variable leaves
/// the policy unchanged; a value that is not a finite number is ignored with a
/// warning. A later [`MotionPreference::apply`] replaces the scale.
pub fn seed_scale_from_env(cx: &mut App) {
    let Ok(text) = std::env::var(SCALE_ENV) else {
        return;
    };
    match MotionPolicy::parse_duration_scale(&text) {
        Some(scale) => cx.set_motion_policy(cx.motion_policy().with_duration_scale(scale)),
        None => tracing::warn!(
            value = %text,
            "{SCALE_ENV} is not a finite number; the duration scale is unchanged"
        ),
    }
}
