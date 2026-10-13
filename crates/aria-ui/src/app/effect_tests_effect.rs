//! Effect: 9 of the cases in [`super`].
//!
//! Split out of `effect_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn every_scalar_parameter_of_every_catalog_effect_sets_and_clamps() {
    let (mut state, _) = with_clip();

    for spec in aria_models::effect_catalog() {
        state
            .add_effect_to_selected(&spec.id)
            .unwrap_or_else(|err| panic!("add {}: {err:?}", spec.id));
        let index = state.selected_ui_clip().expect("clip").effects.len() - 1;

        for (slot, param) in &scalar_params(&spec.id) {
            state
                .set_selected_effect_param(index, slot, param.max + 100.0)
                .unwrap_or_else(|err| panic!("high {}/{}: {err:?}", spec.id, param.name));
            let value = state
                .selected_effect_param(index, slot)
                .unwrap_or_else(|| panic!("{}/{} has no value", spec.id, param.name));
            assert!(
                value <= param.max + 1e-3,
                "{}/{} exceeded {}",
                spec.id,
                param.name,
                param.max
            );

            state
                .set_selected_effect_param(index, slot, param.min - 100.0)
                .expect("low");
            let value = state.selected_effect_param(index, slot).expect("value");
            assert!(
                value >= param.min - 1e-3,
                "{}/{} fell below {}",
                spec.id,
                param.name,
                param.min
            );

            state
                .set_selected_effect_param(index, slot, f32::NAN)
                .expect("nan");
            assert!(
                state
                    .selected_effect_param(index, slot)
                    .expect("value")
                    .is_finite(),
                "{}/{} took a NaN",
                spec.id,
                param.name
            );

            state
                .reset_selected_effect_param(index, slot)
                .expect("reset");
            let value = state.selected_effect_param(index, slot).expect("value");
            assert!(
                (value - param.default).abs() < 1e-3,
                "{}/{} did not return to its catalog default",
                spec.id,
                param.name
            );
        }
    }
}
#[test]
fn a_parameter_edit_touches_only_its_own_slot_and_its_own_effect() {
    let id = effect_with_scalar();
    let (mut state, _) = with_clip();
    state.add_effect_to_selected(id).expect("first");
    state.add_effect_to_selected(id).expect("second");

    let slots = scalar_params(id);
    let (slot, param) = slots.first().expect("a scalar slot");
    let target = (param.min + param.max) / 2.0;

    state
        .set_selected_effect_param(0, slot, target)
        .expect("set first");
    assert!(
        (state.selected_effect_param(0, slot).expect("value") - target).abs() < 1e-3,
        "the edit did not land"
    );
    assert!(
        (state.selected_effect_param(1, slot).expect("value") - param.default).abs() < 1e-3,
        "editing one effect changed its twin"
    );

    // Every other slot of the edited effect must still hold its default.
    for (other, other_spec) in slots.iter().skip(1) {
        assert!(
            (state.selected_effect_param(0, other).expect("value") - other_spec.default).abs()
                < 1e-3,
            "{} was reset by a sibling slot edit",
            other_spec.name
        );
    }
}
#[test]
fn reordering_moves_one_effect_and_the_ends_refuse_to_leave_the_chain() {
    let (mut state, _) = with_clip();
    let ids: Vec<&'static str> = aria_models::shipped_effect_catalog()
        .iter()
        .take(3)
        .map(|spec| spec.id.as_str())
        .collect();
    assert!(
        ids.len() >= 3,
        "the catalog must have three effects to reorder"
    );
    for id in &ids {
        state.add_effect_to_selected(*id).expect("add");
    }

    let chain = |state: &crate::app::AppState| -> Vec<String> {
        state
            .selected_ui_clip()
            .expect("clip")
            .effects
            .iter()
            .map(|effect| effect.id.to_string())
            .collect()
    };
    assert_eq!(chain(&state), ids);

    state.move_selected_effect_down(0).expect("down");
    assert_eq!(
        chain(&state),
        vec![ids[1].to_string(), ids[0].to_string(), ids[2].to_string()],
        "down must swap with the next effect only"
    );

    state.move_selected_effect_up(1).expect("up");
    assert_eq!(chain(&state), ids, "up must undo that move");

    // The ends cannot leave the chain, and both must be no-ops rather than errors.
    let before = chain(&state);
    state.move_selected_effect_up(0).expect("top up");
    state
        .move_selected_effect_down(ids.len() - 1)
        .expect("bottom down");
    state.move_selected_effect(0, 99).expect("out of range");
    state.move_selected_effect(99, 0).expect("out of range");
    state.move_selected_effect(1, 1).expect("nowhere");
    assert_eq!(chain(&state), before, "a refused move must change nothing");
}
#[test]
fn a_non_scalar_or_missing_slot_is_inert() {
    let (mut state, _) = with_clip();
    let non_scalar = aria_models::shipped_effect_catalog()
        .iter()
        .find_map(|spec| {
            spec.params
                .iter()
                .position(|param| param.kind != EffectParamKind::Scalar)
                .map(|slot| (spec.id.as_str(), slot))
        });

    if let Some((id, slot)) = non_scalar {
        state.add_effect_to_selected(id).expect("add");
        let before = state.selected_effect_param(0, slot);
        // A colour or vec2 slot is not settable through SetEffectParam: the UI
        // must drop it rather than send a command the model refuses.
        state
            .set_selected_effect_param(0, slot, 0.5)
            .expect("inert");
        assert_eq!(state.selected_effect_param(0, slot), before);
        assert!(
            !scalar_params(id).iter().any(|(s, _)| s == slot),
            "a non-scalar slot must not be offered as a stepper"
        );
    }

    // Missing effect and missing slot are both inert.
    let id = effect_with_scalar();
    state.add_effect_to_selected(id).expect("add");
    let count = state.selected_ui_clip().expect("clip").effects.len();
    state
        .set_selected_effect_param(count + 5, 0, 1.0)
        .expect("inert");
    state.set_selected_effect_param(0, 999, 1.0).expect("inert");
    let spec = effect_spec(id).expect("spec");
    assert_eq!(
        state.selected_ui_clip().expect("clip").effects.len(),
        count,
        "an inert edit must not change the chain"
    );
    assert!(!spec.params.is_empty());
}
#[test]
fn effect_commands_are_inert_with_no_selection() {
    let id = effect_with_scalar();
    let (mut state, _) = with_clip();
    state.add_effect_to_selected(id).expect("add");
    let before = state.selected_ui_clip().expect("clip").effects;

    state.clear_selection();
    state.set_selected_effect_param(0, 0, 1.0).expect("inert");
    state.move_selected_effect(0, 0).expect("inert");
    state.move_selected_effect_down(0).expect("inert");

    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert_eq!(clip.effects, before);
}
/// The Add list must be the model catalog, not the renderable subset. Hiding a
/// catalog entry with no GPU pass made an effect the project format accepts look
/// nonexistent; offering it labelled passthrough is the contract. The list is
/// derived from the catalog at run time, so a new effect is covered here and a new
/// unrenderable one is labelled rather than dropped.
#[test]
fn the_add_list_offers_every_model_effect_and_marks_the_unrenderable_ones() {
    let (state, _) = with_clip();
    let list = state.effect_browse_list();
    let catalog = aria_models::effect_catalog();
    assert_eq!(
        list.len(),
        catalog.len(),
        "the Add list is not the model catalog"
    );

    let service = state.ctx.try_get::<dyn aria_plugin::EffectService>();
    for (spec, (offered, label)) in catalog.iter().zip(&list) {
        let id = offered.id.as_str();
        assert_eq!(id, spec.id, "the Add list reordered the catalog");
        assert!(
            label.starts_with(spec.label.as_str()),
            "{id} lost its display name"
        );
        let renderable = service.as_ref().is_some_and(|svc| svc.renders_effect(id));
        assert_eq!(
            label.contains("passthrough"),
            !renderable,
            "{id} is labelled passthrough={} but renderable={renderable}",
            label.contains("passthrough")
        );
    }
}
/// Renderability is what the mounted effect service can dispatch, not what the
/// compositor was compiled with. With no GPU plugin mounted nothing can dispatch a
/// pass, so every entry says passthrough instead of promising a pass that would
/// never run.
#[test]
fn with_no_effect_service_every_entry_is_marked_passthrough() {
    let state = crate::app::AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("fx-no-service"),
    )
    .expect("state without a GPU plugin");
    assert!(
        state
            .ctx
            .try_get::<dyn aria_plugin::EffectService>()
            .is_none()
    );
    let list = state.effect_browse_list();
    assert_eq!(list.len(), aria_models::effect_catalog().len());
    for (spec, label) in list {
        assert!(
            label.contains("passthrough"),
            "{} claims a pass with no service to dispatch it",
            spec.id
        );
    }
}
/// The class closed below is "a slot the inspector cannot reach". Colour and vec2
/// slots have no constant command, so they were unreachable while every other slot
/// was a stepper. The slot list is read from the catalog at run time, so a new
/// colour or vec2 parameter is covered here without a second list, and a new
/// parameter kind turns the projection test red.

