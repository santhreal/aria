//! `look_commands` cases.

use super::*;
use crate::wire;

#[test]
fn look_commands_lower_and_guard() {
    let (project, _, _, _, clip, title) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipFilter(wire::SetClipFilter {
            clip,
            filter: Some(wire::WireFilter {
                id: "vivid".to_string(),
                intensity: Some(0.9),
            }),
        }),
    );
    match edit {
        EditCommand::SetClipFilter { filter, .. } => {
            let filter = filter.expect("filter set");
            assert_eq!(filter.id, "vivid");
            assert!((filter.intensity.sample_at(0.0) - 0.9).abs() < 1e-6);
        }
        other => panic!("expected SetClipFilter, got {other:?}"),
    }

    let edit = lower(
        &project,
        WireCommand::SetClipAnimation(wire::SetClipAnimation {
            clip,
            slot: wire::WireAnimationSlot::In,
            animation: Some("fade_in".to_string()),
            speed: None,
            intensity: None,
            stagger: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipAnimation {
            clip: ClipId::from_raw(clip),
            slot: AnimationSlot::In,
            animation: Some(AnimationRef::new("fade_in")),
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetClipAnimation(wire::SetClipAnimation {
            clip,
            slot: wire::WireAnimationSlot::Out,
            animation: Some("fade_in".to_string()),
            speed: None,
            intensity: None,
            stagger: None,
        }),
    );
    assert!(msg.contains("does not fit"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::SetClipMask(wire::SetClipMask {
            clip: title,
            mask: Some(wire::WireMask {
                kind: wire::WireMaskKind::Circle,
                feather: None,
                invert: None,
                center: None,
                size: None,
                rotation: None,
                roundness: None,
            }),
        }),
    );
    assert!(msg.contains("generated clip"), "{msg}");
}

#[test]
fn mask_with_geometry_lowers_to_constants() {
    let (project, _, _, _, clip, _) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipMask(wire::SetClipMask {
            clip,
            mask: Some(wire::WireMask {
                kind: wire::WireMaskKind::Rectangle,
                feather: Some(0.25),
                invert: Some(true),
                center: Some([0.1, -0.2]),
                size: Some([0.8, 0.6]),
                rotation: Some(15.0),
                roundness: Some(0.3),
            }),
        }),
    );
    match edit {
        EditCommand::SetClipMask {
            mask: Some(mask), ..
        } => {
            assert_eq!(mask.kind, MaskKind::Rectangle);
            assert_eq!(mask.feather, Param::Constant(0.25));
            assert!(mask.invert);
            assert_eq!(mask.center, Param::Constant([0.1, -0.2]));
            assert_eq!(mask.size, Param::Constant([0.8, 0.6]));
            assert_eq!(mask.rotation, Param::Constant(15.0));
            assert_eq!(mask.roundness, Param::Constant(0.3));
        }
        other => panic!("expected SetClipMask with geometry, got {other:?}"),
    }

    let msg = reject(
        &project,
        WireCommand::SetClipMask(wire::SetClipMask {
            clip,
            mask: Some(wire::WireMask {
                kind: wire::WireMaskKind::Circle,
                feather: None,
                invert: None,
                center: None,
                size: Some([0.0, 1.0]),
                rotation: None,
                roundness: None,
            }),
        }),
    );
    assert!(msg.contains("mask size"), "{msg}");
}

#[test]
fn set_clip_adjustments_lowers_new_sliders_and_rejects_ranges() {
    let (project, _, _, _, clip, _) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipAdjustments(wire::SetClipAdjustments {
            clip,
            tint: Some(-0.5),
            hue: Some(1.0),
            highlights: Some(0.25),
            shadows: Some(-0.25),
            sharpness: Some(0.75),
            vignette: Some(0.4),
            ..Default::default()
        }),
    );
    match edit {
        EditCommand::SetClipAdjustments { adjust, .. } => {
            assert_eq!(adjust.tint, (-0.5f32).into());
            assert_eq!(adjust.hue, 1.0.into());
            assert_eq!(adjust.highlights, 0.25.into());
            assert_eq!(adjust.shadows, (-0.25f32).into());
            assert_eq!(adjust.sharpness, 0.75.into());
            assert_eq!(adjust.vignette, 0.4.into());
        }
        other => panic!("expected SetClipAdjustments, got {other:?}"),
    }

    let msg = reject(
        &project,
        WireCommand::SetClipAdjustments(wire::SetClipAdjustments {
            clip,
            sharpness: Some(-0.5),
            ..Default::default()
        }),
    );
    assert!(msg.contains("sharpness"), "{msg}");

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Look {
                param: wire::WireLookParam::AdjustHue,
            },
            at: 0.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Look {
                param: aria_models::LookParam::AdjustHue,
            },
            at: RationalTime::new(0, R24),
            value: ParamValue::Scalar(0.5),
            easing: Easing::Linear,
            tangents: None,
        }
    );
}

