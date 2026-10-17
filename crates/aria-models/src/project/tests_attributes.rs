//! Remove Attributes in the model: each group returns to a new clip's values,
//! keyframes and drivers included, and nothing outside the chosen groups moves.
//!
//! Closes the class where a reset leaves part of a group behind (a keyframe
//! curve, a driver that keeps overriding the reset value, a blend mode beside a
//! reset opacity) or reaches into a group nobody chose. The groups are read from
//! `ClipAttribute::ALL` and edited through an exhaustive match, so a new group
//! does not compile here until it has an edit. Does not cover the engine's undo
//! record or the dialog; those are `aria-engine`'s and `aria-ui`'s.

use super::*;
use crate::clip::DriverCode;
use crate::look::{BlendMode, ChromaKey, Filter, Mask, MaskKind};
use crate::{AnimatedTransform, ClipAttribute};

fn media_clip() -> (Project, ClipId) {
    let (mut project, media, track) = project_with_media(500);
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    (project, clip)
}

/// Put something into `attribute` that a new clip does not hold.
fn set(project: &mut Project, clip: ClipId, attribute: ClipAttribute) {
    match attribute {
        ClipAttribute::Motion => project
            .set_param_constant(clip, ClipParam::Position, ParamValue::Vec2([0.25, -0.5]))
            .unwrap(),
        ClipAttribute::Opacity => project.set_blend_mode(clip, BlendMode::Multiply).unwrap(),
        ClipAttribute::Crop => project
            .set_clip_crop(clip, crate::CropRect::FULL, true, false, None)
            .unwrap(),
        ClipAttribute::TimeRemapping => project
            .set_clip_speed_curve(clip, Some(crate::clip::speed_preset("montage").unwrap()))
            .unwrap(),
        ClipAttribute::Volume => project
            .set_clip_audio(clip, Some(0.5), rt(0), rt(0))
            .unwrap(),
        ClipAttribute::Pan => project
            .set_param_constant(clip, ClipParam::Pan, ParamValue::Scalar(-0.75))
            .unwrap(),
        ClipAttribute::Effects => {
            project.add_effect(clip, "gaussian_blur").unwrap();
        }
        ClipAttribute::AudioEffects => project
            .timeline_mut()
            .clip_mut(clip)
            .unwrap()
            .audio_effects
            .push(crate::audio_effects::AudioEffectInstance::new(
                "example.gain",
            )),
        ClipAttribute::Grade => project
            .set_clip_filter(clip, Some(Filter::new("noir")))
            .unwrap(),
        ClipAttribute::Mask => {
            project
                .add_clip_mask(clip, Mask::new(MaskKind::Circle))
                .unwrap();
            project
                .add_clip_mask(clip, Mask::new(MaskKind::Rectangle))
                .unwrap();
        }
        ClipAttribute::ChromaKey => project
            .set_clip_chroma_key(
                clip,
                Some(ChromaKey {
                    rgb: [3, 240, 17],
                    strength: 0.72.into(),
                    shadow: 0.18.into(),
                }),
            )
            .unwrap(),
    }
}

fn held(project: &Project, clip: ClipId) -> Vec<ClipAttribute> {
    let clip = project.clip(clip).unwrap();
    ClipAttribute::ALL
        .into_iter()
        .filter(|attribute| attribute.is_set(clip))
        .collect()
}

#[test]
fn a_new_clip_holds_no_group() {
    let (project, clip) = media_clip();
    assert_eq!(held(&project, clip), Vec::<ClipAttribute>::new());
}

/// Each group, set alone, reads as set, and removing it leaves the clip equal
/// to a new one: the reset is whole, and no other group was touched.
#[test]
fn removing_a_group_returns_the_clip_to_a_new_clip() {
    for attribute in ClipAttribute::ALL {
        let (mut project, clip) = media_clip();
        let fresh = project.clip(clip).unwrap().clone();
        set(&mut project, clip, attribute);
        assert_eq!(
            held(&project, clip),
            vec![attribute],
            "{attribute:?} set alone"
        );
        project.remove_clip_attributes(clip, &[attribute]).unwrap();
        assert_eq!(
            project.clip(clip).unwrap(),
            &fresh,
            "removing {attribute:?} left part of it behind"
        );
    }
}

