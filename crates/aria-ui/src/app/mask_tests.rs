//! Contracts for the mask and chroma-key surface.
//!
//! `SetClipMask` and `SetClipChroma` each carry the whole value, so the class
//! closed here is "editing one knob resets the shape, the offset, or the key
//! colour". The knob and shape lists are enumerated from the model at run time,
//! so a new mask shape or knob fails until it is covered.

use super::premiere_tests::with_clip;

#[test]
fn every_mask_shape_applies_and_the_mask_clears() {
    let (mut state, _) = with_clip();
    assert!(
        state.selected_ui_clip().expect("clip").mask.is_none(),
        "a clip starts unmasked"
    );

    let shapes = aria_models::mask_catalog();
    assert!(!shapes.is_empty());
    for spec in shapes {
        state
            .set_selected_mask_kind(spec.kind)
            .unwrap_or_else(|err| panic!("apply {:?}: {err:?}", spec.kind));
        let mask = state
            .selected_ui_clip()
            .expect("clip")
            .mask
            .expect("mask applied");
        assert_eq!(
            mask.kind, spec.kind,
            "{:?} did not reach the projection",
            spec.kind
        );
    }

    state.clear_selected_mask().expect("clear");
    assert!(state.selected_ui_clip().expect("clip").mask.is_none());
}

/// Every mask row, with a target inside its own declared range.
///
/// Derived from the catalog at run time: a property the model gains appears here
/// without editing this file, and one that stops reaching a control disappears
/// from the panel and fails the sweep in `param_rows_tests.rs`.
fn mask_targets(
    state: &crate::app::AppState,
) -> Vec<(aria_models::ClipParam, Option<u8>, gpui::SharedString, f32)> {
    state
        .param_rows("mask")
        .into_iter()
        .map(|row| (row.param, row.component, row.id, row.range.clamp(0.5)))
        .collect()
}

#[test]
fn every_mask_row_moves_alone_and_a_shape_change_keeps_them() {
    use aria_models::MaskKind;

    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");

    let targets = mask_targets(&state);
    assert!(!targets.is_empty(), "a masked clip offered no rows");
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

    // Each edit addressed one property, so the earlier ones must still hold.
    for (param, component, id, target) in &targets {
        let now = state.param_scalar(*param, *component).expect("read");
        assert!(
            (now - target).abs() < 1e-3,
            "{id} was reset by a later row edit"
        );
    }

    // Changing the shape must not throw away the shaping.
    state
        .set_selected_mask_kind(MaskKind::Star)
        .expect("reshape");
    assert_eq!(
        state
            .selected_ui_clip()
            .expect("clip")
            .mask
            .expect("mask")
            .kind,
        MaskKind::Star
    );
    for (param, component, id, target) in &targets {
        let now = state.param_scalar(*param, *component).expect("read");
        assert!(
            (now - target).abs() < 1e-3,
            "{id} was reset by a shape change"
        );
    }
}

#[test]
fn mask_rows_clamp_to_the_validated_range() {
    use aria_models::MaskKind;

    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Rectangle)
        .expect("mask");
    let rows = state.param_rows("mask");
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
    use aria_models::MaskKind;
    let (mut state, _) = with_clip();

    // Every mask edit is inert before a shape exists: no panic, no mask.
    state.toggle_selected_mask_invert().expect("inert invert");
    let feather = aria_models::ClipParam::Look {
        param: aria_models::LookParam::MaskFeather,
    };
    state
        .set_param_scalar(feather, None, 0.5)
        .expect("inert row");
    state.nudge_param(feather, None, 0.5).expect("inert nudge");
    assert!(state.selected_ui_clip().expect("clip").mask.is_none());

    state.set_selected_mask_kind(MaskKind::Heart).expect("mask");
    state.toggle_selected_mask_invert().expect("invert");
    let mask = state.selected_ui_clip().expect("clip").mask.expect("mask");
    assert!(mask.invert);
    assert_eq!(mask.kind, MaskKind::Heart, "invert must keep the shape");

    state.toggle_selected_mask_invert().expect("uninvert");
    assert!(
        !state
            .selected_ui_clip()
            .expect("clip")
            .mask
            .expect("mask")
            .invert
    );
}

#[test]
fn a_property_the_clip_does_not_have_is_inert() {
    use aria_models::{ClipParam, MaskKind, ShapeParam};
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");
    let before = state.selected_ui_clip().expect("clip").mask.expect("mask");
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
    assert_eq!(
        state.selected_ui_clip().expect("clip").mask.expect("mask"),
        before
    );
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

    // Re-keying keeps the tolerance the operator dialled in.
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
    use aria_models::MaskKind;
    let (mut state, _) = with_clip();
    state.clear_selection();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("inert");
    state.clear_selected_mask().expect("inert");
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
    assert!(clip.mask.is_none());
    assert!(clip.chroma.is_none());
}
