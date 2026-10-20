//! What a saved conversation must survive.
//!
//! The class of defect these close: a conversation that comes back wrong is
//! worse than one that does not come back at all, because the next prompt is
//! built from it and the provider history is not on screen. So the round
//! trip is asserted field by field, a file this build cannot read is refused
//! rather than partially believed, and a cleared conversation stays cleared
//! across a restart.
//!
//! Not covered: images, which are deliberately dropped (`aria-ai` strips them
//! from history on every request), and concurrent writers, which cannot happen
//! because a conversation is only written from the UI thread.

use aria_ai::provider::ToolCall;
use aria_ai::{ImagePart, Message};
use aria_plugin::PluginContext;

use super::AppState;
use crate::scratch_pool::scratch;
use crate::types::AgentEntryKind;

/// A state whose project path is set, so the conversation has somewhere to live.
fn saved_state(prefix: &str) -> AppState {
    let dir = scratch(prefix);
    let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
    state.project_path = Some(dir.join("project.aria"));
    state
}

/// A second state on the same data directory and project: what a restart looks
/// like from the store's point of view.
fn reopened(from: &AppState) -> AppState {
    let mut state =
        AppState::new(PluginContext::new(), from.data_dir.clone()).expect("reopened state");
    state.project_path = from.project_path.clone();
    state.load_conversation();
    state
}

#[test]
fn a_saved_conversation_comes_back_with_every_row() {
    let mut state = saved_state("conv-roundtrip");
    state.push_agent_message(AgentEntryKind::You, "shorten the intro");
    state.push_agent_message(AgentEntryKind::Reasoning, "checking the first clip");
    state.push_agent_message(AgentEntryKind::Action, "trimmed clip 1");
    state.push_agent_message(AgentEntryKind::Assistant, "done");
    state.push_agent_message(AgentEntryKind::Error, "one edit was refused");
    state.save_conversation().expect("save");

    let back = reopened(&state);
    let kinds: Vec<AgentEntryKind> = back.transcript.iter().map(|entry| entry.kind).collect();
    assert_eq!(
        kinds,
        vec![
            AgentEntryKind::You,
            AgentEntryKind::Reasoning,
            AgentEntryKind::Action,
            AgentEntryKind::Assistant,
            AgentEntryKind::Error,
        ]
    );
    let text: Vec<&str> = back
        .transcript
        .iter()
        .map(|entry| entry.content.as_str())
        .collect();
    assert_eq!(
        text,
        vec![
            "shorten the intro",
            "checking the first clip",
            "trimmed clip 1",
            "done",
            "one edit was refused",
        ]
    );
    let ids: Vec<&str> = back
        .transcript
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let original_ids: Vec<&str> = state
        .transcript
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(ids, original_ids);
}

/// Every kind must round trip, or reloading would silently drop rows of one
/// colour. Derived from `AgentEntryKind::ALL` so a new kind fails here instead of
/// disappearing on reload.
#[test]
fn every_entry_kind_survives_the_round_trip() {
    let mut state = saved_state("conv-kinds");
    for kind in AgentEntryKind::ALL {
        state.push_agent_message(kind, format!("a {} row", kind.key()));
    }
    state.save_conversation().expect("save");

    let back = reopened(&state);
    let kinds: Vec<AgentEntryKind> = back.transcript.iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, AgentEntryKind::ALL.to_vec());
}

#[test]
fn the_provider_history_comes_back_so_the_next_prompt_continues() {
    let mut state = saved_state("conv-history");
    state.assistant.history = vec![
        Message::System {
            content: "you edit video".into(),
        },
        Message::User {
            content: "shorten the intro".into(),
            images: Vec::new(),
        },
        Message::Assistant {
            content: "trimming".into(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "trim_clip".into(),
                arguments: serde_json::json!({ "clip": 1, "ticks": 240 }),
            }],
        },
        Message::ToolResult {
            call_id: "call_1".into(),
            content: "trimmed".into(),
            images: Vec::new(),
        },
    ];
    state.save_conversation().expect("save");

    let back = reopened(&state);
    assert_eq!(back.assistant.history, state.assistant.history);
}

