//! `inverse_edits` tests covering params and keyframes 2. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

/// An explicit canvas size is one undoable unit with the rest of the canvas, and
/// an unencodable size is refused rather than clamped or stored: no later stage
/// has to decide what a 1921-pixel or 12000-pixel canvas meant.
#[test]
fn set_canvas_resolution_is_undoable_and_refuses_unencodable_sizes() {
    let (_dir, mut engine) = temp_engine();
    let canvas = |engine: &aria_engine::Engine| engine.project().timeline().canvas();

    engine
        .apply(Command::Edit(EditCommand::SetCanvas {
            aspect: CanvasAspect::Auto,
            background: [0, 0, 0],
            resolution: Some([3840, 2160]),
        }))
        .expect("set canvas size");
    assert_eq!(canvas(&engine).resolution, Some([3840, 2160]));

    for bad in [[1921, 1080], [0, 0], [12000, 1080]] {
        let error = engine
            .apply(Command::Edit(EditCommand::SetCanvas {
                aspect: CanvasAspect::Auto,
                background: [0, 0, 0],
                resolution: Some(bad),
            }))
            .expect_err("unencodable canvas should be refused");
        assert!(
            error.to_string().contains("not encodable"),
            "{bad:?} refused with an unhelpful message: {error}"
        );
        // The refusal left the project alone, so a mistake never costs the
        // size that was already set.
        assert_eq!(canvas(&engine).resolution, Some([3840, 2160]));
    }

    assert!(engine.undo());
    assert!(canvas(&engine).resolution.is_none());
    assert!(engine.redo());
    assert_eq!(canvas(&engine).resolution, Some([3840, 2160]));
}

#[test]
fn set_clip_crop_rejects_invalid_rects_and_audio_lanes() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Audio, "A1");
    let audio_clip = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    // Audio lanes have no frame to crop.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipCrop {
                clip: audio_clip,
                crop: CropRect::FULL,
                flip_h: true,
                flip_v: false,
                at: None,
            }))
            .is_err()
    );

    // Out-of-frame rects bounce without touching the clip.
    let text = text_clip(&mut engine);
    let before = engine.project().clip(text).unwrap().clone();
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipCrop {
                clip: text,
                crop: CropRect {
                    x: 0.8,
                    y: 0.0,
                    w: 0.5,
                    h: 1.0
                },
                flip_h: false,
                flip_v: false,
                at: None,
            }))
            .is_err()
    );
    assert_eq!(engine.project().clip(text).unwrap(), &before);
}

#[test]
fn set_clip_speed_rejects_generated_clips() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    let before = engine.project().clip(clip_id).unwrap().clone();

    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipSpeed {
                clip: clip_id,
                speed: aria_models::Rational::new(2, 1),
                reversed: false,
            }))
            .is_err()
    );
    // The rejection left the clip untouched.
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
}

#[test]
fn remove_param_keyframe_undo_restores_curve() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    for (tick, value) in [(0, 0.0), (24, 1.0)] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Scale,
                at: rt(tick),
                value: ParamValue::Scalar(value + 1.0),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("keyframe");
    }

    engine
        .apply(Command::Edit(EditCommand::RemoveParamKeyframe {
            clip: clip_id,
            param: ClipParam::Scale,
            at: rt(24),
        }))
        .expect("remove");
    let scale = |engine: &aria_engine::Engine| {
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .scale
            .clone()
    };
    assert_eq!(scale(&engine).keyframes().len(), 1);

    assert!(engine.undo());
    assert_eq!(scale(&engine).keyframes().len(), 2);

    // Removing a keyframe that isn't there is rejected and pushes no history.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::RemoveParamKeyframe {
                clip: clip_id,
                param: ClipParam::Scale,
                at: rt(7),
            }))
            .is_err()
    );
    assert_eq!(scale(&engine).keyframes().len(), 2);
}

#[test]
fn set_param_constant_undo_restores_keyframes() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    for tick in [0, 24] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Rotation,
                at: rt(tick),
                value: ParamValue::Scalar(tick as f32),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("keyframe");
    }

    engine
        .apply(Command::Edit(EditCommand::SetParamConstant {
            clip: clip_id,
            param: ClipParam::Rotation,
            value: ParamValue::Scalar(90.0),
        }))
        .expect("flatten");
    let rotation = |engine: &aria_engine::Engine| {
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .rotation
            .clone()
    };
    assert_eq!(rotation(&engine).constant(), Some(90.0));

    assert!(engine.undo());
    assert_eq!(rotation(&engine).keyframes().len(), 2);
    assert!(engine.redo());
    assert_eq!(rotation(&engine).constant(), Some(90.0));
}

#[test]
fn param_keyframe_outside_clip_rejected() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine); // [0, 48)

    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Opacity,
                at: rt(48), // exclusive end — outside
                value: ParamValue::Scalar(0.5),
                easing: Easing::Linear,
                tangents: None,
            }))
            .is_err()
    );
    assert!(
        !engine
            .project()
            .clip(clip_id)
            .unwrap()
            .transform
            .is_animated()
    );
}

#[test]
fn transform_gesture_at_playhead_keyframes_animated_property() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    // Animate scale 1 → 3 across the clip.
    for (tick, value) in [(0, 1.0), (40, 3.0)] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Scale,
                at: rt(tick),
                value: ParamValue::Scalar(value),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("keyframe");
    }

    // A gesture commit at tick 20 (sampled scale 2.0 → user dragged to 2.5).
    engine
        .apply(Command::Edit(EditCommand::SetClipTransform {
            clip: clip_id,
            transform: ClipTransform {
                scale: 2.5.into(),
                ..ClipTransform::IDENTITY
            },
            at: Some(rt(20)),
        }))
        .expect("compose");

    let transform = engine.project().clip(clip_id).unwrap().transform.clone();
    // Scale gained a keyframe; the endpoints survive.
    assert_eq!(transform.scale.keyframes().len(), 3);
    assert_eq!(transform.sample(20).scale, 2.5.into());
    assert_eq!(transform.sample(0).scale, 1.0.into());
    assert_eq!(transform.sample(40).scale, 3.0.into());
    // Un-animated properties stay constant.
    assert!(!transform.position.is_animated());
}
