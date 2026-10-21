//! Effects as data (v1 roadmap M4): a clip carries a list of
//! [`EffectInstance`]s, each `{effect_id, params}`. The model never holds
//! shader code — the compositor owns the WGSL and maps ids to GPU passes.
//!
//! The [`effect_catalog`] here is the validation + UI source of truth
//! (display names, parameter defaults / ranges). It is drift-checked against
//! the compositor's renderable descriptors from `aria-engine`, so the two
//! crates can never disagree on which ids and parameter names exist.

mod catalog;
mod curve;
mod instance;
mod param_check;
mod params;
mod stored;

pub(crate) use catalog::check_effect_params;
pub use catalog::{
    EffectBounds, EffectSpec, OTHER_VIDEO_CATEGORY, effect_catalog, effect_spec,
    shipped_effect_catalog,
};
pub use curve::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectCurvePoint, EffectGradientStop,
    evaluate_effect_curve_lut, evaluate_effect_gradient_lut, sample_effect_curve,
    sample_effect_gradient,
};
pub use instance::{EFFECT_NAME_MAX_CHARS, EffectInstance};
pub(crate) use param_check::{
    check_params, choice_check, curve_check, gradient_check, holds_scalar, holds_text, holds_vec2,
    int_range_check, linear_color_check, range_check, scalar_or_int_range_check, stored_slot,
    vector_range_check,
};
pub use params::{
    CANVAS_PX_UNIT, EffectChoiceOption, EffectGroupSpec, EffectParamDisplay, EffectParamKind,
    EffectParamSpec, EffectVisibilityCondition,
};
pub use stored::{StoredParams, canvas_px_float_ranges, default_flattened_param_floats};

#[cfg(test)]
use serde::Serialize;

#[cfg(test)]
use crate::clip::ParamValue;
#[cfg(test)]
use crate::error::ModelError;
#[cfg(test)]
use crate::param::{Easing, Param};

#[cfg(test)]
#[path = "../effects_typed_tests.rs"]
pub(crate) mod typed_tests;

#[cfg(test)]
#[path = "../effects_name_tests.rs"]
mod name_tests;

#[cfg(test)]
mod tests;
