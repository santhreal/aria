//! What the assistant's app tools must guarantee.
//!
//! The class of defect these close: a host tool is an open vocabulary — anyone
//! can add one — and the loop dispatches on its name. So a new tool can shadow a
//! validated edit command, ship a schema the model cannot fill, skip
//! confirmation, or be advertised with no dispatch arm behind it. Every
//! assertion here is derived from
//! [`crate::app::assistant_host::host_tool_specs`] rather than a written list,
//! so adding a tool that breaks one of those rules turns this suite red instead
//! of shipping.
//!
//! Not covered: `media_screenshot_timeline` past its argument check, which needs
//! a GPU (the render itself is `aria-render`'s to test), and the provider loop
//! that decides *when* to call a tool, which is `aria-ai`'s.

use std::sync::atomic::{AtomicBool, Ordering};

use aria_ai::tools::{ToolHost, ToolTier};
use aria_plugin::PluginContext;
use aria_settings::Autonomy;

use super::AppState;
use super::assistant::Beat;
use super::assistant_host::{HostRequest, ShellHost, host_tool_specs};
use crate::types::AgentEntryKind;

fn state() -> AppState {
    let mut state =
        AppState::new(PluginContext::new(), crate::scratch_pool::scratch("host")).expect("state");
    state.new_project().expect("project");
    state
}

/// A host over a real session engine, plus the queue its requests arrive on.
fn host(state: &AppState, autonomy: Autonomy) -> (ShellHost, crate::handoff::Wakes<Beat>) {
    let (tx, rx) = crate::handoff::wakes();
    (
        ShellHost::new(
            std::sync::Arc::clone(&state.engine),
            tx,
            autonomy,
            None,
            PluginContext::clone(&state.ctx),
        ),
        rx,
    )
}

/// Every name the agent loop reserves for itself.
fn builtin_names() -> Vec<String> {
    let mut names: Vec<String> = aria_ai::wire::tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    names.push("describe_project".to_string());
    names.push("read_skill".to_string());
    names.push("commit_progress".to_string());
    names
}

#[test]
fn no_app_tool_can_shadow_a_validated_edit_command() {
    let builtins = builtin_names();
    for spec in host_tool_specs() {
        assert!(
            !builtins.contains(&spec.name),
            "{} collides with a built-in tool, so the loop would drop it silently",
            spec.name
        );
    }
}

#[test]
fn every_app_tool_is_namespaced() {
    for spec in host_tool_specs() {
        assert!(
            aria_ai::is_host_tool_name(&spec.name),
            "{} is not in the reserved host namespace, so it reads as an edit command",
            spec.name
        );
    }
}

#[test]
fn app_tool_names_are_unique() {
    let mut names: Vec<String> = host_tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(count, names.len(), "two app tools share a name");
}

/// A schema the model cannot fill is a tool the model cannot call. Every
/// required argument must exist in `properties`, and every tool must describe
/// itself.
#[test]
fn every_app_tool_ships_a_usable_schema() {
    for spec in host_tool_specs() {
        assert!(
            !spec.description.trim().is_empty(),
            "{} has no description",
            spec.name
        );
        assert_eq!(
            spec.parameters.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "{}'s parameters are not an object schema",
            spec.name
        );
        let properties = spec
            .parameters
            .get("properties")
            .and_then(|p| p.as_object())
            .unwrap_or_else(|| panic!("{} has no properties object", spec.name));
        let required = spec
            .parameters
            .get("required")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        for name in required {
            let name = name.as_str().expect("a required name is a string");
            assert!(
                properties.contains_key(name),
                "{} requires '{name}' but does not declare it",
                spec.name
            );
        }
    }
}

/// Every advertised tool must have a dispatch arm. Called with no arguments and
/// a stopped turn, a wired tool complains about its arguments or the stop; an
/// unwired one says it does not exist.
#[test]
fn every_advertised_app_tool_dispatches() {
    let state = state();
    let (mut host, _rx) = host(&state, Autonomy::Full);
    let cancel = AtomicBool::new(true);
    for spec in host_tool_specs() {
        let error = host
            .call(&spec.name, &serde_json::json!({}), &cancel)
            .expect_err("no tool succeeds with empty arguments and a stopped turn");
        assert!(
            !error.contains("unknown tool"),
            "{} is advertised with no dispatch arm: {error}",
            spec.name
        );
    }
}

