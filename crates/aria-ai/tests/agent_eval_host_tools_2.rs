//! `agent_eval` tests covering host tools (part 2). A `#[path]` child of the
//! `agent_eval` test binary; the tests reach their fixtures through `super::*`.

use super::*;

#[test]
fn app_controls_use_fresh_state_for_a_looped_playback_workflow() {
    let (mut bridge, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![("state", "app_state", serde_json::json!({}))]),
        tool_turn(vec![
            (
                "loop",
                "app_loop_range",
                serde_json::json!({
                    "action": "set",
                    "start_seconds": 115.0,
                    "end_seconds": 120.0
                }),
            ),
            (
                "panel",
                "app_panel",
                serde_json::json!({ "panel": "library", "action": "hide" }),
            ),
            ("theme", "app_theme", serde_json::json!({ "theme": "blue" })),
            (
                "play",
                "app_playback",
                serde_json::json!({ "action": "play" }),
            ),
        ]),
        text_turn("Playing the last five seconds on a loop with the library hidden."),
    ]);
    let specs = [
        "app_state",
        "app_loop_range",
        "app_panel",
        "app_theme",
        "app_playback",
    ]
    .into_iter()
    .map(host_spec)
    .collect();
    let mut tool_host = ScriptedHost::new(
        specs,
        vec![
            Ok(ToolOutput::text(
                r#"{"sequence":{"duration_seconds":120.0}}"#,
            )),
            Ok(ToolOutput::text(
                r#"{"state":{"transport":{"loop_enabled":true,"range_in_seconds":115.0,"range_out_seconds":120.0}}}"#,
            )),
            Ok(ToolOutput::text(
                r#"{"state":{"panels":{"library":false}}}"#,
            )),
            Ok(ToolOutput::text(r#"{"state":{"theme":"blue"}}"#)),
            Ok(ToolOutput::text(
                r#"{"state":{"transport":{"playing":true}}}"#,
            )),
        ],
    );

    let (outcome, events) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "play the last 5 seconds looped, hide the library, and use the dark theme",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(outcome.actions.is_empty());
    assert_eq!(
        tool_host.calls,
        vec![
            ("app_state".into(), serde_json::json!({})),
            (
                "app_loop_range".into(),
                serde_json::json!({
                    "action": "set",
                    "start_seconds": 115.0,
                    "end_seconds": 120.0
                }),
            ),
            (
                "app_panel".into(),
                serde_json::json!({ "panel": "library", "action": "hide" }),
            ),
            ("app_theme".into(), serde_json::json!({ "theme": "blue" }),),
            (
                "app_playback".into(),
                serde_json::json!({ "action": "play" }),
            ),
        ]
    );
    let host_actions = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::HostAction { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        host_actions,
        vec![
            "app_state",
            "app_loop_range",
            "app_panel",
            "app_theme",
            "app_playback"
        ]
    );
    let requests = provider.requests();
    let final_tool_results = requests[2]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id, content, ..
            } if call_id != "state" => Some((call_id.as_str(), content.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(final_tool_results.len(), 4);
    assert!(
        final_tool_results
            .iter()
            .any(|(id, content)| *id == "play" && content.contains("\"playing\":true"))
    );
}

#[test]
fn host_rejection_reports_and_the_prompt_still_completes() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "media_screenshot_preview",
            serde_json::json!({}),
        )]),
        text_turn("Couldn't grab the preview."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_screenshot_preview")],
        vec![Err("preview is not rendered yet".into())],
    );

    let (outcome, events) = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "look at the preview",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    let second = &provider.requests()[1];
    match second.last().unwrap() {
        Message::ToolResult {
            content, images, ..
        } => {
            assert_eq!(content, "rejected: preview is not rendered yet");
            assert!(images.is_empty());
        }
        other => panic!("expected tool result, got {other:?}"),
    }
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::HostAction { .. })),
        "a rejected call is not an action"
    );
}

