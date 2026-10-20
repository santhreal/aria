//! Contracts for the mask and chroma-key surface.
//!
//! `SetClipMask` and `SetClipChroma` each carry the whole value, so the class
//! closed here is "editing one knob resets the shape, the offset, or the key
//! colour", and for the mask stack, "an edit lands on a mask other than the one
//! it names". The knob and shape lists are enumerated from the model at run
//! time, so a new mask shape or knob fails until it is covered.

use aria_models::{MaskKind, MaskMode, mask_param_group};

use super::AppState;
use super::premiere_tests::with_clip;

/// The kinds of the selected clip's masks, top of the stack first.
fn kinds(state: &AppState) -> Vec<MaskKind> {
    state
        .selected_ui_clip()
        .expect("clip")
        .masks
        .iter()
        .map(|mask| mask.kind)
        .collect()
}

/// A selected clip holding one mask of each of `stack`, in that order.
fn stacked(stack: &[MaskKind]) -> AppState {
    let (mut state, _) = with_clip();
    for kind in stack {
        state.add_selected_mask(*kind).expect("the mask lands");
    }
    assert_eq!(kinds(&state), stack, "the stack did not land in order");
    state
}

#[test]
fn every_mask_shape_adds_a_mask_and_clearing_removes_the_active_one() {
    let (mut state, _) = with_clip();
    assert!(
        state.selected_ui_clip().expect("clip").masks.is_empty(),
        "a clip starts unmasked"
    );
    assert_eq!(state.active_mask(), None);

    let shapes = aria_models::mask_catalog();
    assert!(!shapes.is_empty());
    for (index, spec) in shapes.iter().enumerate() {
        state
            .add_selected_mask(spec.kind)
            .unwrap_or_else(|err| panic!("add {:?}: {err:?}", spec.kind));
        let stack = kinds(&state);
        assert_eq!(stack.len(), index + 1, "{:?} did not add a mask", spec.kind);
        assert_eq!(
            stack[index], spec.kind,
            "{:?} did not reach the projection",
            spec.kind
        );
        assert_eq!(
            state.active_mask(),
            Some(index as u32),
            "a new mask becomes the active one"
        );
    }

    // Clearing removes the active mask, and the one above it becomes active.
    for left in (0..shapes.len()).rev() {
        state.clear_selected_mask().expect("clear");
        assert_eq!(kinds(&state).len(), left);
        assert_eq!(state.active_mask(), left.checked_sub(1).map(|i| i as u32));
    }
}

#[test]
fn every_add_mask_palette_row_adds_its_own_shape() {
    // Enumerated from the model, so a row that adds the wrong shape, or a shape
    // with no row, is red.
    for kind in MaskKind::ALL {
        let id = format!("pal-mask-add-{}", kind.id());
        let row = crate::commands::BUILTIN_COMMANDS
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("{id} is not a palette row"));
        let mut state = stacked(&[MaskKind::Circle]);
        (row.run)(&mut state);
        assert_eq!(kinds(&state), [MaskKind::Circle, kind], "{id}");
        assert_eq!(
            state.active_mask(),
            Some(1),
            "{id} left the new mask inactive"
        );
    }
}

#[test]
fn the_remove_mask_menu_row_is_disabled_until_the_clip_has_a_mask() {
    let guard = crate::chrome::menu_guards::mask;
    let (state, _) = with_clip();
    assert_eq!(
        guard("seq-mask-clear", &state),
        Some("The selected clip has no mask.")
    );
    let state = stacked(&[MaskKind::Star]);
    assert_eq!(guard("seq-mask-clear", &state), None);
}

/// Every row of the mask at `index`, with a target inside its own declared range.
///
/// Derived from the catalog at run time: a property the model gains appears here
/// without editing this file, and one that stops reaching a control disappears
/// from the panel and fails the sweep in `param_rows_tests.rs`.
fn mask_targets(
    state: &AppState,
    index: u32,
) -> Vec<(aria_models::ClipParam, Option<u8>, gpui::SharedString, f32)> {
    state
        .param_rows(&mask_param_group(index))
        .into_iter()
        .map(|row| (row.param, row.component, row.id, row.range.clamp(0.5)))
        .collect()
}

