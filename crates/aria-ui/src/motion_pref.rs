//! How much motion the app is allowed: the operator's choice, persisted.
//!
//! Every duration in the catalog is multiplied by [`speed_scale`], and every
//! primitive already asks [`reduced_motion`] whether to animate at all. So a
//! preference is two numbers rather than a per-animation setting, and adding a
//! speed is one variant here — the label, the multiplier, and whether it counts
//! as reduced motion are all named in one place.

use serde::{Deserialize, Serialize};

use super::*;

/// The motion speeds offered in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MotionPreference {
    /// The catalog as designed.
    #[default]
    Full,
    /// The same curves over a shorter span, for an operator who knows the app
    /// and reads a transition as latency.
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

    /// Install the choice. Both halves are set together because a scale left
    /// over from the previous choice would apply to the next transition that
    /// ran, which is the one the operator is watching.
    pub fn apply(self, cx: &mut App) {
        set_speed_scale(self.scale());
        set_reduced_motion(cx, self.reduced());
    }
}

/// The multiplier every catalog duration passes through.
pub fn speed_scale() -> f32 {
    f32::from_bits(MOTION_SCALE.load(Ordering::Relaxed))
}

/// Set the multiplier every catalog duration passes through.
///
/// A non-finite value is refused rather than stored: it would make every
/// duration NaN, and a transition that never finishes is worse than a fast one.
pub fn set_speed_scale(scale: f32) {
    if !scale.is_finite() {
        return;
    }
    MOTION_SCALE.store(
        scale.clamp(MIN_SCALE, MAX_SCALE).to_bits(),
        Ordering::Relaxed,
    );
}

/// Set the setting, and the mirror the context-free surfaces read. Both are
/// written here because a mirror that lags the setting is a pane that keeps
/// sliding after motion was turned off.
pub fn set_reduced_motion(cx: &mut App, reduced: bool) {
    set_reduced_mirror(reduced);
    cx.set_reduce_motion(reduced);
}

pub fn reduced_motion(cx: &App) -> bool {
    cx.reduce_motion()
}
