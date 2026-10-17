//! Presets carry a mask's keyframed properties by the mask's index in the stack.
//!
//! Closes the class where a preset resolves property ids against the rows every
//! clip declares only, so a keyframed mask row that the Save-as-preset draft
//! offers fails to capture, and a preset naming one fails to validate or apply.
//! Does not cover a preset that carries masks themselves: a preset writes onto
//! masks the target clip already has.

use super::*;
use crate::look::{Mask, MaskKind};
use crate::media::MediaSource;
use crate::param::Easing;
use crate::project::Project;
use crate::time::{Rational, RationalTime, TimeRange};
use crate::track::TrackKind;
use crate::{ClipId, MaskParam};

fn fps24() -> Rational {
    Rational::new(24, 1)
}

fn t(ticks: i64) -> RationalTime {
    RationalTime::new(ticks, fps24())
}

fn range(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, fps24())
}

const FEATHER_1: ClipParam = ClipParam::Mask {
    mask: 1,
    param: MaskParam::Feather,
};

/// Two video clips, at 0 and at 100, with `masks` circle masks each, and a
/// feather fade on the first clip's second mask at ticks 10 and 40.
fn faded(masks: [usize; 2]) -> (Project, ClipId, ClipId) {
    let mut project = Project::new("Preset masks", fps24());
    let media = project.add_media(MediaSource::new(
        "/media/sample.mp4",
        1920,
        1080,
        fps24(),
        240,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let mut ids = Vec::with_capacity(2);
    for (start, count) in [0, 100].into_iter().zip(masks) {
        let clip = Clip::from_media(media, range(0, 100), range(start, 100));
        let id = clip.id;
        project.timeline_mut().add_clip(track, clip).unwrap();
        for _ in 0..count {
            project
                .add_clip_mask(id, Mask::new(MaskKind::Circle))
                .unwrap();
        }
        ids.push(id);
    }
    for (tick, value) in [(10, 0.2), (40, 0.6)] {
        project
            .set_param_keyframe(
                ids[0],
                FEATHER_1,
                t(tick),
                ParamValue::Scalar(value),
                Easing::Linear,
                None,
            )
            .unwrap();
    }
    (project, ids[0], ids[1])
}

#[test]
fn a_keyframed_mask_property_captures_and_lands_on_the_same_mask_of_the_target() {
    let (mut project, a, b) = faded([2, 2]);
    let source = project.clip(a).unwrap();
    let parts = PresetParts::all_of(source);
    assert_eq!(parts.properties, vec!["mask-1-feather".to_string()]);
    let preset = Preset::capture("Mask fade", source, 10, &parts).unwrap();
    let ticks: Vec<i64> = preset.properties[0]
        .curve
        .keyframes()
        .iter()
        .map(|key| key.tick)
        .collect();
    assert_eq!(
        ticks,
        vec![0, 30],
        "the capture did not shift to the anchor"
    );

    project.apply_preset(b, &preset, t(105)).unwrap();
    let target = project.clip(b).unwrap();
    let landed: Vec<(i64, f32)> = target.masks[1]
        .feather
        .keyframes()
        .iter()
        .map(|key| (key.tick, key.value))
        .collect();
    assert_eq!(landed, vec![(5, 0.2), (35, 0.6)]);
    assert!(
        target.masks[0].feather.keyframes().is_empty(),
        "the preset wrote onto the wrong mask"
    );
}

#[test]
fn a_mask_the_target_lacks_is_refused_and_the_target_keeps_its_masks() {
    for masks in [0, 1] {
        let (mut project, a, b) = faded([2, masks]);
        let source = project.clip(a).unwrap();
        let preset = Preset::capture("Mask fade", source, 0, &PresetParts::all_of(source)).unwrap();
        let before = project.clip(b).unwrap().masks.clone();
        let err = project
            .apply_preset(b, &preset, t(100))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mask-1-feather") && err.contains("not available"),
            "{masks} mask(s): {err}"
        );
        assert_eq!(project.clip(b).unwrap().masks, before);
    }
}

#[test]
fn a_preset_names_a_mask_row_by_its_canonical_id_and_holds_it_to_its_range() {
    let preset = |id: &str, value: f32| {
        Preset::new(
            "Opacity",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: id.into(),
                curve: Param::Constant(ParamValue::Scalar(value)),
            }],
        )
    };
    preset("mask-3-opacity", 0.5).validate().unwrap();
    for id in [
        "mask-03-opacity",
        "mask-3-colour",
        "mask-opacity",
        "effect-0-radius",
    ] {
        let err = preset(id, 0.5).validate().unwrap_err().to_string();
        assert!(err.contains("unknown property id"), "{id}: {err}");
    }
    let err = preset("mask-3-opacity", 2.0)
        .validate()
        .unwrap_err()
        .to_string();
    assert!(err.contains("mask-3-opacity"), "{err}");
}