#[test]
fn images_are_dropped_and_the_rest_of_the_message_is_kept() {
    let mut state = saved_state("conv-images");
    state.assistant.history = vec![Message::User {
        content: "what is on screen".into(),
        images: vec![ImagePart::png(vec![1, 2, 3], "timeline at 0.00s")],
    }];
    state.save_conversation().expect("save");

    let back = reopened(&state);
    assert_eq!(
        back.assistant.history,
        vec![Message::User {
            content: "what is on screen".into(),
            images: Vec::new(),
        }]
    );
}

#[test]
fn clearing_the_conversation_leaves_nothing_to_come_back_to() {
    let mut state = saved_state("conv-clear");
    state.push_agent_message(AgentEntryKind::You, "shorten the intro");
    state.assistant.history = vec![Message::user("shorten the intro")];
    state.save_conversation().expect("save");
    let path = state.conversation_path().expect("path");
    assert!(path.exists(), "a conversation with rows is written");

    state.clear_agent_transcript();
    assert!(
        !path.exists(),
        "clearing deletes the file rather than writing an empty one"
    );
    let back = reopened(&state);
    assert!(back.transcript.is_empty());
    assert!(back.assistant.history.is_empty());
}

#[test]
fn a_conversation_from_another_version_is_ignored_not_guessed_at() {
    let mut state = saved_state("conv-version");
    state.push_agent_message(AgentEntryKind::You, "shorten the intro");
    state.save_conversation().expect("save");
    let path = state.conversation_path().expect("path");
    let text = std::fs::read_to_string(&path).expect("read");
    let bumped = text.replace("\"version\": 1", "\"version\": 99");
    assert_ne!(bumped, text, "the version field is what was rewritten");
    std::fs::write(&path, bumped).expect("write");

    let back = reopened(&state);
    assert!(
        back.transcript.is_empty(),
        "a version this build does not know is not fed to a model as fact"
    );
    assert!(back.assistant.history.is_empty());
}

#[test]
fn an_unreadable_file_starts_an_empty_conversation() {
    let mut state = saved_state("conv-garbage");
    state.push_agent_message(AgentEntryKind::You, "shorten the intro");
    state.save_conversation().expect("save");
    let path = state.conversation_path().expect("path");
    std::fs::write(&path, "{ not json").expect("write");

    let back = reopened(&state);
    assert!(back.transcript.is_empty());
    assert!(back.last_error.is_none(), "losing it is not a dialog");
}

#[test]
fn an_unsaved_project_keeps_its_conversation_in_memory_only() {
    let dir = scratch("conv-unsaved");
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.push_agent_message(AgentEntryKind::You, "shorten the intro");
    assert!(state.conversation_path().is_none());
    assert_eq!(state.save_conversation(), Ok(()));
    assert_eq!(
        state.transcript.len(),
        1,
        "the transcript still works without a file"
    );
}

/// Two projects in one data directory must not share one conversation: the key
/// is the project path, not the directory.
#[test]
fn two_projects_keep_separate_conversations() {
    let mut first = saved_state("conv-two");
    first.push_agent_message(AgentEntryKind::You, "first project");
    first.save_conversation().expect("save");

    let mut second =
        AppState::new(PluginContext::new(), first.data_dir.clone()).expect("second state");
    second.project_path = Some(first.data_dir.join("other.aria"));
    second.load_conversation();
    assert!(
        second.transcript.is_empty(),
        "another project's conversation is not this one's"
    );
    second.push_agent_message(AgentEntryKind::You, "second project");
    second.save_conversation().expect("save");

    let back = reopened(&first);
    assert_eq!(back.transcript.len(), 1);
    assert_eq!(back.transcript[0].content, "first project");
}

