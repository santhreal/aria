//! Golden JSON for representative commands.
//!
//! A roundtrip proves a payload survives the trip; a golden pins the bytes a
//! shell and the agent actually read, so a field rename or an elided default
//! shows up as a diff instead of as a silently different protocol.
use super::*;

// --- goldens ----------------------------------------------------------------

#[test]
fn golden_import() {
    let cmd = Command::Project(ProjectCommand::Import {
        path: PathBuf::from("/media/clip.mp4"),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "Import", "path": "/media/clip.mp4"})
    );
}

/// An export carries the choices that were made and nothing else.
///
/// The class this closes: `format` arrived after shells were already sending
/// `Export`. If an unmade choice serialized as a concrete value, or an older
/// payload failed to parse, the field would have changed what an existing
/// caller's export writes — the one thing a new field must never do.
#[test]
fn export_format_carries_only_the_choices_that_were_made() {
    let project_default = Command::Project(ProjectCommand::Export {
        path: PathBuf::from("/out/final.mp4"),
        format: ExportFormat::PROJECT,
    });
    assert_eq!(
        serde_json::to_value(&project_default).unwrap(),
        json!({"type": "Export", "path": "/out/final.mp4", "format": {}}),
        "an unmade choice is an absent field, not a default invented on the way out"
    );

    let older: Command =
        serde_json::from_value(json!({"type": "Export", "path": "/out/final.mp4"})).unwrap();
    assert_eq!(
        older, project_default,
        "a payload written before the field existed still means the project's own output"
    );

    let chosen = Command::Project(ProjectCommand::Export {
        path: PathBuf::from("/out/final.mp4"),
        format: ExportFormat {
            size: Some((1280, 720)),
            video_codec: Some(VideoCodec::H265),
            bitrate_kbps: Some(8_000),
            range: Some((t(30), t(90))),
            ..ExportFormat::PROJECT
        },
    });
    assert_eq!(
        serde_json::to_value(&chosen).unwrap(),
        json!({
            "type": "Export",
            "path": "/out/final.mp4",
            "format": {
                "size": [1280, 720],
                "video_codec": "h265",
                "bitrate_kbps": 8000,
                "range": [
                    {"value": 30, "rate": {"num": 30, "den": 1}},
                    {"value": 90, "rate": {"num": 30, "den": 1}},
                ],
            },
        })
    );
    assert_eq!(
        serde_json::from_value::<Command>(serde_json::to_value(&chosen).unwrap()).unwrap(),
        chosen,
        "the request survives the trip it was serialized for"
    );
}

#[test]
fn golden_add_clip() {
    let cmd = Command::Edit(EditCommand::AddClip {
        track: track(1),
        media: media(2),
        source: tr(0, 90),
        start: t(30),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "AddClip",
            "track": 1,
            "media": 2,
            "source": {
                "start": {"value": 0, "rate": {"num": 30, "den": 1}},
                "duration": {"value": 90, "rate": {"num": 30, "den": 1}},
            },
            "start": {"value": 30, "rate": {"num": 30, "den": 1}},
        })
    );
}

#[test]
fn golden_extract_audio() {
    let explicit = Command::Edit(EditCommand::ExtractAudio {
        clip: clip(4),
        to_track: Some(track(2)),
    });
    assert_eq!(
        serde_json::to_value(&explicit).unwrap(),
        json!({"type": "ExtractAudio", "clip": 4, "to_track": 2})
    );

    let automatic = Command::Edit(EditCommand::ExtractAudio {
        clip: clip(4),
        to_track: None,
    });
    assert_eq!(
        serde_json::to_value(&automatic).unwrap(),
        json!({"type": "ExtractAudio", "clip": 4, "to_track": null})
    );
}

#[test]
fn golden_duplicate_clip() {
    let cmd = Command::Edit(EditCommand::DuplicateClip {
        clip: clip(4),
        to_track: track(2),
        start: t(120),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "DuplicateClip",
            "clip": 4,
            "to_track": 2,
            "start": {"value": 120, "rate": {"num": 30, "den": 1}},
        })
    );
}

