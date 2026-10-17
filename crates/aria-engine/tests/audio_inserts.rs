//! Insert chain commands through the engine: every gesture on a clip's, a
//! lane's and the master's chain applies, reports the outcome its target
//! implies, undoes to the chain as it was, and redoes to the chain it made.
//!
//! Its own test binary because it registers audio effects, which are
//! process-wide. Every test registers ids no other test uses.
//!
//! The project is built in the model rather than imported, so a clip with sound
//! needs no decoder: an insert edit reads the pool's `has_audio`, not the file.

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AudioEffectInstance, AudioEffectSpec, AudioInsertTarget, ClipId, Easing, EffectParamKind,
    EffectParamSpec, Keyframe, MediaSource, Param, Project, Rational, RationalTime, Registered,
    TimeRange, TrackId, TrackKind, register_audio_effect,
};

const R24: Rational = Rational::FPS_24;

fn register(id: &str, sidechain: bool) -> Registered {
    let threshold = EffectParamSpec {
        name: "threshold".into(),
        label: "Threshold".into(),
        kind: EffectParamKind::Scalar,
        default: -12.0,
        min: -60.0,
        max: 0.0,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
        ..Default::default()
    };
    register_audio_effect(AudioEffectSpec {
        id: id.into(),
        category: String::new(),
        label: id.into(),
        params: vec![threshold],
        latency: 32,
        sidechain,
    })
    .expect("a unique, well-formed audio effect")
    .0
}

/// An engine over a project with a clip of sound on an audio lane and a
/// second audio lane to key from.
struct Stage {
    engine: Engine,
    music: TrackId,
    voice: TrackId,
    clip: ClipId,
}

fn stage() -> Stage {
    let mut project = Project::new("inserts", R24);
    let music = project.add_track(TrackKind::Audio, "Music");
    let voice = project.add_track(TrackKind::Audio, "Voice");
    let score = project.add_media(MediaSource::new("/media/score.wav", 0, 0, R24, 480, true));
    let clip = project
        .add_clip(
            music,
            score,
            TimeRange::at_rate(0, 96, R24),
            RationalTime::new(0, R24),
        )
        .expect("a clip of sound");
    let engine = Engine::with_project(EngineConfig { undo_limit: 64 }, project).expect("engine");
    Stage {
        engine,
        music,
        voice,
        clip,
    }
}

fn chain(engine: &Engine, target: AudioInsertTarget) -> Vec<AudioEffectInstance> {
    engine
        .project()
        .audio_inserts(target)
        .expect("the chain exists")
        .to_vec()
}

fn expected_outcome(target: AudioInsertTarget) -> EditOutcome {
    match target {
        AudioInsertTarget::Clip(clip) => EditOutcome::Updated(clip),
        AudioInsertTarget::Track(lane) => EditOutcome::UpdatedTrack(lane),
        AudioInsertTarget::Master => EditOutcome::UpdatedMaster,
    }
}

/// Apply `command` to the chain at `target`, then check that undo returns the
/// chain to what it was and redo to what the command made, and that the step
/// is one history entry with the command's label.
fn round_trip(engine: &mut Engine, target: AudioInsertTarget, command: EditCommand) {
    let label = Command::Edit(command.clone()).label();
    let before = chain(engine, target);
    let depth = engine.history_depth();
    let outcome = engine
        .apply(Command::Edit(command.clone()))
        .unwrap_or_else(|err| panic!("{command:?} was refused: {err}"));
    assert_eq!(
        outcome,
        ApplyOutcome::Edited(expected_outcome(target)),
        "{command:?}"
    );
    let after = chain(engine, target);
    assert_ne!(after, before, "{command:?} changed nothing");
    assert_eq!(engine.history_depth(), depth + 1, "{command:?}");
    assert_eq!(
        engine
            .history_entries()
            .last()
            .map(|entry| entry.label.to_string()),
        Some(label.to_owned()),
        "{command:?}"
    );

    assert!(engine.undo(), "{command:?} did not undo");
    assert_eq!(chain(engine, target), before, "undo of {command:?}");
    assert!(engine.redo(), "{command:?} did not redo");
    assert_eq!(chain(engine, target), after, "redo of {command:?}");
}

