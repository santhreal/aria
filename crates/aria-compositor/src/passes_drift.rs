//! WGSL-versus-catalog drift guard for [`super::EFFECTS`] and
//! [`super::TRANSITIONS`].
//!
//! What is compared: the parameter slots an effect derives from
//! `aria_models::effect_catalog` against the `params` uniform components the
//! row's own WGSL actually reads. Both sides are derived at run time — the slots
//! from the catalog, the reads from parsing the shader sources the table embeds —
//! so nothing here counts effects, names a shader file, or repeats a slot layout.
//!
//! The shipped catalog has to agree with every shader, which is what
//! [`every_shipped_effect_is_renderable`] asserts: a shipped effect that would
//! be refused is a bug in this repository, not a user's bad file. The refusal
//! path itself is exercised by [`a_catalog_the_shader_disagrees_with_is_refused`]
//! against doctored slots, because the real catalog is a process-wide `LazyLock`
//! and cannot be swapped mid-run.
//!
//! Also checked, because each is a way to add an effect and have it silently do
//! nothing: an inert rule naming a parameter the catalog does not carry, an
//! `effect_*.wgsl` no row references, and a transition shader that ignores the
//! progress (or, when two rows share it, the mode) it is handed.

use std::collections::BTreeSet;
use std::path::Path;

use crate::passes_uniform::{Read, declared_param_slots, param_reads, reads_of, slots_for};

use super::{
    EFFECTS, INERT_PARAM_OFFSET_BY_ID, PassParam, TRANSITIONS, TransitionPass, effect_descriptor,
    effect_is_noop, shipped_effect_descriptors, shipped_transition_ids, slots_agree,
    transition_pass,
};

/// Every row of the registry renders a shipped catalog effect, and every shipped
/// catalog effect with a row is renderable.
///
/// Both directions fail: a row for an id the catalog dropped is a pass nothing
/// can reach, and a catalog effect whose derived slots its shader disagrees with
/// is refused at startup — correct for a user's file, a defect for the one this
/// repository ships.
#[test]
fn every_shipped_effect_is_renderable() {
    for row in EFFECTS {
        let spec = aria_models::effect_spec(row.id)
            .unwrap_or_else(|| panic!("registry row '{}' renders no catalog effect", row.id));
        assert!(
            effect_descriptor(row.id).is_some(),
            "shipped effect '{}' was refused: its {} catalog parameters do not \
             match what its shader reads",
            spec.id,
            spec.params.len()
        );
    }
    for desc in shipped_effect_descriptors() {
        assert!(
            EFFECTS.iter().any(|row| row.id == desc.id),
            "'{}' is renderable with no registry row behind it",
            desc.id
        );
    }
}

/// The slots a renderable effect reports are the catalog's, flattened.
///
/// This is the claim the crate boundary rests on: the engine packs values by
/// catalog order and this crate feeds them to a shader by slot, and there is now
/// one list rather than two, so the only way they can disagree is a bug in the
/// derivation itself.
#[test]
fn effect_descriptor_has_one_owner() {
    for desc in shipped_effect_descriptors() {
        let spec =
            aria_models::effect_spec(desc.id).expect("a renderable effect is in the catalog");
        let names: Vec<&str> = desc.params.iter().map(|param| param.name).collect();
        let catalog: Vec<&str> = spec
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        assert_eq!(
            names, catalog,
            "effect '{}' reports parameters the catalog does not name",
            desc.id
        );
        let widths: Vec<u8> = desc.params.iter().map(|param| param.floats).collect();
        let expected: Vec<u8> = spec
            .params
            .iter()
            .map(|param| match param.kind {
                aria_models::EffectParamKind::Scalar => 1,
                aria_models::EffectParamKind::Vec2 => 2,
                aria_models::EffectParamKind::Color => 4,
            })
            .collect();
        assert_eq!(
            widths, expected,
            "effect '{}' flattens its catalog parameters to the wrong widths",
            desc.id
        );
    }
}

