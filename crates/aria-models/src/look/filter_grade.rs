//! What a filter preset does to the picture, as one offset per adjustment row.
//!
//! A filter recipe used to be its own hand-written list of the same sliders,
//! written out beside the model's. The two drifted: the recipe had no key for
//! the colour wheels while the grade had twelve, and because the recipe read
//! with `#[serde(default)]`, a misspelled key in a catalog file was skipped —
//! a preset that quietly did nothing.
//!
//! Both are gone. The offsets expand from [`crate::adjust_slider_rows`], so a
//! new adjustment is a key here by existing, and a key this build does not know
//! refuses the file rather than being ignored: a catalog file is refused whole
//! and the shipped set stays in effect, which is exactly the outcome a typo
//! should have.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

/// One offset a filter recipe may carry.
#[derive(Debug, Clone, Copy)]
pub struct FilterOffset {
    /// The key a catalog file writes it under: the adjustment's own field name.
    pub name: &'static str,
    /// Read the offset off a recipe.
    pub get: fn(&FilterGrade) -> f32,
    /// Write the offset on a recipe.
    pub get_mut: fn(&mut FilterGrade) -> &mut f32,
}

macro_rules! define_filter_grade {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// A filter's grade offsets at full intensity, one per adjustment row.
        ///
        /// Every offset is zero when the recipe leaves it out, including the
        /// wheels: a recipe states a *difference* from the clip's own grade, so
        /// its rest value is no change rather than the slider's neutral.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct FilterGrade {
            $(
                #[doc = $doc]
                pub $field: f32,
            )*
        }

        impl Default for FilterGrade {
            fn default() -> Self {
                Self { $($field: 0.0,)* }
            }
        }

        impl FilterGrade {
            /// Every offset a recipe may carry, in adjustment row order.
            pub const OFFSETS: &'static [FilterOffset] = &[
                $(
                    FilterOffset {
                        name: stringify!($field),
                        get: |grade| grade.$field,
                        get_mut: |grade| &mut grade.$field,
                    },
                )*
            ];

            /// How many offsets there are, for a fixed-size seen-key set.
            pub const COUNT: usize = Self::OFFSETS.len();

            /// Every key a recipe may carry, for a refusal that lists them.
            pub const KEYS: &'static [&'static str] = &[$(stringify!($field),)*];
        }
    };
}

crate::adjust_slider_rows!(define_filter_grade);

impl FilterGrade {
    /// Whether the recipe changes nothing.
    pub fn is_empty(&self) -> bool {
        Self::OFFSETS.iter().all(|offset| (offset.get)(self) == 0.0)
    }

    /// The offset written under `name`, or `None` when no adjustment has it.
    pub fn offset_of(name: &str) -> Option<&'static FilterOffset> {
        Self::OFFSETS.iter().find(|offset| offset.name == name)
    }
}

/// A recipe reads from whatever subset a file carries. An absent key is no
/// change; an unknown key, and a key given twice, refuse the file.
impl<'de> Deserialize<'de> for FilterGrade {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Recipe;

        impl<'de> Visitor<'de> for Recipe {
            type Value = FilterGrade;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("filter grade offsets, keyed by adjustment name")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut grade = FilterGrade::default();
                let mut seen = [false; FilterGrade::COUNT];
                while let Some(key) = map.next_key::<String>()? {
                    let at = FilterGrade::OFFSETS
                        .iter()
                        .position(|offset| offset.name == key)
                        .ok_or_else(|| de::Error::unknown_field(&key, FilterGrade::KEYS))?;
                    if seen[at] {
                        return Err(de::Error::duplicate_field(FilterGrade::OFFSETS[at].name));
                    }
                    seen[at] = true;
                    *(FilterGrade::OFFSETS[at].get_mut)(&mut grade) = map.next_value()?;
                }
                Ok(grade)
            }
        }

        deserializer.deserialize_map(Recipe)
    }
}
