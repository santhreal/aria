//! `look_setters` cases.

use super::*;

#[test]
fn look_setters_reject_wrong_content() {
    let (mut project, media_id, _) = project_with_media(100);

    // Generated clips have no source pixels to key or stabilize, and take a
    // mask as footage does: it cuts what the generator draws.
    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let bar = project
        .add_generated(fx, Generator::Adjustment, tr(0, 24))
        .unwrap();
    let mask = Mask::new(MaskKind::Linear);
    assert_eq!(project.add_clip_mask(bar, mask.clone()).unwrap(), 0);
    assert_eq!(project.timeline().clip(bar).unwrap().masks, vec![mask]);
    assert!(
        project
            .set_clip_stabilize(bar, Some(StabilizeLevel::Recommended))
            .is_err()
    );
    // …but filters and adjustments are exactly what lane bars persist.
    assert!(
        project
            .set_clip_filter(bar, Some(Filter::new("noir")))
            .is_ok()
    );
    assert!(
        project
            .set_clip_adjustments(
                bar,
                ColorAdjustments {
                    contrast: (-0.5).into(),
                    ..Default::default()
                }
            )
            .is_ok()
    );

    // Audio-lane clips show no pixels.
    let audio = project.add_track(TrackKind::Audio, "A1");
    let aclip = project.add_clip(audio, media_id, tr(0, 50), rt(0)).unwrap();
    assert!(
        project
            .set_clip_filter(aclip, Some(Filter::new("warm")))
            .is_err()
    );
    assert!(
        matches!(
            project.set_blend_mode(aclip, BlendMode::Multiply),
            Err(ModelError::NotOnPicture { .. })
        ),
        "blend modes need pixels"
    );
    assert!(
        matches!(
            project.set_motion_blur(
                aclip,
                MotionBlur {
                    enabled: true,
                    ..MotionBlur::default()
                }
            ),
            Err(ModelError::NotOnPicture { .. })
        ),
        "motion blur needs pixels"
    );
    assert!(
        matches!(
            project.set_layer_styles(
                aclip,
                LayerStyles {
                    shadow: Some(LayerShadow::default()),
                    ..Default::default()
                }
            ),
            Err(ModelError::NotOnPicture { .. })
        ),
        "layer styles need pixels"
    );

    // Stills have no motion to stabilize.
    let image = project.add_media(MediaSource::image("/tmp/photo.png", 800, 600));
    let video = project.add_track(TrackKind::Video, "V2");
    let still = project
        .add_clip(
            video,
            image,
            tr_at(0, 100, crate::media::STILL_TICK_RATE),
            rt(0),
        )
        .unwrap();
    assert!(
        project
            .set_clip_stabilize(still, Some(StabilizeLevel::Smooth))
            .is_err()
    );
    assert!(
        project
            .add_clip_mask(still, Mask::new(MaskKind::Heart))
            .is_ok(),
        "masks apply to stills"
    );

    // Unknown ids and out-of-range values are rejected.
    let (mut p2, m2, t2) = project_with_media(100);
    let clip = p2.add_clip(t2, m2, tr(0, 50), rt(0)).unwrap();
    assert!(p2.set_clip_filter(clip, Some(Filter::new("nope"))).is_err());
    let mut hot = Filter::new("vivid");
    hot.intensity = 1.5.into();
    assert!(p2.set_clip_filter(clip, Some(hot)).is_err());
}

