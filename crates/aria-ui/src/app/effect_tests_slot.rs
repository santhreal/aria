//! Effect color and vector slot write tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `effect_tests.rs` through `use super::*`.

use super::*;

/// A write that changes nothing writes no edit, so a click on the swatch already
/// picked costs no undo entry.
#[test]
fn a_non_scalar_write_that_changes_nothing_writes_no_edit() {
    use crate::app::clip_effects::{color_params, vec2_params};

    let (mut state, _) = with_clip();
    state.add_effect_to_selected("color_overlay").expect("add");
    let colors = color_params("color_overlay");
    let (color_slot, color_spec) = colors.first().expect("a color slot");
    let points = vec2_params("color_overlay");
    let (vec_slot, vec_spec) = points.first().expect("a vec2 slot");

    let before = state.engine().lock().revision();
    state
        .set_selected_effect_color(0, color_slot, color_spec.default_color)
        .expect("same color");
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
                EffectParamKind::Scalar | EffectParamKind::Angle => assert_eq!(
                    state.selected_effect_param(index, slot),
                    Some(param.default),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Int => assert_eq!(
                    state.selected_effect_param(index, slot),
                    Some(param.default_int as f32),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Color => assert_eq!(
                    state.selected_effect_color(index, slot),
                    Some(param.default_color_u8()),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Vec2 | EffectParamKind::Size => assert_eq!(
                    state.selected_effect_vec2(index, slot),
                    Some(param.default_vec2),
                    "{}/{}",
                    spec.id,
                    param.name
                ),
                EffectParamKind::Point3
                | EffectParamKind::Bool
                | EffectParamKind::Choice
                | EffectParamKind::Curve
                | EffectParamKind::Gradient
                | EffectParamKind::Text
                | EffectParamKind::Font
                | EffectParamKind::File
                | EffectParamKind::Layer => {
                    assert!(
                        effect.values.get(slot).is_some(),
                        "{}/{}",
                        spec.id,
                        param.name
                    );
                }
            }
        }
    }
}