#[test]
fn every_mask_row_moves_alone_and_structural_edits_keep_them() {
    let mut state = stacked(&[MaskKind::Rectangle, MaskKind::Circle]);

    let targets = mask_targets(&state, 0);
    assert!(!targets.is_empty(), "a masked clip offered no rows");
    let second = mask_targets(&state, 1);
    assert!(!second.is_empty(), "the second mask offered no rows");
    let untouched: Vec<f32> = second
        .iter()
        .map(|(param, component, _, _)| state.param_scalar(*param, *component).expect("read"))
        .collect();
    for (param, component, id, target) in &targets {
        state
            .set_param_scalar(*param, *component, *target)
            .unwrap_or_else(|err| panic!("set {id}: {err:?}"));
        let now = state.param_scalar(*param, *component).expect("read");
        assert!(
            (now - target).abs() < 1e-3,
            "{id} did not take its value ({now} vs {target})"
        );
    }

    let hold = |state: &AppState, what: &str| {
        // Each edit addressed one property of one mask, so the earlier ones
        // must still hold and the other mask must not have moved.
        for (param, component, id, target) in &targets {
            let now = state.param_scalar(*param, *component).expect("read");
            assert!((now - target).abs() < 1e-3, "{id} was reset by {what}");
        }
        for ((param, component, id, _), before) in second.iter().zip(&untouched) {
            let now = state.param_scalar(*param, *component).expect("read");
            assert!((now - before).abs() < 1e-6, "{id} moved on {what}");
        }
    };
    hold(&state, "a later row edit");

    state
        .set_selected_mask_mode(0, MaskMode::Subtract)
        .expect("mode");
    hold(&state, "a mode change");
    state.toggle_selected_mask_invert(0).expect("invert");
    hold(&state, "an invert");
    state.set_active_mask(0);
    state
        .set_selected_mask_geometry([0.1, -0.1], [0.4, 0.3], 15.0)
        .expect("geometry of the first mask");
    let masks = state.selected_ui_clip().expect("clip").masks;
    assert_eq!(
        masks[0].center,
        [0.1, -0.1],
        "the geometry missed the active mask"
    );
    assert_eq!(
        masks[0].mode,
        MaskMode::Subtract,
        "the geometry edit reset the mode"
    );
    assert!(masks[0].invert, "the geometry edit reset the invert");
    assert_eq!(
        masks[1].center,
        [0.0, 0.0],
        "the geometry reached the other mask"
    );
}

#[test]
fn mask_rows_clamp_to_the_validated_range() {
    let mut state = stacked(&[MaskKind::Rectangle]);
    let rows = state.param_rows(&mask_param_group(0));
    assert!(!rows.is_empty());
    for row in rows {
        let (param, component, id) = (row.param, row.component, row.id);
        state
            .set_param_scalar(param, component, 1e6)
            .unwrap_or_else(|err| panic!("high {id}: {err:?}"));
        let high = state.param_scalar(param, component).expect("read");
        if let Some(max) = row.range.max {
            assert!(high <= max + 1e-3, "{id} exceeded {max}");
        }
        assert!(high.is_finite(), "{id} took a non-finite high value");

        state
            .set_param_scalar(param, component, -1e6)
            .unwrap_or_else(|err| panic!("low {id}: {err:?}"));
        let low = state.param_scalar(param, component).expect("read");
        if let Some(min) = row.range.min {
            assert!(low >= min - 1e-3, "{id} fell below {min}");
        }

        // Non-finite input must be dropped, not written and then rejected.
        state
            .set_param_scalar(param, component, f32::NAN)
            .expect("nan");
        let after = state.param_scalar(param, component).expect("read");
        assert!(after.is_finite(), "{id} took a NaN");
        assert!(
            (after - low).abs() < 1e-6,
            "{id} moved on a NaN instead of ignoring it"
        );
    }
}

#[test]
fn mask_invert_toggles_and_mask_edits_need_a_mask() {
    let (mut state, _) = with_clip();

    // Every mask edit is inert before a mask exists: no panic, no mask.
    state.toggle_active_mask_invert().expect("inert invert");
    state.toggle_selected_mask_invert(0).expect("inert invert");
    state
        .set_selected_mask_mode(0, MaskMode::Intersect)
        .expect("inert mode");
    let feather = aria_models::ClipParam::Mask {
        mask: 0,
        param: aria_models::MaskParam::Feather,
    };
    state
        .set_param_scalar(feather, None, 0.5)
        .expect("inert row");
    state.nudge_param(feather, None, 0.5).expect("inert nudge");
    assert!(state.selected_ui_clip().expect("clip").masks.is_empty());

    state.add_selected_mask(MaskKind::Heart).expect("mask");
    state.toggle_active_mask_invert().expect("invert");
    let mask = state.active_ui_mask().expect("mask");
    assert!(mask.invert);
    assert_eq!(mask.kind, MaskKind::Heart, "invert must keep the shape");

    state.toggle_selected_mask_invert(0).expect("uninvert");
    assert!(!state.active_ui_mask().expect("mask").invert);
}