#[test]
fn animation_slots_enforce_capcut_exclusivity() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();

    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("fade_in")))
        .unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::Out, Some(AnimationRef::new("drop")))
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.animation_in.as_ref().unwrap().id, "fade_in");
    assert_eq!(c.animation_out.as_ref().unwrap().id, "drop");

    // A combo replaces both sides…
    project
        .set_clip_animation(clip, AnimationSlot::Combo, Some(AnimationRef::new("pulse")))
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert!(c.animation_in.is_none() && c.animation_out.is_none());
    assert_eq!(c.animation_combo.as_ref().unwrap().id, "pulse");

    // …and an entrance evicts the combo again.
    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("zoom_in")))
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert!(c.animation_combo.is_none());

    // Wrong slot, unknown id, and text-only presets on media are rejected.
    assert!(
        project
            .set_clip_animation(clip, AnimationSlot::Out, Some(AnimationRef::new("fade_in")))
            .is_err()
    );
    assert!(
        project
            .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("nope")))
            .is_err()
    );
    assert!(
        project
            .set_clip_animation(
                clip,
                AnimationSlot::Combo,
                Some(AnimationRef::new("typewriter"))
            )
            .is_err()
    );

    // Text clips accept the text-only presets.
    let text = project.add_track(TrackKind::Text, "T1");
    let title = project
        .add_generated(text, Generator::text("Hi"), tr(0, 24))
        .unwrap();
    assert!(
        project
            .set_clip_animation(
                title,
                AnimationSlot::Combo,
                Some(AnimationRef::new("typewriter"))
            )
            .is_ok()
    );
}

#[test]
fn audio_role_tags_audio_lane_clips_only() {
    let (mut project, media_id, video) = project_with_media(100);
    let vclip = project.add_clip(video, media_id, tr(0, 50), rt(0)).unwrap();
    assert!(
        project
            .set_clip_audio_role(vclip, Some(AudioRole::Music))
            .is_err()
    );

    let audio = project.add_track(TrackKind::Audio, "A1");
    let aclip = project.add_clip(audio, media_id, tr(0, 50), rt(0)).unwrap();
    project
        .set_clip_audio_role(aclip, Some(AudioRole::Voiceover))
        .unwrap();
    assert_eq!(
        project.clip(aclip).unwrap().audio_role,
        Some(AudioRole::Voiceover)
    );
    project.set_clip_audio_role(aclip, None).unwrap();
    assert!(project.clip(aclip).unwrap().audio_role.is_none());
}

#[test]
fn text_effect_presets_bake_onto_the_style() {
    let mut project = Project::new("t", R24);
    let track = project.add_track(TrackKind::Text, "T1");

    let style = crate::TextStyle {
        effect_preset: Some("neon".into()),
        ..Default::default()
    };
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "Glow".into(),
                style,
            },
            tr(0, 24),
        )
        .unwrap();

    let ClipSource::Generated(Generator::Text { style, .. }) = &project.clip(clip).unwrap().content
    else {
        panic!("expected text");
    };
    assert_eq!(style.effect_preset.as_deref(), Some("neon"));
    let spec = crate::look::text_effect_spec("neon").unwrap();
    assert_eq!(style.stroke, spec.stroke);
    assert_eq!(style.shadow, spec.shadow);
    assert_eq!(style.background, spec.background);

    // Sent again with the preset it holds, a title keeps the treatments it was
    // sent with: a stroke edited after the preset survives a content edit.
    let edited = crate::TextStroke {
        rgba: [9, 8, 7, 255].into(),
        width: 3.0.into(),
    };
    let restyled = |preset: &str| crate::TextStyle {
        effect_preset: Some(preset.into()),
        stroke: Some(edited.clone()),
        ..Default::default()
    };
    project
        .set_generator(
            clip,
            Generator::Text {
                content: "Glow again".into(),
                style: restyled("neon"),
            },
        )
        .unwrap();
    let ClipSource::Generated(Generator::Text { style, .. }) = &project.clip(clip).unwrap().content
    else {
        panic!("expected text");
    };
    assert_eq!(style.stroke.as_ref(), Some(&edited));

    // A different preset is a new choice, and it is baked.
    let other = crate::text_effect_catalog()
        .iter()
        .find(|spec| spec.id != "neon")
        .expect("the catalog ships a second preset");
    project
        .set_generator(
            clip,
            Generator::Text {
                content: "Glow again".into(),
                style: restyled(&other.id),
            },
        )
        .unwrap();
    let ClipSource::Generated(Generator::Text { style, .. }) = &project.clip(clip).unwrap().content
    else {
        panic!("expected text");
    };
    assert_eq!(style.stroke, other.stroke);
    assert_eq!(style.shadow, other.shadow);
    assert_eq!(style.background, other.background);

    // Unknown presets are rejected at the door.
    let bad = crate::TextStyle {
        effect_preset: Some("nope".into()),
        ..Default::default()
    };
    assert!(
        project
            .set_generator(
                clip,
                Generator::Text {
                    content: "Glow".into(),
                    style: bad,
                }
            )
            .is_err()
    );

    // Clearing the preset keeps whatever treatments the style carries.
    let manual = crate::TextStyle {
        effect_preset: None,
        stroke: Some(crate::TextStroke {
            rgba: [1, 2, 3, 255].into(),
            width: 2.0.into(),
        }),
        ..Default::default()
    };
    project
        .set_generator(
            clip,
            Generator::Text {
                content: "Glow".into(),
                style: manual.clone(),
            },
        )
        .unwrap();
    let ClipSource::Generated(Generator::Text { style, .. }) = &project.clip(clip).unwrap().content
    else {
        panic!("expected text");
    };
    assert_eq!(style.stroke, manual.stroke);
}