/// Editing v2 effect parameter slots updates projection values, toggles
/// `visible_when` rows, and restores cleanly across undo/redo.
#[test]
fn v2_effect_slot_edits_and_visibility_roundtrip_with_undo() {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        use aria_models::{
            EffectChoiceOption, EffectGroupSpec, EffectParamKind, EffectParamSpec, EffectSpec,
            EffectVisibilityCondition, register_effect,
        };
        let (_guard, _) = register_effect(EffectSpec {
            id: "test.ui.v2_slots".into(),
            label: "V2 Slots".into(),
            category: "Stylize".into(),
            params: vec![
                EffectParamSpec {
                    name: "enable_detail".into(),
                    label: "Enable Detail".into(),
                    kind: EffectParamKind::Bool,
                    default_bool: false,
                    group: Some("detail".into()),
                    ..Default::default()
                },
                EffectParamSpec {
                    name: "mode".into(),
                    label: "Mode".into(),
                    kind: EffectParamKind::Choice,
                    choices: vec![
                        EffectChoiceOption::new("soft", "Soft"),
                        EffectChoiceOption::new("crisp", "Crisp"),
                    ],
                    default_choice: Some("soft".into()),
                    visible_when: Some(EffectVisibilityCondition::new("enable_detail", "true")),
                    group: Some("detail".into()),
                    ..Default::default()
                },
                EffectParamSpec {
                    name: "steps".into(),
                    label: "Steps".into(),
                    kind: EffectParamKind::Int,
                    default_int: 2,
                    int_min: 1,
                    int_max: 8,
                    ..Default::default()
                },
                EffectParamSpec {
                    name: "origin".into(),
                    label: "Origin".into(),
                    kind: EffectParamKind::Point3,
                    default_vec3: [0.0, 0.0, 0.0],
                    min: -1.0,
                    max: 1.0,
                    ..Default::default()
                },
                EffectParamSpec {
                    name: "caption".into(),
                    label: "Caption".into(),
                    kind: EffectParamKind::Text,
                    default_text: "initial".into(),
                    ..Default::default()
                },
            ],
            groups: vec![EffectGroupSpec {
                id: "detail".into(),
                label: "Detail".into(),
                collapsed: true,
            }],
            ..Default::default()
        })
        .expect("register test effect");

        let (mut state, _) = with_clip();
        state
            .add_effect_to_selected("test.ui.v2_slots")
            .expect("add");
        use crate::surfaces::effects_rack::SlotEdit;

        // Group starts collapsed per spec default; toggling opens and re-collapses it.
        assert!(state.is_effect_group_collapsed(0, "detail", true));
        state.toggle_effect_group(0, "detail");
        assert!(!state.is_effect_group_collapsed(0, "detail", true));
        state.toggle_effect_group(0, "detail");
        assert!(state.is_effect_group_collapsed(0, "detail", true));

        // Initially slot 1 (`mode`) is hidden because `enable_detail` is false.
        let fx0 = &state.selected_ui_clip().expect("clip").effects[0];
        assert_eq!(fx0.visible.as_slice(), &[true, false, true, true, true]);

        // Flip `enable_detail` (slot 0) -> slot 1 becomes visible.
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::ToggleBool)
            .expect("set bool");
        let fx1 = &state.selected_ui_clip().expect("clip").effects[0];
        assert_eq!(fx1.visible.as_slice(), &[true, true, true, true, true]);
        assert_eq!(fx1.values[0], crate::types::UiEffectValue::Bool(true));

        // Edit choice, int, point3, and text slots, then undo back to default bool.
        state
            .edit_selected_effect_slot(0, 1, SlotEdit::Choice("crisp".into()))
            .expect("set choice");
        state
            .edit_selected_effect_slot(0, 2, SlotEdit::NudgeInt(4))
            .expect("set int");
        state
            .set_selected_effect_point3(0, 3, [0.25, -0.5, 0.75])
            .expect("set point3");
        state
            .edit_selected_effect_slot(0, 4, SlotEdit::Text("hello".into()))
            .expect("set text");

        let fx2 = &state.selected_ui_clip().expect("clip").effects[0];
        assert_eq!(
            fx2.values[1],
            crate::types::UiEffectValue::Choice("crisp".into())
        );
        assert_eq!(fx2.values[2], crate::types::UiEffectValue::Int(6));
        assert_eq!(
            fx2.values[3],
            crate::types::UiEffectValue::Point3([0.25, -0.5, 0.75])
        );
        assert_eq!(
            fx2.values[4],
            crate::types::UiEffectValue::Text("hello".into())
        );

        for _ in 0..5 {
            assert!(state.undo());
        }
        let fx_undone = &state.selected_ui_clip().expect("clip").effects[0];
        assert_eq!(
            fx_undone.values[0],
            crate::types::UiEffectValue::Bool(false)
        );
        assert_eq!(
            fx_undone.visible.as_slice(),
            &[true, false, true, true, true]
        );
    });
}

