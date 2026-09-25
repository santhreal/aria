//! `look_setters` cases.

use super::*;

#[test]
fn look_setters_reject_wrong_content() {
    let (mut project, media_id, _) = project_with_media(100);

    // Generated clips have no source pixels to mask/key/stabilize.
    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let bar = project
        .add_generated(fx, Generator::Adjustment, tr(0, 24))
        .unwrap();
    assert!(
        project
            .set_clip_mask(bar, Some(Mask::new(MaskKind::Linear)))
            .is_err()
    );
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
            Err(ModelError::IncompatibleTrackKind { .. })
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
            Err(ModelError::IncompatibleTrackKind { .. })
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
            Err(ModelError::IncompatibleTrackKind { .. })
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
            .set_clip_mask(still, Some(Mask::new(MaskKind::Heart)))
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
        .set_clip_mask(styled, Some(Mask::new(MaskKind::Star)))
        .unwrap();
    project
        .set_clip_filter(styled, Some(Filter::new("sunset")))
        .unwrap();

    let plain_json = serde_json::to_value(project.clip(plain).unwrap()).unwrap();
    for key in [
        "mask",
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
    assert_eq!(styled_json["mask"]["kind"], "star");
    assert_eq!(styled_json["filter"]["id"], "sunset");

    // The whole project (with looks) survives a save/load roundtrip.
    let json = serde_json::to_value(&project).unwrap();
    let back: Project = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.clip(styled).unwrap().mask,
        project.clip(styled).unwrap().mask
    );
    assert_eq!(
        back.clip(styled).unwrap().filter,
        project.clip(styled).unwrap().filter
    );
    assert!(back.clip(plain).unwrap().mask.is_none());
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
