//! Resolve persisted clip look fields into compositor [`ColorGrade`] values.

use aria_compositor::ColorGrade;
use aria_models::{ColorAdjustments, Filter, FilterGrade};

/// The [`ColorGrade`] for one filter-catalog id.
///
/// The numbers come from the catalog, not from here: a preset is data, so a
/// filter added to `filters.toml` grades correctly with no change in the
/// renderer. The mapping from a recipe's offsets onto the compositor's grade
/// expands from the slider rows, so a new adjustment is carried by a preset
/// without a line here.
///
/// Returns `None` for an unknown id, so an unknown preset behaves as identity
/// while manual adjustments still apply.
pub(crate) fn preset_recipe(id: &str) -> Option<ColorGrade> {
    Some(recipe_grade(aria_models::filter_spec(id)?.grade))
}

/// Fold a clip's filter preset and manual adjustments into one compositor grade.
#[cfg(test)]
pub(crate) fn effective_grade(filter: Option<&Filter>, adjust: &ColorAdjustments) -> ColorGrade {
    effective_grade_at(filter, adjust, 0)
}

/// Fold a clip's filter preset and manual adjustments at a clip-local tick.
pub(crate) fn effective_grade_at(
    filter: Option<&Filter>,
    adjust: &ColorAdjustments,
    tick: i64,
) -> ColorGrade {
    let intensity = filter
        .map(|f| ColorGrade::clamp_unit(f.intensity.sample(tick)))
        .unwrap_or(0.0);
    let preset = filter
        .and_then(|f| preset_recipe(&f.id))
        .unwrap_or(ColorGrade::IDENTITY);

    let mut grade = scale_grade(preset, intensity);
    fold_adjustments(&mut grade, adjust, tick);
    clamp_grade(grade)
}

/// Resolve a grade for compositor submission. `None` is the identity fast path.
#[cfg(test)]
pub(crate) fn resolve_color_grade(
    filter: Option<&Filter>,
    adjust: &ColorAdjustments,
) -> Option<ColorGrade> {
    let grade = effective_grade(filter, adjust);
    (!grade.is_identity()).then_some(grade)
}

/// Resolve a grade at a clip-local tick.
pub(crate) fn resolve_color_grade_at(
    filter: Option<&Filter>,
    adjust: &ColorAdjustments,
    tick: i64,
) -> Option<ColorGrade> {
    let grade = effective_grade_at(filter, adjust, tick);
    (!grade.is_identity()).then_some(grade)
}

/// How each slider shape reads a preset offset.
///
/// A recipe states a difference from the clip's own grade, so its rest value
/// is zero for every row. Zero is no change for an offset and a black frame
/// for a multiplier, which is the one place the two families disagree.
macro_rules! grade_from_preset {
    (signed, $offset:expr) => {
        $offset
    };
    (positive, $offset:expr) => {
        $offset
    };
    (multiplicative, $offset:expr) => {
        non_zero_or_one($offset)
    };
}

/// How each slider shape carries a filter's intensity: an offset scales toward
/// zero, a multiplier toward one.
macro_rules! grade_scaled {
    (signed, $value:expr, $intensity:expr) => {
        $value * $intensity
    };
    (positive, $value:expr, $intensity:expr) => {
        $value * $intensity
    };
    (multiplicative, $value:expr, $intensity:expr) => {
        1.0 + ($value - 1.0) * $intensity
    };
}

/// How each slider shape takes a manual adjustment: offsets add, multipliers
/// multiply, and each is range-checked by the shape's own limits.
macro_rules! grade_fold {
    (signed, $field:expr, $value:expr) => {
        $field += ColorGrade::clamp_adjust($value)
    };
    (positive, $field:expr, $value:expr) => {
        $field += ColorGrade::clamp_unit($value)
    };
    (multiplicative, $field:expr, $value:expr) => {
        $field *= ColorGrade::clamp_positive($value)
    };
}

/// The range a folded slider is held to, by shape.
macro_rules! grade_clamped {
    (signed, $value:expr) => {
        ColorGrade::clamp_adjust($value)
    };
    (positive, $value:expr) => {
        ColorGrade::clamp_unit($value)
    };
    (multiplicative, $value:expr) => {
        ColorGrade::clamp_positive($value)
    };
}

/// Every step between a persisted look and a compositor grade, expanded from
/// the slider rows: a new adjustment is read from a recipe, scaled by a
/// filter's intensity, folded and clamped without a line here.
macro_rules! define_grade_folding {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// A filter recipe's offsets as a grade at full intensity.
        fn recipe_grade(recipe: FilterGrade) -> ColorGrade {
            ColorGrade {
                $($field: grade_from_preset!($shape, recipe.$field),)*
            }
        }

        /// A preset grade weighted by the filter's intensity.
        fn scale_grade(grade: ColorGrade, intensity: f32) -> ColorGrade {
            ColorGrade {
                $($field: grade_scaled!($shape, grade.$field, intensity),)*
            }
        }

        /// Fold the clip's own sliders, sampled at `tick`, onto a preset grade.
        fn fold_adjustments(grade: &mut ColorGrade, adjust: &ColorAdjustments, tick: i64) {
            $(grade_fold!($shape, grade.$field, adjust.$field.sample(tick));)*
        }

        /// Hold every slider to its shape's range after folding.
        fn clamp_grade(mut grade: ColorGrade) -> ColorGrade {
            $(grade.$field = grade_clamped!($shape, grade.$field);)*
            grade
        }
    };
}

