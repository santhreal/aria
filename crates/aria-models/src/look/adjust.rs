// --- Color adjustments -----------------------------------------------------------
//
// Every field here comes from the row list in `look/adjust_rows.rs`: the
// struct, its `Default`, its save format and the slider table are four
// expansions of one list, so a new adjustment is one row and a field that no
// row declares cannot exist.

use std::fmt;

use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ModelError;
use crate::param::Param;

/// Documented neutral value for lift controls (offsets shadows/floor, neutral at 0.0).
pub const LIFT_NEUTRAL: f32 = 0.0;
/// Documented neutral value for gamma controls (bends midtones, neutral at 1.0).
pub const GAMMA_NEUTRAL: f32 = 1.0;
/// Documented neutral value for gain controls (scales highlights/top, neutral at 1.0).
pub const GAIN_NEUTRAL: f32 = 1.0;

/// One grade slider: what it is called on the wire, where it sits at rest, the
/// range a write is validated against, and how to reach it.
///
/// Every sweep over the grade reads this table — neutrality, validation, the
/// save format, the parameter axis and the assistant's description of a graded
/// clip — so a slider is declared once in `adjust_rows.rs` and every surface
/// follows.
pub struct AdjustSlider {
    /// Field name, which is also the serde name and the assistant's argument
    /// name.
    pub name: &'static str,
    /// Stable catalog id a keyframe row and a preset address the slider by.
    pub id: &'static str,
    /// The parameter axis a keyframe on this slider is addressed by.
    pub axis: crate::clip::LookParam,
    /// What a surface shows.
    pub label: &'static str,
    /// Value at which this slider does nothing.
    pub neutral: f32,
    pub min: f32,
    pub max: f32,
    /// The field itself.
    pub get: fn(&ColorAdjustments) -> &Param<f32>,
    /// The field itself, for a surface that writes one slider by row.
    pub get_mut: fn(&mut ColorAdjustments) -> &mut Param<f32>,
}

impl fmt::Debug for AdjustSlider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Function pointers are not Debug; the row identity is the name.
        f.debug_struct("AdjustSlider")
            .field("name", &self.name)
            .field("id", &self.id)
            .field("axis", &self.axis)
            .field("label", &self.label)
            .field("neutral", &self.neutral)
            .field("min", &self.min)
            .field("max", &self.max)
            .finish_non_exhaustive()
    }
}

impl AdjustSlider {
    /// True iff this slider is a constant sitting where it does nothing, which
    /// is what keeps it out of a save.
    pub fn at_neutral(&self, adjust: &ColorAdjustments) -> bool {
        (self.get)(adjust).constant() == Some(self.neutral)
    }
}

macro_rules! adjust_shape_neutral {
    (signed) => {
        0.0
    };
    (positive) => {
        0.0
    };
    (multiplicative) => {
        GAMMA_NEUTRAL
    };
}

macro_rules! define_adjust {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// Manual color grade (CapCut adjust panel), one field per row of
        /// [`adjust_slider_rows!`](crate::adjust_slider_rows). Lives on visual
        /// clips and on `Generator::Adjustment` lane bars.
        ///
        /// A slider's range and its neutral value come from the shape its row
        /// declares: signed sliders are `[-1, 1]` around `0`, one-directional
        /// sliders are `[0, 1]` from `0` (there is no softening and no inverse
        /// vignette), and multiplicative sliders are `[0, 4]` around `1`.
        ///
        /// Saves carry only the sliders that are not sitting at neutral, and a
        /// save written before a row existed reads back with that row at
        /// neutral.
        #[derive(Debug, Clone, PartialEq)]
        pub struct ColorAdjustments {
            $(
                #[doc = $doc]
                pub $field: Param<f32>,
            )*
        }

        impl ColorAdjustments {
            /// Every slider, in panel order — which is also save order.
            pub const SLIDERS: &'static [AdjustSlider] = &[
                $($shape(
                    stringify!($field),
                    $id,
                    crate::clip::LookParam::$variant,
                    $label,
                    |a| &a.$field,
                    |a| &mut a.$field,
                ),)*
            ];

            /// How many sliders a grade carries.
            pub const COUNT: usize = Self::SLIDERS.len();

            /// Every slider at its neutral constant.
            pub const DEFAULT: Self = Self {
                $($field: Param::Constant(adjust_shape_neutral!($shape)),)*
            };
        }

        impl Default for ColorAdjustments {
            fn default() -> Self {
                Self::DEFAULT
            }
        }
    };
}

