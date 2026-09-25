//! `inverse_edits` tests covering params and keyframes 1. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn undo_redo_set_clip_transform_oscillates() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Text, "T1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text("title"),
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    let moved = ClipTransform {
        position: [0.25, -0.1],
        scale: 0.5.into(),
        rotation: 45.0,
        opacity: 0.8,
        ..ClipTransform::IDENTITY
    };
    engine
        .apply(Command::Edit(EditCommand::SetClipTransform {
            clip: clip_id,
            transform: moved,
            at: None,
        }))
        .expect("set transform");

    let transform =
        |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().transform.clone();
    assert_eq!(transform(&engine), moved.into());

    assert!(engine.undo());
    assert!(transform(&engine).is_identity());

    assert!(engine.redo());
    assert_eq!(transform(&engine), moved.into());
}

#[test]
fn invalid_transform_rejected_and_state_unchanged() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Text, "T1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text("title"),
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    // Zero scale is rejected; the clip keeps its identity transform.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipTransform {
                clip: clip_id,
                transform: ClipTransform {
                    scale: 0.0.into(),
                    ..ClipTransform::IDENTITY
                },
                at: None,
            }))
            .is_err()
    );
    assert!(
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .is_identity()
    );
}

#[test]
fn set_param_keyframe_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: ClipParam::Opacity,
            at: rt(0),
            value: ParamValue::Scalar(0.0),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("first keyframe");
    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: ClipParam::Opacity,
            at: rt(24),
            value: ParamValue::Scalar(1.0),
            easing: Easing::EaseInOut,
            tangents: None,
        }))
        .expect("second keyframe");

    let opacity = |engine: &aria_engine::Engine| {
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .opacity
            .clone()
    };
    assert_eq!(opacity(&engine).keyframes().len(), 2);

    // Undo peels one keyframe at a time; the first undo restores the
    // single-keyframe curve, the second the constant.
    assert!(engine.undo());
    assert_eq!(opacity(&engine).keyframes().len(), 1);
    assert!(engine.undo());
    assert!(!opacity(&engine).is_animated());
    assert_eq!(opacity(&engine).constant(), Some(1.0));

    assert!(engine.redo());
    assert!(engine.redo());
    assert_eq!(opacity(&engine).keyframes().len(), 2);
    assert_eq!(
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .sample(12)
            .opacity,
        0.5
    );
}

#[test]
fn set_clip_speed_undo_redo_roundtrip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::SetClipSpeed {
            clip: clip_id,
            speed: aria_models::Rational::new(2, 1),
            reversed: true,
        }))
        .expect("retime");
    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();
    assert_eq!(clip(&engine).timeline.duration.value, 24);
    assert!(clip(&engine).reversed);

    // One undo restores speed, direction, AND the re-derived duration.
    assert!(engine.undo());
    let restored = clip(&engine);
    assert_eq!(restored.timeline.duration.value, 48);
    assert_eq!(restored.speed, aria_models::Rational::new(1, 1));
    assert!(!restored.reversed && !restored.is_retimed());

    assert!(engine.redo());
    assert_eq!(clip(&engine).timeline.duration.value, 24);
    assert!(clip(&engine).is_retimed());
}

#[test]
fn set_speed_curve_undo_redo_roundtrip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );
    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();

    // A montage ramp (average 2×) halves the footprint and marks the clip
    // retimed, just like a constant 2× — but as a curve.
    let curve = aria_models::speed_preset("montage").unwrap();
    let avg = {
        let mut probe = clip(&engine);
        probe.speed_curve = curve.clone();
        probe.speed_curve_average()
    };
    let expected = (48.0 / avg).round() as i64;
    engine
        .apply(Command::Edit(EditCommand::SetSpeedCurve {
            clip: clip_id,
            curve: Some(curve.clone()),
        }))
        .expect("set ramp");
    assert_eq!(clip(&engine).timeline.duration.value, expected);
    assert!(clip(&engine).has_speed_curve() && clip(&engine).is_retimed());

    // One undo restores the flat ramp AND the original duration.
    assert!(engine.undo());
    let restored = clip(&engine);
    assert_eq!(restored.timeline.duration.value, 48);
    assert!(!restored.has_speed_curve() && !restored.is_retimed());

    assert!(engine.redo());
    assert_eq!(clip(&engine).speed_curve, curve);
    assert_eq!(clip(&engine).timeline.duration.value, expected);
}

#[test]
fn set_speed_curve_rejects_generated_clips() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetSpeedCurve {
                clip: clip_id,
                curve: Some(aria_models::speed_preset("hero").unwrap()),
            }))
            .is_err()
    );
}

#[test]
fn set_clip_audio_undo_redo_roundtrip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Audio, "A1");
    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::SetClipAudio {
            clip: clip_id,
            volume: Some(0.5),
            fade_in: rt(12),
            fade_out: rt(24),
        }))
        .expect("set audio");
    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();
    assert_eq!(clip(&engine).volume.constant(), Some(0.5));
    assert_eq!((clip(&engine).fade_in, clip(&engine).fade_out), (12, 24));

    // One undo restores volume and both fades.
    assert!(engine.undo());
    let restored = clip(&engine);
    assert_eq!(restored.volume.constant(), Some(1.0));
    assert_eq!((restored.fade_in, restored.fade_out), (0, 0));
    assert!(!restored.has_custom_audio());

    assert!(engine.redo());
    assert!(clip(&engine).has_custom_audio());
    assert_eq!(clip(&engine).volume.constant(), Some(0.5));
}

