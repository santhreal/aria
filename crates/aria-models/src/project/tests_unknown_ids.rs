//! An unknown catalog id is refused with every id the call takes.
//!
//! WHY: a refusal that names only the bad id leaves a script or the assistant
//! with no way to recover but a second lookup. Each case derives the expected
//! ids from the shipped catalog at run time, so a new transition, effect or
//! animation is covered without an edit here, and checks each shipped id is
//! both listed and accepted by the same call. Ids that plugins register at run
//! time are listed too; this suite registers none, so it does not cover them.

use super::*;
use crate::look::animation_catalog;

/// The ids after `prefix` in `refusal`, which must start with it.
fn listed(refusal: ModelError, prefix: &str) -> Vec<String> {
    let ModelError::InvalidParam(why) = refusal else {
        panic!("an unknown id must be an invalid parameter, got {refusal:?}");
    };
    let ids = why
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("{why:?} must start {prefix:?}"));
    ids.split(", ").map(str::to_owned).collect()
}

#[test]
fn an_unknown_transition_lists_every_transition_a_cut_takes() {
    let (mut project, left, _right, _track) = project_with_abutting_pair();
    let refused = project.add_transition(left, "warp_speed").unwrap_err();
    let ids = listed(
        refused,
        "unknown transition 'warp_speed'; available transitions: ",
    );
    let audio: Vec<&str> = crate::AudioTransition::ALL.map(|t| t.id()).to_vec();
    assert_eq!(ids[ids.len() - audio.len()..], audio);
    for spec in crate::shipped_transition_catalog() {
        assert!(
            ids.contains(&spec.id),
            "{} is not listed in {ids:?}",
            spec.id
        );
        project.add_transition(left, &spec.id).unwrap();
    }
    let (mut audio_lane, cut, _) = {
        let mut p = Project::new("test", R24);
        let media = p.add_media(sample_media(R24, 240));
        let track = p.add_track(TrackKind::Audio, "A1");
        let left = p.add_clip(track, media, tr(24, 48), rt(0)).unwrap();
        let right = p.add_clip(track, media, tr(96, 48), rt(48)).unwrap();
        (p, left, right)
    };
    for id in audio {
        audio_lane.add_transition(cut, id).unwrap();
    }
    let lone = crate::transition::Transition::new(left, left, "warp_speed", 12);
    let from_spec = listed(
        lone.spec().unwrap_err(),
        "unknown transition 'warp_speed'; available transitions: ",
    );
    for spec in crate::shipped_transition_catalog() {
        assert!(from_spec.contains(&spec.id), "{} is not listed", spec.id);
    }
    assert!(from_spec.ends_with(&ids[ids.len() - 3..]), "{from_spec:?}");
}

#[test]
fn an_unknown_effect_lists_every_effect_a_clip_takes() {
    let (mut project, left, _right, _track) = project_with_abutting_pair();
    let refused = project.add_effect(left, "kaleidoscope").unwrap_err();
    let ids = listed(
        refused,
        "unknown effect 'kaleidoscope'; available effects: ",
    );
    for spec in crate::shipped_effect_catalog() {
        assert!(
            ids.contains(&spec.id),
            "{} is not listed in {ids:?}",
            spec.id
        );
        project.add_effect(left, &spec.id).unwrap();
    }
}

#[test]
fn an_unknown_animation_lists_the_presets_of_its_slot() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Text, "Titles");
    let title = project
        .add_generated(track, Generator::text("Hi"), tr(0, 24))
        .unwrap();
    for slot in [AnimationSlot::In, AnimationSlot::Out, AnimationSlot::Combo] {
        let refused = project
            .set_clip_animation(title, slot, Some(AnimationRef::new("warp_drive")))
            .unwrap_err();
        let ids = listed(
            refused,
            &format!(
                "unknown animation 'warp_drive'; available {} animations: ",
                slot.id()
            ),
        );
        let fits: Vec<&str> = animation_catalog()
            .iter()
            .filter(|spec| spec.slot == slot)
            .map(|spec| spec.id.as_str())
            .collect();
        assert_eq!(ids, fits);
        for id in &ids {
            project
                .set_clip_animation(title, slot, Some(AnimationRef::new(id)))
                .unwrap();
        }
    }
}

#[test]
fn a_known_animation_in_the_wrong_place_says_where_it_plays() {
    let (mut project, solid, _right, _track) = project_with_abutting_pair();
    for spec in animation_catalog() {
        for slot in [AnimationSlot::In, AnimationSlot::Out, AnimationSlot::Combo] {
            let result = project.set_clip_animation(solid, slot, Some(AnimationRef::new(&spec.id)));
            let expected = if spec.slot != slot {
                Some(format!(
                    "animation '{}' plays in the {} slot, not the {} slot",
                    spec.id,
                    spec.slot.id(),
                    slot.id()
                ))
            } else if spec.text_only {
                Some(format!("animation '{}' plays on text clips only", spec.id))
            } else {
                None
            };
            assert_eq!(
                result.err(),
                expected.map(ModelError::InvalidParam),
                "{} in the {} slot",
                spec.id,
                slot.id()
            );
        }
    }
}