#[test]
fn system_host_tools_fail_closed_without_an_approval_broker() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "system_cache_clear",
            serde_json::json!({ "cache": "proxies" }),
        )]),
        text_turn("I couldn't clear it without approval."),
    ]);
    let mut spec = host_spec("system_cache_clear");
    spec.tier = ToolTier::System;
    let mut tool_host = ScriptedHost::new(vec![spec], vec![Ok(ToolOutput::text("cache cleared"))]);

    let (outcome, _) = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "clear the proxy cache",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(
        tool_host.calls.is_empty(),
        "execution never runs before authorization"
    );
    let second = &provider.requests()[1];
    match second.last().unwrap() {
        Message::ToolResult { content, .. } => {
            assert!(content.contains("requires confirmation"), "{content}");
            assert!(content.contains("no approval broker"), "{content}");
        }
        other => panic!("expected tool result, got {other:?}"),
    }
}

/// The two fuses are independent: an edit cap of zero still lets host
/// tools run, and the host cap aborts with a full rollback.
#[test]
fn host_calls_have_their_own_cap_separate_from_the_edit_fuse() {
    // Edit cap 0, one host call: completes.
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "media_screenshot_preview",
            serde_json::json!({}),
        )]),
        text_turn("Looked."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_screenshot_preview")],
        vec![Ok(ToolOutput::text("looks fine"))],
    );
    let config = AgentConfig {
        max_tool_calls: 0,
        ..Default::default()
    };
    let (outcome, _) = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "look at the preview",
        &config,
    );
    assert_eq!(outcome.status, PromptStatus::Completed);

    // Host cap 1, one applied edit then two host calls: the second host
    // call trips the cap and the whole prompt rolls back.
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        )]),
        tool_turn(vec![
            ("call_2", "media_screenshot_preview", serde_json::json!({})),
            ("call_3", "media_screenshot_preview", serde_json::json!({})),
        ]),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_screenshot_preview")],
        vec![Ok(ToolOutput::text("looks fine"))],
    );
    let config = AgentConfig {
        max_host_calls: 1,
        ..Default::default()
    };
    let (outcome, _) = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "split then stare at the preview forever",
        &config,
    );
    match &outcome.status {
        PromptStatus::Aborted(reason) => assert!(reason.contains("1-host-call cap"), "{reason}"),
        other => panic!("expected abort, got {other:?}"),
    }
    assert_eq!(tool_host.calls.len(), 1, "the cap trips before the call");
    assert_eq!(
        host.engine.project().timeline().clip_count(),
        1,
        "the applied split was rolled back"
    );
    assert!(!host.engine.undo(), "an aborted prompt leaves no history");
}

/// Defense in depth: a host spec reusing a built-in name is neither
/// offered to the provider nor dispatched — the built-in wins.
#[test]
fn host_tool_name_collisions_lose_to_the_built_ins() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        )]),
        tool_turn(vec![("call_2", "describe_project", serde_json::json!({}))]),
        text_turn("Split it."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![
            host_spec("split_clip"),
            host_spec("describe_project"),
            host_spec("app_ping"),
            host_spec("app_ping"),
            host_spec("app-Bad"),
        ],
        Vec::new(),
    );

    let (outcome, _) = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "split the clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(
        tool_host.calls.is_empty(),
        "both colliding names dispatched as built-ins"
    );
    assert_eq!(outcome.actions.len(), 1, "the real split applied");
    assert_eq!(host.engine.project().timeline().clip_count(), 2);

    // The offered tool list carries each name once and keeps the
    // legitimately-namespaced host tool.
    let offered = &provider.tool_names()[0];
    assert_eq!(
        offered
            .iter()
            .filter(|n| n.as_str() == "split_clip")
            .count(),
        1
    );
    assert_eq!(
        offered
            .iter()
            .filter(|n| n.as_str() == "describe_project")
            .count(),
        1
    );
    assert_eq!(
        offered.iter().filter(|n| n.as_str() == "app_ping").count(),
        1,
        "duplicate host specs are offered once"
    );
    assert!(
        !offered.iter().any(|n| n == "app-Bad"),
        "malformed host names never reach the model"
    );
}
