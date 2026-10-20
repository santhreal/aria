//! The agent loop: prompt in, validated-and-applied command group out,
//! every step observable.
//!
//! The loop's whole world is the [`EngineBridge`] — it cannot name a file
//! path, a socket, or a UI type. One prompt = one history group: the
//! bridge's group markers wrap the run, failed individual commands are
//! reported back to the model (which may correct course), and the group
//! rolls back only when the prompt aborts (cancellation, provider error,
//! turn or host-call cap exceeded). Reaching the edit cap is gentler:
//! further edits are refused and the run ends keeping everything already
//! applied. A dry run applies too, into the same group, and rolls that group
//! back before it returns: each step is validated against the plan so far, and
//! the plan comes back for the UI's preview card with the ids the rehearsal
//! produced.
//!
//! Beyond edits, the embedder can wire a [`ToolHost`] of app tools, while
//! the bridge can expose strictly read-only senses of its exact project
//! state. The latter is what lets a model inspect edits inside rehearsal
//! without accidentally looking at the untouched live project. Both are
//! dispatched by exact name and charged against the host-call cap. The
//! built-in `commit_progress` tool records phase breaks so a long task's
//! live replay can land as several undo steps
//! ([`PromptOutcome::phase_breaks`]).

use std::sync::atomic::AtomicBool;

use crate::describe::{EditorContext, SummaryRequest};
use crate::extend::AgentExtensions;
use crate::provider::{
    ChatProvider, ChatRequest, FinishReason, ImagePart, Message, ProviderError, ProviderStreamEvent,
};
use crate::tools::{HostToolSpec, ToolHost, is_host_tool_name};
use crate::wire::{self, WireCommand};

mod bridge;
mod history;
mod prompt;
mod transcript;
mod types;

#[cfg(test)]
mod tests;

pub use bridge::{Applied, EngineBridge};
use history::{collect_turn_messages, enforce_image_budget, enforce_tool_output_image_budget};
pub use prompt::system_prompt;
use prompt::{HOST_TOOLS_RULES, commit_progress_spec, engine_sense_rules, read_skill_result};
pub use transcript::describe_action;
use transcript::host_action_summary;
pub use types::{ActionLogEntry, AgentConfig, AgentEvent, PromptOutcome, PromptStatus};

/// Run one prompt with only the validated edit vocabulary.
///
/// Kept as the compatibility/default entry point for embedders that do not
/// expose application tools. Use [`run_prompt_with_host`] to add senses,
/// app control, jobs, or other namespaced capabilities.
#[allow(clippy::too_many_arguments)]
pub fn run_prompt(
    provider: &dyn ChatProvider,
    bridge: &mut dyn EngineBridge,
    context: &EditorContext,
    extensions: &AgentExtensions,
    history: &[Message],
    prompt: &str,
    config: &AgentConfig,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(AgentEvent),
) -> PromptOutcome {
    run_prompt_with_host(
        provider,
        bridge,
        &mut crate::tools::NullToolHost,
        context,
        extensions,
        history,
        prompt,
        config,
        cancel,
        on_event,
    )
}

