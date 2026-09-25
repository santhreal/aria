//! The assistant's own rules: what refuses a prompt, how a stream becomes a
//! transcript, and what a finished turn remembers.
//!
//! No provider is reached here and none needs to be. Everything asserted is a
//! decision this crate makes on its own — the refusal order, which bubble a
//! delta joins, whether an interrupted turn is remembered — and each one used to
//! be reachable only by watching a real model answer.
//!
//! What is deliberately not covered: the provider call itself, and the job
//! thread. Both belong to `aria-ai` and `aria-jobs`, which test them; a test
//! here would only prove that a mock returns what the mock was told to.

use super::AppState;
use super::assistant::{Beat, Finished, PlanStep};
use crate::types::AgentEntryKind;
use aria_ai::Message;
use aria_ai::agent::PromptStatus;
use aria_ai::wire::{AddMarker, WireCommand};
use aria_plugin::PluginContext;
fn state() -> AppState {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("assistant"),
    )
    .expect("state");
    state.new_project().expect("project");
    state
}
fn finished(text: &str, status: PromptStatus, edits: usize) -> Finished {
    Finished {
        text: text.to_string(),
        status,
        messages: vec![Message::User {
            content: "hello".into(),
            images: Vec::new(),
        }],
        edits,
        plan: Vec::new(),
    }
}
fn kinds(state: &AppState) -> Vec<AgentEntryKind> {
    state.transcript.iter().map(|entry| entry.kind).collect()
}
/// Two markers as a plan: enough to prove a plan is applied whole, and that a
/// second step is not skipped.
fn marker_plan() -> Vec<PlanStep> {
    [1.0_f64, 2.0]
        .into_iter()
        .map(|at| PlanStep {
            description: format!("add marker at {at:.2}s"),
            command: WireCommand::AddMarker(AddMarker {
                at,
                name: Some(format!("m{at}")),
                color: None,
            }),
            // The plan names no id an earlier step made, so there is nothing to
            // rewrite when it replays.
            outcome: None,
        })
        .collect()
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "assistant_tests_conversation.rs"]
mod assistant_tests_conversation;
#[path = "assistant_tests_turn.rs"]
mod assistant_tests_turn;
