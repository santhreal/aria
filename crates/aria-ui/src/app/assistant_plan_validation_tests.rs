//! A plan is validated against the plan so far, and replays onto the ids the
//! replay creates.
//!
//! The class this closes: a dry run that validates every step against the
//! project as it stands refuses steps Apply would accept. `add_effect` then
//! `set_effect_param` on the same clip refused the second call as an effect
//! index that is not there, and the same held for any step naming a clip, track
//! or marker an earlier step creates. The operator reads that as the plan being
//! impossible, when the plan was fine and only the rehearsal was blind.
//!
//! Two halves, because a rehearsal has two. It applies each step into a group so
//! the next one is validated the way Apply will run it, then rolls that group
//! back. What it created is gone with it, so Apply maps the rehearsal's id onto
//! the entity the replay creates in its place.
//!
//! What this does not catch: the provider. The turns are scripted, so what a
//! model would do with a refusal is `aria-ai`'s coverage, not this file's.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aria_ai::agent::{AgentConfig, AgentEvent, PromptOutcome, PromptStatus, run_prompt};
use aria_ai::provider::{ChatTurn, FinishReason, ToolCall};
use aria_ai::providers::ScriptedProvider;
use aria_ai::wire::{
    AddEffect, AddMarker, AddTrack, DuplicateClip, SetEffectParam, SetMarker, SplitClip,
    WireTrackKind,
};
use aria_ai::{AgentExtensions, EditorContext, WireCommand};
use aria_commands::EditOutcome;
use aria_models::{ClipId, MarkerId, TrackId};

use super::{Beat, PlanStep, SessionBridge};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;

/// An id the rehearsal handed out and the rollback took away. Far above
/// anything a fixture allocates, so a step that was never remapped names
/// nothing rather than something.
const REHEARSED: u64 = 900_001;

fn tool_turn(calls: Vec<(&str, &str, serde_json::Value)>) -> ChatTurn {
    ChatTurn {
        text: String::new(),
        reasoning_summary: String::new(),
        tool_calls: calls
            .into_iter()
            .map(|(id, name, arguments)| ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments,
            })
            .collect(),
        finish: FinishReason::ToolCalls,
    }
}

fn text_turn(text: &str) -> ChatTurn {
    ChatTurn {
        text: text.to_string(),
        reasoning_summary: String::new(),
        tool_calls: Vec::new(),
        finish: FinishReason::Stop,
    }
}

/// Run one prompt as a dry run over this state's engine, through the same
/// bridge the shipped turn uses.
fn rehearse(state: &AppState, provider: &ScriptedProvider) -> PromptOutcome {
    let (beats, _wakes) = crate::handoff::wakes::<Beat>();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut bridge = SessionBridge {
        engine: Arc::clone(state.engine()),
        beats,
        cancel: Arc::clone(&cancel),
    };
    let config = AgentConfig {
        dry_run: true,
        ..AgentConfig::default()
    };
    let mut seen = |_: AgentEvent| {};
    run_prompt(
        provider,
        &mut bridge,
        &EditorContext::default(),
        &AgentExtensions::default(),
        &[],
        "plan it",
        &config,
        &cancel,
        &mut seen,
    )
}

fn effect_count(state: &AppState, clip: ClipId) -> usize {
    state
        .engine()
        .lock()
        .project()
        .clip(clip)
        .expect("the clip is still there")
        .effects
        .len()
}

/// The rehearsal's own contract: a step is validated against the plan so far,
/// and the projection it was validated against does not survive the turn.
#[test]
fn a_dry_run_validates_a_step_against_the_plan_so_far() {
    let (state, clip) = with_clip();
    let raw = clip.raw();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "add_effect",
                serde_json::json!({ "clip": raw, "effect": "gaussian_blur" }),
            ),
            (
                "call_2",
                "set_effect_param",
                serde_json::json!({ "clip": raw, "index": 0, "param": "radius", "value": 8.0 }),
            ),
        ]),
        text_turn("Planned a blur and its radius."),
    ]);

    let outcome = rehearse(&state, &provider);

    assert_eq!(outcome.status, PromptStatus::DryRun);
    assert_eq!(
        outcome.actions.len(),
        2,
        "the second step was refused against a project the first step had changed"
    );
    assert_eq!(
        outcome.actions[1].command,
        WireCommand::SetEffectParam(SetEffectParam {
            clip: raw,
            index: 0,
            param: "radius".into(),
            value: Some(8.0),
            position: None,
            rgba: None,
        })
    );
    assert_eq!(
        effect_count(&state, clip),
        0,
        "the rehearsal left its projection in the project"
    );
}