#[test]
fn look_fields_roundtrip_and_stay_off_the_wire_when_unset() {
    let (mut project, media_id, track) = project_with_media(100);
    let plain = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let styled = project
        .add_clip(track, media_id, tr(50, 50), rt(50))
        .unwrap();
    project
        .add_clip_mask(styled, Mask::new(MaskKind::Star))
        .unwrap();
    project
        .set_clip_filter(styled, Some(Filter::new("sunset")))
        .unwrap();

    let plain_json = serde_json::to_value(project.clip(plain).unwrap()).unwrap();
    for key in [
        "masks",
        "chroma_key",
        "stabilize",
        "filter",
        "adjust",
        "audio_role",
    ] {
        assert!(
            plain_json.get(key).is_none(),
            "untouched clip should not serialize `{key}`"
        );
    }
    let styled_json = serde_json::to_value(project.clip(styled).unwrap()).unwrap();
    assert_eq!(styled_json["masks"][0]["kind"], "star");
    assert_eq!(styled_json["filter"]["id"], "sunset");

    // The whole project (with looks) survives a save/load roundtrip.
    let json = serde_json::to_value(&project).unwrap();
    let back: Project = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.clip(styled).unwrap().masks,
        project.clip(styled).unwrap().masks
    );
    assert_eq!(
        back.clip(styled).unwrap().filter,
        project.clip(styled).unwrap().filter
    );
    assert!(back.clip(plain).unwrap().masks.is_empty());
}

#[test]
fn speed_preset_catalog_ids_resolve_and_match_back() {
    for spec in crate::speed_preset_catalog() {
        let curve = crate::speed_preset(&spec.id)
            .unwrap_or_else(|| panic!("catalog id '{}' has no curve", spec.id));
        crate::validate_speed_curve(&curve)
            .unwrap_or_else(|e| panic!("preset '{}' curve invalid: {e}", spec.id));
        assert_eq!(crate::speed_preset_id(&curve), Some(spec.id.as_str()));
    }
    assert_eq!(crate::speed_preset_id(&Param::Constant(1.0)), None);
}

