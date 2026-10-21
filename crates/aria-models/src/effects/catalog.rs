use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use super::param_check::check_params;
use super::params::{EffectGroupSpec, EffectParamSpec};

/// Category label used when an [`EffectSpec`] leaves [`EffectSpec::category`] empty.
pub const OTHER_VIDEO_CATEGORY: &str = "Other";

/// Pixel margin adjustments an effect applies to its layer's bounding box.
///
/// Positive values expand the layer bounds outward (e.g. blur or drop shadow
/// overhang); negative values shrink them inward.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectBounds {
    #[serde(default)]
    pub left: f32,
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
}

impl EffectBounds {
    /// Zero margin expansion on all four sides.
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// Uniform margin expansion on all four sides.
    pub const fn uniform(margin: f32) -> Self {
        Self {
            left: margin,
            top: margin,
            right: margin,
            bottom: margin,
        }
    }

    /// Explicit `(left, top, right, bottom)` margin expansion.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// `true` when all four margins are zero.
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    /// Combine two effect bound adjustments in sequence.
    pub fn combine(self, other: Self) -> Self {
        Self {
            left: self.left + other.left,
            top: self.top + other.top,
            right: self.right + other.right,
            bottom: self.bottom + other.bottom,
        }
    }

    /// Apply these margins to a `(center, size)` quad in canvas pixels.
    pub fn apply_to_quad(self, center: [f32; 2], size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
        if self.is_zero() {
            return (center, size);
        }
        let new_w = (size[0] + self.left + self.right).max(0.0);
        let new_h = (size[1] + self.top + self.bottom).max(0.0);
        let new_cx = center[0] + (self.right - self.left) * 0.5;
        let new_cy = center[1] + (self.bottom - self.top) * 0.5;
        ([new_cx, new_cy], [new_w, new_h])
    }
}

/// Specification of a video effect or generator in the catalog.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectSpec {
    /// Stable identifier (e.g. `"gaussian_blur"`).
    pub id: String,
    /// Display name (e.g. `"Gaussian Blur"`).
    pub label: String,
    /// Browser category (e.g. `"Blur"`). Empty falls under [`OTHER_VIDEO_CATEGORY`].
    #[serde(default)]
    pub category: String,
    /// Parameter definitions in display order.
    #[serde(default)]
    pub params: Vec<EffectParamSpec>,
    /// Optional inspector parameter groups in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<EffectGroupSpec>,
    /// `true` when this entry synthesizes a frame from `uv`, time, and
    /// parameters alone without reading an input texture.
    #[serde(default)]
    pub generator: bool,
    /// `true` when this effect's output at tick `t` depends on frames before
    /// `t` (temporal feedback, echo, motion trails).
    #[serde(default)]
    pub stateful: bool,
    /// Bounding-box expansion/shrink margins in pixels.
    #[serde(default)]
    pub bounds: EffectBounds,
    /// `true` when the effect samples outside `0.0..=1.0` source UVs.
    #[serde(default, alias = "reads-outside")]
    pub reads_outside: bool,
    /// `true` when the effect output depends on playback time even when
    /// parameters are static.
    #[serde(default, alias = "time-dependent")]
    pub time_dependent: bool,
}

impl EffectSpec {
    /// Browser category label, defaulting to [`OTHER_VIDEO_CATEGORY`] when empty.
    pub fn category(&self) -> &str {
        if self.category.is_empty() {
            OTHER_VIDEO_CATEGORY
        } else {
            &self.category
        }
    }

    /// Look up a parameter specification by name.
    pub fn param(&self, name: &str) -> Option<&EffectParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Look up a parameter specification by positional slot index.
    pub fn param_at(&self, index: usize) -> Option<&EffectParamSpec> {
        self.params.get(index)
    }

    /// Look up a group specification by id.
    pub fn group(&self, id: &str) -> Option<&EffectGroupSpec> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Set bounding-box expansion/shrink margins.
    pub fn with_bounds(mut self, bounds: EffectBounds) -> Self {
        self.bounds = bounds;
        self
    }

    /// Mark whether the effect reads outside the unit UV rectangle.
    pub fn with_reads_outside(mut self, reads_outside: bool) -> Self {
        self.reads_outside = reads_outside;
        self
    }

    /// Mark whether the effect depends on time even with static parameters.
    pub fn with_time_dependent(mut self, time_dependent: bool) -> Self {
        self.time_dependent = time_dependent;
        self
    }
}

/// The shipped effects, read once. Shipped set at `data/effects.toml`.
static SHIPPED: LazyLock<Vec<Arc<EffectSpec>>> = LazyLock::new(|| {
    crate::catalog::load(
        "effects",
        include_str!("../../data/effects.toml"),
        |entries: &[EffectSpec]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
            entries.iter().try_for_each(check_effect_params)
        },
    )
    .into_iter()
    .map(Arc::new)
    .collect()
});

/// The parameter rules every effect follows, shipped or registered: each
/// parameter is named, no name repeats, and no range is inverted.
pub(crate) fn check_effect_params(spec: &EffectSpec) -> Result<(), String> {
    check_params(&spec.id, &spec.params)
}

/// The effects that ship with the app, without registered ones.
pub fn shipped_effect_catalog() -> &'static [Arc<EffectSpec>] {
    &SHIPPED
}

/// Every effect the model knows about now (validation + UI browsing): the
/// shipped set, then effects registered by plugins.
pub fn effect_catalog() -> crate::Catalog<EffectSpec> {
    crate::registered::effect_catalog()
}

/// The catalog entry for `id`, shipped or registered, or `None`.
pub fn effect_spec(id: &str) -> Option<Arc<EffectSpec>> {
    crate::registered::effect_spec(id)
}