crate::adjust_slider_rows!(define_adjust);

impl ColorAdjustments {
    /// True iff every slider sits at neutral — the serde skip predicate.
    pub fn is_neutral(&self) -> bool {
        Self::SLIDERS.iter().all(|slider| slider.at_neutral(self))
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        for slider in Self::SLIDERS {
            validate_adjust_param(slider.name, (slider.get)(self), slider.min, slider.max)?;
        }
        Ok(())
    }

    /// The slider a parameter axis addresses, or `None` when the axis is not a
    /// grade slider.
    pub fn slider_of(param: crate::clip::LookParam) -> Option<&'static AdjustSlider> {
        Self::SLIDERS.iter().find(|slider| slider.axis == param)
    }
}

/// Only the sliders that are doing something reach a save, in row order.
impl Serialize for ColorAdjustments {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let written = Self::SLIDERS
            .iter()
            .filter(|slider| !slider.at_neutral(self))
            .count();
        let mut map = serializer.serialize_map(Some(written))?;
        for slider in Self::SLIDERS {
            if !slider.at_neutral(self) {
                map.serialize_entry(slider.name, (slider.get)(self))?;
            }
        }
        map.end()
    }
}

/// A grade reads back from whatever subset a save carries: an absent slider is
/// at neutral, and a slider this build does not know is skipped rather than
/// refused, so a project written by a newer build still opens.
impl<'de> Deserialize<'de> for ColorAdjustments {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Grade;

        impl<'de> Visitor<'de> for Grade {
            type Value = ColorAdjustments;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a colour grade, keyed by slider name")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut adjust = ColorAdjustments::default();
                let mut seen = [false; ColorAdjustments::COUNT];
                while let Some(key) = map.next_key::<String>()? {
                    let found = ColorAdjustments::SLIDERS
                        .iter()
                        .position(|slider| slider.name == key);
                    match found {
                        Some(at) => {
                            if seen[at] {
                                return Err(de::Error::duplicate_field(
                                    ColorAdjustments::SLIDERS[at].name,
                                ));
                            }
                            seen[at] = true;
                            let slider = &ColorAdjustments::SLIDERS[at];
                            *(slider.get_mut)(&mut adjust) = map.next_value()?;
                        }
                        None => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(adjust)
            }
        }

        deserializer.deserialize_map(Grade)
    }
}

/// A signed slider: neutral in the middle of `[-1, 1]`.
const fn signed(
    name: &'static str,
    id: &'static str,
    axis: crate::clip::LookParam,
    label: &'static str,
    get: fn(&ColorAdjustments) -> &Param<f32>,
    get_mut: fn(&mut ColorAdjustments) -> &mut Param<f32>,
) -> AdjustSlider {
    AdjustSlider {
        name,
        id,
        axis,
        label,
        neutral: 0.0,
        min: -1.0,
        max: 1.0,
        get,
        get_mut,
    }
}

/// A one-directional slider: off at zero, no negative half.
const fn positive(
    name: &'static str,
    id: &'static str,
    axis: crate::clip::LookParam,
    label: &'static str,
    get: fn(&ColorAdjustments) -> &Param<f32>,
    get_mut: fn(&mut ColorAdjustments) -> &mut Param<f32>,
) -> AdjustSlider {
    AdjustSlider {
        name,
        id,
        axis,
        label,
        neutral: 0.0,
        min: 0.0,
        max: 1.0,
        get,
        get_mut,
    }
}

/// A multiplicative slider (gamma, gain): neutral at unity.
const fn multiplicative(
    name: &'static str,
    id: &'static str,
    axis: crate::clip::LookParam,
    label: &'static str,
    get: fn(&ColorAdjustments) -> &Param<f32>,
    get_mut: fn(&mut ColorAdjustments) -> &mut Param<f32>,
) -> AdjustSlider {
    AdjustSlider {
        name,
        id,
        axis,
        label,
        neutral: GAMMA_NEUTRAL,
        min: 0.0,
        max: 4.0,
        get,
        get_mut,
    }
}

fn validate_adjust_param(
    what: &str,
    param: &Param<f32>,
    lo: f32,
    hi: f32,
) -> Result<(), ModelError> {
    param.validate_shape()?;
    param.for_each_value(|value| {
        if value.is_finite() && (*value >= lo && *value <= hi) {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "{what} = {value} out of range [{lo}, {hi}]"
            )))
        }
    })
}

#[cfg(test)]
#[path = "adjust_catalog_tests.rs"]
mod adjust_catalog;
