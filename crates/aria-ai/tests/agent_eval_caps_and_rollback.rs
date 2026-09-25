//! `agent_eval` tests covering caps and rollback. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `agent_eval` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn model_corrects_course_after_a_rejection() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "remove_clip",
            serde_json::json!({ "clip": 999 }),
        )]),
        tool_turn(vec![(
            "call_2",
            "remove_clip",
            serde_json::json!({ "clip": clip }),
        )]),
        text_turn("Removed the clip."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "delete the clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1, "only the corrected call applied");
    assert_eq!(host.engine.project().timeline().clip_count(), 0);

    // The rejection went back to the model as the tool result, naming the
    // ids that do exist.
    let second_request = &provider.requests()[1];
    let last = second_request.last().unwrap();
    match last {
        Message::ToolResult {
            call_id, content, ..
        } => {
            assert_eq!(call_id, "call_1");
            assert!(
                content.contains("rejected: clip 999 does not exist"),
                "{content}"
            );
            assert!(content.contains(&clip.to_string()), "{content}");
        }
        other => panic!("expected tool result, got {other:?}"),
    }
}

#[test]
fn cap_trip_keeps_the_edits_already_applied() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![tool_turn(vec![
        (
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        ),
        (
            "call_2",
            "add_track",
            serde_json::json!({ "kind": "text", "name": "Titles" }),
        ),
    ])]);

    let config = AgentConfig {
        max_tool_calls: 1,
        ..Default::default()
    };
    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "go wild",
        &config,
    );

    // The cap does not fail the prompt: the run completes keeping the
    // in-cap edits, refusing the rest.
    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(outcome.text.contains("1-edit cap"), "{}", outcome.text);
    assert_eq!(outcome.actions.len(), 1, "only the in-cap edit applied");
    assert_eq!(host.engine.project().timeline().clip_count(), 2);
    assert_eq!(
        host.engine.project().timeline().track_count(),
        1,
        "the over-cap add_track was refused"
    );
    // The kept edits are one undoable history entry, like any prompt.
    assert!(host.engine.undo());
    assert_eq!(host.engine.project().timeline().clip_count(), 1);
    assert!(!host.engine.undo(), "exactly one history entry per prompt");
}

#[test]
fn reasoning_summaries_stream_across_tool_rounds_without_entering_history() {
    let (mut host, _, _, clip) = fixture();
    let first_summary = "I should inspect the current timeline.";
    let second_summary = "The selected clip can be split safely.";
    let final_summary = "The edit succeeded, so I can answer.";
    let provider = ScriptedProvider::new(vec![
        ChatTurn {
            text: String::new(),
            reasoning_summary: first_summary.into(),
            tool_calls: vec![ToolCall {
                id: "inspect".into(),
                name: "describe_project".into(),
                arguments: serde_json::json!({}),
            }],
            finish: FinishReason::ToolCalls,
        },
        ChatTurn {
            text: String::new(),
            reasoning_summary: second_summary.into(),
            tool_calls: vec![ToolCall {
                id: "edit".into(),
                name: "split_clip".into(),
                arguments: serde_json::json!({"clip": clip, "at": 5.0}),
            }],
            finish: FinishReason::ToolCalls,
        },
        ChatTurn {
            text: "Split the clip at 5 seconds.".into(),
            reasoning_summary: final_summary.into(),
            tool_calls: Vec::new(),
            finish: FinishReason::Stop,
        },
    ]);

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "split the clip after inspecting it",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(host.engine.project().timeline().clip_count(), 2);
    // A provider hands text over in pieces, so the contract is what the deltas
    // concatenate to and in which order, never one event per summary.
    let summaries = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ReasoningDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(
        summaries,
        format!("{first_summary}{second_summary}{final_summary}")
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Action(_)))
    );
    let answer = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TextDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(answer, "Split the clip at 5 seconds.");

    let requests = provider.requests();
    for message in requests
        .iter()
        .flatten()
        .chain(outcome.turn_messages.iter())
    {
        let content = match message {
            Message::System { content }
            | Message::User { content, .. }
            | Message::Assistant { content, .. }
            | Message::ToolResult { content, .. } => content,
        };
        for summary in [first_summary, second_summary, final_summary] {
            assert!(
                !content.contains(summary),
                "reasoning summary leaked into model history: {content}"
            );
        }
    }
}

