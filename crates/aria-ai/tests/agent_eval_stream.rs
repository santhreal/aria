//! `agent_eval` tests covering the streaming contract: an answer reaches the
//! panel in pieces while the turn is still running, and a turn stopped mid-answer
//! leaves the project where it started. A `#[path]` child of the `agent_eval`
//! test binary, so `use super::*` reaches its fixtures unchanged.
//!
//! The provider double streams in [`aria_ai::providers::ScriptedProvider`]-sized
//! chunks and checks the flag before each one, which is the only way a test can
//! land a cancellation mid-answer rather than between turns.

use super::*;

use std::path::Path;
use std::sync::atomic::Ordering;

/// Long enough to arrive as several deltas.
const ANSWER: &str = "Split the clip at five seconds, then trimmed the tail so the \
                      sequence ends on the beat.";

fn text_deltas(events: &[AgentEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TextDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

/// Runs the loop with a cancel flag the caller's `watch` may set, so a test can
/// stop the turn on a chosen delta.
fn run_cancelling(
    provider: &ScriptedProvider,
    host: &mut EngineHost,
    prompt: &str,
    watch: &mut dyn FnMut(&AgentEvent, &AtomicBool),
) -> (aria_ai::PromptOutcome, Vec<AgentEvent>) {
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let outcome = run_prompt_with_host(
        provider,
        host,
        &mut NullToolHost,
        &EditorContext::default(),
        &aria_ai::AgentExtensions::default(),
        &[],
        prompt,
        &AgentConfig::default(),
        &cancel,
        &mut |event| {
            watch(&event, &cancel);
            events.push(event);
        },
    );
    (outcome, events)
}

#[test]
fn an_answer_streams_in_pieces_that_concatenate_to_the_text() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn(ANSWER)]);

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "what did you do",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    let deltas = text_deltas(&events);
    assert!(
        deltas.len() > 1,
        "a long answer arrived as one event, so nothing streamed: {deltas:?}"
    );
    assert!(
        deltas.iter().all(|delta| !delta.is_empty()),
        "an empty delta makes the panel notify for nothing: {deltas:?}"
    );
    assert!(
        deltas.iter().all(|delta| *delta != ANSWER),
        "one delta carried the whole answer: {deltas:?}"
    );
    assert_eq!(deltas.concat(), ANSWER);
    assert_eq!(outcome.text, ANSWER);
}

#[test]
fn cancelling_mid_stream_stops_the_deltas() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn(ANSWER)]);

    let mut seen = 0_usize;
    let (outcome, events) = run_cancelling(
        &provider,
        &mut host,
        "answer at length",
        &mut |event, cancel| {
            if matches!(event, AgentEvent::TextDelta(_)) {
                seen += 1;
                if seen == 2 {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        },
    );

    assert_eq!(
        outcome.status,
        PromptStatus::Aborted("cancelled".to_string())
    );
    let deltas = text_deltas(&events);
    assert_eq!(
        deltas.len(),
        2,
        "the stream ran past the cancellation: {deltas:?}"
    );
    assert!(
        !deltas.concat().is_empty() && ANSWER.starts_with(&deltas.concat()),
        "the partial answer is not a prefix of the scripted one: {deltas:?}"
    );
    assert!(
        outcome.text.is_empty(),
        "a stopped turn reported an answer it never finished: {}",
        outcome.text
    );
    assert!(
        outcome.turn_messages.is_empty(),
        "a stopped turn contributed to history"
    );
}

#[test]
fn cancelling_mid_stream_rolls_back_the_edits_the_turn_had_applied() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        )]),
        text_turn(ANSWER),
    ]);

    let (outcome, events) = run_cancelling(
        &provider,
        &mut host,
        "split it and tell me about it",
        &mut |event, cancel| {
            if matches!(event, AgentEvent::TextDelta(_)) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    );

    assert_eq!(
        outcome.status,
        PromptStatus::Aborted("cancelled".to_string())
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Action(_))),
        "the split was never reported, so this is not the mid-turn case"
    );
    assert!(
        outcome
            .actions
            .iter()
            .any(|action| action.description.contains("split")),
        "the action log lost the edit it rolled back: {:?}",
        outcome.actions
    );
    assert_eq!(
        host.engine.project().timeline().clip_count(),
        1,
        "the split survived a cancelled turn"
    );
    assert!(
        !host.engine.undo(),
        "a rolled-back turn left an undo entry to press"
    );
}

/// Cancellation is a flag, not a thread kill: a `chat` that never reads it runs
/// to completion after Stop is pressed, and the panel keeps printing
/// an answer nobody is waiting for. Derived from the source, so a provider file
/// joining the crate with an ignored flag fails this instead of shipping.
#[test]
fn every_shipped_provider_reads_the_cancellation_flag_while_streaming() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = Vec::new();
    let mut offenders = Vec::new();
    let mut pending = vec![src.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read the crate source") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read a source file");
            if !text.contains("impl ChatProvider for") {
                continue;
            }
            let name = path
                .strip_prefix(&src)
                .unwrap_or(&path)
                .display()
                .to_string();
            checked.push(name.clone());
            // Bound the flag under its own name, then either read it or hand it
            // to whoever does — a delegating provider passes it down.
            let bound = text.contains("cancel: &AtomicBool");
            let used = text.contains("cancel.load(") || text.contains(", cancel,");
            if !bound || !used {
                offenders.push(name);
            }
        }
    }
    assert!(
        !checked.is_empty(),
        "no provider was found to check, so this rule guards nothing"
    );
    assert!(
        offenders.is_empty(),
        "these providers ignore cancellation: {offenders:?} (checked {checked:?})"
    );
}