#[test]
fn every_effect_reads_exactly_the_floats_its_catalog_declares() {
    for desc in shipped_effect_descriptors() {
        let row = EFFECTS
            .iter()
            .find(|row| row.id == desc.id)
            .expect("a renderable effect has a row");
        let declared = slots_for(desc.float_count());
        let read = reads_of(row.shaders);

        let missing: Vec<Read> = declared.difference(&read).copied().collect();
        let extra: Vec<Read> = read.difference(&declared).copied().collect();
        assert!(
            missing.is_empty(),
            "effect '{}' declares {:?} but its WGSL never reads params{:?} \
             (declared slots: {:?})",
            desc.id,
            desc.params,
            missing,
            declared
        );
        assert!(
            extra.is_empty(),
            "effect '{}' WGSL reads params{:?}, which no declared parameter \
             covers (declared slots: {:?} from {:?})",
            desc.id,
            extra,
            declared,
            desc.params
        );
    }
}

/// A catalog that adds, drops or retypes a parameter is refused rather than
/// rendered.
///
/// The derivation is what protects the uniform from an override file, so the
/// predicate it refuses on is exercised against doctored slot lists: the shipped
/// ones agree, and the same list with one parameter added, one removed, or one
/// widened does not. Doctored here rather than through a catalog file because
/// the catalog is read once per process and cannot be swapped mid-run.
#[test]
fn a_catalog_the_shader_disagrees_with_is_refused() {
    for desc in shipped_effect_descriptors() {
        let row = EFFECTS
            .iter()
            .find(|row| row.id == desc.id)
            .expect("a renderable effect has a row");
        assert!(
            slots_agree(desc.params, row.shaders),
            "shipped effect '{}' does not agree with its own shader",
            desc.id
        );

        let mut extra = desc.params.to_vec();
        extra.push(PassParam::scalar("bonus"));
        assert!(
            !slots_agree(&extra, row.shaders),
            "effect '{}' would accept a catalog with a parameter its shader never \
             reads",
            desc.id
        );

        let (_, fewer) = desc
            .params
            .split_first()
            .expect("every shipped effect takes at least one parameter");
        assert!(
            !slots_agree(fewer, row.shaders),
            "effect '{}' would accept a catalog missing a parameter its shader \
             reads",
            desc.id
        );

        let widened: Vec<PassParam> = desc
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                if index == 0 {
                    PassParam {
                        name: param.name,
                        floats: param.floats + 1,
                        offset: param.offset,
                    }
                } else {
                    *param
                }
            })
            .collect();
        assert!(
            !slots_agree(&widened, row.shaders),
            "effect '{}' would accept a catalog that retyped its first parameter \
             to a wider kind",
            desc.id
        );
    }
}

#[test]
fn no_effect_overflows_the_uniform_its_shader_declares() {
    for desc in shipped_effect_descriptors() {
        let row = EFFECTS
            .iter()
            .find(|row| row.id == desc.id)
            .expect("a renderable effect has a row");
        for source in row.shaders {
            let slots = declared_param_slots(source).unwrap_or_else(|| {
                panic!(
                    "effect '{}' has a shader with no `params: array<vec4<f32>, N>` uniform",
                    desc.id
                )
            });
            assert!(
                desc.float_count() <= slots * 4,
                "effect '{}' flattens to {} floats, past the {} its shader uniform holds",
                desc.id,
                desc.float_count(),
                slots * 4
            );
        }
    }
}

#[test]
fn every_inert_rule_names_a_parameter_of_its_own_effect() {
    for row in EFFECTS {
        let id = row.id;
        let Some(inert) = row.inert else {
            // No inert rule means the pass always draws, even unparameterized.
            assert!(
                !effect_is_noop(id, &[]),
                "effect '{id}' has no inert rule but is skipped anyway"
            );
            continue;
        };
        let desc = effect_descriptor(id).expect("a shipped effect is renderable");
        let offset = *INERT_PARAM_OFFSET_BY_ID
            .get(id)
            .unwrap_or_else(|| panic!("effect '{id}' is inert on undeclared '{}'", inert.param));

        let mut params = vec![inert.value + 1.0; desc.float_count()];
        assert!(
            !effect_is_noop(id, &params),
            "effect '{id}' is skipped with '{}' above {}",
            inert.param,
            inert.value
        );
        params[offset] = inert.value;
        assert!(
            effect_is_noop(id, &params),
            "effect '{id}' still draws with '{}' at its inert value {}",
            inert.param,
            inert.value
        );
        // A wrong-arity instance must NOT be reported inert. Reporting it inert
        // would let `effects_need_offscreen` drop the whole effect chain before
        // `validate_and_pack_effect_params` ever sees it, so a malformed
        // instance would vanish instead of returning
        // `CompositorError::InvalidEffectParams`. Staying "not a no-op" keeps
        // the draw path the only place that decides, and it fails closed.
        assert!(
            !effect_is_noop(id, &[]),
            "effect '{id}' with no parameters must reach validation, not be skipped"
        );
        let mut overlong = vec![inert.value; desc.float_count() + 1];
        overlong[offset] = inert.value;
        assert!(
            !effect_is_noop(id, &overlong),
            "effect '{id}' with a surplus parameter must reach validation even \
             though '{}' sits at its inert value",
            inert.param
        );
    }
}