#[test]
fn golden_freeze_frame() {
    let cmd = Command::Edit(EditCommand::FreezeFrame {
        clip: clip(4),
        at: t(45),
        duration: t(30),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "FreezeFrame",
            "clip": 4,
            "at": {"value": 45, "rate": {"num": 30, "den": 1}},
            "duration": {"value": 30, "rate": {"num": 30, "den": 1}},
        })
    );
}

#[test]
fn golden_split_clip() {
    let cmd = Command::Edit(EditCommand::SplitClip {
        clip: clip(4),
        at: t(45),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "SplitClip",
            "clip": 4,
            "at": {"value": 45, "rate": {"num": 30, "den": 1}},
        })
    );
}

#[test]
fn golden_unlink_clips() {
    let cmd = Command::Edit(EditCommand::UnlinkClips {
        clips: vec![clip(4), clip(5)],
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "UnlinkClips", "clips": [4, 5]})
    );
}

#[test]
fn golden_move_effect() {
    let cmd = Command::Edit(EditCommand::MoveEffect {
        clip: clip(4),
        from_index: 0,
        to_index: 2,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "MoveEffect", "clip": 4, "from_index": 0, "to_index": 2})
    );
}

/// A bypassed effect is a flag on the instance, and a chain saved before the
/// flag existed loads with every effect on.
#[test]
fn golden_effect_bypass() {
    let cmd = Command::Edit(EditCommand::SetEffectBypass {
        clip: clip(4),
        index: 1,
        bypass: true,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetEffectBypass", "clip": 4, "index": 1, "bypass": true})
    );

    let mut effect = aria_models::EffectInstance::new("gaussian_blur");
    assert_eq!(
        serde_json::to_value(&effect).unwrap(),
        json!({"effect_id": "gaussian_blur"}),
        "an effect that is on writes no flag"
    );
    effect.bypass = true;
    assert_eq!(
        serde_json::to_value(&effect).unwrap(),
        json!({"effect_id": "gaussian_blur", "bypass": true})
    );
    let older: aria_models::EffectInstance =
        serde_json::from_value(json!({"effect_id": "gaussian_blur"})).unwrap();
    assert!(
        !older.bypass,
        "a chain saved before the flag renders every effect"
    );
}

/// The chain is one field for all three kinds: a clip and a lane by id, the
/// master by name.
#[test]
fn golden_audio_insert_targets() {
    let cases = [
        (
            EditCommand::AddAudioEffect {
                target: aria_commands::AudioInsertTarget::Clip(clip(4)),
                effect_id: "example.compressor".into(),
            },
            json!({"type": "AddAudioEffect", "target": {"Clip": 4}, "effect_id": "example.compressor"}),
        ),
        (
            EditCommand::SetAudioEffectBypass {
                target: aria_commands::AudioInsertTarget::Track(track(2)),
                index: 1,
                bypass: true,
            },
            json!({"type": "SetAudioEffectBypass", "target": {"Track": 2}, "index": 1, "bypass": true}),
        ),
        (
            EditCommand::SetAudioEffectSidechain {
                target: aria_commands::AudioInsertTarget::Master,
                index: 0,
                key: Some(track(3)),
            },
            json!({"type": "SetAudioEffectSidechain", "target": "Master", "index": 0, "key": 3}),
        ),
        (
            EditCommand::SetAudioEffectParam {
                target: aria_commands::AudioInsertTarget::Master,
                index: 0,
                param: 2,
                value: Param::Constant(-18.0),
            },
            json!({"type": "SetAudioEffectParam", "target": "Master", "index": 0, "param": 2, "value": -18.0}),
        ),
    ];
    for (command, wire) in cases {
        assert_eq!(serde_json::to_value(Command::Edit(command)).unwrap(), wire);
    }
}

#[test]
fn golden_set_param_keyframe() {
    let cmd = Command::Edit(EditCommand::SetParamKeyframe {
        clip: clip(4),
        param: ClipParam::Scale,
        at: t(45),
        value: ParamValue::Scalar(1.5),
        easing: Easing::EaseInOut,
        tangents: None,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "SetParamKeyframe",
            "clip": 4,
            "param": "scale",
            "at": {"value": 45, "rate": {"num": 30, "den": 1}},
            "value": {"scalar": 1.5},
            "easing": "ease_in_out",
        })
    );
}

