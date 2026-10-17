//! `inverse_edits` tests covering looks and audio role. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

// --- Phase I look commands ---------------------------------------------------

#[test]
fn set_clip_blend_mode_undo_redo_roundtrip() {
    use aria_models::BlendMode;

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
    let before = engine.project().clip(clip_id).unwrap().clone();
    assert_eq!(before.blend_mode, BlendMode::Normal);

    engine
        .apply(Command::Edit(EditCommand::SetClipBlendMode {
            clip: clip_id,
            mode: BlendMode::Multiply,
        }))
        .expect("blend");
    assert_eq!(
        engine.project().clip(clip_id).unwrap().blend_mode,
        BlendMode::Multiply
    );

    // Inverse (undo) restores the pre-edit clip snapshot.
    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
    assert!(engine.redo());
    assert_eq!(
        engine.project().clip(clip_id).unwrap().blend_mode,
        BlendMode::Multiply
    );
}

#[test]
fn set_clip_motion_blur_undo_redo_roundtrip() {
    use aria_models::MotionBlur;

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
    let before = engine.project().clip(clip_id).unwrap().clone();
    assert!(before.motion_blur.is_default());

    let blur = MotionBlur {
        enabled: true,
        shutter_deg: 270.0,
        samples: 12,
    };
    engine
        .apply(Command::Edit(EditCommand::SetClipMotionBlur {
            clip: clip_id,
            motion_blur: blur,
        }))
        .expect("motion blur");
    assert_eq!(engine.project().clip(clip_id).unwrap().motion_blur, blur);

    // Inverse (undo) restores the pre-edit clip snapshot.
    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
    assert!(engine.redo());
    assert_eq!(engine.project().clip(clip_id).unwrap().motion_blur, blur);
}

#[test]
fn set_clip_layer_styles_undo_redo_roundtrip() {
    use aria_models::{LayerShadow, LayerStyles, Param};

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
    let before = engine.project().clip(clip_id).unwrap().clone();
    assert!(before.styles.is_empty());

    let styles = LayerStyles {
        shadow: Some(LayerShadow {
            rgba: Param::Constant([0, 0, 0, 128]),
            offset: Param::Constant([4.0, 4.0]),
            blur: Param::Constant(8.0),
        }),
        ..Default::default()
    };
    engine
        .apply(Command::Edit(EditCommand::SetClipLayerStyles {
            clip: clip_id,
            styles: styles.clone(),
        }))
        .expect("styles");
    assert_eq!(engine.project().clip(clip_id).unwrap().styles, styles);

    // Inverse (undo) restores the pre-edit clip snapshot (styles cleared).
    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
    assert!(engine.project().clip(clip_id).unwrap().styles.is_empty());
    assert!(engine.redo());
    assert_eq!(engine.project().clip(clip_id).unwrap().styles, styles);
}

#[test]
fn look_commands_undo_redo_roundtrip() {
    use aria_models::{
        AnimationRef, AnimationSlot, ChromaKey, ColorAdjustments, Filter, Mask, MaskKind,
        StabilizeLevel,
    };

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

    // Mask, chroma, stabilize, filter, adjust — five history entries.
    engine
        .apply(Command::Edit(EditCommand::AddClipMask {
            clip: clip_id,
            mask: Mask::new(MaskKind::Heart),
        }))
        .expect("mask");
    engine
        .apply(Command::Edit(EditCommand::SetClipChroma {
            clip: clip_id,
            chroma: Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.7.into(),
                shadow: 0.1.into(),
            }),
        }))
        .expect("chroma");
    engine
        .apply(Command::Edit(EditCommand::SetClipStabilize {
            clip: clip_id,
            stabilize: Some(StabilizeLevel::Recommended),
        }))
        .expect("stabilize");
    engine
        .apply(Command::Edit(EditCommand::SetClipFilter {
            clip: clip_id,
            filter: Some(Filter::new("noir")),
        }))
        .expect("filter");
    engine
        .apply(Command::Edit(EditCommand::SetClipAdjustments {
            clip: clip_id,
            adjust: ColorAdjustments {
                exposure: 0.5.into(),
                ..Default::default()
            },
        }))
        .expect("adjust");

    let styled = clip(&engine);
    assert_eq!(styled.masks[0].kind, MaskKind::Heart);
    assert_eq!(styled.chroma_key.as_ref().unwrap().rgb, [0, 255, 0]);
    assert_eq!(styled.stabilize, Some(StabilizeLevel::Recommended));
    assert_eq!(styled.filter.as_ref().unwrap().id, "noir");
    assert_eq!(styled.adjust.exposure.sample(0), 0.5);

    // Undo peels them off one at a time (each is one history entry)…
    assert!(engine.undo());
    assert!(clip(&engine).adjust.is_neutral());
    assert!(engine.undo());
    assert!(clip(&engine).filter.is_none());
    assert!(engine.undo());
    assert!(clip(&engine).stabilize.is_none());
    assert!(engine.undo());
    assert!(clip(&engine).chroma_key.is_none());
    assert!(engine.undo());
    assert!(clip(&engine).masks.is_empty());

    // …and redo brings the whole look back.
    for _ in 0..5 {
        assert!(engine.redo());
    }
    assert_eq!(clip(&engine), styled);

    // Animations: a combo evicts the entrance in one undoable step.
    //
    // WHY the entrance is checked after the undo: the inverse used to snapshot
    // only the slot the command named. Setting a combo evicts the entrance and
    // the exit, so the snapshot of the combo slot alone was `None` and undo
    // restored `None` to the combo while leaving the two it displaced gone for
    // good. The entrance was unrecoverable through undo, which is data loss, not
    // a wrong pixel. The inverse now carries all three slots.
    engine
        .apply(Command::Edit(EditCommand::SetClipAnimation {
            clip: clip_id,
            slot: AnimationSlot::In,
            animation: Some(AnimationRef::new("zoom_in")),
        }))
        .expect("animation in");
    engine
        .apply(Command::Edit(EditCommand::SetClipAnimation {
            clip: clip_id,
            slot: AnimationSlot::Out,
            animation: Some(AnimationRef::new("zoom_out")),
        }))
        .expect("animation out");
    engine
        .apply(Command::Edit(EditCommand::SetClipAnimation {
            clip: clip_id,
            slot: AnimationSlot::Combo,
            animation: Some(AnimationRef::new("pulse")),
        }))
        .expect("animation combo");
    let c = clip(&engine);
    assert!(c.animation_in.is_none());
    assert!(c.animation_out.is_none());
    assert_eq!(c.animation_combo.as_ref().unwrap().id, "pulse");

    // One undo brings back both slots the combo displaced.
    assert!(engine.undo());
    let c = clip(&engine);
    assert_eq!(c.animation_in.as_ref().unwrap().id, "zoom_in");
    assert_eq!(c.animation_out.as_ref().unwrap().id, "zoom_out");
    assert!(c.animation_combo.is_none());

    // And redo evicts them again, so the pair is symmetric rather than one-way.
    assert!(engine.redo());
    let c = clip(&engine);
    assert!(c.animation_in.is_none());
    assert!(c.animation_out.is_none());
    assert_eq!(c.animation_combo.as_ref().unwrap().id, "pulse");
}

