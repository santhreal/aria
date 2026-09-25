//! The eval harness: scripted prompts against a real engine, zero network.
//!
//! Each test scripts the provider's turns, runs the full agent loop
//! through an `EngineBridge` backed by a real `Engine`, and asserts on
//! the final timeline, the action log, and the undo history. This is how
//! agent regressions get caught in CI without a live model.

use std::sync::atomic::AtomicBool;

use aria_ai::agent::{
    AgentConfig, AgentEvent, Applied, EngineBridge, PromptStatus, run_prompt_with_host,
};
use aria_ai::provider::{ChatTurn, FinishReason, ImagePart, Message, ToolCall};
use aria_ai::providers::ScriptedProvider;
use aria_ai::tools::{HostToolSpec, NullToolHost, ToolHost, ToolOutput, ToolTier};
use aria_ai::{
    EditorContext, Lowered, ProjectSummary, QueuedWork, SummaryRequest, WireCommand, summarize,
    summarize_for, validate,
};
use aria_commands::EditOutcome;
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    Generator, LinkId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind,
};

const R24: Rational = Rational::FPS_24;

/// A real engine behind the loop's bridge.
struct EngineHost {
    engine: Engine,
    sense_specs: Vec<HostToolSpec>,
    sense_outputs: std::collections::VecDeque<Result<ToolOutput, String>>,
    sense_calls: Vec<(String, serde_json::Value)>,
    sense_clip_counts: Vec<usize>,
    before_host_outputs: std::collections::VecDeque<Result<(), String>>,
    after_host_outputs: std::collections::VecDeque<Result<(), String>>,
    before_host_calls: Vec<(String, serde_json::Value)>,
    after_host_calls: Vec<(String, serde_json::Value, bool)>,
    /// Work the loop handed over instead of applying, in the order it arrived.
    /// A shell would start each one; this records that it was asked to.
    queued: Vec<QueuedWork>,
}

impl EngineHost {
    fn new(project: Project) -> Self {
        let config = EngineConfig { undo_limit: 64 };
        Self {
            engine: Engine::with_project(config, project).expect("engine"),
            sense_specs: Vec::new(),
            sense_outputs: std::collections::VecDeque::new(),
            sense_calls: Vec::new(),
            sense_clip_counts: Vec::new(),
            before_host_outputs: std::collections::VecDeque::new(),
            after_host_outputs: std::collections::VecDeque::new(),
            before_host_calls: Vec::new(),
            after_host_calls: Vec::new(),
            queued: Vec::new(),
        }
    }
}

impl EngineBridge for EngineHost {
    fn summary(&mut self, request: SummaryRequest) -> ProjectSummary {
        summarize_for(self.engine.project(), request)
    }

    fn sense_tools(&self) -> Vec<HostToolSpec> {
        self.sense_specs.clone()
    }

    fn sense(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        _cancel: &AtomicBool,
    ) -> Result<ToolOutput, String> {
        self.sense_clip_counts
            .push(self.engine.project().timeline().clip_count());
        self.sense_calls.push((name.to_string(), arguments.clone()));
        self.sense_outputs
            .pop_front()
            .unwrap_or_else(|| Err("scripted engine sense ran out of outputs".into()))
    }

    fn before_host_call(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        self.before_host_calls
            .push((name.to_string(), arguments.clone()));
        self.before_host_outputs.pop_front().unwrap_or(Ok(()))
    }

    fn after_host_call(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        result: Result<&ToolOutput, &str>,
    ) -> Result<(), String> {
        self.after_host_calls
            .push((name.to_string(), arguments.clone(), result.is_ok()));
        self.after_host_outputs.pop_front().unwrap_or(Ok(()))
    }

    fn apply(&mut self, command: &WireCommand) -> Result<Applied, String> {
        match validate(command, self.engine.project()).map_err(|r| r.message)? {
            Lowered::Edit(lowered) => match self.engine.apply(lowered) {
                Ok(ApplyOutcome::Edited(outcome)) => Ok(Applied::Edited(outcome)),
                Ok(other) => Err(format!("unexpected engine outcome: {other:?}")),
                Err(e) => Err(e.to_string()),
            },
            // A shell would start the work here. This one records the ask and
            // answers the way a shell does: with what started, not with what
            // finished.
            Lowered::Queued(work) => {
                self.queued.push(work);
                let QueuedWork::RenderAndReplace { clip } = work;
                Ok(Applied::Queued(format!(
                    "started rendering clip {}; it replaces the clip when the render finishes",
                    clip.raw()
                )))
            }
        }
    }