#[test]
fn golden_set_param_keyframe_with_spatial_tangents() {
    let cmd = Command::Edit(EditCommand::SetParamKeyframe {
        clip: clip(4),
        param: ClipParam::Position,
        at: t(45),
        value: ParamValue::Vec2([0.25, -0.5]),
        easing: Easing::Linear,
        tangents: Some(SpatialTangents {
            out_t: [0.0, 0.5],
            in_t: [-0.25, 0.0],
        }),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "SetParamKeyframe",
            "clip": 4,
            "param": "position",
            "at": {"value": 45, "rate": {"num": 30, "den": 1}},
            "value": {"vec2": [0.25, -0.5]},
            "easing": "linear",
            "tangents": {"o": [0.0, 0.5], "i": [-0.25, 0.0]},
        })
    );
}

#[test]
fn golden_set_canvas() {
    let cmd = Command::Edit(EditCommand::SetCanvas {
        aspect: CanvasAspect::Tall9x16,
        background: [0, 0, 0],
        resolution: None,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetCanvas", "aspect": "9:16", "background": [0, 0, 0]})
    );
}

/// A derived canvas size stays off the wire; an explicit one is written, so a
/// project saved before canvas sizes existed still deserializes and a delivery
/// size survives a round trip.
#[test]
fn golden_set_canvas_with_explicit_size() {
    let cmd = Command::Edit(EditCommand::SetCanvas {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some([3840, 2160]),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "SetCanvas",
            "aspect": "auto",
            "background": [0, 0, 0],
            "resolution": [3840, 2160],
        })
    );
}