/// A layer pick the model refuses (its own clip, or a cycle through another
/// clip's layer parameter) leaves the project untouched and records the reason
/// against that clip, effect, and slot for the effect row. An accepted pick
/// clears it.
///
/// Catches: a refusal that is dropped instead of recorded, one keyed to the
/// wrong row or clip, and one that outlives the next accepted pick. Does not
/// catch a refusal the draw then fails to paint.
#[test]
fn a_refused_layer_pick_is_recorded_on_its_row_and_cleared_by_an_accepted_one() {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        use crate::surfaces::effects_rack::SlotEdit;
        use aria_models::{ClipId, EffectParamSpec, EffectSpec, MediaId, TrackId, register_effect};

        let (_guard, _) = register_effect(EffectSpec {
            id: "test.ui.layer_refusal".into(),
            label: "Layer Refusal".into(),
            category: "Stylize".into(),
            params: vec![EffectParamSpec {
                name: "matte".into(),
                label: "Matte".into(),
                kind: EffectParamKind::Layer,
                ..Default::default()
            }],
            ..Default::default()
        })
        .expect("register test effect");

        let (mut state, first) = with_clip();
        let lane = state.store.sequence.tracks.first().expect("one lane");
        let track = TrackId::from_raw(lane.id);
        let end = lane
            .clips
            .iter()
            .map(|clip| clip.timeline_end_ticks)
            .max()
            .expect("the fixture clip");
        let media = MediaId::from_raw(state.store.media.first().expect("one pool item").id);
        state
            .drop_media_on_timeline(media, Some(track), end)
            .expect("place a second clip");
        state.sync_from_engine();
        let second = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| ClipId::from_raw(clip.id))
            .find(|&clip| clip != first)
            .expect("a second clip");

        // First points at second: accepted.
        state.select_clip(first);
        state
            .add_effect_to_selected("test.ui.layer_refusal")
            .expect("add to first");
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::Layer(Some(second)))
            .expect("first references second");
        assert_eq!(state.effect_layer_refusal(0, 0), None);

        // Second points back at first: a cycle, refused and recorded.
        state.select_clip(second);
        state
            .add_effect_to_selected("test.ui.layer_refusal")
            .expect("add to second");
        let before = state.engine().lock().revision();
        let cycle = state.edit_selected_effect_slot(0, 0, SlotEdit::Layer(Some(first)));
        assert!(cycle.is_err(), "a layer-reference cycle was accepted");
        assert_eq!(state.engine().lock().revision(), before);
        let message = state.effect_layer_refusal(0, 0).expect("refusal recorded");
        assert!(message.contains("cycle"), "{message}");
        assert_eq!(state.effect_layer_refusal(0, 1), None, "wrong slot");
        assert_eq!(state.effect_layer_refusal(1, 0), None, "wrong effect");
        state.select_clip(first);
        assert_eq!(state.effect_layer_refusal(0, 0), None, "wrong clip");

        // Its own clip: refused, and the reason replaces the cycle's.
        state.select_clip(second);
        assert!(
            state
                .edit_selected_effect_slot(0, 0, SlotEdit::Layer(Some(second)))
                .is_err(),
            "a self-reference was accepted"
        );
        let message = state.effect_layer_refusal(0, 0).expect("refusal recorded");
        assert!(message.contains("itself"), "{message}");

        // An accepted pick clears it.
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::Layer(None))
            .expect("clear the reference");
        assert_eq!(state.effect_layer_refusal(0, 0), None);
    });
}

/// Transition parameters update through `edit_selected_transition_slot` and
/// restore when undone.
#[test]
fn transition_parameter_edit_restores_on_undo_and_redo() {
    let (mut state, clip) = crate::app::fixtures::blended_junction();
    state.select_clip(clip);
    state
        .set_selected_transition("wipe_left")
        .expect("set wipe_left");

    use crate::surfaces::clip_pick::TransitionSlotEdit;
    let tr0 = state
        .selected_ui_clip()
        .expect("clip")
        .transition
        .expect("transition");
    assert_eq!(
        tr0.values.as_ref(),
        &[crate::types::UiEffectValue::Scalar(0.0)]
    );

    state
        .edit_selected_transition_slot(0, TransitionSlotEdit::NudgeScalar(0.35))
        .expect("set feather");
    let tr1 = state
        .selected_ui_clip()
        .expect("clip")
        .transition
        .expect("transition");
    assert_eq!(
        tr1.values.as_ref(),
        &[crate::types::UiEffectValue::Scalar(0.35)]
    );

    assert!(state.undo());
    let tr_undone = state
        .selected_ui_clip()
        .expect("clip")
        .transition
        .expect("transition");
    assert_eq!(
        tr_undone.values.as_ref(),
        &[crate::types::UiEffectValue::Scalar(0.0)]
    );

    assert!(state.redo());
    let tr_redone = state
        .selected_ui_clip()
        .expect("clip")
        .transition
        .expect("transition");
    assert_eq!(
        tr_redone.values.as_ref(),
        &[crate::types::UiEffectValue::Scalar(0.35)]
    );
}

