//! `inverse_edits` tests covering effects. A `#[path]` child of the
//! `inverse_edits` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn add_effect_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("add effect");
    let effects =
        |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().effects.clone();
    assert_eq!(effects(&engine).len(), 1);
    assert_eq!(effects(&engine)[0].effect_id, "gaussian_blur");

    assert!(engine.undo());
    assert!(effects(&engine).is_empty());
    assert!(engine.redo());
    assert_eq!(effects(&engine).len(), 1);
}

#[test]
fn remove_effect_undo_restores_it() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "vignette".into(),
        }))
        .expect("add effect");
    engine
        .apply(Command::Edit(EditCommand::RemoveEffect {
            clip: clip_id,
            index: 0,
        }))
        .expect("remove effect");
    assert!(engine.project().clip(clip_id).unwrap().effects.is_empty());

    assert!(engine.undo());
    let effects = engine.project().clip(clip_id).unwrap().effects.clone();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].effect_id, "vignette");
}

#[test]
fn set_effect_param_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "vignette".into(),
        }))
        .expect("add effect");
    engine
        .apply(Command::Edit(EditCommand::SetEffectParam {
            clip: clip_id,
            index: 0,
            param: 0,
            value: 0.75,
        }))
        .expect("set param");

    let amount = |engine: &aria_engine::Engine| {
        engine.project().clip(clip_id).unwrap().effects[0].sample_param("amount", 0.0)
    };
    assert_eq!(amount(&engine), Some(0.75));

    // Undo restores the default (param absent → catalog default 0.6).
    assert!(engine.undo());
    assert_eq!(amount(&engine), Some(0.6));
    assert!(engine.redo());
    assert_eq!(amount(&engine), Some(0.75));
}

#[test]
fn move_effect_undo_redo_restores_exact_instances() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    for effect_id in ["gaussian_blur", "glitch", "vignette"] {
        engine
            .apply(Command::Edit(EditCommand::AddEffect {
                clip: clip_id,
                effect_id: effect_id.into(),
            }))
            .expect("add effect");
    }
    engine
        .apply(Command::Edit(EditCommand::SetEffectParam {
            clip: clip_id,
            index: 0,
            param: 0,
            value: 12.0,
        }))
        .expect("set constant effect param");
    for (tick, value, easing) in [(0, 0.2, Easing::EaseIn), (24, 0.8, Easing::EaseOut)] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Effect {
                    effect: 1,
                    param: 0,
                },
                at: rt(tick),
                value: ParamValue::Scalar(value),
                easing,
                tangents: None,
            }))
            .expect("set animated effect param");
    }

    let before = engine.project().clip(clip_id).unwrap().effects.clone();
    let expected = vec![before[1].clone(), before[2].clone(), before[0].clone()];
    engine
        .apply(Command::Edit(EditCommand::MoveEffect {
            clip: clip_id,
            from_index: 0,
            to_index: 2,
        }))
        .expect("move effect");
    assert_eq!(engine.project().clip(clip_id).unwrap().effects, expected);

    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip_id).unwrap().effects, before);
    assert!(engine.redo());
    assert_eq!(engine.project().clip(clip_id).unwrap().effects, expected);
}

#[test]
fn effect_param_keyframe_through_clip_param() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("add effect");

    let fx_param = ClipParam::Effect {
        effect: 0,
        param: 0,
    };
    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: fx_param,
            at: rt(0),
            value: ParamValue::Scalar(0.0),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("kf0");
    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: fx_param,
            at: rt(24),
            value: ParamValue::Scalar(8.0),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("kf24");

    let radius_at = |engine: &aria_engine::Engine, tick: f64| {
        engine.project().clip(clip_id).unwrap().effects[0].sample_param("radius", tick)
    };
    assert_eq!(radius_at(&engine, 12.0), Some(4.0));

    // Undo peels keyframes one at a time, like the transform path.
    assert!(engine.undo());
    assert!(engine.undo());
    assert_eq!(radius_at(&engine, 12.0), Some(4.0)); // back to default constant
}

#[test]
fn effect_commands_reject_unknown_ids_without_history() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    assert!(
        engine
            .apply(Command::Edit(EditCommand::AddEffect {
                clip: clip_id,
                effect_id: "no_such_effect".into(),
            }))
            .is_err()
    );
    assert!(
        engine
            .apply(Command::Edit(EditCommand::RemoveEffect {
                clip: clip_id,
                index: 0,
            }))
            .is_err()
    );
    // Only the add-track + add-generated steps are undoable; failed effect
    // edits push nothing.
    assert!(engine.undo());
    assert!(engine.undo());
    assert!(!engine.can_undo());
}