#[test]
fn golden_set_sequence_rate() {
    let cmd = Command::Edit(EditCommand::SetSequenceRate {
        frame_rate: Rational::FPS_60,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetSequenceRate", "frame_rate": {"num": 60, "den": 1}})
    );
}

#[test]
fn golden_set_audio_layout() {
    let cmd = Command::Edit(EditCommand::SetAudioLayout {
        layout: AudioLayout::Surround51,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetAudioLayout", "layout": "5.1"})
    );
}

#[test]
fn golden_set_time_display() {
    let cmd = Command::Edit(EditCommand::SetTimeDisplay {
        display: TimeDisplay::FeetFrames,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetTimeDisplay", "display": "feet_frames"})
    );
}

#[test]
fn golden_add_generated_solid() {
    let cmd = Command::Edit(EditCommand::AddGenerated {
        track: track(1),
        generator: Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        timeline: tr(0, 60),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "AddGenerated",
            "track": 1,
            "generator": {"SolidColor": {"rgba": [255, 0, 0, 255]}},
            "timeline": {
                "start": {"value": 0, "rate": {"num": 30, "den": 1}},
                "duration": {"value": 60, "rate": {"num": 30, "den": 1}},
            },
        })
    );
}

#[test]
fn golden_speed_curve_forms() {
    // A constant curve serializes as the bare value…
    let constant = Command::Edit(EditCommand::SetSpeedCurve {
        clip: clip(4),
        curve: Some(Param::Constant(1.0)),
    });
    assert_eq!(
        serde_json::to_value(&constant).unwrap(),
        json!({"type": "SetSpeedCurve", "clip": 4, "curve": 1.0})
    );

    // …and a keyframed curve as the `{"kf": [...]}` map (the project-file
    // format), roundtripping through `Command`.
    let keyframed: Command = serde_json::from_value(json!({
        "type": "SetSpeedCurve",
        "clip": 4,
        "curve": {"kf": [
            {"t": 0, "v": 1.0},
            {"t": 500, "v": 2.0, "e": "ease_in"},
        ]},
    }))
    .unwrap();
    let json = serde_json::to_value(&keyframed).unwrap();
    assert_eq!(json["curve"]["kf"][1]["v"], 2.0);
    let back: Command = serde_json::from_value(json).unwrap();
    assert_eq!(back, keyframed);
}

#[test]
fn golden_clear_speed_curve_is_null() {
    let cmd = Command::Edit(EditCommand::SetSpeedCurve {
        clip: clip(4),
        curve: None,
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({"type": "SetSpeedCurve", "clip": 4, "curve": null})
    );
}

#[test]
fn golden_look_commands() {
    // Snake_case ids on the wire; zero/default sub-fields elided.
    let add = Command::Edit(EditCommand::AddClipMask {
        clip: clip(4),
        mask: Mask::new(MaskKind::Circle),
    });
    assert_eq!(
        serde_json::to_value(&add).unwrap(),
        json!({"type": "AddClipMask", "clip": 4, "mask": {"kind": "circle"}})
    );

    // A mode other than add, and non-default opacity and expansion, are on
    // the wire by their snake_case names.
    let mut subtract = Mask::new(MaskKind::Rectangle);
    subtract.mode = MaskMode::Subtract;
    subtract.opacity = 0.5f32.into();
    subtract.expansion = (-12.0f32).into();
    let set = Command::Edit(EditCommand::SetClipMask {
        clip: clip(4),
        index: 1,
        mask: subtract,
    });
    assert_eq!(
        serde_json::to_value(&set).unwrap(),
        json!({
            "type": "SetClipMask",
            "clip": 4,
            "index": 1,
            "mask": {"kind": "rectangle", "mode": "subtract", "opacity": 0.5, "expansion": -12.0},
        })
    );

    let remove = Command::Edit(EditCommand::RemoveClipMask {
        clip: clip(4),
        index: 0,
    });
    assert_eq!(
        serde_json::to_value(&remove).unwrap(),
        json!({"type": "RemoveClipMask", "clip": 4, "index": 0})
    );

    let reorder = Command::Edit(EditCommand::MoveClipMask {
        clip: clip(4),
        from_index: 0,
        to_index: 2,
    });
    assert_eq!(
        serde_json::to_value(&reorder).unwrap(),
        json!({"type": "MoveClipMask", "clip": 4, "from_index": 0, "to_index": 2})
    );

    let blend = Command::Edit(EditCommand::SetClipBlendMode {
        clip: clip(4),
        mode: BlendMode::Multiply,
    });
    assert_eq!(
        serde_json::to_value(&blend).unwrap(),
        json!({"type": "SetClipBlendMode", "clip": 4, "mode": "multiply"})
    );

    let motion_blur = Command::Edit(EditCommand::SetClipMotionBlur {
        clip: clip(4),
        motion_blur: MotionBlur {
            enabled: true,
            shutter_deg: 180.0,
            samples: 8,
        },
    });
    assert_eq!(
        serde_json::to_value(&motion_blur).unwrap(),
        json!({
            "type": "SetClipMotionBlur",
            "clip": 4,
            "motion_blur": {
                "enabled": true,
                "shutter_deg": 180.0,
                "samples": 8
            }
        })
    );

    let styles = Command::Edit(EditCommand::SetClipLayerStyles {
        clip: clip(4),
        styles: LayerStyles {
            shadow: Some(LayerShadow {
                rgba: Param::Constant([0, 0, 0, 128]),
                offset: Param::Constant([4.0, 4.0]),
                blur: Param::Constant(8.0),
            }),
            ..Default::default()
        },
    });
    assert_eq!(
        serde_json::to_value(&styles).unwrap(),
        json!({
            "type": "SetClipLayerStyles",
            "clip": 4,
            "styles": {
                "shadow": {
                    "rgba": [0, 0, 0, 128],
                    "offset": [4.0, 4.0],
                    "blur": 8.0
                }
            }
        })
    );

    let chroma = Command::Edit(EditCommand::SetClipChroma {
        clip: clip(4),
        chroma: Some(ChromaKey {
            rgb: [0, 255, 0],
            strength: 0.5.into(),
            shadow: 0.0.into(),
        }),
    });
    assert_eq!(
        serde_json::to_value(&chroma).unwrap(),
        json!({
            "type": "SetClipChroma",
            "clip": 4,
            "chroma": {"rgb": [0, 255, 0], "strength": 0.5},
        })
    );

    let stabilize = Command::Edit(EditCommand::SetClipStabilize {
        clip: clip(4),
        stabilize: Some(StabilizeLevel::MaxSmooth),
    });
    assert_eq!(
        serde_json::to_value(&stabilize).unwrap(),
        json!({"type": "SetClipStabilize", "clip": 4, "stabilize": "max_smooth"})
    );

    let filter = Command::Edit(EditCommand::SetClipFilter {
        clip: clip(4),
        filter: Some(Filter {
            id: "vivid".into(),
            intensity: 0.75.into(),
        }),
    });
    assert_eq!(
        serde_json::to_value(&filter).unwrap(),
        json!({
            "type": "SetClipFilter",
            "clip": 4,
            "filter": {"id": "vivid", "intensity": 0.75},
        })
    );

    let adjust = Command::Edit(EditCommand::SetClipAdjustments {
        clip: clip(4),
        adjust: ColorAdjustments {
            brightness: 0.25.into(),
            ..Default::default()
        },
    });
    assert_eq!(
        serde_json::to_value(&adjust).unwrap(),
        json!({
            "type": "SetClipAdjustments",
            "clip": 4,
            "adjust": {"brightness": 0.25},
        })
    );

    let animation = Command::Edit(EditCommand::SetClipAnimation {
        clip: clip(4),
        slot: AnimationSlot::Combo,
        animation: Some(AnimationRef::new("pulse")),
    });
    assert_eq!(
        serde_json::to_value(&animation).unwrap(),
        json!({
            "type": "SetClipAnimation",
            "clip": 4,
            "slot": "combo",
            "animation": {"id": "pulse"},
        })
    );

    // Clearing is an explicit null (same convention as SetSpeedCurve).
    let clear_role = Command::Edit(EditCommand::SetAudioRole {
        clip: clip(4),
        role: None,
    });
    assert_eq!(
        serde_json::to_value(&clear_role).unwrap(),
        json!({"type": "SetAudioRole", "clip": 4, "role": null})
    );
    let role = Command::Edit(EditCommand::SetAudioRole {
        clip: clip(4),
        role: Some(AudioRole::Sfx),
    });
    assert_eq!(
        serde_json::to_value(&role).unwrap(),
        json!({"type": "SetAudioRole", "clip": 4, "role": "sfx"})
    );
}

#[test]
fn golden_apply_preset() {
    let cmd = Command::Edit(EditCommand::ApplyPreset {
        clip: clip(4),
        preset: aria_commands::Preset::new(
            "Warm Blur",
            vec![aria_commands::EffectInstance::new("gaussian_blur")],
            Vec::new(),
            Vec::new(),
        ),
        at: t(15),
    });
    assert_eq!(
        serde_json::to_value(&cmd).unwrap(),
        json!({
            "type": "ApplyPreset",
            "clip": 4,
            "preset": {
                "version": 1,
                "name": "Warm Blur",
                "effects": [{"effect_id": "gaussian_blur"}],
                "audio_effects": [],
                "properties": [],
            },
            "at": {"value": 15, "rate": {"num": 30, "den": 1}},
        })
    );
}