/// A step naming a clip an earlier step created replays onto the clip the
/// replay created.
#[test]
fn a_planned_clip_is_remapped_on_apply() {
    let (mut state, clip) = with_clip();
    state.assistant.plan = vec![
        PlanStep {
            description: "split it".into(),
            command: WireCommand::SplitClip(SplitClip {
                clip: clip.raw(),
                at: 1.0,
            }),
            outcome: Some(EditOutcome::Created(ClipId::from_raw(REHEARSED))),
        },
        PlanStep {
            description: "blur the second half".into(),
            command: WireCommand::AddEffect(AddEffect {
                clip: REHEARSED,
                effect: "gaussian_blur".into(),
            }),
            outcome: None,
        },
    ]
    .into();

    assert_eq!(state.apply_agent_plan().expect("the plan applies"), 2);

    let engine = state.engine().lock();
    let timeline = engine.project().timeline();
    let carried: Vec<u64> = timeline
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .filter(|placed| !placed.effects.is_empty())
        .map(|placed| placed.id.raw())
        .collect();
    assert_eq!(carried.len(), 1, "the effect landed on more than one clip");
    assert_ne!(
        carried[0],
        clip.raw(),
        "the effect landed on the clip that was split, not on the half the plan named"
    );
    assert_ne!(carried[0], REHEARSED, "a rehearsal id survived the replay");
}

/// A step naming a track an earlier step created replays onto the track the
/// replay created.
#[test]
fn a_planned_track_is_remapped_on_apply() {
    let (mut state, clip) = with_clip();
    state.assistant.plan = vec![
        PlanStep {
            description: "add a lane".into(),
            command: WireCommand::AddTrack(AddTrack {
                kind: WireTrackKind::Video,
                name: "V2".into(),
                index: None,
            }),
            outcome: Some(EditOutcome::CreatedTrack(TrackId::from_raw(REHEARSED))),
        },
        PlanStep {
            description: "copy the clip onto it".into(),
            command: WireCommand::DuplicateClip(DuplicateClip {
                clip: clip.raw(),
                to_track: REHEARSED,
                start: 5.0,
            }),
            outcome: None,
        },
    ]
    .into();

    assert_eq!(state.apply_agent_plan().expect("the plan applies"), 2);

    let engine = state.engine().lock();
    let timeline = engine.project().timeline();
    let lane = timeline
        .tracks_ordered()
        .find(|track| track.name == "V2")
        .expect("the plan's lane");
    assert_eq!(
        lane.clips().count(),
        1,
        "the copy did not land on the lane the plan created"
    );
}

/// A step naming a marker an earlier step created replays onto the marker the
/// replay created.
#[test]
fn a_planned_marker_is_remapped_on_apply() {
    let (mut state, _clip) = with_clip();
    state.assistant.plan = vec![
        PlanStep {
            description: "flag the drop".into(),
            command: WireCommand::AddMarker(AddMarker {
                at: 1.0,
                name: Some("drop".into()),
                color: None,
            }),
            outcome: Some(EditOutcome::CreatedMarker(MarkerId::from_raw(REHEARSED))),
        },
        PlanStep {
            description: "rename it".into(),
            command: WireCommand::SetMarker(SetMarker {
                marker: REHEARSED,
                at: None,
                name: Some("cue".into()),
                color: None,
                comment: None,
            }),
            outcome: None,
        },
    ]
    .into();

    assert_eq!(state.apply_agent_plan().expect("the plan applies"), 2);

    let engine = state.engine().lock();
    let markers = engine.project().timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(
        markers[0].name, "cue",
        "the rename went to a marker the replay never created"
    );
}
