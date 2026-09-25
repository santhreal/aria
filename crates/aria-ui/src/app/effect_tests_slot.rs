//! Slot writes: 2 of the cases in [`super`].
//!
//! Split out of `effect_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

/// A write that changes nothing writes no edit, so a click on the swatch already
/// picked costs no undo entry.
#[test]
fn a_non_scalar_write_that_changes_nothing_writes_no_edit() {
    use crate::app::clip_effects::{color_params, vec2_params};

    let (mut state, _) = with_clip();
    state.add_effect_to_selected("color_overlay").expect("add");
    let (color_slot, color_spec) = color_params("color_overlay")
        .first()
        .copied()
        .expect("a colour slot");
    let (vec_slot, vec_spec) = vec2_params("color_overlay")
        .first()
        .copied()
        .expect("a vec2 slot");

    let before = state.engine().lock().revision();
    state
        .set_selected_effect_color(0, color_slot, color_spec.default_color)
        .expect("same colour");
    state
        .set_selected_effect_vec2(0, vec_slot, vec_spec.default_vec2)
        .expect("same point");
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "re-picking the current value wrote an edit"
    );

    // A missing effect, a missing slot, and a slot of the wrong kind are inert.
    state
        .set_selected_effect_color(9, color_slot, [1, 2, 3, 4])
        .expect("inert");
    state
        .set_selected_effect_color(0, 999, [1, 2, 3, 4])
        .expect("inert");
    state
        .set_selected_effect_color(0, vec_slot, [1, 2, 3, 4])
        .expect("inert");
    state
        .set_selected_effect_vec2(0, color_slot, [0.1, 0.1])
        .expect("inert");
    assert_eq!(state.engine().lock().revision(), before);
}
/// The projection carries one typed value per catalog slot. A new parameter
/// kind with no value representation turns this red.
#[test]
fn every_slot_of_every_effect_projects_a_value_of_its_own_kind() {
    let (mut state, _) = with_clip();
    for spec in aria_models::effect_catalog() {
        state.add_effect_to_selected(&spec.id).expect("add");
        let index = state.selected_ui_clip().expect("clip").effects.len() - 1;
        let effect = state.selected_ui_clip().expect("clip").effects[index].clone();
        assert_eq!(effect.values.len(), spec.params.len(), "{}", spec.id);

        for (slot, param) in spec.params.iter().enumerate() {
            match param.kind {
                EffectParamKind::Scalar => assert_eq!(
                    state.selected_effect_param(index, slot),
                    Some(param.default),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Color => assert_eq!(
                    state.selected_effect_color(index, slot),
                    Some(param.default_color),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Vec2 => assert_eq!(
                    state.selected_effect_vec2(index, slot),
                    Some(param.default_vec2),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
            }
        }
    }
}