#[test]
fn mask_center_keyframe_uses_position_on_masked_clip() {
    let (mut project, _, _, _, clip, _) = fixture();
    project
        .set_clip_mask(ClipId::from_raw(clip), Some(Mask::new(MaskKind::Circle)))
        .unwrap();

    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Look {
                param: wire::WireLookParam::MaskCenter,
            },
            at: 1.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(msg.contains("position"), "{msg}");

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Look {
                param: wire::WireLookParam::MaskCenter,
            },
            at: 1.0,
            value: None,
            position: Some([0.25, -0.1]),
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Look {
                param: aria_models::LookParam::MaskCenter,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([0.25, -0.1]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    // Tagged nesting on the wire: {"look":{"param":"mask_center"}}
    let from_json = WireCommand::from_tool_call(
        "set_param_keyframe",
        serde_json::json!({
            "clip": clip,
            "param": { "look": { "param": "mask_center" } },
            "at": 1.0,
            "position": [0.25, -0.1],
        }),
    )
    .unwrap();
    let WireCommand::SetParamKeyframe(args) = from_json else {
        panic!("expected SetParamKeyframe");
    };
    assert_eq!(
        args.param,
        wire::WireClipParam::Look {
            param: wire::WireLookParam::MaskCenter
        }
    );
    assert_eq!(args.position, Some([0.25, -0.1]));
}

#[test]
fn extract_audio_lowers_to_explicit_target_and_preflights_failures() {
    let (mut project, media, video_track, _text, video_clip, title) = fixture();
    let audio = project.add_track(TrackKind::Audio, "A1");
    let edit = lower(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::ExtractAudio {
            clip: ClipId::from_raw(video_clip),
            to_track: Some(audio),
        }
    );

    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: title,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("generated"), "{msg}");

    project
        .timeline_mut()
        .track_mut(TrackId::from_raw(video_track))
        .unwrap()
        .locked = true;
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("video track"), "{msg}");
    assert!(msg.contains("locked"), "{msg}");
    project
        .timeline_mut()
        .track_mut(TrackId::from_raw(video_track))
        .unwrap()
        .locked = false;

    project.timeline_mut().track_mut(audio).unwrap().locked = true;
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("locked"), "{msg}");
    project.timeline_mut().track_mut(audio).unwrap().locked = false;

    project
        .add_clip(
            audio,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("exact timeline range"), "{msg}");
    assert!(msg.contains("choose or add a free audio track"), "{msg}");
}

#[test]
fn duplicate_clip_lowers_at_timeline_rate_without_mutating() {
    let (mut project, _, _, _, clip, _) = fixture();
    let destination = project.add_track(TrackKind::Video, "V2");
    let before = serde_json::to_value(&project).unwrap();

    let edit = lower(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: destination.raw(),
            start: 12.25,
        }),
    );

    assert_eq!(
        edit,
        EditCommand::DuplicateClip {
            clip: ClipId::from_raw(clip),
            to_track: destination,
            start: RationalTime::new(294, R24),
        }
    );
    assert_eq!(
        serde_json::to_value(&project).unwrap(),
        before,
        "validation must not mutate the project"
    );
}