#[test]
fn apply_preset_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    // Add an initial effect.
    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "vignette".to_string(),
        }))
        .expect("add vignette");

    let mut curve = aria_models::Param::Constant(aria_models::ParamValue::Scalar(1.0));
    curve.set_keyframe(
        0,
        aria_models::ParamValue::Scalar(0.25),
        aria_models::Easing::Linear,
    );
    curve.set_keyframe(
        15,
        aria_models::ParamValue::Scalar(0.9),
        aria_models::Easing::EaseOut,
    );
    let preset = aria_models::Preset::new(
        "Dual Blur + Fade",
        vec![
            aria_models::EffectInstance::new("gaussian_blur"),
            aria_models::EffectInstance::new("vignette"),
        ],
        Vec::new(),
        vec![aria_models::PresetProperty {
            id: "opacity".to_string(),
            curve,
        }],
    );

    let at = aria_models::RationalTime::new(10, aria_models::Rational::new(24, 1));
    engine
        .apply(Command::Edit(EditCommand::ApplyPreset {
            clip: clip_id,
            preset,
            at,
        }))
        .expect("apply preset");

    {
        let clip = engine.project().clip(clip_id).unwrap();
        assert_eq!(clip.effects.len(), 3);
        assert_eq!(clip.effects[0].effect_id, "vignette");
        assert_eq!(clip.effects[1].effect_id, "gaussian_blur");
        assert_eq!(clip.effects[2].effect_id, "vignette");
        let kfs = clip.transform.opacity.keyframes();
        assert_eq!(kfs.len(), 2);
        assert_eq!(kfs[0].tick, 10);
        assert_eq!(kfs[1].tick, 25);
    }

    // Undo restores the single vignette and constant opacity.
    assert!(engine.undo());
    {
        let clip = engine.project().clip(clip_id).unwrap();
        assert_eq!(clip.effects.len(), 1);
        assert_eq!(clip.effects[0].effect_id, "vignette");
        assert!(clip.transform.opacity.keyframes().is_empty());
    }

    // Redo re-applies the preset.
    assert!(engine.redo());
    {
        let clip = engine.project().clip(clip_id).unwrap();
        assert_eq!(clip.effects.len(), 3);
        assert_eq!(clip.effects[0].effect_id, "vignette");
        assert_eq!(clip.effects[1].effect_id, "gaussian_blur");
        assert_eq!(clip.effects[2].effect_id, "vignette");
        assert_eq!(clip.transform.opacity.keyframes().len(), 2);
    }
}

/// Bypass switches one effect off without touching its parameters or its
/// neighbours, one undo brings it back, and an index past the chain is refused
/// with the chain unchanged.
#[test]
fn set_effect_bypass_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);
    for effect_id in ["gaussian_blur", "vignette"] {
        engine
            .apply(Command::Edit(EditCommand::AddEffect {
                clip: clip_id,
                effect_id: effect_id.into(),
            }))
            .expect("add effect");
    }
    engine
        .apply(Command::Edit(EditCommand::SetEffectParam {
            clip: clip_id,
            index: 1,
            param: 0,
            value: 0.75,
        }))
        .expect("set param");
    let effects =
        |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().effects.clone();
    let before = effects(&engine);
    assert!(
        before.iter().all(|effect| !effect.bypass),
        "an added effect is on"
    );

    engine
        .apply(Command::Edit(EditCommand::SetEffectBypass {
            clip: clip_id,
            index: 1,
            bypass: true,
        }))
        .expect("bypass");
    let mut expected = before.clone();
    expected[1].bypass = true;
    assert_eq!(
        effects(&engine),
        expected,
        "only the flag on the named effect changed"
    );

    assert!(engine.undo());
    assert_eq!(effects(&engine), before);
    assert!(engine.redo());
    assert_eq!(effects(&engine), expected);

    let refused = engine.apply(Command::Edit(EditCommand::SetEffectBypass {
        clip: clip_id,
        index: 2,
        bypass: true,
    }));
    assert!(refused.is_err(), "an index past the chain is refused");
    assert_eq!(effects(&engine), expected);
}