/// Every colour and vec2 slot of every catalog effect: set, clamp, reset, undo.
#[test]
fn every_colour_and_vec2_slot_sets_clamps_resets_and_undoes() {
    use crate::app::clip_effects::{PARAM_COLORS, color_params, vec2_params};

    let (mut state, _) = with_clip();
    let mut exercised = 0usize;

    for spec in aria_models::effect_catalog() {
        state
            .add_effect_to_selected(&spec.id)
            .unwrap_or_else(|err| panic!("add {}: {err:?}", spec.id));
        let index = state.selected_ui_clip().expect("clip").effects.len() - 1;

        for (slot, param) in &color_params(&spec.id) {
            exercised += 1;
            for (_, _, rgba) in PARAM_COLORS {
                state
                    .set_selected_effect_color(index, slot, rgba)
                    .unwrap_or_else(|err| panic!("{}/{}: {err:?}", spec.id, param.name));
                assert_eq!(
                    state.selected_effect_color(index, slot),
                    Some(rgba),
                    "{}/{} did not take the colour",
                    spec.id,
                    param.name
                );
            }
            let last = PARAM_COLORS[PARAM_COLORS.len() - 1].2;
            state
                .reset_selected_effect_non_scalar(index, slot)
                .expect("reset");
            assert_eq!(
                state.selected_effect_color(index, slot),
                Some(param.default_color),
                "{}/{} did not return to its catalog default",
                spec.id,
                param.name
            );
            // Only when the reset actually changed something: a default that is
            // already the last palette colour writes nothing to undo.
            if param.default_color != last {
                assert!(state.undo(), "the reset must be undoable");
                assert_eq!(
                    state.selected_effect_color(index, slot),
                    Some(last),
                    "one undo must restore the previous colour"
                );
            }
        }

        for (slot, param) in &vec2_params(&spec.id) {
            exercised += 1;
            state
                .set_selected_effect_vec2(index, slot, [param.max + 100.0, param.min - 100.0])
                .unwrap_or_else(|err| panic!("{}/{}: {err:?}", spec.id, param.name));
            let value = state
                .selected_effect_vec2(index, slot)
                .unwrap_or_else(|| panic!("{}/{} has no value", spec.id, param.name));
            assert!(
                value[0] <= param.max + 1e-3 && value[1] >= param.min - 1e-3,
                "{}/{} escaped [{}, {}]: {value:?}",
                spec.id,
                param.name,
                param.min,
                param.max
            );

            state
                .set_selected_effect_vec2(index, slot, [f32::NAN, 0.0])
                .expect("nan");
            assert_eq!(
                state.selected_effect_vec2(index, slot),
                Some(value),
                "{}/{} took a NaN",
                spec.id,
                param.name
            );

            let stepped = state.selected_effect_vec2(index, slot).expect("value");
            state
                .nudge_selected_effect_vec2(index, slot, 1, param.min - param.max)
                .expect("nudge y");
            let after = state.selected_effect_vec2(index, slot).expect("value");
            assert_eq!(after[0], stepped[0], "a y step moved x");

            state
                .reset_selected_effect_non_scalar(index, slot)
                .expect("reset");
            assert_eq!(
                state.selected_effect_vec2(index, slot),
                Some(param.default_vec2),
                "{}/{} did not return to its catalog default",
                spec.id,
                param.name
            );
        }
    }

    assert!(
        exercised > 0,
        "the catalog has no colour or vec2 slot: this test proves nothing"
    );
}
/// A colour write lands at the playhead when it is over the clip, and at the clip
/// start otherwise, so setting a colour from elsewhere in the sequence means the
/// clip's colour rather than a keyframe on a tick the clip does not own.
#[test]
fn a_non_scalar_write_lands_at_the_playhead_or_at_the_clip_start() {
    use crate::app::clip_effects::color_params;

    let (mut state, clip) = with_clip();
    state.add_effect_to_selected("color_overlay").expect("add");
    let (slot, _) = color_params("color_overlay")
        .first()
        .expect("color_overlay has a colour slot");

    let (start, end) = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .map(|c| (c.timeline_start_ticks, c.timeline_end_ticks))
        .expect("clip bounds");
    assert!(end - start > 4, "the fixture clip must span several ticks");

    // Playhead off the clip: the write must still be the clip's colour, which the
    // projection reads at the clip's first tick.
    state.playhead = aria_models::RationalTime::new(end + 10, state.store.sequence.frame_rate);
    let far = [220, 60, 60, 255];
    state
        .set_selected_effect_color(0, slot, far)
        .expect("off-clip write");
    assert_eq!(state.selected_effect_color(0, slot), Some(far));

    // Playhead inside the clip: the write is a keyframe there, so the value at the
    // clip's first tick is unchanged.
    state.playhead = aria_models::RationalTime::new(end - 1, state.store.sequence.frame_rate);
    let near = [60, 120, 230, 255];
    state
        .set_selected_effect_color(0, slot, near)
        .expect("on-clip write");
    assert_eq!(
        state.selected_effect_color(0, slot),
        Some(far),
        "a keyframe at the end of the clip must not move the clip's first tick"
    );
}
