//! Mask rows: one set per mask in the stack, each reading its own mask.
//!
//! Closes the class where a mask property is addressed by kind alone, so two
//! masks on one clip share one row and an edit to the second lands on the
//! first. Does not cover writing through a row; that is the project's
//! `tests_mask_stack.rs`.

use super::*;
use crate::MaskParam;

fn masked(kinds: &[MaskKind]) -> Clip {
    let mut clip = Clip::generated(
        Generator::SolidColor {
            rgba: [255, 255, 255, 255],
        },
        tr(0, 48),
    );
    clip.masks = kinds.iter().map(|&kind| Mask::new(kind)).collect();
    clip
}

fn mask_rows(clip: &Clip) -> Vec<ParamSpec> {
    clip_params_for(clip)
        .into_iter()
        .filter(|row| matches!(row.param, ClipParam::Mask { .. }))
        .collect()
}

#[test]
fn each_mask_contributes_the_rows_its_shape_draws_with() {
    let clip = masked(&[MaskKind::Circle, MaskKind::Rectangle, MaskKind::Path]);
    let ids: Vec<String> = mask_rows(&clip)
        .iter()
        .map(|row| format!("{} {}", row.group, row.id))
        .collect();
    assert_eq!(
        ids,
        [
            "mask-0 mask-0-feather",
            "mask-0 mask-0-center",
            "mask-0 mask-0-size",
            "mask-0 mask-0-rotation",
            "mask-0 mask-0-opacity",
            "mask-0 mask-0-expansion",
            "mask-1 mask-1-feather",
            "mask-1 mask-1-center",
            "mask-1 mask-1-size",
            "mask-1 mask-1-rotation",
            "mask-1 mask-1-roundness",
            "mask-1 mask-1-opacity",
            "mask-1 mask-1-expansion",
            "mask-2 mask-2-feather",
            "mask-2 mask-2-center",
            "mask-2 mask-2-rotation",
            "mask-2 mask-2-opacity",
            "mask-2 mask-2-expansion",
        ]
    );
    let roundness = mask_rows(&clip)
        .into_iter()
        .find(|row| row.id == "mask-1-roundness")
        .expect("roundness row");
    assert_eq!(roundness.label, "Mask 2 Roundness");
    assert_eq!(
        roundness.param,
        ClipParam::Mask {
            mask: 1,
            param: MaskParam::Roundness
        }
    );
}

#[test]
fn a_clip_without_masks_has_no_mask_rows() {
    assert!(mask_rows(&masked(&[])).is_empty());
    let mut visited = 0;
    visit_clip_params(&masked(&[]), |row| {
        if matches!(row.param, ClipParam::Mask { .. }) {
            visited += 1;
        }
    });
    assert_eq!(visited, 0);
}

#[test]
fn every_mask_row_samples_its_declared_kind_from_its_own_mask() {
    let mut clip = masked(&[MaskKind::Rectangle, MaskKind::Rectangle]);
    for (index, mask) in clip.masks.iter_mut().enumerate() {
        let n = index as f32 + 1.0;
        mask.feather = Param::Constant(0.1 * n);
        mask.center = Param::Constant([0.2 * n, -0.2 * n]);
        mask.size = Param::Constant([0.3 * n, 0.4 * n]);
        mask.rotation = Param::Constant(10.0 * n);
        mask.roundness = Param::Constant(0.15 * n);
        mask.opacity = Param::Constant(0.45 * n);
        mask.expansion = Param::Constant(-5.0 * n);
    }
    let rows = mask_rows(&clip);
    assert_eq!(rows.len(), 2 * MaskParam::ALL.len());
    for row in &rows {
        let ClipParam::Mask { mask, param } = row.param else {
            unreachable!("filtered to mask rows")
        };
        let n = mask as f32 + 1.0;
        let want = match param {
            MaskParam::Feather => ParamValue::Scalar(0.1 * n),
            MaskParam::Center => ParamValue::Vec2([0.2 * n, -0.2 * n]),
            MaskParam::Size => ParamValue::Vec2([0.3 * n, 0.4 * n]),
            MaskParam::Rotation => ParamValue::Scalar(10.0 * n),
            MaskParam::Roundness => ParamValue::Scalar(0.15 * n),
            MaskParam::Opacity => ParamValue::Scalar(0.45 * n),
            MaskParam::Expansion => ParamValue::Scalar(-5.0 * n),
        };
        assert_eq!(row.sample(&clip, 0), Some(want), "{}", row.id);
        assert_eq!(row.kind, param.kind(), "{}", row.id);
        assert_eq!(row.range, param.range(), "{}", row.id);
        assert!(row.available(&clip), "{}", row.id);
    }
}

#[test]
fn a_mask_row_past_the_end_of_the_stack_is_unavailable_and_samples_nothing() {
    let three = masked(&[MaskKind::Circle, MaskKind::Circle, MaskKind::Circle]);
    let one = masked(&[MaskKind::Circle]);
    let third = mask_rows(&three)
        .into_iter()
        .find(|row| row.id == "mask-2-feather")
        .expect("third mask feather");
    assert!(third.available(&three));
    assert!(!third.available(&one));
    assert_eq!(third.sample(&one, 0), None);
    assert!(third.keyframes(&one).is_empty());
}

/// A surface that holds a row id, a script or a saved reference, gets back the
/// property the row addresses. The ids come from the rows themselves, so a
/// new mask property or a renamed id fails here rather than resolving to
/// nothing in Python.
#[test]
fn every_mask_row_id_parses_back_to_its_property_and_nothing_else_does() {
    let kinds: Vec<MaskKind> = MaskKind::ALL
        .iter()
        .copied()
        .cycle()
        .take(crate::MAX_CLIP_MASKS)
        .collect();
    let clip = masked(&kinds);
    let rows = clip_params_for(&clip);
    let mut parsed = 0;
    for row in &rows {
        match row.param {
            ClipParam::Mask { .. } => {
                assert_eq!(mask_row_param(&row.id), Some(row.param), "{}", row.id);
                parsed += 1;
            }
            _ => assert_eq!(mask_row_param(&row.id), None, "{}", row.id),
        }
    }
    assert!(
        parsed > crate::MAX_CLIP_MASKS * 5,
        "only {parsed} mask rows"
    );

    assert_eq!(
        mask_row_param("mask-4294967295-expansion"),
        Some(ClipParam::Mask {
            mask: u32::MAX,
            param: MaskParam::Expansion,
        })
    );
    for id in [
        "mask-01-feather",
        "mask-+1-feather",
        "mask--feather",
        "mask-1-",
        "mask-1-colour",
        "mask-1-feather-",
        "mask-4294967296-feather",
        "mask-feather",
        "Mask-1-feather",
        "",
    ] {
        assert_eq!(mask_row_param(id), None, "{id:?} parsed");
    }
}
