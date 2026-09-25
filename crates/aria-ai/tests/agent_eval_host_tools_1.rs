//! `agent_eval` tests covering host tools 1. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `agent_eval` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

/// Host dispatch round-trip: the call reaches the host with its exact
/// arguments, the output's text and image ride back as the tool result,
/// and the transcript hears about both its action and bounded image.
#[test]
fn host_tool_round_trip_carries_arguments_images_and_events() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "media_screenshot_preview",
            serde_json::json!({ "at": 1.5 }),
        )]),
        text_turn("Here's the preview."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_screenshot_preview")],
        vec![Ok(ToolOutput {
            text: "screenshot taken at 1.50s\n(1280x720 png)".into(),
            images: vec![ImagePart::png(vec![1, 2, 3], "preview at 1.50s")],
        })],
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
    assert!(outcome.actions.is_empty(), "host calls are not edits");
    assert_eq!(
        tool_host.calls,
        vec![(
            "media_screenshot_preview".to_string(),
            serde_json::json!({ "at": 1.5 })
        )]
    );

    // The next request carries the result — text and image both.
    let second = &provider.requests()[1];
    match second.last().unwrap() {
        Message::ToolResult {
            call_id,
            content,
            images,
        } => {
            assert_eq!(call_id, "call_1");
            assert_eq!(content, "screenshot taken at 1.50s\n(1280x720 png)");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].label, "preview at 1.50s");
            assert_eq!(*images[0].data, vec![1, 2, 3]);
        }
        other => panic!("expected tool result, got {other:?}"),
    }

    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::HostAction { name, summary }
                if name == "media_screenshot_preview" && summary == "screenshot taken at 1.50s"
        )),
        "{events:?}"
    );
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::Image(image) if image.label == "preview at 1.50s")
    ));
}

#[test]
fn host_pre_hook_rejection_skips_authorization_dispatch_and_post() {
    let (mut bridge, _, _, _) = fixture();
    bridge
        .before_host_outputs
        .push_back(Err("project operations must run before staged edits".into()));
    let arguments = serde_json::json!({ "path": "/tmp/new.mp4" });
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![("call_1", "project_import_media", arguments.clone())]),
        text_turn("I need to import before editing."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("project_import_media")],
        vec![Ok(ToolOutput::text("imported"))],
    );

    let (outcome, _) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "import this",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(
        bridge.before_host_calls,
        vec![("project_import_media".into(), arguments)]
    );
    assert!(bridge.after_host_calls.is_empty());
    assert!(tool_host.authorizations.is_empty());
    assert!(tool_host.calls.is_empty());
    match provider.requests()[1].last().unwrap() {
        Message::ToolResult { content, .. } => {
            assert!(
                content.contains("rejected: project operations must run before staged edits"),
                "{content}"
            );
        }
        other => panic!("expected tool result, got {other:?}"),
    }
}

#[test]
fn host_post_hook_runs_after_success_and_failure_without_changing_outputs() {
    let (mut bridge, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "media_screenshot_preview",
                serde_json::json!({ "at": 1.0 }),
            ),
            (
                "call_2",
                "media_screenshot_preview",
                serde_json::json!({ "at": 2.0 }),
            ),
        ]),
        text_turn("Checked both frames."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_screenshot_preview")],
        vec![
            Ok(ToolOutput {
                text: "first frame".into(),
                images: vec![ImagePart::png(vec![4, 5, 6], "first")],
            }),
            Err("second frame unavailable".into()),
        ],
    );

    let (outcome, events) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "check two frames",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(
        bridge
            .after_host_calls
            .iter()
            .map(|(name, arguments, succeeded)| (
                name.as_str(),
                arguments["at"].as_f64(),
                *succeeded
            ))
            .collect::<Vec<_>>(),
        vec![
            ("media_screenshot_preview", Some(1.0), true),
            ("media_screenshot_preview", Some(2.0), false),
        ]
    );
    let requests = provider.requests();
    let results = requests[1]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id,
                content,
                images,
            } => Some((call_id.as_str(), content.as_str(), images.as_slice())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "call_1");
    assert_eq!(results[0].1, "first frame");
    assert_eq!(results[0].2[0].label, "first");
    assert_eq!(results[1].0, "call_2");
    assert_eq!(results[1].1, "rejected: second frame unavailable");
    assert!(results[1].2.is_empty());
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Image(image) if image.label == "first"))
    );
}

#[test]
fn host_post_hook_failure_aborts_and_rolls_back_before_more_work() {
    let (mut bridge, _, _, clip) = fixture();
    bridge
        .after_host_outputs
        .push_back(Err("live project snapshot did not reply".into()));
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "edit_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        )]),
        tool_turn(vec![
            ("host_1", "app_ping", serde_json::json!({})),
            (
                "edit_2",
                "add_track",
                serde_json::json!({ "kind": "text", "name": "Too late" }),
            ),
        ]),
        text_turn("This turn must never run."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("app_ping")],
        vec![Ok(ToolOutput::text("pong"))],
    );

    let (outcome, events) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "split, ping, then add a track",
        &AgentConfig::default(),
    );

    match &outcome.status {
        PromptStatus::Aborted(reason) => {
            assert!(reason.contains("reconciliation failed"), "{reason}");
            assert!(
                reason.contains("host effects may already have occurred"),
                "{reason}"
            );
        }
        other => panic!("expected abort, got {other:?}"),
    }
    assert_eq!(provider.requests().len(), 2, "no later provider turn");
    assert_eq!(outcome.actions.len(), 1, "the later edit never ran");
    assert_eq!(tool_host.calls.len(), 1, "host dispatch ran exactly once");
    assert_eq!(bridge.after_host_calls.len(), 1);
    assert_eq!(
        bridge.engine.project().timeline().clip_count(),
        1,
        "the staged split was rolled back"
    );
    assert_eq!(bridge.engine.project().timeline().track_count(), 1);
    assert!(!bridge.engine.undo(), "the aborted group left no history");
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, AgentEvent::HostAction { .. })),
        "a result that could not be reconciled is not surfaced as completed"
    );
}

#[test]
fn inline_image_events_match_the_request_wide_budget() {
    let (mut bridge, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            ("call_1", "media_frame", serde_json::json!({ "at": 1 })),
            ("call_2", "media_frame", serde_json::json!({ "at": 2 })),
        ]),
        text_turn("Compared the frames."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_frame")],
        vec![
            Ok(ToolOutput {
                text: "old frame".into(),
                images: vec![ImagePart::png(vec![1, 2, 3], "old")],
            }),
            Ok(ToolOutput {
                text: "new frame".into(),
                images: vec![ImagePart::png(vec![4, 5, 6], "new")],
            }),
        ],
    );
    let config = AgentConfig {
        max_images: 1,
        max_image_bytes: 10,
        ..AgentConfig::default()
    };

    let (outcome, events) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "compare two frames",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    let requests = provider.requests();
    let results: Vec<_> = requests[1]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                content, images, ..
            } => Some((content, images)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert!(results[0].0.contains("image no longer attached: old"));
    assert!(results[0].1.is_empty());
    assert_eq!(results[1].1[0].label, "new");

    let labels: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Image(image) => Some(image.label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, vec!["new"]);
}