/// With every group set, removing one leaves every other group as it was.
#[test]
fn removing_one_group_keeps_the_others() {
    for attribute in ClipAttribute::ALL {
        let (mut project, clip) = media_clip();
        for each in ClipAttribute::ALL {
            set(&mut project, clip, each);
        }
        assert_eq!(held(&project, clip), ClipAttribute::ALL.to_vec());
        project.remove_clip_attributes(clip, &[attribute]).unwrap();
        let rest: Vec<ClipAttribute> = ClipAttribute::ALL
            .into_iter()
            .filter(|each| *each != attribute)
            .collect();
        assert_eq!(held(&project, clip), rest, "removing {attribute:?}");
    }
}

/// A driver is part of the group of the property it drives: it reads the group
/// as set, goes with the group, and stays when another group is removed.
#[test]
fn a_driver_goes_with_its_group_and_only_with_it() {
    let (mut project, clip) = media_clip();
    let code = DriverCode::new("aria.expressions", "value * 0.5");
    project
        .set_param_driver(clip, ClipParam::Opacity, Some(code))
        .unwrap();
    assert_eq!(held(&project, clip), vec![ClipAttribute::Opacity]);
    project
        .remove_clip_attributes(clip, &[ClipAttribute::Motion])
        .unwrap();
    assert!(
        project
            .clip(clip)
            .unwrap()
            .driver(ClipParam::Opacity)
            .is_some()
    );
    project
        .remove_clip_attributes(clip, &[ClipAttribute::Opacity])
        .unwrap();
    assert!(project.clip(clip).unwrap().drivers.is_empty());
}

/// Keyframes are part of the value: a keyed position is reset to the constant
/// a new clip holds, not to the value at one frame.
#[test]
fn a_keyed_property_returns_to_a_constant() {
    let (mut project, clip) = media_clip();
    for (tick, x) in [(0, 0.0), (48, 0.5)] {
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                rt(tick),
                ParamValue::Vec2([x, 0.0]),
                crate::param::Easing::Linear,
                None,
            )
            .unwrap();
    }
    assert!(project.clip(clip).unwrap().transform.position.is_animated());
    project
        .remove_clip_attributes(clip, &[ClipAttribute::Motion])
        .unwrap();
    assert_eq!(
        project.clip(clip).unwrap().transform.position,
        AnimatedTransform::identity().position
    );
}

/// Clearing the ramp re-derives the duration; when the longer clip would run
/// into its neighbour the removal fails and no group of the clip changes.
#[test]
fn a_ramp_that_cannot_be_cleared_changes_nothing() {
    let (mut project, media, track) = project_with_media(500);
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    // A ramp averaging above 1x shortens the clip; a neighbour placed at its
    // new end leaves no room for the original length.
    set(&mut project, clip, ClipAttribute::TimeRemapping);
    let end = project.clip(clip).unwrap().timeline.end().unwrap().value;
    assert!(end < 100, "the montage ramp shortens the clip");
    project
        .add_clip(track, media, tr(200, 50), rt(end))
        .unwrap();
    set(&mut project, clip, ClipAttribute::Effects);
    let before = project.clip(clip).unwrap().clone();
    let refused = project.remove_clip_attributes(
        clip,
        &[ClipAttribute::TimeRemapping, ClipAttribute::Effects],
    );
    assert!(
        refused.is_err(),
        "the cleared ramp would overlap the neighbour"
    );
    assert_eq!(project.clip(clip).unwrap(), &before);
}

#[test]
fn an_unknown_clip_is_refused() {
    let (mut project, _) = media_clip();
    let stray = ClipId::next();
    assert!(matches!(
        project.remove_clip_attributes(stray, &[ClipAttribute::Motion]),
        Err(ModelError::UnknownClip(id)) if id == stray
    ));
}

/// The fx badge on a timeline bar is purple for a change to a property every
/// clip has and yellow for something applied to the clip. The split is the
/// motion, opacity, crop, time remapping, volume and pan groups against the
/// effect chains, the grade, the masks and the key.
#[test]
fn the_intrinsic_groups_are_the_properties_every_clip_has() {
    let intrinsic: Vec<ClipAttribute> = ClipAttribute::ALL
        .into_iter()
        .filter(|attribute| attribute.is_intrinsic())
        .collect();
    assert_eq!(
        intrinsic,
        [
            ClipAttribute::Motion,
            ClipAttribute::Opacity,
            ClipAttribute::Crop,
            ClipAttribute::TimeRemapping,
            ClipAttribute::Volume,
            ClipAttribute::Pan,
        ]
    );
}