/// Run one prompt to completion against `bridge` and `host`.
///
/// `context` is the send-time editor snapshot (selection, playhead);
/// `history` is the prior conversation in this session (the caller's
/// accumulated `turn_messages`, with no system message — a fresh one is
/// regenerated here so the current project state always wins); `host` is
/// the embedder's tool surface (pass [`crate::tools::NullToolHost`] when
/// there is none); `on_event` receives streamed text and applied actions
/// for the UI. The returned [`PromptOutcome::turn_messages`] is this
/// turn's contribution to append.
#[allow(clippy::too_many_arguments)]
pub fn run_prompt_with_host(
    provider: &dyn ChatProvider,
    bridge: &mut dyn EngineBridge,
    host: &mut dyn ToolHost,
    context: &EditorContext,
    extensions: &AgentExtensions,
    history: &[Message],
    prompt: &str,
    config: &AgentConfig,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(AgentEvent),
) -> PromptOutcome {
    let summary = bridge.summary(SummaryRequest::Session {
        playhead_seconds: context.playhead_seconds,
    });
    let mut tools = wire::tool_specs();
    tools.push(wire::describe_project_spec());
    if !extensions.skills.is_empty() {
        tools.push(crate::extend::read_skill_spec());
    }
    tools.push(commit_progress_spec());
    // Built-in names always win: a colliding host spec is dropped here —
    // never sent, never dispatched — so a host can neither shadow the edit
    // vocabulary nor the loop's own tools. (`read_skill` stays reserved
    // even when no skills are loaded.) Deduplication is two short linear
    // scans; the `HashSet` this replaces had to own a copy of every tool
    // name just to answer a question over a handful of rows.
    let mut sense_specs: Vec<HostToolSpec> = Vec::new();
    for spec in bridge.sense_tools() {
        if !is_host_tool_name(&spec.name) || spec.tier != crate::tools::ToolTier::ReadOnly {
            continue;
        }
        if spec.name == "read_skill" || tools.iter().any(|tool| tool.name == spec.name) {
            continue;
        }
        if sense_specs.iter().any(|kept| kept.name == spec.name) {
            continue;
        }
        sense_specs.push(spec);
    }
    // The accepted specs are *moved* into the wire catalog rather than copied
    // into it: `parameters` is a whole JSON schema per tool. `tools` outlives
    // the loop, so the contiguous range each group lands in is what dispatch
    // routes on, and no second owned copy of any spec has to exist.
    let sense_start = tools.len();
    tools.extend(sense_specs.into_iter().map(|spec| wire::ToolSpec {
        name: spec.name,
        description: spec.description,
        parameters: spec.parameters,
    }));
    let sense_end = tools.len();

    let mut host_specs: Vec<HostToolSpec> = Vec::new();
    for spec in host.tools() {
        if !is_host_tool_name(&spec.name) {
            continue;
        }
        if tools.iter().any(|tool| tool.name == spec.name) {
            continue;
        }
        if host_specs.iter().any(|kept| kept.name == spec.name) {
            continue;
        }
        host_specs.push(spec);
    }
    // `ToolSpec` carries no tier, so the one field authorization still needs is
    // kept beside the catalog. `ToolTier` is `Copy`, so this is the whole cost
    // of not retaining the specs themselves.
    let host_tiers: Vec<crate::tools::ToolTier> = host_specs.iter().map(|spec| spec.tier).collect();
    let host_start = tools.len();
    tools.extend(host_specs.into_iter().map(|spec| wire::ToolSpec {
        name: spec.name,
        description: spec.description,
        parameters: spec.parameters,
    }));
    let host_end = tools.len();

    let mut system = system_prompt(&summary, context, extensions);
    if sense_start != sense_end || host_start != host_end {
        system.push_str(HOST_TOOLS_RULES);
    }
    if sense_start != sense_end {
        system.push_str(&engine_sense_rules(&tools[sense_start..sense_end]));
    }
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(Message::System { content: system });
    messages.extend_from_slice(history);
    // This turn's own messages start here (the user prompt and everything
    // the loop appends), kept so we can hand them back as `turn_messages`.
    let turn_start = messages.len();
    messages.push(Message::user(prompt));

    let mut actions: Vec<ActionLogEntry> = Vec::new();
    let mut phase_breaks: Vec<usize> = Vec::new();
    let mut edit_calls = 0usize;
    let mut host_calls = 0usize;
    // Set when an edit call lands past `max_tool_calls`: the run ends after
    // the current turn's calls are answered, keeping everything applied,
    // rather than burning turns until the turn cap rolls the prompt back.
    let mut edit_cap_tripped = false;
    let mut final_text = String::new();
    // The first image-bearing tool result is appended after the current user
    // message. Images are surfaced only after the whole request budget has run,
    // immediately before those exact attachments are sent to the provider.
    let mut image_event_cursor = messages.len();
    // Call ids of `describe_project` results, collapsed in `turn_messages`
    // so the session history never carries a full stale project blob.
    let mut describe_call_ids: Vec<String> = Vec::new();

    // A dry run opens a group too: it is what the rehearsal is undone through,
    // so the projection every step is validated against costs one rollback
    // rather than a second project instance.
    bridge.begin_group();
    let abort = |bridge: &mut dyn EngineBridge, actions: Vec<ActionLogEntry>, reason: String| {
        bridge.rollback_group();
        PromptOutcome {
            text: String::new(),
            actions,
            // Rolled back ⇒ no phases survive to group.
            phase_breaks: Vec::new(),
            status: PromptStatus::Aborted(reason),
            turn_messages: Vec::new(),
        }
    };

    for _turn in 0..config.max_turns {
        enforce_image_budget(&mut messages, config.max_images, config.max_image_bytes);
        for message in messages.iter().skip(image_event_cursor) {
            if let Message::ToolResult { images, .. } = message {
                for image in images {
                    on_event(AgentEvent::Image(image.clone()));
                }
            }
        }
        image_event_cursor = messages.len();
        let turn = {
            let mut forward = |event: ProviderStreamEvent<'_>| match event {
                ProviderStreamEvent::TextDelta(delta) => {
                    on_event(AgentEvent::TextDelta(delta.to_string()));
                }
                ProviderStreamEvent::ReasoningSummaryDelta(delta) => {
                    on_event(AgentEvent::ReasoningDelta(delta.to_string()));
                }
            };
            match provider.chat(
                &ChatRequest {
                    messages: &messages,
                    tools: &tools,
                },
                cancel,
                &mut forward,
            ) {
                Ok(turn) => turn,
                Err(ProviderError::Cancelled) => {
                    return abort(bridge, actions, "cancelled".to_string());
                }
                Err(e) => return abort(bridge, actions, e.to_string()),
            }
        };

        if turn.tool_calls.is_empty() {
            final_text = turn.text;
            if turn.finish == FinishReason::Length {
                return abort(
                    bridge,
                    actions,
                    "the model ran out of tokens mid-answer".to_string(),
                );
            }
            break;
        }

        let tool_calls = turn.tool_calls.clone();
        messages.push(Message::Assistant {
            content: turn.text,
            tool_calls: turn.tool_calls,
        });

        for call in tool_calls {
            // Only host successes attach images; every other path is text.
            let mut images: Vec<ImagePart> = Vec::new();
            let result: String = if call.name == "describe_project" {
                describe_call_ids.push(call.id.clone());
                let request = crate::describe::requested(&call.arguments, context.playhead_seconds);
                let state = serde_json::json!({
                    "project": bridge.summary(request),
                    "editor": context,
                });
                state.to_string()
            } else if call.name == "read_skill" && !extensions.skills.is_empty() {
                // Read-only like describe_project: answered from the
                // preloaded skill set, no dispatch, no edit-cap charge.
                read_skill_result(&extensions.skills, &call.arguments)
            } else if call.name == "commit_progress" {
                // Free (charges neither cap): marking a phase must never
                // compete with the work it delimits.
                let committed = phase_breaks.last().copied().unwrap_or(0);
                if actions.len() > committed {
                    phase_breaks.push(actions.len());
                    format!(
                        "ok: committed phase {} ({} edits)",
                        phase_breaks.len(),
                        actions.len() - committed
                    )
                } else {
                    // No break recorded — an empty phase would replay as an
                    // empty undo group.
                    "nothing new to commit — make edits first".to_string()
                }
            } else if tools[sense_start..sense_end]
                .iter()
                .any(|spec| spec.name == call.name)
            {
                host_calls += 1;
                if host_calls > config.max_host_calls {
                    return abort(
                        bridge,
                        actions,
                        format!(
                            "exceeded the {}-host-call cap for one prompt",
                            config.max_host_calls
                        ),
                    );
                }
                match bridge.sense(&call.name, &call.arguments, cancel) {
                    Err(reason) => format!("rejected: {reason}"),
                    Ok(output) => {
                        let mut content = if output.text.is_empty() {
                            "ok".to_string()
                        } else {
                            output.text
                        };
                        images = output.images;
                        enforce_tool_output_image_budget(
                            &mut content,
                            &mut images,
                            config.max_images,
                            config.max_image_bytes,
                        );
                        on_event(AgentEvent::HostAction {
                            name: call.name,
                            summary: host_action_summary(&content),
                        });
                        content
                    }
                }
            } else if let Some(tier_index) = tools[host_start..host_end]
                .iter()
                .position(|spec| spec.name == call.name)
            {
                host_calls += 1;
                if host_calls > config.max_host_calls {
                    return abort(
                        bridge,
                        actions,
                        format!(
                            "exceeded the {}-host-call cap for one prompt",
                            config.max_host_calls
                        ),
                    );
                }
                match bridge.before_host_call(&call.name, &call.arguments) {
                    Err(reason) => format!("rejected: {reason}"),
                    Ok(()) => {
                        match host.authorize(
                            &call.name,
                            &call.arguments,
                            host_tiers[tier_index],
                            cancel,
                        ) {
                            Err(reason) => format!("rejected: {reason}"),
                            Ok(()) => {
                                let host_result = host.call(&call.name, &call.arguments, cancel);
                                let borrowed_result = match &host_result {
                                    Ok(output) => Ok(output),
                                    Err(reason) => Err(reason.as_str()),
                                };
                                if let Err(reason) = bridge.after_host_call(
                                    &call.name,
                                    &call.arguments,
                                    borrowed_result,
                                ) {
                                    return abort(
                                        bridge,
                                        actions,
                                        format!(
                                            "host tool '{}' was dispatched, but reconciliation \
                                             failed: {reason}; host effects may already have \
                                             occurred",
                                            call.name
                                        ),
                                    );
                                }
                                match host_result {
                                    Err(reason) => format!("rejected: {reason}"),
                                    Ok(output) => {
                                        let mut content = if output.text.is_empty() {
                                            "ok".to_string()
                                        } else {
                                            output.text
                                        };
                                        images = output.images;
                                        enforce_tool_output_image_budget(
                                            &mut content,
                                            &mut images,
                                            config.max_images,
                                            config.max_image_bytes,
                                        );
                                        on_event(AgentEvent::HostAction {
                                            name: call.name,
                                            summary: host_action_summary(&content),
                                        });
                                        content
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                edit_calls += 1;
                if edit_calls > config.max_tool_calls {
                    // The runaway fuse is not a failure: edits already applied
                    // stay, this and further edit calls are refused, and the
                    // run ends after this turn's calls are answered.
                    edit_cap_tripped = true;
                    messages.push(Message::ToolResult {
                        call_id: call.id,
                        content: format!(
                            "rejected: reached the {}-edit cap for one prompt; the edits \
                             already applied are kept",
                            config.max_tool_calls
                        ),
                        images,
                    });
                    continue;
                }
                match WireCommand::from_tool_call(&call.name, call.arguments) {
                    Err(reason) => format!("rejected: {reason}"),
                    Ok(command) => {
                        let applied = if config.dry_run {
                            bridge
                                .rehearse(&command)
                                .map(|outcome| outcome.map(Applied::Edited))
                        } else {
                            bridge.apply(&command).map(Some)
                        };
                        match applied {
                            Err(reason) => format!("rejected: {reason}"),
                            Ok(outcome) => {
                                // A queued render reports the shell's own line:
                                // it names what started, where the generic
                                // description would read as work already done.
                                let description = match outcome.as_ref() {
                                    Some(Applied::Queued(started)) => started.clone(),
                                    other => {
                                        describe_action(&command, other.and_then(Applied::edited))
                                    }
                                };
                                // Formatted before the entry takes ownership of
                                // the description, so the log entry and the
                                // model-facing result share one allocation
                                // instead of copying the string between them.
                                let result = if config.dry_run {
                                    format!("validated (dry run, not yet applied): {description}")
                                } else {
                                    format!("ok: {description}")
                                };
                                let entry = ActionLogEntry {
                                    command,
                                    description,
                                    outcome: outcome.as_ref().and_then(Applied::edited).copied(),
                                };
                                on_event(AgentEvent::Action(entry.clone()));
                                actions.push(entry);
                                result
                            }
                        }
                    }
                }
            };
            messages.push(Message::ToolResult {
                call_id: call.id,
                content: result,
                images,
            });
        }

        if edit_cap_tripped {
            final_text = format!(
                "Reached the {}-edit cap for one prompt; kept the {} edit{} already applied.",
                config.max_tool_calls,
                actions.len(),
                if actions.len() == 1 { "" } else { "s" }
            );
            break;
        }

        if _turn + 1 == config.max_turns {
            return abort(
                bridge,
                actions,
                format!("exceeded the {}-turn cap for one prompt", config.max_turns),
            );
        }
    }

    let turn_messages =
        collect_turn_messages(messages, turn_start, &describe_call_ids, &final_text);
    if config.dry_run {
        // The plan leaves, the projection does not: every step the rehearsal
        // applied is undone here, so Apply starts from the project still on
        // screen.
        bridge.rollback_group();
        return PromptOutcome {
            text: final_text,
            actions,
            phase_breaks,
            status: PromptStatus::DryRun,
            turn_messages,
        };
    }
    bridge.end_group();
    PromptOutcome {
        text: final_text,
        actions,
        phase_breaks,
        status: PromptStatus::Completed,
        turn_messages,
    }
}
