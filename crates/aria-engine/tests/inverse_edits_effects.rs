//! `inverse_edits` tests covering effects. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

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
fn apply_effect_preset_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = text_clip(&mut engine);

    // Add an initial effect.
    engine
        .apply(Command::Edit(EditCommand::AddEffect {
            clip: clip_id,
            effect_id: "vignette".to_string(),
        }))
        .expect("add vignette");

    let preset = aria_models::EffectPreset::new(
        "Dual Blur",
        vec![
            aria_models::EffectInstance::new("gaussian_blur"),
            aria_models::EffectInstance::new("vignette"),
        ],
    );

    engine
        .apply(Command::Edit(EditCommand::ApplyEffectPreset {
            clip: clip_id,
            preset,
        }))
        .expect("apply preset");

    let effects =
        |engine: &aria_engine::Engine| engine.project().clip(clip_id).unwrap().effects.to_vec();

    assert_eq!(effects(&engine).len(), 2);
    assert_eq!(effects(&engine)[0].effect_id, "gaussian_blur");
    assert_eq!(effects(&engine)[1].effect_id, "vignette");

    // Undo restores the single vignette.
    assert!(engine.undo());
    assert_eq!(effects(&engine).len(), 1);
    assert_eq!(effects(&engine)[0].effect_id, "vignette");

    // Redo re-applies the preset.
    assert!(engine.redo());
    assert_eq!(effects(&engine).len(), 2);
    assert_eq!(effects(&engine)[0].effect_id, "gaussian_blur");
}
