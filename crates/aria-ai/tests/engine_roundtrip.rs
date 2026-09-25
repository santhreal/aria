//! Wire commands → validation → a real `Engine`: the full Phase 1 path.
//!
//! One engine instance for the whole scenario (engine construction spins a
//! headless GPU context). Media is a pool entry with a synthetic path —
//! edit commands never decode, so no file is needed.

use aria_ai::wire::{self, WireCommand, WireGenerator};
use aria_ai::{Lowered, summarize, validate};
use aria_commands::{Command, EditOutcome};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AnimationRef, AudioRole, ClipParam, Easing, Generator, LinkId, MediaSource, ParamValue,
    Project, Rational, RationalTime, TimeRange, TrackKind,
};

const R24: Rational = Rational::FPS_24;

fn engine_with(project: Project) -> Engine {
    let config = EngineConfig { undo_limit: 64 };
    Engine::with_project(config, project).expect("engine")
}

fn apply(engine: &mut Engine, command: WireCommand) -> ApplyOutcome {
    let lowered = match validate(&command, engine.project()) {
        Ok(Lowered::Edit(lowered)) => lowered,
        Ok(other) => panic!("{command:?} is not an edit: {other:?}"),
        Err(r) => panic!("{command:?} rejected: {r}"),
    };
    engine
        .apply(lowered)
        .unwrap_or_else(|e| panic!("{command:?} failed in engine: {e}"))
}

fn created_clip(outcome: ApplyOutcome) -> u64 {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id.raw(),
        other => panic!("expected created clip, got {other:?}"),
    }
}

fn created_track(outcome: ApplyOutcome) -> u64 {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => id.raw(),
        other => panic!("expected created track, got {other:?}"),
    }
}

#[path = "engine_roundtrip_scenario_and_undo.rs"]
mod scenario_and_undo;

#[path = "engine_roundtrip_groups_and_rejections.rs"]
mod groups_and_rejections;