#[test]
fn provider_failure_mid_prompt_rolls_back() {
    let (mut host, _, _, clip) = fixture();
    // One successful edit turn, then the script runs dry — which the loop
    // sees as a provider error on the next turn.
    let provider = ScriptedProvider::new(vec![tool_turn(vec![(
        "call_1",
        "split_clip",
        serde_json::json!({ "clip": clip, "at": 5.0 }),
    )])]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "split the clip",
        &AgentConfig::default(),
    );

    assert!(matches!(outcome.status, PromptStatus::Aborted(_)));
    assert_eq!(
        host.engine.project().timeline().clip_count(),
        1,
        "the applied split must be rolled back"
    );
    assert!(!host.engine.undo());
}

/// Multi-turn memory: the first prompt's `turn_messages` carry the whole
/// turn, and threading them into the next prompt puts the prior dialogue
/// into the request — behind a freshly regenerated system prompt — so the
/// model can answer "what did you just do?".
#[test]
fn session_history_threads_prior_turns_into_the_next_prompt() {
    let (mut host, _media, _track, clip) = fixture();
    let ctx = EditorContext {
        selected_clips: vec![clip],
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);

    let first = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 4.0 }),
        )]),
        text_turn("Split the clip at 4.00s into two."),
    ]);
    let outcome1 = run_prompt_with_host(
        &first,
        &mut host,
        &mut NullToolHost,
        &ctx,
        &aria_ai::AgentExtensions::default(),
        &[],
        "split the selected clip in half",
        &AgentConfig::default(),
        &cancel,
        &mut |_| {},
    );
    assert_eq!(outcome1.status, PromptStatus::Completed);

    // The turn carries: the user prompt, the assistant's tool call, the
    // tool result, and the final text answer.
    let kinds: Vec<&str> = outcome1.turn_messages.iter().map(message_kind).collect();
    assert_eq!(kinds, ["user", "assistant", "tool", "assistant"]);

    let second = ScriptedProvider::new(vec![text_turn("I split it into two clips.")]);
    let _ = run_prompt_with_host(
        &second,
        &mut host,
        &mut NullToolHost,
        &ctx,
        &aria_ai::AgentExtensions::default(),
        &outcome1.turn_messages,
        "what did you just do?",
        &AgentConfig::default(),
        &cancel,
        &mut |_| {},
    );

    let sent = second.requests();
    assert_eq!(sent.len(), 1, "the second prompt is one provider call");
    let convo = &sent[0];
    assert!(
        matches!(convo[0], Message::System { .. }),
        "a fresh system prompt leads every request"
    );
    assert!(
        convo.iter().any(|m| matches!(
            m,
            Message::User { content, .. } if content == "split the selected clip in half"
        )),
        "the prior user turn is remembered"
    );
    assert!(
        matches!(
            convo.last().unwrap(),
            Message::User { content, .. } if content == "what did you just do?"
        ),
        "the newest user message comes last"
    );
}

/// `commit_progress` records phase breaks between edits, refuses empty
/// phases, and charges neither the edit fuse nor the host cap.
#[test]
fn commit_progress_records_phase_breaks_without_charging_caps() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        // Nothing to commit yet.
        tool_turn(vec![("c0", "commit_progress", serde_json::json!({}))]),
        tool_turn(vec![
            ("c1", "add_marker", serde_json::json!({ "at": 1.0 })),
            ("c2", "add_marker", serde_json::json!({ "at": 2.0 })),
        ]),
        // First commit lands; the immediate second one is empty.
        tool_turn(vec![
            ("c3", "commit_progress", serde_json::json!({})),
            ("c4", "commit_progress", serde_json::json!({})),
        ]),
        tool_turn(vec![("c5", "add_marker", serde_json::json!({ "at": 3.0 }))]),
        text_turn("Three markers, two phases."),
    ]);

    // Exactly three edit slots and zero host slots: commits must charge
    // neither cap for this to complete.
    let config = AgentConfig {
        max_tool_calls: 3,
        max_host_calls: 0,
        ..Default::default()
    };
    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "mark the beats in phases",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 3);
    assert_eq!(outcome.phase_breaks, vec![2]);

    let results: Vec<(String, String)> = provider
        .requests()
        .iter()
        .flat_map(|msgs| msgs.iter())
        .filter_map(|m| match m {
            Message::ToolResult {
                call_id, content, ..
            } => Some((call_id.clone(), content.clone())),
            _ => None,
        })
        .collect();
    let result_for = |id: &str| {
        results
            .iter()
            .find(|(call_id, _)| call_id == id)
            .map(|(_, content)| content.as_str())
            .unwrap_or_else(|| panic!("no result for {id}"))
    };
    assert_eq!(result_for("c0"), "nothing new to commit — make edits first");
    assert_eq!(result_for("c3"), "ok: committed phase 1 (2 edits)");
    assert_eq!(result_for("c4"), "nothing new to commit — make edits first");
}