#[test]
fn every_effect_shader_file_belongs_to_a_row() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
    let referenced: BTreeSet<&str> = EFFECTS
        .iter()
        .flat_map(|pass| pass.shaders.iter().copied())
        .collect();

    for entry in std::fs::read_dir(&dir).expect("shaders directory") {
        let path = entry.expect("shader dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("shader file name")
            .to_owned();
        if !name.starts_with("effect_") || !name.ends_with(".wgsl") {
            continue;
        }
        // The passthrough is what an id with no row draws, so it has no row.
        if name == "effect_passthrough.wgsl" {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("shader source");
        assert!(
            referenced.contains(source.as_str()),
            "{name} is an effect shader no registry row references: either give \
             it a row in EFFECTS or delete it"
        );
    }
}

#[test]
fn every_transition_shader_reads_the_progress_it_is_handed() {
    for row in TRANSITIONS {
        let reads = param_reads(row.pass.shader());
        assert!(
            reads.contains(&(0, 0)),
            "transition '{}' never reads params.x, so progress does nothing",
            row.id
        );

        let shared = TRANSITIONS
            .iter()
            .filter(|other| other.pass.shader() == row.pass.shader())
            .count();
        if shared > 1 {
            assert!(
                reads.contains(&(0, 1)),
                "transition '{}' shares its shader with {} other rows but the \
                 shader never reads params.y, so every row renders the same",
                row.id,
                shared - 1
            );
        }
    }
}

#[test]
fn the_public_catalogs_are_projections_of_the_rows() {
    // Catalog order, because that is where an effect's parameters come from.
    let effect_ids: Vec<&str> = shipped_effect_descriptors().iter().map(|d| d.id).collect();
    let catalog_ids: Vec<&str> = aria_models::shipped_effect_catalog()
        .iter()
        .map(|spec| spec.id.as_str())
        .filter(|id| EFFECTS.iter().any(|row| row.id == *id))
        .collect();
    assert_eq!(effect_ids, catalog_ids);

    let transition_row_ids: Vec<&str> = TRANSITIONS.iter().map(|r| r.id).collect();
    assert_eq!(shipped_transition_ids().to_vec(), transition_row_ids);
    for row in TRANSITIONS {
        assert_eq!(
            transition_pass(row.id),
            row.pass,
            "transition '{}' dispatches to another row's pass",
            row.id
        );
    }
    // An id with no row still has to render something.
    assert_eq!(
        transition_pass("not_a_transition"),
        TransitionPass::Crossfade
    );
}

#[test]
fn pass_descriptors_have_exact_precomputed_float_counts_and_offsets() {
    for desc in shipped_effect_descriptors() {
        let mut expected_offset = 0usize;
        for param in desc.params {
            assert_eq!(
                param.offset as usize, expected_offset,
                "param '{}' of effect '{}' has mismatching precomputed offset",
                param.name, desc.id
            );
            assert_eq!(
                desc.param_offset(param.name),
                Some(expected_offset),
                "param_offset lookup mismatch for '{}' in effect '{}'",
                param.name,
                desc.id
            );
            expected_offset += usize::from(param.floats);
        }
        assert_eq!(
            desc.float_count(),
            expected_offset,
            "precomputed float_count mismatch for effect '{}'",
            desc.id
        );
    }

    for desc in super::shipped_transition_descriptors() {
        assert_eq!(desc.float_count(), 1);
        assert_eq!(desc.param_offset("progress"), Some(0));
        assert_eq!(desc.param_offset("nonexistent"), None);
    }
}
