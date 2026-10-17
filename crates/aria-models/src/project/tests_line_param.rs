//! The property a clip's bar draws as its line: choosing it, refusing one the
//! clip lacks, and keeping it on the property it was chosen for while the clip
//! around it changes.
//!
//! Closes the class where the line silently moves to another property: an
//! effect chain or mask stack edit that shifts indices, a chain replacement,
//! an attribute reset, or a bake that spends the property. Does not cover
//! drawing the line; that is the timeline's.

use super::*;
use crate::look::{Mask, MaskKind};

fn effect(effect: u32) -> ClipParam {
    ClipParam::Effect { effect, param: 0 }
}

fn mask_feather(mask: u32) -> ClipParam {
    ClipParam::Mask {
        mask,
        param: crate::MaskParam::Feather,
    }
}

fn media_clip() -> (Project, ClipId) {
    let (mut project, media, track) = project_with_media(500);
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    (project, clip)
}

fn line(project: &Project, clip: ClipId) -> Option<ClipParam> {
    project.clip(clip).unwrap().line_param
}

fn drawn(project: &Project, clip: ClipId, audio_lane: bool) -> Option<ClipParam> {
    project
        .clip(clip)
        .unwrap()
        .line_spec(audio_lane)
        .map(|row| row.param)
}

#[test]
fn a_bar_draws_its_lane_default_until_a_property_is_chosen() {
    let (mut project, clip) = media_clip();
    assert_eq!(drawn(&project, clip, false), Some(ClipParam::Opacity));

    let before = project
        .set_clip_line_param(clip, Some(ClipParam::Scale))
        .unwrap();
    assert_eq!(before, None);
    assert_eq!(drawn(&project, clip, false), Some(ClipParam::Scale));

    let before = project.set_clip_line_param(clip, None).unwrap();
    assert_eq!(before, Some(ClipParam::Scale));
    assert_eq!(drawn(&project, clip, false), Some(ClipParam::Opacity));

    let media = project.add_media(sample_media(R24, 500));
    let audio = project.add_track(TrackKind::Audio, "A1");
    let sound = project.add_clip(audio, media, tr(0, 48), rt(0)).unwrap();
    assert_eq!(drawn(&project, sound, true), Some(ClipParam::Volume));
    project
        .set_clip_line_param(sound, Some(ClipParam::Pan))
        .unwrap();
    assert_eq!(drawn(&project, sound, true), Some(ClipParam::Pan));
}

#[test]
fn a_property_the_clip_lacks_is_refused_and_the_line_stays() {
    let (mut project, clip) = media_clip();
    project
        .set_clip_line_param(clip, Some(ClipParam::Rotation))
        .unwrap();
    for param in [effect(0), mask_feather(0)] {
        let err = project
            .set_clip_line_param(clip, Some(param))
            .unwrap_err()
            .to_string();
        assert!(err.contains("fx badge menu"), "{param:?}: {err}");
        assert_eq!(line(&project, clip), Some(ClipParam::Rotation));
    }

    let media = project.add_media(sample_media(R24, 500));
    let audio = project.add_track(TrackKind::Audio, "A1");
    let sound = project.add_clip(audio, media, tr(0, 48), rt(0)).unwrap();
    for picture in [ClipParam::Opacity, ClipParam::Position, ClipParam::Scale] {
        assert!(
            project.set_clip_line_param(sound, Some(picture)).is_err(),
            "{picture:?} is a picture property and an audio lane has no picture"
        );
    }
    assert_eq!(line(&project, sound), None);

    let text_track = project.add_track(TrackKind::Text, "T");
    let text = project
        .add_generated(text_track, Generator::text("Hi"), tr(0, 24))
        .unwrap();
    assert!(
        project
            .set_clip_line_param(text, Some(ClipParam::Volume))
            .is_err(),
        "a generated clip has no sound"
    );
    assert_eq!(line(&project, text), None);

    let gone = ClipId::from_raw(9_999);
    assert!(matches!(
        project.set_clip_line_param(gone, None),
        Err(ModelError::UnknownClip(id)) if id == gone
    ));
}