#[test]
fn mode_invert_move_and_remove_edit_the_mask_they_name() {
    let stack = [MaskKind::Rectangle, MaskKind::Circle, MaskKind::Star];
    for target in 0..stack.len() as u32 {
        for mode in MaskMode::ALL {
            let mut state = stacked(&stack);
            state.set_selected_mask_mode(target, mode).expect("mode");
            let masks = state.selected_ui_clip().expect("clip").masks;
            for (index, mask) in masks.iter().enumerate() {
                let want = if index as u32 == target {
                    mode
                } else {
                    MaskMode::Add
                };
                assert_eq!(mask.mode, want, "mode {mode:?} on {target} reached {index}");
            }
        }

        let mut state = stacked(&stack);
        state.toggle_selected_mask_invert(target).expect("invert");
        let masks = state.selected_ui_clip().expect("clip").masks;
        for (index, mask) in masks.iter().enumerate() {
            assert_eq!(
                mask.invert,
                index as u32 == target,
                "inverting {target} reached {index}"
            );
        }

        let mut state = stacked(&stack);
        state.remove_selected_mask(target).expect("remove");
        let mut want = stack.to_vec();
        want.remove(target as usize);
        assert_eq!(kinds(&state), want, "removing {target}");
        assert_eq!(
            state.status_message.as_deref(),
            Some(format!("Removed mask {}", target + 1).as_str())
        );

        for to in 0..stack.len() as u32 {
            let mut state = stacked(&stack);
            state.move_selected_mask(target, to).expect("move");
            let mut want = stack.to_vec();
            let moved = want.remove(target as usize);
            want.insert(to as usize, moved);
            assert_eq!(kinds(&state), want, "moving {target} to {to}");
        }
    }
}

/// The active mask is the mask the Viewer handles, the Pen and the tracker
/// edit, so a reorder or a removal that left it pointing at an index would hand
/// those surfaces a different mask.
#[test]
fn the_active_mask_stays_the_same_mask_through_moves_and_removals() {
    let stack = [
        MaskKind::Rectangle,
        MaskKind::Circle,
        MaskKind::Star,
        MaskKind::Heart,
    ];
    let count = stack.len() as u32;
    for active in 0..count {
        for from in 0..count {
            for to in 0..count {
                let mut state = stacked(&stack);
                state.set_active_mask(active);
                let held = state.active_ui_mask().expect("active").kind;
                state.move_selected_mask(from, to).expect("move");
                assert_eq!(
                    state.active_ui_mask().map(|mask| mask.kind),
                    Some(held),
                    "active {active}, moving {from} to {to}"
                );
            }
            let mut state = stacked(&stack);
            state.set_active_mask(active);
            let held = stack[active as usize];
            state.remove_selected_mask(from).expect("remove");
            let now = state.active_ui_mask().map(|mask| mask.kind);
            let want = if from == active {
                // The mask above a removed active mask, or the new top.
                stack[active.saturating_sub(1) as usize + usize::from(active == 0)]
            } else {
                held
            };
            assert_eq!(now, Some(want), "active {active}, removing {from}");
        }
    }
}

#[test]
fn the_active_mask_belongs_to_one_clip_and_clamps_to_its_stack() {
    let mut state = stacked(&[MaskKind::Rectangle, MaskKind::Circle]);
    state.set_active_mask(7);
    assert_eq!(
        state.active_mask(),
        Some(1),
        "an index past the stack reads as the last mask"
    );
    state.set_active_mask(0);
    let clip = state.selected_clip().expect("clip");
    state.clear_selection();
    assert_eq!(state.active_mask(), None, "no selection has no active mask");
    state.select_clip(clip);
    assert_eq!(state.active_mask(), Some(0));
}

