// --- Track matte --------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::param::Param;

use super::{default_zero_param, is_zero_param, validate_unit_param};

/// What a track matte reads out of the layer above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatteSource {
    /// The matte layer's own alpha. What a shape, a title, or a keyed clip
    /// gives.
    Alpha,
    /// The matte layer's luminance: white keeps, black cuts. What a ramp or a
    /// grayscale plate gives.
    Luma,
}

impl MatteSource {
    /// Stable wire/catalog id (the serde name).
    pub const fn id(self) -> &'static str {
        match self {
            MatteSource::Alpha => "alpha",
            MatteSource::Luma => "luma",
        }
    }

    /// Display label.
    pub const fn label(self) -> &'static str {
        match self {
            MatteSource::Alpha => "Alpha",
            MatteSource::Luma => "Luma",
        }
    }

    /// Every source (UI browsing order).
    pub const ALL: [MatteSource; 2] = [MatteSource::Alpha, MatteSource::Luma];
}

/// A track matte: the layer immediately above this clip becomes its stencil
/// instead of being drawn. `None` on the clip ⇔ no matte.
///
/// The matte layer is consumed, not composited, which is what separates this
/// from a blend mode: two clips go in and one comes out. `invert` swaps keep for
/// cut, and `softness` lifts the transition either side of the matte's own edge
/// so a hard-edged shape does not alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackMatte {
    /// Which channel of the layer above is read.
    pub source: MatteSource,
    /// Keep where the matte is transparent (or black) instead of opaque.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// Edge softness, `0` … `1`. `0` uses the matte's own edge.
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub softness: Param<f32>,
}

impl TrackMatte {
    /// A matte reading the layer above's alpha, with a hard edge.
    pub fn alpha() -> Self {
        Self {
            source: MatteSource::Alpha,
            invert: false,
            softness: default_zero_param(),
        }
    }

    /// A matte reading the layer above's luminance, with a hard edge.
    pub fn luma() -> Self {
        Self {
            source: MatteSource::Luma,
            invert: false,
            softness: default_zero_param(),
        }
    }

    /// Validate the animatable softness envelope.
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_unit_param("matte softness", &self.softness)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}