#[test]
fn look_commands_reject_invalid_targets_without_history() {
    use aria_models::{AudioRole, Filter};

    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    let before = engine.project().clip(clip_id).unwrap().clone();
    let history_before = (engine.can_undo(), engine.can_redo());

    // Generated text: no audio lane for a role, and unknown filter ids
    // bounce.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetAudioRole {
                clip: clip_id,
                role: Some(AudioRole::Music),
            }))
            .is_err()
    );
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetClipFilter {
                clip: clip_id,
                filter: Some(Filter::new("no_such_filter")),
            }))
            .is_err()
    );
    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
    assert_eq!((engine.can_undo(), engine.can_redo()), history_before);
}

#[test]
fn set_audio_role_tags_audio_clips_with_undo() {
    use aria_models::AudioRole;

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
        .apply(Command::Edit(EditCommand::SetAudioRole {
            clip: clip_id,
            role: Some(AudioRole::Extracted),
        }))
        .expect("role");
    assert_eq!(
        engine.project().clip(clip_id).unwrap().audio_role,
        Some(AudioRole::Extracted)
    );
    assert!(engine.undo());
    assert!(engine.project().clip(clip_id).unwrap().audio_role.is_none());
    assert!(engine.redo());
    assert_eq!(
        engine.project().clip(clip_id).unwrap().audio_role,
        Some(AudioRole::Extracted)
    );
}

#[test]
fn fine_grained_look_undo_preserves_unrelated_clip_edits() {
    use aria_models::{BlendMode, Filter, MotionBlur};

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

    // 1. Set blend mode
    engine
        .apply(Command::Edit(EditCommand::SetClipBlendMode {
            clip: clip_id,
            mode: BlendMode::Screen,
        }))
        .expect("blend");

    // 2. Set filter
    engine
        .apply(Command::Edit(EditCommand::SetClipFilter {
            clip: clip_id,
            filter: Some(Filter::new("sunset")),
        }))
        .expect("filter");

    // 3. Set motion blur
    let mb = MotionBlur {
        enabled: true,
        shutter_deg: 180.0,
        samples: 8,
    };
    engine
        .apply(Command::Edit(EditCommand::SetClipMotionBlur {
            clip: clip_id,
            motion_blur: mb,
        }))
        .expect("motion blur");

    let clip = |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().clone();

    assert_eq!(clip(&engine).blend_mode, BlendMode::Screen);
    assert!(clip(&engine).filter.is_some());
    assert_eq!(clip(&engine).motion_blur, mb);

    // Undo 3 (motion blur): blend mode and filter must remain untouched
    assert!(engine.undo());
    assert_eq!(clip(&engine).motion_blur, MotionBlur::default());
    assert_eq!(clip(&engine).blend_mode, BlendMode::Screen);
    assert!(clip(&engine).filter.is_some());

    // Undo 2 (filter): blend mode must remain untouched
    assert!(engine.undo());
    assert!(clip(&engine).filter.is_none());
    assert_eq!(clip(&engine).blend_mode, BlendMode::Screen);

    // Undo 1 (blend mode): blend mode back to normal
    assert!(engine.undo());
    assert_eq!(clip(&engine).blend_mode, BlendMode::Normal);
}