    fn rehearse(&mut self, command: &WireCommand) -> Result<Option<EditOutcome>, String> {
        match validate(command, self.engine.project()).map_err(|r| r.message)? {
            Lowered::Edit(lowered) => match self.engine.apply(lowered) {
                Ok(ApplyOutcome::Edited(outcome)) => Ok(Some(outcome)),
                Ok(other) => Err(format!("unexpected engine outcome: {other:?}")),
                Err(e) => Err(e.to_string()),
            },
            // A rehearsal starts no work, so the ask is not recorded either:
            // this bridge's `queued` is what a shell was told to do.
            Lowered::Queued(_) => Ok(None),
        }
    }

    fn begin_group(&mut self) {
        self.engine.begin_group();
    }

    fn end_group(&mut self) {
        self.engine.commit_group();
    }

    fn rollback_group(&mut self) {
        self.engine.rollback_group();
    }
}

/// 24 fps project, one video track, one 10 s clip (of a 60 s source) at 0 s.
/// Built directly on the `Project` so the engine starts with empty history.
fn fixture() -> (EngineHost, u64, u64, u64) {
    let mut project = Project::new("eval", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/eval.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    (
        EngineHost::new(project),
        media.raw(),
        track.raw(),
        clip.raw(),
    )
}

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

fn run_with(
    provider: &dyn aria_ai::provider::ChatProvider,
    host: &mut EngineHost,
    tool_host: &mut dyn ToolHost,
    context: &EditorContext,
    prompt: &str,
    config: &AgentConfig,
) -> (aria_ai::PromptOutcome, Vec<AgentEvent>) {
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let outcome = run_prompt_with_host(
        provider,
        host,
        tool_host,
        context,
        &aria_ai::AgentExtensions::default(),
        &[],
        prompt,
        config,
        &cancel,
        &mut |e| events.push(e),
    );
    (outcome, events)
}

fn run(
    provider: &ScriptedProvider,
    host: &mut EngineHost,
    context: &EditorContext,
    prompt: &str,
    config: &AgentConfig,
) -> (aria_ai::PromptOutcome, Vec<AgentEvent>) {
    run_with(provider, host, &mut NullToolHost, context, prompt, config)
}

/// Scripted [`ToolHost`] double: canned outputs in call order, every
/// call recorded for assertions.
struct ScriptedHost {
    specs: Vec<HostToolSpec>,
    outputs: std::collections::VecDeque<Result<ToolOutput, String>>,
    authorizations: Vec<(String, serde_json::Value, ToolTier)>,
    calls: Vec<(String, serde_json::Value)>,
}

impl ScriptedHost {
    fn new(specs: Vec<HostToolSpec>, outputs: Vec<Result<ToolOutput, String>>) -> Self {
        Self {
            specs,
            outputs: outputs.into(),
            authorizations: Vec::new(),
            calls: Vec::new(),
        }
    }
}

impl ToolHost for ScriptedHost {
    fn tools(&self) -> Vec<HostToolSpec> {
        self.specs.clone()
    }

    fn authorize(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        tier: ToolTier,
        _cancel: &AtomicBool,
    ) -> Result<(), String> {
        self.authorizations
            .push((name.to_string(), arguments.clone(), tier));
        match tier {
            ToolTier::ReadOnly | ToolTier::Workspace => Ok(()),
            ToolTier::System => Err(format!(
                "system tool '{name}' requires confirmation, but this host has no approval broker"
            )),
        }
    }

    fn call(
        &mut self,
        name: &str,
        arguments: &serde_json::Value,
        _cancel: &AtomicBool,
    ) -> Result<ToolOutput, String> {
        self.calls.push((name.to_string(), arguments.clone()));
        self.outputs
            .pop_front()
            .unwrap_or_else(|| Err("scripted host ran out of outputs".into()))
    }
}

fn host_spec(name: &'static str) -> HostToolSpec {
    HostToolSpec {
        name: name.into(),
        description: "test tool".into(),
        parameters: serde_json::json!({ "type": "object", "properties": {} }),
        tier: ToolTier::ReadOnly,
    }
}

/// A model simulator: creates a text track, reads the new track's id out
/// of the tool result (the way a real model does), then places the title
/// on it. Static scripts can't thread runtime ids; this can.
struct TitleAddingModel;

impl aria_ai::provider::ChatProvider for TitleAddingModel {
    fn chat(
        &self,
        request: &aria_ai::provider::ChatRequest<'_>,
        _cancel: &AtomicBool,
        _on_event: &mut dyn FnMut(aria_ai::provider::ProviderStreamEvent<'_>),
    ) -> Result<ChatTurn, aria_ai::provider::ProviderError> {
        let last = request.messages.last().unwrap();
        Ok(match last {
            Message::User { .. } => tool_turn(vec![(
                "call_1",
                "add_track",
                serde_json::json!({ "kind": "text", "name": "Titles" }),
            )]),
            Message::ToolResult {
                call_id, content, ..
            } if call_id == "call_1" => {
                // "ok: added text track 'Titles' (track 42)"
                let id: u64 = content
                    .rsplit("(track ")
                    .next()
                    .and_then(|s| s.trim_end_matches(')').parse().ok())
                    .expect("track id in tool result");
                tool_turn(vec![(
                    "call_2",
                    "add_generated",
                    serde_json::json!({
                        "track": id,
                        "generator": { "type": "text", "content": "INTRO" },
                        "start": 0.0,
                        "duration": 3.0,
                    }),
                )])
            }
            _ => text_turn("Added the INTRO title."),
        })
    }
}

/// Creates the required audio lane first, then threads that runtime track id
/// into the explicit-target extraction tool call.
struct AudioExtractingModel {
    clip: u64,
}

impl aria_ai::provider::ChatProvider for AudioExtractingModel {
    fn chat(
        &self,
        request: &aria_ai::provider::ChatRequest<'_>,
        _cancel: &AtomicBool,
        _on_event: &mut dyn FnMut(aria_ai::provider::ProviderStreamEvent<'_>),
    ) -> Result<ChatTurn, aria_ai::provider::ProviderError> {
        let last = request.messages.last().unwrap();
        Ok(match last {
            Message::User { .. } => tool_turn(vec![(
                "extract_track",
                "add_track",
                serde_json::json!({ "kind": "audio", "name": "Extracted" }),
            )]),
            Message::ToolResult {
                call_id, content, ..
            } if call_id == "extract_track" => {
                let track: u64 = content
                    .rsplit("(track ")
                    .next()
                    .and_then(|text| text.trim_end_matches(')').parse().ok())
                    .expect("created track id in tool result");
                tool_turn(vec![(
                    "extract_clip",
                    "extract_audio",
                    serde_json::json!({ "clip": self.clip, "track": track }),
                )])
            }
            _ => text_turn("Extracted the clip's audio."),
        })
    }
}

fn message_kind(m: &Message) -> &'static str {
    match m {
        Message::System { .. } => "system",
        Message::User { .. } => "user",
        Message::Assistant { .. } => "assistant",
        Message::ToolResult { .. } => "tool",
    }
}

#[path = "agent_eval_host_tools_1.rs"]
mod host_tools_1;

#[path = "agent_eval_host_tools_2.rs"]
mod host_tools_2;

#[path = "agent_eval_senses.rs"]
mod senses;

#[path = "agent_eval_keyframes_and_effects_1.rs"]
mod keyframes_and_effects_1;

#[path = "agent_eval_keyframes_and_effects_2.rs"]
mod keyframes_and_effects_2;

#[path = "agent_eval_caps_and_rollback.rs"]
mod caps_and_rollback;

#[path = "agent_eval_prompt_edits_1.rs"]
mod prompt_edits_1;

#[path = "agent_eval_prompt_edits_2.rs"]
mod prompt_edits_2;

#[path = "agent_eval_stream.rs"]
mod stream;

#[path = "agent_eval_queued_work.rs"]
mod queued_work;