#[test]
fn a_property_the_clip_does_not_have_is_inert() {
    use aria_models::{ClipParam, ShapeParam};
    let mut state = stacked(&[MaskKind::Circle]);
    let before = state.active_ui_mask().expect("mask");
    // A shape property on a masked media clip: no row, so nothing to write.
    state
        .set_param_scalar(
            ClipParam::Shape {
                param: ShapeParam::CornerRadius,
            },
            None,
            0.9,
        )
        .expect("inert");
    // A property of a mask the clip does not have.
    state
        .set_param_scalar(
            ClipParam::Mask {
                mask: 1,
                param: aria_models::MaskParam::Feather,
            },
            None,
            0.9,
        )
        .expect("inert");
    assert_eq!(state.active_ui_mask().expect("mask"), before);
    assert_eq!(kinds(&state), [MaskKind::Circle]);
}

#[test]
fn chroma_seeds_a_usable_tolerance_and_keeps_the_colour_across_rows() {
    use aria_models::{ClipParam, LookParam};

    let (mut state, _) = with_clip();
    assert!(state.selected_ui_clip().expect("clip").chroma.is_none());

    // Every chroma edit is inert before a key colour exists.
    let strength = ClipParam::Look {
        param: LookParam::ChromaStrength,
    };
    state.set_param_scalar(strength, None, 0.9).expect("inert");
    assert!(state.selected_ui_clip().expect("clip").chroma.is_none());

    state.set_selected_chroma_color([0, 177, 64]).expect("key");
    let chroma = state
        .selected_ui_clip()
        .expect("clip")
        .chroma
        .expect("chroma");
    assert_eq!(chroma.rgb, [0, 177, 64]);
    assert!(
        chroma.strength > 0.0,
        "the first key must do something visible"
    );

    let rows = state.param_rows("chroma");
    assert!(!rows.is_empty(), "a keyed clip offered no chroma rows");
    for row in &rows {
        state
            .set_param_scalar(row.param, row.component, 0.75)
            .unwrap_or_else(|err| panic!("set {}: {err:?}", row.id));
    }
    for row in &rows {
        let now = state.param_scalar(row.param, row.component).expect("read");
        assert!(
            (now - 0.75).abs() < 1e-3,
            "{} was reset by a later row edit",
            row.id
        );
    }
    assert_eq!(
        state
            .selected_ui_clip()
            .expect("clip")
            .chroma
            .expect("chroma")
            .rgb,
        [0, 177, 64],
        "a row edit must keep the colour"
    );

    // Re-keying keeps the dialled-in tolerance.
    state
        .set_selected_chroma_color([0, 71, 187])
        .expect("rekey");
    let chroma = state
        .selected_ui_clip()
        .expect("clip")
        .chroma
        .expect("chroma");
    assert_eq!(chroma.rgb, [0, 71, 187]);
    assert!((state.param_scalar(strength, None).expect("read") - 0.75).abs() < 1e-3);

    state.clear_selected_chroma().expect("clear");
    assert!(state.selected_ui_clip().expect("clip").chroma.is_none());
}

#[test]
fn chroma_rows_clamp_to_unit_range() {
    let (mut state, _) = with_clip();
    state.set_selected_chroma_color([0, 177, 64]).expect("key");
    let rows = state.param_rows("chroma");
    assert!(!rows.is_empty());
    for row in rows {
        state
            .set_param_scalar(row.param, row.component, 9.0)
            .expect("high");
        let high = state.param_scalar(row.param, row.component).expect("read");
        assert!(high <= 1.0 + 1e-6, "{} exceeded 1", row.id);
        state
            .set_param_scalar(row.param, row.component, -9.0)
            .expect("low");
        let low = state.param_scalar(row.param, row.component).expect("read");
        assert!(low >= -1e-6, "{} fell below 0", row.id);
    }
}

#[test]
fn mask_and_chroma_commands_are_inert_with_no_selection() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    state.add_selected_mask(MaskKind::Circle).expect("inert");
    state.clear_selected_mask().expect("inert");
    state.remove_selected_mask(0).expect("inert");
    state.move_selected_mask(0, 1).expect("inert");
    state.set_active_mask(0);
    assert_eq!(state.active_mask(), None);
    state
        .set_selected_chroma_color([0, 177, 64])
        .expect("inert");
    state.clear_selected_chroma().expect("inert");

    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert!(clip.masks.is_empty());
    assert!(clip.chroma.is_none());
}