#[test]
fn every_gesture_on_every_kind_of_chain_undoes_and_redoes_exactly() {
    let _plain = register("engine.plain", false);
    let _keyed = register("engine.keyed", true);
    let Stage {
        mut engine,
        music,
        voice,
        clip,
    } = stage();
    for target in [
        AudioInsertTarget::Clip(clip),
        AudioInsertTarget::Track(music),
        AudioInsertTarget::Master,
    ] {
        for effect_id in ["engine.plain", "engine.keyed"] {
            round_trip(
                &mut engine,
                target,
                EditCommand::AddAudioEffect {
                    target,
                    effect_id: effect_id.into(),
                },
            );
        }
        round_trip(
            &mut engine,
            target,
            EditCommand::SetAudioEffectParam {
                target,
                index: 0,
                param: 0,
                value: Param::Keyframed {
                    keyframes: vec![
                        Keyframe::new(0, -30.0, Easing::Linear),
                        Keyframe::new(48, -6.0, Easing::Linear),
                    ],
                },
            },
        );
        round_trip(
            &mut engine,
            target,
            EditCommand::SetAudioEffectBypass {
                target,
                index: 1,
                bypass: true,
            },
        );
        if target.clip().is_none() {
            round_trip(
                &mut engine,
                target,
                EditCommand::SetAudioEffectSidechain {
                    target,
                    index: 1,
                    key: Some(voice),
                },
            );
        }
        round_trip(
            &mut engine,
            target,
            EditCommand::MoveAudioEffect {
                target,
                from_index: 1,
                to_index: 0,
            },
        );
        let moved = chain(&engine, target);
        assert_eq!(moved[0].effect_id, "engine.keyed");
        assert!(moved[0].bypass);
        assert_eq!(moved[1].sample_param("threshold", 24.0), Some(-18.0));
        round_trip(
            &mut engine,
            target,
            EditCommand::RemoveAudioEffect { target, index: 1 },
        );
        assert_eq!(chain(&engine, target).len(), 1);
    }
}

#[test]
fn a_refused_insert_edit_changes_no_chain_and_records_no_step() {
    let _plain = register("engine.refused", false);
    let Stage {
        mut engine,
        music,
        voice,
        clip,
    } = stage();
    let lane = AudioInsertTarget::Track(music);
    for _ in 0..aria_models::STRIP_INSERTS {
        engine
            .apply(Command::Edit(EditCommand::AddAudioEffect {
                target: lane,
                effect_id: "engine.refused".into(),
            }))
            .expect("room on the strip");
    }
    let refusals = [
        EditCommand::AddAudioEffect {
            target: lane,
            effect_id: "engine.refused".into(),
        },
        EditCommand::AddAudioEffect {
            target: AudioInsertTarget::Master,
            effect_id: "engine.unregistered".into(),
        },
        EditCommand::AddAudioEffect {
            target: AudioInsertTarget::Clip(ClipId::from_raw(9_999)),
            effect_id: "engine.refused".into(),
        },
        EditCommand::RemoveAudioEffect {
            target: lane,
            index: aria_models::STRIP_INSERTS,
        },
        EditCommand::MoveAudioEffect {
            target: lane,
            from_index: 0,
            to_index: aria_models::STRIP_INSERTS,
        },
        EditCommand::SetAudioEffectParam {
            target: lane,
            index: 0,
            param: 0,
            value: Param::Constant(12.0),
        },
        EditCommand::SetAudioEffectParam {
            target: lane,
            index: 0,
            param: 1,
            value: Param::Constant(-6.0),
        },
        EditCommand::SetAudioEffectBypass {
            target: AudioInsertTarget::Clip(clip),
            index: 0,
            bypass: true,
        },
        EditCommand::SetAudioEffectSidechain {
            target: lane,
            index: 0,
            key: Some(voice),
        },
    ];
    let lanes = [
        lane,
        AudioInsertTarget::Clip(clip),
        AudioInsertTarget::Master,
    ];
    for command in refusals {
        let before: Vec<_> = lanes.iter().map(|target| chain(&engine, *target)).collect();
        let depth = engine.history_depth();
        assert!(
            engine.apply(Command::Edit(command.clone())).is_err(),
            "{command:?} was accepted"
        );
        let after: Vec<_> = lanes.iter().map(|target| chain(&engine, *target)).collect();
        assert_eq!(after, before, "{command:?} changed a chain");
        assert_eq!(engine.history_depth(), depth, "{command:?} recorded a step");
    }
}

/// An undo writes the chain back whole, so an insert whose processor was
/// unregistered after it was removed still comes back with its values.
#[test]
fn undo_restores_an_insert_whose_effect_is_no_longer_registered() {
    let guard = register("engine.gone", false);
    let Stage {
        mut engine, music, ..
    } = stage();
    let lane = AudioInsertTarget::Track(music);
    for command in [
        EditCommand::AddAudioEffect {
            target: lane,
            effect_id: "engine.gone".into(),
        },
        EditCommand::SetAudioEffectParam {
            target: lane,
            index: 0,
            param: 0,
            value: Param::Constant(-40.0),
        },
        EditCommand::RemoveAudioEffect {
            target: lane,
            index: 0,
        },
    ] {
        engine
            .apply(Command::Edit(command))
            .expect("an insert edit");
    }
    drop(guard);

    assert!(engine.undo());
    let back = chain(&engine, lane);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].effect_id, "engine.gone");
    assert_eq!(
        back[0].params.get("threshold"),
        Some(&Param::Constant(-40.0))
    );
    assert!(back[0].spec().is_none());
    assert!(engine.redo());
    assert!(chain(&engine, lane).is_empty());
}