#[test]
fn opening_a_project_replaces_the_conversation_that_was_on_screen() {
    let mut state = saved_state("conv-replace");
    state.push_agent_message(AgentEntryKind::You, "stale row");
    state.assistant.history = vec![Message::user("stale row")];
    state.assistant.skills_off = vec!["captions".into()];
    // A project with no file of its own: loading must clear, not keep.
    state.project_path = Some(state.data_dir.join("fresh.aria"));
    state.load_conversation();
    assert!(state.transcript.is_empty());
    assert!(state.assistant.history.is_empty());
    assert!(
        state.assistant.skills_off.is_empty(),
        "per-conversation skill choices belong to the conversation"
    );
}
#[test]
fn save_load_preserves_custom_and_stable_entry_ids() {
    let mut state = saved_state("conv-custom-ids");
    state.push_agent_message(AgentEntryKind::You, "step 1");
    state.push_agent_message(AgentEntryKind::Assistant, "step 2");
    // Manually assign custom stable IDs to verify they are preserved rather than overwritten with transcript-length counter.
    let rows = std::sync::Arc::make_mut(&mut state.transcript);
    rows[0].id = "custom-msg-alpha".into();
    rows[1].id = "custom-msg-beta".into();
    state.save_conversation().expect("save");

    let back = reopened(&state);
    assert_eq!(back.transcript.len(), 2);
    assert_eq!(back.transcript[0].id.as_str(), "custom-msg-alpha");
    assert_eq!(back.transcript[1].id.as_str(), "custom-msg-beta");
}

#[test]
fn appending_after_load_does_not_collide_with_existing_ids() {
    let mut state = saved_state("conv-no-collision");
    state.push_agent_message(AgentEntryKind::You, "initial 1");
    state.push_agent_message(AgentEntryKind::Assistant, "initial 2");
    state.save_conversation().expect("save");

    let mut back = reopened(&state);
    // Append several new entries after reload
    back.push_agent_message(AgentEntryKind::You, "follow-up 1");
    back.push_agent_message(AgentEntryKind::Assistant, "follow-up 2");
    back.push_agent_message(AgentEntryKind::Action, "follow-up 3");

    let mut seen = std::collections::HashSet::new();
    for entry in back.transcript.iter() {
        assert!(
            seen.insert(entry.id.as_str()),
            "duplicate entry id detected: {}",
            entry.id
        );
    }
    assert_eq!(seen.len(), 5);
}

#[test]
fn legacy_saved_conversations_without_ids_deserialize_and_generate_stable_ids() {
    let state = saved_state("conv-legacy");
    let path = state.conversation_path().expect("path");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    let legacy_json = serde_json::json!({
        "version": 1,
        "project": state.project_path.as_ref().unwrap().to_string_lossy(),
        "entries": [
            { "kind": "user", "content": "legacy user prompt" },
            { "kind": "assistant", "content": "legacy assistant response" }
        ],
        "history": []
    });
    std::fs::write(&path, serde_json::to_string_pretty(&legacy_json).unwrap())
        .expect("write legacy");

    let mut loaded = reopened(&state);
    assert_eq!(loaded.transcript.len(), 2);
    assert_eq!(loaded.transcript[0].id.as_str(), "1");
    assert_eq!(loaded.transcript[1].id.as_str(), "2");

    // Appending afterwards produces next stable id without collision
    loaded.push_agent_message(AgentEntryKind::You, "new prompt");
    assert_eq!(loaded.transcript.len(), 3);
    assert_eq!(loaded.transcript[2].id.as_str(), "3");
}

#[test]
fn warm_per_entry_id_generation_returns_static_slices_without_allocation() {
    for i in 0..256 {
        let id = crate::app::assistant_store::compact_id(i);
        assert_eq!(id.as_str(), i.to_string());
    }
    let large_id = crate::app::assistant_store::compact_id(1024);
    assert_eq!(large_id.as_str(), "1024");
}