#[test]
fn a_tool_that_was_never_advertised_is_refused_by_name() {
    let state = state();
    let (mut host, _rx) = host(&state, Autonomy::Full);
    let error = host
        .call(
            "app_delete_everything",
            &serde_json::json!({}),
            &AtomicBool::new(false),
        )
        .expect_err("an unknown tool is not run");
    assert!(error.contains("app_delete_everything"), "{error}");
}

#[test]
fn looking_and_workspace_work_never_asks() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Ask);
    let cancel = AtomicBool::new(false);
    for spec in host_tool_specs() {
        if spec.tier == ToolTier::System {
            continue;
        }
        host.authorize(&spec.name, &serde_json::json!({}), spec.tier, &cancel)
            .unwrap_or_else(|err| panic!("{} was gated: {err}", spec.name));
    }
    assert!(
        rx.try_next().is_none(),
        "a read-only or workspace tool asked the operator for permission"
    );
}

#[test]
fn a_destructive_tool_runs_only_after_you_allow_it() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Ask);
    let answer = std::thread::spawn(move || {
        let beat = pollster::block_on(rx.next()).expect("the host asks");
        let Beat::Ask(ask) = beat else {
            panic!("a destructive tool sent something other than a question");
        };
        assert!(
            ask.summary.contains("preview"),
            "the question does not say what would happen: {}",
            ask.summary
        );
        ask.reply.send(true).expect("the host is still waiting");
    });
    host.authorize(
        "system_clear_cache",
        &serde_json::json!({ "cache": "preview" }),
        ToolTier::System,
        &AtomicBool::new(false),
    )
    .expect("an allowed call is authorized");
    answer.join().expect("the answering thread");
}

#[test]
fn a_destructive_tool_refused_is_reported_to_the_model() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Ask);
    let answer = std::thread::spawn(move || {
        let Beat::Ask(ask) = pollster::block_on(rx.next()).expect("the host asks") else {
            panic!("expected a question");
        };
        ask.reply.send(false).expect("the host is still waiting");
    });
    let error = host
        .authorize(
            "system_clear_cache",
            &serde_json::json!({ "cache": "preview" }),
            ToolTier::System,
            &AtomicBool::new(false),
        )
        .expect_err("a refused call is not authorized");
    assert!(error.contains("did not allow"), "{error}");
    answer.join().expect("the answering thread");
}

/// A question nobody answers must not park the turn forever: the dropped reply
/// channel is a refusal, not a hang.
#[test]
fn a_dismissed_question_denies_the_call() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Ask);
    let answer = std::thread::spawn(move || {
        let Beat::Ask(ask) = pollster::block_on(rx.next()).expect("the host asks") else {
            panic!("expected a question");
        };
        drop(ask);
    });
    let error = host
        .authorize(
            "system_clear_cache",
            &serde_json::json!({ "cache": "preview" }),
            ToolTier::System,
            &AtomicBool::new(false),
        )
        .expect_err("an unanswered question is not an approval");
    assert!(error.contains("dismissed"), "{error}");
    answer.join().expect("the answering thread");
}

#[test]
fn stopping_the_turn_releases_a_waiting_question() {
    let state = state();
    let (mut host, _rx) = host(&state, Autonomy::Ask);
    let cancel = AtomicBool::new(true);
    let error = host
        .authorize(
            "system_clear_cache",
            &serde_json::json!({ "cache": "preview" }),
            ToolTier::System,
            &cancel,
        )
        .expect_err("a stopped turn does not wait for an answer");
    assert!(error.contains("stopped"), "{error}");
}

/// Full autonomy is the whole point of the setting: it must not ask.
#[test]
fn full_autonomy_runs_a_destructive_tool_without_asking() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Full);
    host.authorize(
        "system_clear_cache",
        &serde_json::json!({ "cache": "preview" }),
        ToolTier::System,
        &AtomicBool::new(false),
    )
    .expect("full autonomy authorizes");
    assert!(rx.try_next().is_none(), "full autonomy still asked");
}

#[test]
fn a_seek_request_moves_the_playhead() {
    let mut state = state();
    let detail = state
        .run_host_request(HostRequest::Seek { seconds: 0.0 })
        .expect("seek");
    assert!(detail.contains("playhead"), "{detail}");
    assert_eq!(state.playhead.value, 0);
}