#[test]
fn effect_chain_edits_keep_the_line_on_its_own_effect() {
    let (mut project, clip) = media_clip();
    for _ in 0..3 {
        project.add_effect(clip, "gaussian_blur").unwrap();
    }
    project.set_clip_line_param(clip, Some(effect(0))).unwrap();

    project.move_effect(clip, 0, 2).unwrap();
    assert_eq!(line(&project, clip), Some(effect(2)));
    project.move_effect(clip, 2, 0).unwrap();
    assert_eq!(line(&project, clip), Some(effect(0)));
    project.move_effect(clip, 1, 2).unwrap();
    assert_eq!(line(&project, clip), Some(effect(0)), "a move around it");

    project.set_clip_line_param(clip, Some(effect(2))).unwrap();
    project.remove_effect(clip, 0).unwrap();
    assert_eq!(line(&project, clip), Some(effect(1)), "a removal before it");
    project.remove_effect(clip, 1).unwrap();
    assert_eq!(line(&project, clip), None, "its own removal");
    assert_eq!(drawn(&project, clip, false), Some(ClipParam::Opacity));

    project.set_clip_line_param(clip, Some(effect(0))).unwrap();
    let chain = project.clip(clip).unwrap().effects.clone();
    project.set_clip_effects(clip, chain).unwrap();
    assert_eq!(line(&project, clip), None, "a replaced chain");

    project.set_clip_line_param(clip, Some(effect(0))).unwrap();
    project
        .remove_clip_attributes(clip, &[crate::ClipAttribute::Effects])
        .unwrap();
    assert_eq!(line(&project, clip), None, "a reset chain");
}

#[test]
fn mask_stack_edits_keep_the_line_on_its_own_mask() {
    let (mut project, clip) = media_clip();
    for _ in 0..3 {
        project
            .add_clip_mask(clip, Mask::new(MaskKind::Circle))
            .unwrap();
    }
    project
        .set_clip_line_param(clip, Some(mask_feather(2)))
        .unwrap();

    project.move_clip_mask(clip, 2, 0).unwrap();
    assert_eq!(line(&project, clip), Some(mask_feather(0)));
    project.remove_clip_mask(clip, 1).unwrap();
    assert_eq!(
        line(&project, clip),
        Some(mask_feather(0)),
        "a removal after it"
    );
    project
        .set_clip_mask(clip, 0, Mask::new(MaskKind::Rectangle))
        .unwrap();
    assert_eq!(
        line(&project, clip),
        Some(mask_feather(0)),
        "a replaced mask"
    );
    project.remove_clip_mask(clip, 0).unwrap();
    assert_eq!(line(&project, clip), None, "its own removal");

    project
        .set_clip_line_param(clip, Some(mask_feather(0)))
        .unwrap();
    project
        .remove_clip_attributes(clip, &[crate::ClipAttribute::Mask])
        .unwrap();
    assert_eq!(line(&project, clip), None, "a reset stack");
}

#[test]
fn a_reset_group_keeps_a_line_on_a_property_that_survives_at_its_default() {
    let (mut project, clip) = media_clip();
    project.add_effect(clip, "gaussian_blur").unwrap();
    project
        .set_clip_line_param(clip, Some(ClipParam::Scale))
        .unwrap();
    project
        .remove_clip_attributes(
            clip,
            &[crate::ClipAttribute::Motion, crate::ClipAttribute::Effects],
        )
        .unwrap();
    assert_eq!(line(&project, clip), Some(ClipParam::Scale));
}

#[test]
fn a_bake_returns_the_line_to_the_default_and_a_restore_brings_it_back() {
    let (mut project, clip) = media_clip();
    project.add_effect(clip, "gaussian_blur").unwrap();
    project.set_clip_line_param(clip, Some(effect(0))).unwrap();
    let rendered = project.add_media(MediaSource::new(
        "/tmp/sample-rendered.mov",
        1920,
        1080,
        R24,
        48,
        true,
    ));

    project.bake_clip(clip, rendered).unwrap();
    assert_eq!(line(&project, clip), None);
    assert_eq!(drawn(&project, clip, false), Some(ClipParam::Opacity));

    project.restore_baked_clip(clip).unwrap();
    assert_eq!(line(&project, clip), Some(effect(0)));
}

#[test]
fn the_line_round_trips_through_a_save_and_is_absent_while_unset() {
    let (mut project, clip) = media_clip();
    let bare = project.save_to_string().unwrap();
    assert!(!bare.contains("\"line_param\""), "{bare}");

    project
        .set_clip_line_param(clip, Some(ClipParam::Rotation))
        .unwrap();
    let saved = project.save_to_string().unwrap();
    assert!(saved.contains("\"line_param\": \"rotation\""), "{saved}");
    let loaded = Project::load_from_string(&saved).unwrap();
    assert_eq!(line(&loaded, clip), Some(ClipParam::Rotation));
}
