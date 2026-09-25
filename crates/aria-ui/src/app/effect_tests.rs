//! Contracts for the effect chain: render order and scalar parameters.
//!
//! The class closed here is "the stack renders in a fixed order and a parameter
//! is set blind". Parameter slots are read from `aria_models::effect_spec` at run
//! time, so a new catalog parameter is covered without a second list.

use super::premiere_tests::with_clip;
use crate::app::clip_effects::scalar_params;
use aria_models::{EffectParamKind, effect_spec};
/// The catalog id of an effect with at least one scalar parameter, so the test
/// exercises a real slot rather than a hand-picked constant.
fn effect_with_scalar() -> &'static str {
    aria_models::effect_catalog()
        .iter()
        .find(|spec| {
            spec.params
                .iter()
                .any(|param| param.kind == EffectParamKind::Scalar)
        })
        .map(|spec| spec.id.as_str())
        .expect("the catalog has an effect with a scalar parameter")
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "effect_tests_effect.rs"]
mod effect_tests_effect;
#[path = "effect_tests_slot.rs"]
mod effect_tests_slot;