#[test]
fn set_clip_audio_rejects_generated_clips_and_long_fades() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    let before = engine.project().clip(clip_id).unwrap().clone();

    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipAudio {
                clip: clip_id,
                volume: Some(0.5),
                fade_in: rt(0),
                fade_out: rt(0),
            }))
            .is_err()
    );
    // The rejection left the clip untouched.
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
}

#[test]
fn set_clip_denoise_undo_redo_roundtrip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Audio, "A1");
    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );
    let denoise = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().denoise;
    assert!(!denoise(&engine), "off by default");

    engine
        .apply(Command::Edit(EditCommand::SetClipDenoise {
            clip: clip_id,
            denoise: true,
        }))
        .expect("enable denoise");
    assert!(denoise(&engine));

    assert!(engine.undo());
    assert!(!denoise(&engine), "undo clears the flag");
    assert!(engine.redo());
    assert!(denoise(&engine), "redo restores it");
}

#[test]
fn set_clip_denoise_rejects_generated_clips() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    let before = engine.project().clip(clip_id).unwrap().clone();

    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipDenoise {
                clip: clip_id,
                denoise: true,
            }))
            .is_err()
    );
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
}

#[test]
fn set_clip_crop_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    let crop = CropRect {
        x: 0.25,
        y: 0.1,
        w: 0.5,
        h: 0.8,
    };

    engine
        .apply(Command::Edit(EditCommand::SetClipCrop {
            clip: clip_id,
            crop,
            flip_h: true,
            flip_v: false,
            at: None,
        }))
        .expect("set crop");
    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();
    assert_eq!(clip(&engine).crop, Param::Constant(crop));
    assert!(clip(&engine).flip_h && !clip(&engine).flip_v);

    // One undo restores the full frame and both flips.
    assert!(engine.undo());
    let restored = clip(&engine);
    assert!(!restored.has_custom_crop());

    assert!(engine.redo());
    assert_eq!(clip(&engine).crop, Param::Constant(crop));
    assert!(clip(&engine).flip_h);
}

#[test]
fn set_canvas_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    assert!(engine.project().timeline().canvas().is_default());

    let outcome = engine
        .apply(Command::Edit(EditCommand::SetCanvas {
            aspect: CanvasAspect::Tall9x16,
            background: [12, 34, 56],
            resolution: None,
        }))
        .expect("set canvas");
    assert!(matches!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::UpdatedCanvas)
    ));
    let canvas = |engine: &aria_engine::Engine| engine.project().timeline().canvas();
    assert_eq!(canvas(&engine).aspect, CanvasAspect::Tall9x16);
    assert_eq!(canvas(&engine).background, [12, 34, 56]);

    // One undo restores both fields.
    assert!(engine.undo());
    assert!(canvas(&engine).is_default());

    assert!(engine.redo());
    assert_eq!(canvas(&engine).aspect, CanvasAspect::Tall9x16);
    assert_eq!(canvas(&engine).background, [12, 34, 56]);
}

#[test]
fn fine_grained_param_and_audio_undo_preserves_unrelated_changes() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    // 1. Edit audio volume
    engine
        .apply(Command::Edit(EditCommand::SetClipAudio {
            clip: clip_id,
            volume: Some(0.42),
            fade_in: rt(0),
            fade_out: rt(0),
        }))
        .expect("audio");

    // 2. Edit crop
    let custom_crop = CropRect {
        x: 0.1,
        y: 0.1,
        w: 0.8,
        h: 0.8,
    };
    engine
        .apply(Command::Edit(EditCommand::SetClipCrop {
            clip: clip_id,
            crop: custom_crop,
            flip_h: false,
            flip_v: true,
            at: None,
        }))
        .expect("crop");

    // 3. Edit position keyframe
    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: ClipParam::Position,
            at: rt(10),
            value: ParamValue::Vec2([0.2, 0.3]),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("keyframe");

    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();

    // Verify all 3 edits are present
    assert_eq!(clip(&engine).volume, Param::Constant(0.42));
    assert_eq!(clip(&engine).crop, Param::Constant(custom_crop));
    assert!(clip(&engine).flip_v);
    assert!(clip(&engine).transform.position.is_animated());

    // Undo 3 (position): volume and crop MUST remain unchanged
    assert!(engine.undo());
    assert!(!clip(&engine).transform.position.is_animated());
    assert_eq!(clip(&engine).volume, Param::Constant(0.42));
    assert_eq!(clip(&engine).crop, Param::Constant(custom_crop));
    assert!(clip(&engine).flip_v);

    // Undo 2 (crop): volume MUST remain unchanged
    assert!(engine.undo());
    assert!(!clip(&engine).has_custom_crop());
    assert!(!clip(&engine).flip_v);
    assert_eq!(clip(&engine).volume, Param::Constant(0.42));

    // Undo 1 (volume): restores default volume
    assert!(engine.undo());
    assert_eq!(clip(&engine).volume, Param::Constant(1.0));
}
