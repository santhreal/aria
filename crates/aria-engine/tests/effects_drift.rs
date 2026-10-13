//! Cross-crate reach guard: is every catalog effect and transition renderable?
//!
//! Parameter drift is no longer possible to test for, because there is no second
//! list to drift: `aria-compositor` derives an effect's slots from
//! `aria_models::effect_catalog` and refuses an effect whose shader disagrees, so
//! names, order and widths have one owner. What that leaves is a reach question,
//! which is what this file asks from the engine — the crate that depends on both:
//! a catalog entry with no pass behind it is browsable, editable and invisible,
//! and a renderable id with no catalog entry cannot be reached at all.
//!
//! A refused effect fails here too, and reads the same as an effect with no
//! shader: absent from the renderable set. That is the intent — the refusal is
//! for a user's catalog file, and the shipped catalog must never trip it.

#[test]
fn every_catalog_effect_is_renderable() {
    let descriptors = aria_compositor::shipped_effect_descriptors();

    for spec in aria_models::effect_catalog() {
        assert!(
            descriptors.iter().any(|desc| desc.id == spec.id),
            "compositor cannot render catalog effect '{}': no pass, or its \
             parameters were refused",
            spec.id
        );
    }

    for desc in descriptors {
        assert!(
            aria_models::effect_spec(desc.id).is_some(),
            "no catalog entry for renderable effect '{}'",
            desc.id
        );
    }
}

#[test]
fn model_transition_catalog_matches_compositor_set() {
    let renderable = aria_compositor::shipped_transition_ids();

    for spec in aria_models::transition_catalog() {
        assert!(
            renderable.contains(&spec.id.as_str()),
            "compositor cannot render catalog transition '{}'",
            spec.id
        );
    }

    for id in renderable {
        assert!(
            aria_models::transition_spec(id).is_some(),
            "no catalog entry for renderable transition '{}'",
            id
        );
    }
}