aria_models::adjust_slider_rows!(define_grade_folding);

fn non_zero_or_one(v: f32) -> f32 {
    if v == 0.0 { 1.0 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_models::filter_catalog;

    #[test]
    fn neutral_look_resolves_to_none() {
        assert!(resolve_color_grade(None, &ColorAdjustments::default()).is_none());
    }

    #[test]
    fn identity_when_no_filter_and_neutral_adjust() {
        let grade = effective_grade(None, &ColorAdjustments::default());
        assert_eq!(grade, ColorGrade::IDENTITY);
        assert!(grade.is_identity());
    }

    #[test]
    fn intensity_zero_with_neutral_adjust_yields_identity() {
        let filter = Filter {
            id: "mono".into(),
            intensity: 0.0.into(),
        };
        let grade = effective_grade(Some(&filter), &ColorAdjustments::default());
        assert_eq!(grade, ColorGrade::IDENTITY);
        assert!(resolve_color_grade(Some(&filter), &ColorAdjustments::default()).is_none());
    }

    /// The recipe is the catalog's own numbers now, so what is worth asserting is
    /// that the mapping carries them: a preset that changes something must not
    /// arrive as the identity, and no field may be dropped on the way across.
    #[test]
    fn every_preset_maps_its_catalog_grade_onto_the_compositors() {
        let mut seen = ColorGrade::IDENTITY;
        for spec in filter_catalog() {
            let grade = preset_recipe(&spec.id).expect("a catalog id has a recipe");
            assert!(
                !grade.is_identity(),
                "'{}' grades nothing: its catalog entry has no offsets",
                spec.id
            );
            seen.brightness += grade.brightness.abs();
            seen.contrast += grade.contrast.abs();
            seen.saturation += grade.saturation.abs();
            seen.temperature += grade.temperature.abs();
            seen.tint += grade.tint.abs();
        }
        // Each of these is non-zero in the shipped set, so a mapping that dropped
        // one of them would show up here rather than as a preset that looks wrong.
        for (value, field) in [
            (seen.brightness, "brightness"),
            (seen.contrast, "contrast"),
            (seen.saturation, "saturation"),
            (seen.temperature, "temperature"),
            (seen.tint, "tint"),
        ] {
            assert!(value > 0.0, "{field} never survives the mapping");
        }
        assert!(preset_recipe("unknown").is_none());
        assert!(preset_recipe("").is_none());
    }

    #[test]
    fn clamps_out_of_range_and_non_finite_inputs() {
        let filter = Filter {
            id: "vivid".into(),
            intensity: f32::NAN.into(),
        };
        let adjust = ColorAdjustments {
            saturation: f32::INFINITY.into(),
            brightness: 2.0.into(),
            contrast: (-3.0).into(),
            exposure: f32::NAN.into(),
            temperature: 1.5.into(),
            ..Default::default()
        };
        let grade = effective_grade(Some(&filter), &adjust);
        assert_eq!(grade.brightness, 1.0);
        assert_eq!(grade.contrast, -1.0);
        assert_eq!(grade.temperature, 1.0);
        assert_eq!(grade.saturation, 0.0);
        assert_eq!(grade.exposure, 0.0);

        let filter = Filter {
            id: "mono".into(),
            intensity: 1.0.into(),
        };
        let adjust = ColorAdjustments {
            saturation: (-1.5).into(),
            ..ColorAdjustments::default()
        };
        let grade = effective_grade(Some(&filter), &adjust);
        assert_eq!(grade.saturation, -1.0);
        assert_eq!(grade.contrast, 0.1);
    }

    #[test]
    fn preset_scaled_by_intensity_plus_adjustments() {
        let filter = Filter {
            id: "warm".into(),
            intensity: 0.5.into(),
        };
        let adjust = ColorAdjustments {
            brightness: 0.1.into(),
            ..ColorAdjustments::default()
        };
        let grade = effective_grade(Some(&filter), &adjust);
        assert!((grade.temperature - 0.2).abs() < 1e-6);
        assert!((grade.brightness - 0.125).abs() < 1e-6);
    }

    #[test]
    fn resolve_color_grade_returns_some_for_non_identity() {
        let grade = resolve_color_grade(
            Some(&Filter {
                id: "mono".into(),
                intensity: 0.5.into(),
            }),
            &ColorAdjustments::default(),
        )
        .unwrap();
        assert!((grade.saturation - (-0.5)).abs() < 1e-5);
    }
}