/// Every edit that needs a picture rejects a clip on a sound lane with
/// [`ModelError::NotOnPicture`] naming that clip and lane, an audio role
/// rejects a clip on a picture lane with [`ModelError::NotOnSound`], and the
/// clip is left as it was.
///
/// Closes the class where such an edit reported `IncompatibleTrackKind`
/// ("track 2 (Audio) cannot hold this clip"), whose copy reads as a drop onto
/// the wrong track though the clip was not moving. The visual parameters are
/// read from [`crate::clip::clip_param_catalog`], so a new one is covered on
/// arrival. Not covered: whether a surface offers these edits for a clip on a
/// sound lane.
#[test]
fn an_edit_on_the_wrong_lane_names_the_clip_and_its_lane() {
    use crate::clip::{ParamScope, clip_param_catalog};

    let (mut project, media_id, video) = project_with_media(100);
    let audio = project.add_track(TrackKind::Audio, "A1");
    let sound = project.add_clip(audio, media_id, tr(0, 50), rt(0)).unwrap();
    let picture = project.add_clip(video, media_id, tr(0, 50), rt(0)).unwrap();
    let before = project.clip(sound).unwrap().clone();
    let mask = Mask::new(MaskKind::Circle);
    let edits: Vec<(String, Result<(), ModelError>)> = vec![
        (
            "add_clip_mask".into(),
            project.add_clip_mask(sound, mask.clone()).map(drop),
        ),
        (
            "set_clip_mask".into(),
            project.set_clip_mask(sound, 0, mask),
        ),
        (
            "set_blend_mode".into(),
            project.set_blend_mode(sound, BlendMode::Screen),
        ),
        (
            "set_motion_blur".into(),
            project.set_motion_blur(sound, MotionBlur::default()),
        ),
        (
            "set_layer_styles".into(),
            project.set_layer_styles(sound, LayerStyles::default()),
        ),
        (
            "set_clip_chroma_key".into(),
            project.set_clip_chroma_key(sound, None),
        ),
        ("set_clip_matte".into(), project.set_clip_matte(sound, None)),
        (
            "set_clip_stabilize".into(),
            project.set_clip_stabilize(sound, None),
        ),
        (
            "set_clip_filter".into(),
            project.set_clip_filter(sound, None),
        ),
        ("set_clip_lut".into(), project.set_clip_lut(sound, None)),
        (
            "set_clip_adjustments".into(),
            project.set_clip_adjustments(sound, ColorAdjustments::default()),
        ),
        (
            "set_clip_animation".into(),
            project.set_clip_animation(sound, AnimationSlot::In, None),
        ),
        (
            "set_clip_crop".into(),
            project.set_clip_crop(sound, CropRect::FULL, true, false, None),
        ),
        (
            "set_transform".into(),
            project.set_transform(sound, ClipTransform::IDENTITY, None),
        ),
        (
            "set_animated_transform".into(),
            project.set_animated_transform(sound, ClipTransform::IDENTITY.into()),
        ),
        (
            "set_param_keyframe_tangents".into(),
            project.set_param_keyframe_tangents(sound, ClipParam::Position, rt(0), None),
        ),
        (
            "add_effect".into(),
            project.add_effect(sound, "gaussian_blur").map(drop),
        ),
        (
            "set_clip_effects".into(),
            project.set_clip_effects(sound, Vec::new()),
        ),
    ];
    let params = clip_param_catalog()
        .iter()
        .filter(|spec| spec.scope == ParamScope::Visual)
        .map(|spec| {
            (
                format!("remove_param_keyframe {}", spec.id),
                project.remove_param_keyframe(sound, spec.param, rt(0)),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !params.is_empty(),
        "the registry declares no visual parameter"
    );
    for (edit, result) in edits.into_iter().chain(params) {
        assert_eq!(
            result,
            Err(ModelError::NotOnPicture {
                clip: sound,
                track: audio,
            }),
            "{edit}"
        );
    }
    assert_eq!(project.clip(sound), Some(&before));

    assert_eq!(
        project.set_clip_audio_role(picture, Some(AudioRole::Music)),
        Err(ModelError::NotOnSound {
            clip: picture,
            track: video,
        })
    );
    assert_eq!(project.clip(picture).unwrap().audio_role, None);
    project
        .set_clip_audio_role(sound, Some(AudioRole::Music))
        .unwrap();

    match project.set_generator(picture, Generator::text("title")) {
        Err(ModelError::InvalidParam(why)) => assert_eq!(
            why,
            format!("clip {picture} is media-backed and has no generator to replace")
        ),
        other => panic!("a generator write on a media clip returned {other:?}"),
    }
}