/// "Reset returns the property to its default, a keyframe when animated", and a
/// value edit on an animated property keys it at the playhead. Before, Reset
/// deleted the stored slot, which dropped every keyframe, and an integer edit
/// replaced the curve with a constant. Covers the scalar and integer write paths
/// and Reset on both, on a constant and on an animated slot; the colour, vector
/// and point setters share the same landing and are covered by the landing test
/// in `effect_tests_effect`.
#[test]
fn reset_and_value_edits_key_an_animated_effect_slot_and_keep_its_curve() {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        use crate::surfaces::effects_rack::SlotEdit;
        use aria_models::{
            ClipParam, EffectParamKind, EffectParamSpec, EffectSpec, RationalTime, register_effect,
        };

        let (_guard, _) = register_effect(EffectSpec {
            id: "test.ui.animated_slots".into(),
            label: "Animated Slots".into(),
            category: "Stylize".into(),
            params: vec![
                EffectParamSpec {
                    name: "amount".into(),
                    label: "Amount".into(),
                    kind: EffectParamKind::Scalar,
                    default: 0.5,
                    min: 0.0,
                    max: 1.0,
                    ..Default::default()
                },
                EffectParamSpec {
                    name: "steps".into(),
                    label: "Steps".into(),
                    kind: EffectParamKind::Int,
                    default_int: 2,
                    int_min: 1,
                    int_max: 8,
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
        .expect("register test effect");

        let (mut state, clip) = with_clip();
        state
            .add_effect_to_selected("test.ui.animated_slots")
            .expect("add");
        let rate = state.store.sequence.frame_rate;
        let start = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|c| c.id == clip.raw())
            .map(|c| c.timeline_start_ticks)
            .expect("clip start");
        let slot = |param: u32| ClipParam::Effect { effect: 0, param };
        let keys = |state: &crate::app::AppState, param: u32| -> Vec<i64> {
            state
                .keyframes_of(slot(param))
                .iter()
                .map(|mark| mark.tick)
                .collect()
        };

        // Constant: edits stay constant, and a reset of a constant slot is still the
        // catalog default.
        state.playhead = RationalTime::new(start, rate);
        state.set_selected_effect_param(0, 0, 0.9).expect("scalar");
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::Reset)
            .expect("reset");
        assert_eq!(state.selected_effect_param(0, 0), Some(0.5));
        state.set_selected_effect_param(0, 0, 0.2).expect("scalar");
        state.set_selected_effect_int(0, 1, 5).expect("int");
        assert_eq!(keys(&state, 0), Vec::<i64>::new());
        assert_eq!(keys(&state, 1), Vec::<i64>::new());

        // Animate both at the clip start, then edit two frames in.
        state.add_keyframe_at_playhead(slot(0)).expect("animate");
        state.add_keyframe_at_playhead(slot(1)).expect("animate");
        let at = start + 2;
        state.playhead = RationalTime::new(at, rate);
        state
            .set_selected_effect_param(0, 0, 0.9)
            .expect("scalar key");
        state
            .edit_selected_effect_slot(0, 1, SlotEdit::NudgeInt(2))
            .expect("int key");
        assert_eq!(keys(&state, 0), vec![start, at]);
        assert_eq!(keys(&state, 1), vec![start, at]);
        assert_eq!(state.selected_effect_param(0, 0), Some(0.9));
        assert_eq!(state.selected_effect_int(0, 1), Some(7));

        // Reset keys the default where the row reads and keeps the curve.
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::Reset)
            .expect("reset");
        state
            .edit_selected_effect_slot(0, 1, SlotEdit::Reset)
            .expect("reset");
        assert_eq!(keys(&state, 0), vec![start, at], "reset dropped the curve");
        assert_eq!(keys(&state, 1), vec![start, at], "reset dropped the curve");
        assert_eq!(state.selected_effect_param(0, 0), Some(0.5));
        assert_eq!(state.selected_effect_int(0, 1), Some(2));
        state.playhead = RationalTime::new(start, rate);
        assert_eq!(
            state.selected_effect_param(0, 0),
            Some(0.2),
            "the start moved"
        );
        assert_eq!(state.selected_effect_int(0, 1), Some(5), "the start moved");

        // Each reset is one undo step.
        assert!(state.undo());
        state.playhead = RationalTime::new(at, rate);
        assert_eq!(state.selected_effect_int(0, 1), Some(7));
        assert_eq!(state.selected_effect_param(0, 0), Some(0.5));
    });
}
