//! One row per multi-keyframe easing preset.
//!
//! A preset used to be a variant plus three separate arms: one for its wire id,
//! one to parse that id back, and one to pick its waypoint table. The label a
//! picker shows was in none of them, so the editor kept its own three-entry
//! table of `(variant, label)` pairs beside a count — a list that goes stale in
//! silence the moment a fourth preset ships.
//!
//! A row carries the identity, the label, and the shape. `index_of` is an
//! exhaustive match over [`PiecewiseEasingPreset`], so a new preset cannot
//! compile until it names a row, and [`piecewise_presets`] is what a picker
//! walks instead of listing variants.

use super::*;
use crate::param::Easing;

/// Waypoint in normalized segment space: `(t_frac, v_frac, easing)`.
///
/// `t_frac`/`v_frac` are fractions of the from→to tick/value delta; `v_frac`
/// may exceed `[0,1]` for overshoot (elastic / back).
pub type Waypoint = (f32, f32, Easing);

/// What one multi-keyframe easing preset is called and what shape it expands to.
#[derive(Debug, Clone, Copy)]
pub struct PiecewisePresetSpec {
    /// The preset this row describes.
    pub preset: PiecewiseEasingPreset,
    /// Stable wire / UI id.
    pub id: &'static str,
    /// Operator-facing label, for a picker row or a chip.
    pub label: &'static str,
    /// The extrema the segment expands into, first at `t_frac == 0.0` and last
    /// at `t_frac == 1.0`.
    pub waypoints: &'static [Waypoint],
}

/// Penner bounce-out extrema (visual approximation).
///
/// Landings at t≈0.3636 / 0.7272 / 0.9090 / 1.0 (v=1); valleys at
/// 0.5454 (v=0.75), 0.8181 (v=0.9375), 0.9545 (v≈0.9844). EaseOut leaves
/// landings (falling into a valley); EaseIn arrives at the next landing.
const BOUNCE_OUT: &[Waypoint] = &[
    (0.0, 0.0, Easing::EaseIn),
    (0.3636, 1.0, Easing::EaseOut),
    (0.5454, 0.75, Easing::EaseIn),
    (0.7272, 1.0, Easing::EaseOut),
    (0.8181, 0.9375, Easing::EaseIn),
    (0.9090, 1.0, Easing::EaseOut),
    (0.9545, 0.984375, Easing::EaseIn),
    (1.0, 1.0, Easing::Linear),
];

/// Decaying oscillation that overshoots the target, then converges.
const ELASTIC_OUT: &[Waypoint] = &[
    (0.0, 0.0, Easing::EaseInOut),
    (0.3, 1.25, Easing::EaseInOut),
    (0.55, 0.875, Easing::EaseInOut),
    (0.75, 1.0625, Easing::EaseInOut),
    (0.9, 0.96875, Easing::EaseInOut),
    (1.0, 1.0, Easing::Linear),
];

/// 10% overshoot then settle (three keyframes).
const BACK_OUT: &[Waypoint] = &[
    (0.0, 0.0, Easing::EaseOut),
    (0.6, 1.1, Easing::EaseInOut),
    (1.0, 1.0, Easing::Linear),
];

/// The shipped presets, in the order a picker offers them.
pub(super) const SHIPPED: &[PiecewisePresetSpec] = &[
    PiecewisePresetSpec {
        preset: PiecewiseEasingPreset::BounceOut,
        id: "bounce_out",
        label: "Bounce out",
        waypoints: BOUNCE_OUT,
    },
    PiecewisePresetSpec {
        preset: PiecewiseEasingPreset::ElasticOut,
        id: "elastic_out",
        label: "Elastic out",
        waypoints: ELASTIC_OUT,
    },
    PiecewisePresetSpec {
        preset: PiecewiseEasingPreset::BackOut,
        id: "back_out",
        label: "Back out",
        waypoints: BACK_OUT,
    },
];

/// Which row describes `preset`.
///
/// Exhaustive on purpose: a preset added without a row does not compile.
const fn index_of(preset: PiecewiseEasingPreset) -> usize {
    match preset {
        PiecewiseEasingPreset::BounceOut => 0,
        PiecewiseEasingPreset::ElasticOut => 1,
        PiecewiseEasingPreset::BackOut => 2,
    }
}

/// Every preset, in declaration order. A picker walks this instead of keeping
/// its own list, so a new preset appears there by existing.
pub const fn piecewise_presets() -> &'static [PiecewisePresetSpec] {
    SHIPPED
}

/// The row with this wire id, for a surface that addresses a preset by name.
pub fn piecewise_preset_by_id(id: &str) -> Option<&'static PiecewisePresetSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

impl PiecewiseEasingPreset {
    /// The row describing this preset.
    pub const fn spec(self) -> &'static PiecewisePresetSpec {
        &SHIPPED[index_of(self)]
    }

    /// Stable wire / UI id (`bounce_out`, …).
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// Operator-facing label.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Parse a stable id.
    pub fn from_id(id: &str) -> Option<Self> {
        piecewise_preset_by_id(id).map(|spec| spec.preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exhaustive `index_of` match is the compile-time gate; this proves
    /// each arm points at its *own* row rather than a neighbour's, which would
    /// compile and silently expand the wrong shape. Both sides come from
    /// [`SHIPPED`]: nothing here counts variants.
    #[test]
    fn a_row_per_piecewise_preset() {
        for (index, spec) in SHIPPED.iter().enumerate() {
            assert_eq!(
                index_of(spec.preset),
                index,
                "the '{}' row resolves to '{}' instead",
                spec.id,
                SHIPPED[index_of(spec.preset)].id
            );
            assert_eq!(spec.preset.id(), spec.id);
            assert_eq!(
                PiecewiseEasingPreset::from_id(spec.id),
                Some(spec.preset),
                "'{}' is not reachable by id",
                spec.id
            );
        }
    }

    #[test]
    fn every_row_is_named_and_pinned_at_both_ends() {
        let ids: std::collections::BTreeSet<_> = SHIPPED.iter().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), SHIPPED.len(), "two rows share an id");
        let labels: std::collections::BTreeSet<_> = SHIPPED.iter().map(|spec| spec.label).collect();
        assert_eq!(labels.len(), SHIPPED.len(), "two rows share a label");
        for spec in SHIPPED {
            assert!(!spec.label.is_empty(), "'{}' has no label", spec.id);
            let first = spec.waypoints.first().expect("a preset has waypoints");
            let last = spec.waypoints.last().expect("a preset has waypoints");
            assert_eq!(first.0, 0.0, "'{}' does not start at the segment", spec.id);
            assert_eq!(first.1, 0.0, "'{}' does not start at the value", spec.id);
            assert_eq!(last.0, 1.0, "'{}' does not end at the segment", spec.id);
            assert_eq!(last.1, 1.0, "'{}' does not land on the value", spec.id);
        }
    }

    #[test]
    fn an_unknown_id_is_refused() {
        assert_eq!(PiecewiseEasingPreset::from_id("no_such_preset"), None);
    }
}
