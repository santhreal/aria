//! The mask stack through the project: adding, replacing, removing and moving
//! masks, and writing one mask's properties by index.
//!
//! Closes the class where an index edit lands on the wrong mask or an invalid
//! edit lands at all: an add past the cap, a replacement or move out of range,
//! a keyframe written to mask 1 that changes mask 0. Driver re-addressing is in
//! `tests_drivers.rs`; rendering the stack is the compositor's.

use super::*;
use crate::look::{MAX_CLIP_MASKS, Mask, MaskKind, MaskMode};
use crate::{MaskParam, ParamValue};

fn media_clip() -> (Project, ClipId) {
    let (mut project, media, track) = project_with_media(500);
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    (project, clip)
}

fn kinds(project: &Project, clip: ClipId) -> Vec<MaskKind> {
    project
        .clip(clip)
        .unwrap()
        .masks
        .iter()
        .map(|mask| mask.kind)
        .collect()
}

#[test]
fn masks_append_to_the_bottom_of_the_stack_up_to_the_cap() {
    let (mut project, clip) = media_clip();
    assert_eq!(
        project.add_clip_mask(clip, Mask::new(MaskKind::Circle)),
        Ok(0)
    );
    assert_eq!(
        project.add_clip_mask(clip, Mask::new(MaskKind::Star)),
        Ok(1)
    );
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Circle, MaskKind::Star]
    );

    for _ in 2..MAX_CLIP_MASKS {
        project
            .add_clip_mask(clip, Mask::new(MaskKind::Heart))
            .unwrap();
    }
    let err = project
        .add_clip_mask(clip, Mask::new(MaskKind::Heart))
        .unwrap_err()
        .to_string();
    assert!(err.contains("already carries"), "{err}");
    assert_eq!(project.clip(clip).unwrap().masks.len(), MAX_CLIP_MASKS);
}

#[test]
fn an_invalid_mask_or_a_non_visual_clip_is_refused_and_nothing_changes() {
    let (mut project, clip) = media_clip();
    let mut faint = Mask::new(MaskKind::Circle);
    faint.opacity = 1.5.into();
    assert!(project.add_clip_mask(clip, faint.clone()).is_err());
    let mut swollen = Mask::new(MaskKind::Circle);
    swollen.expansion = (crate::look::MAX_MASK_EXPANSION * 2.0).into();
    assert!(project.add_clip_mask(clip, swollen).is_err());
    assert!(project.clip(clip).unwrap().masks.is_empty());

    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    assert!(project.set_clip_mask(clip, 0, faint).is_err());
    assert_eq!(kinds(&project, clip), vec![MaskKind::Circle]);

    let media = project.add_media(sample_media(R24, 500));
    let audio = project.add_track(TrackKind::Audio, "A1");
    let sound = project.add_clip(audio, media, tr(0, 48), rt(0)).unwrap();
    assert!(matches!(
        project.add_clip_mask(sound, Mask::new(MaskKind::Circle)),
        Err(ModelError::IncompatibleTrackKind { .. })
    ));
}

#[test]
fn replace_remove_and_move_address_one_index_and_refuse_a_missing_one() {
    let (mut project, clip) = media_clip();
    for kind in [MaskKind::Circle, MaskKind::Star, MaskKind::Heart] {
        project.add_clip_mask(clip, Mask::new(kind)).unwrap();
    }

    let mut subtract = Mask::new(MaskKind::Rectangle);
    subtract.mode = MaskMode::Subtract;
    project.set_clip_mask(clip, 1, subtract.clone()).unwrap();
    assert_eq!(project.clip(clip).unwrap().masks[1], subtract);
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Circle, MaskKind::Rectangle, MaskKind::Heart]
    );

    project.move_clip_mask(clip, 0, 2).unwrap();
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Rectangle, MaskKind::Heart, MaskKind::Circle]
    );
    project.move_clip_mask(clip, 2, 0).unwrap();
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Circle, MaskKind::Rectangle, MaskKind::Heart]
    );
    project.move_clip_mask(clip, 1, 1).unwrap();
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Circle, MaskKind::Rectangle, MaskKind::Heart]
    );

    assert_eq!(project.remove_clip_mask(clip, 1), Ok(subtract));
    assert_eq!(
        kinds(&project, clip),
        vec![MaskKind::Circle, MaskKind::Heart]
    );

    let before = project.clip(clip).unwrap().masks.clone();
    assert!(
        project
            .set_clip_mask(clip, 2, Mask::new(MaskKind::Star))
            .is_err()
    );
    assert!(project.remove_clip_mask(clip, 2).is_err());
    assert!(project.move_clip_mask(clip, 0, 2).is_err());
    assert!(project.move_clip_mask(clip, 2, 0).is_err());
    assert_eq!(project.clip(clip).unwrap().masks, before);
}

#[test]
fn a_mask_property_write_reaches_only_the_mask_it_names() {
    let (mut project, clip) = media_clip();
    for _ in 0..2 {
        project
            .add_clip_mask(clip, Mask::new(MaskKind::Circle))
            .unwrap();
    }
    let opacity = |mask| ClipParam::Mask {
        mask,
        param: MaskParam::Opacity,
    };
    let expansion = |mask| ClipParam::Mask {
        mask,
        param: MaskParam::Expansion,
    };
    project
        .set_param_keyframe(
            clip,
            opacity(1),
            rt(0),
            ParamValue::Scalar(0.25),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_constant(clip, expansion(1), ParamValue::Scalar(-30.0))
        .unwrap();

    let masks = &project.clip(clip).unwrap().masks;
    assert_eq!(masks[0], Mask::new(MaskKind::Circle));
    assert_eq!(masks[1].opacity.sample(0), 0.25);
    assert_eq!(masks[1].expansion, crate::param::Param::Constant(-30.0));

    // Out of range for the property, the wrong kind, and a mask that is not
    // there are all refused.
    for (param, value) in [
        (opacity(1), ParamValue::Scalar(1.5)),
        (
            expansion(1),
            ParamValue::Scalar(crate::look::MAX_MASK_EXPANSION + 1.0),
        ),
        (opacity(1), ParamValue::Vec2([0.5, 0.5])),
        (opacity(2), ParamValue::Scalar(0.5)),
    ] {
        assert!(
            project.set_param_constant(clip, param, value).is_err(),
            "{param:?} = {value:?} was accepted"
        );
    }
}