#[test]
fn a_bad_cache_name_is_refused_and_names_itself() {
    let mut state = state();
    let error = state
        .run_host_request(HostRequest::ClearCache {
            key: "not-a-cache".into(),
        })
        .expect_err("an unknown cache is not cleared");
    assert!(error.contains("not-a-cache"), "{error}");
}

/// The model has to be told what happened, and the operator has to see it. One
/// applied request produces exactly one transcript row either way.
#[test]
fn an_applied_request_answers_the_model_and_shows_in_the_transcript() {
    let mut state = state();
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Apply {
        request: HostRequest::Seek { seconds: 0.0 },
        reply: tx,
    });
    let answer = rx.recv().expect("the host is answered");
    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(
        state
            .transcript
            .iter()
            .filter(|entry| entry.kind == AgentEntryKind::Action)
            .count(),
        1
    );
}

#[test]
fn a_failed_request_says_what_failed() {
    let mut state = state();
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Apply {
        request: HostRequest::ClearCache {
            key: "not-a-cache".into(),
        },
        reply: tx,
    });
    assert!(rx.recv().expect("answered").is_err());
    let row = state.transcript.last().expect("a row");
    assert_eq!(row.kind, AgentEntryKind::Action);
    assert!(row.content.contains("failed"), "{}", row.content);
    assert!(row.content.contains("not-a-cache"), "{}", row.content);
}

#[test]
fn an_answer_with_no_question_pending_does_nothing() {
    let mut state = state();
    state.answer_agent_ask(true);
    assert!(state.transcript.is_empty());
}

#[test]
fn ending_the_turn_drops_a_question_nobody_answered() {
    let mut state = state();
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Ask(super::assistant_host::PendingAsk {
        name: "system_clear_cache".into(),
        summary: "empty the preview cache".into(),
        reply: tx,
    }));
    assert!(state.assistant.pending_ask.is_some());
    state.finish_agent_turn(None);
    assert!(state.assistant.pending_ask.is_none());
    assert!(
        rx.recv().is_err(),
        "the reply channel outlived the turn, so the host would still be waiting"
    );
}

#[test]
fn stopping_the_turn_drops_a_question_nobody_answered() {
    let mut state = state();
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Ask(super::assistant_host::PendingAsk {
        name: "system_clear_cache".into(),
        summary: "empty the preview cache".into(),
        reply: tx,
    }));
    state.cancel_agent_prompt();
    assert!(state.assistant.pending_ask.is_none());
    assert!(rx.recv().is_err());
}

/// The Settings row is derived, not written: it must count the same tables the
/// turn uses, so it cannot describe a capability the build does not have.
#[test]
fn settings_counts_the_real_vocabularies() {
    let state = state();
    let line = state.agent_capabilities();
    assert!(
        line.contains(&format!("{} edit(s)", aria_ai::wire::tool_specs().len())),
        "{line}"
    );
    assert!(
        line.contains(&format!("{} app tool(s)", host_tool_specs().len())),
        "{line}"
    );
}

#[test]
fn settings_says_whether_a_destructive_call_will_ask() {
    let mut state = state();
    state.set_autonomy(Autonomy::Ask).expect("ask");
    assert!(state.agent_phases().contains("confirmation"));
    state.set_autonomy(Autonomy::Full).expect("full");
    assert!(state.agent_phases().contains("no confirmation"));
}

/// Ordering matters for the transcript: a question is only shown while it is
/// live, so the operator never clicks Allow on a call that already returned.
#[test]
fn a_question_is_shown_once_and_cleared_when_answered() {
    let mut state = state();
    let (tx, _rx) = crossbeam_channel::bounded(1);
    state.fold_beat(Beat::Ask(super::assistant_host::PendingAsk {
        name: "system_clear_cache".into(),
        summary: "empty the preview cache".into(),
        reply: tx,
    }));
    let asked = state
        .transcript
        .iter()
        .filter(|entry| entry.content.contains("waiting for you"))
        .count();
    assert_eq!(asked, 1);
    state.answer_agent_ask(false);
    assert!(state.assistant.pending_ask.is_none());
    assert!(
        state
            .transcript
            .last()
            .expect("a row")
            .content
            .contains("refused")
    );
    let _ = Ordering::Relaxed;
}
