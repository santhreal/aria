//! `agent_eval` tests covering senses. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `agent_eval` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn which_clips_have_no_audio_answers_from_pushed_state() {
    // Two sources, one silent. The summary pushed into the system prompt
    // must already carry the facts ("which clips have no audio?" is the
    // roadmap's canonical Q&A example) so the model answers in one turn,
    // no tool calls.
    let mut project = Project::new("eval-audio", R24);
    let talk = project.add_media(MediaSource::new(
        "/tmp/talk.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        true,
    ));
    let broll = project.add_media(MediaSource::new(
        "/tmp/broll.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            track,
            talk,
            TimeRange::at_rate(0, 120, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let silent_clip = project
        .add_clip(
            track,
            broll,
            TimeRange::at_rate(0, 120, R24),
            RationalTime::new(120, R24),
        )
        .unwrap()
        .raw();
    let mut host = EngineHost::new(project);

    let provider = ScriptedProvider::new(vec![text_turn(&format!(
        "Only clip {silent_clip} (broll.mp4, 5.00s–10.00s) has no audio."
    ))]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "which clips have no audio?",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(outcome.actions.is_empty());
    assert!(outcome.text.contains("broll.mp4"));
    assert!(!host.engine.undo(), "answering records no history");

    // One provider turn was enough, and the pushed state held the facts
    // plus the rule that says to answer from it.
    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "answered without tool calls");
    match &requests[0][0] {
        Message::System { content } => {
            assert!(content.contains("\"has_audio\":false"), "{content}");
            assert!(content.contains("broll.mp4"), "{content}");
            assert!(content.contains("answer directly from"));
        }
        other => panic!("expected system message, got {other:?}"),
    }
}

#[test]
fn describe_project_feeds_state_back_without_counting_as_an_edit() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![("call_1", "describe_project", serde_json::json!({}))]),
        text_turn("There is one clip on one video track."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "what's in this project?",
        &AgentConfig {
            max_tool_calls: 0, // describe_project must not count against the cap
            ..Default::default()
        },
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(outcome.actions.is_empty());

    let second_request = &provider.requests()[1];
    match second_request.last().unwrap() {
        Message::ToolResult { content, .. } => {
            assert!(content.contains("\"project\""), "{content}");
            assert!(content.contains("eval.mp4"), "{content}");
        }
        other => panic!("expected tool result, got {other:?}"),
    }
}

/// Skill retrieval-and-follow: with skills loaded, the index (names +
/// descriptions only) enters the system prompt, `read_skill` returns the
/// body without counting as an edit, and the model then edits per the
/// procedure — the whole prompt still one undo group.
#[test]
fn read_skill_feeds_the_procedure_then_edits_follow() {
    let (mut host, _media, _track, clip) = fixture();
    let cancel = AtomicBool::new(false);
    let extensions = aria_ai::AgentExtensions {
        rules: "[user]\nprefer tight cuts".into(),
        skills: vec![aria_ai::Skill {
            id: "tight-open".into(),
            name: "Tight opening".into(),
            description: "Trim the first seconds off the opening clip.".into(),
            body: "Step 1: trim_clip the first clip, raising start by 3.".into(),
        }],
    };
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "read_skill",
            serde_json::json!({ "id": "tight-open" }),
        )]),
        tool_turn(vec![(
            "call_2",
            "trim_clip",
            serde_json::json!({ "clip": clip, "start": 3.0, "duration": 7.0 }),
        )]),
        text_turn("Tightened the opening per the skill."),
    ]);

    let cfg = AgentConfig {
        max_tool_calls: 1, // read_skill must not consume the only edit slot
        ..AgentConfig::default()
    };
    let mut events = Vec::new();
    let outcome = run_prompt_with_host(
        &provider,
        &mut host,
        &mut NullToolHost,
        &EditorContext::default(),
        &extensions,
        &[],
        "tighten the opening",
        &cfg,
        &cancel,
        &mut |e| events.push(e),
    );
    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert!(outcome.actions[0].description.starts_with("trimmed clip"));

    // The system prompt carried the rules and the index, not the body.
    let sent = provider.requests();
    let Message::System { content } = &sent[0][0] else {
        panic!("first message is the system prompt");
    };
    assert!(content.contains("prefer tight cuts"));
    assert!(content.contains("tight-open (Tight opening)"));
    assert!(!content.contains("Step 1: trim_clip"));

    // The read_skill tool result delivered the body verbatim.
    let body_result = sent[1].iter().find_map(|m| match m {
        Message::ToolResult { content, .. } => Some(content.clone()),
        _ => None,
    });
    assert!(
        body_result
            .expect("read_skill result in the second request")
            .contains("Step 1: trim_clip"),
    );

    assert!(host.engine.undo(), "one undo entry for the whole prompt");
}

/// `describe_project` results are large and the fresh system snapshot
/// supersedes them, so history keeps only a placeholder — never a full
/// stale project blob.
#[test]
fn describe_project_results_are_collapsed_in_history() {
    let (mut host, _media, _track, _clip) = fixture();
    let cancel = AtomicBool::new(false);
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![("call_1", "describe_project", serde_json::json!({}))]),
        text_turn("There is one clip on one video track."),
    ]);
    let outcome = run_prompt_with_host(
        &provider,
        &mut host,
        &mut NullToolHost,
        &EditorContext::default(),
        &aria_ai::AgentExtensions::default(),
        &[],
        "what's on the timeline?",
        &AgentConfig::default(),
        &cancel,
        &mut |_| {},
    );
    assert_eq!(outcome.status, PromptStatus::Completed);

    let tool_result = outcome
        .turn_messages
        .iter()
        .find_map(|m| match m {
            Message::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("the describe_project tool result");
    assert!(
        tool_result.contains("project state omitted"),
        "the blob is collapsed: {tool_result}"
    );
    assert!(
        !tool_result.contains("\"tracks\""),
        "no full project json survives in history"
    );
}

/// A bridge-owned sense runs against the already-mutated rehearsal state,
/// which is the key distinction from a host screenshot of the live project.
#[test]
fn engine_sense_observes_rehearsed_edits_and_returns_images() {
    let (mut host, _, _, clip) = fixture();
    host.sense_specs = vec![host_spec("media_screenshot_preview")];
    host.sense_outputs.push_back(Ok(ToolOutput {
        text: "sandbox preview at 5.00s".into(),
        images: vec![ImagePart::png(vec![7, 8, 9], "sandbox preview")],
    }));
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "split_clip",
            serde_json::json!({ "clip": clip, "at": 5.0 }),
        )]),
        tool_turn(vec![(
            "call_2",
            "media_screenshot_preview",
            serde_json::json!({ "at": 5.0 }),
        )]),
        text_turn("The rehearsed split looks correct."),
    ]);

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "split the clip and check the result",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(host.sense_clip_counts, vec![2]);
    let requests = provider.requests();
    let Message::System { content } = &requests[0][0] else {
        panic!("first message should be the system prompt");
    };
    assert!(
        content.contains("inspect the complete current project snapshot and sandbox"),
        "{content}"
    );
    assert!(content.contains("schematic timeline map"), "{content}");
    assert!(content.contains("media_screenshot_preview"), "{content}");
    assert_eq!(
        host.sense_calls,
        vec![(
            "media_screenshot_preview".into(),
            serde_json::json!({ "at": 5.0 })
        )]
    );
    match requests[2].last().unwrap() {
        Message::ToolResult {
            content, images, ..
        } => {
            assert_eq!(content, "sandbox preview at 5.00s");
            assert_eq!(images[0].label, "sandbox preview");
            assert_eq!(*images[0].data, vec![7, 8, 9]);
        }
        other => panic!("expected sense result, got {other:?}"),
    }
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::HostAction { name, .. } if name == "media_screenshot_preview"
    )));
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::Image(image) if image.label == "sandbox preview")
    ));
}

#[test]
fn media_pool_sheet_sense_dispatches_and_survives_the_image_budget() {
    let (mut host, _, _, _) = fixture();
    host.sense_specs = vec![host_spec("media_pool_sheet")];
    host.sense_outputs.push_back(Ok(ToolOutput {
        text: "Media-pool contact sheet page 1 of 2.".into(),
        images: vec![ImagePart::png(
            vec![0x89, 0x50, 0x4e, 0x47],
            "media pool sheet page 1 of 2",
        )],
    }));
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "media_pool_sheet",
            serde_json::json!({ "page": 1, "max_width": 1024 }),
        )]),
        text_turn("I found footage for the freestyle edit."),
    ]);
    let config = AgentConfig {
        max_images: 1,
        max_image_bytes: 16,
        ..AgentConfig::default()
    };

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "freestyle an edit from my media library",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(
        host.sense_calls,
        vec![(
            "media_pool_sheet".into(),
            serde_json::json!({ "page": 1, "max_width": 1024 })
        )]
    );
    let requests = provider.requests();
    let Message::System { content } = &requests[0][0] else {
        panic!("first message should be the system prompt");
    };
    assert!(content.contains("freestyle edits"), "{content}");
    assert!(content.contains("media_pool_sheet"), "{content}");
    match requests[1].last().unwrap() {
        Message::ToolResult {
            content, images, ..
        } => {
            assert_eq!(content, "Media-pool contact sheet page 1 of 2.");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].label, "media pool sheet page 1 of 2");
            assert_eq!(*images[0].data, vec![0x89, 0x50, 0x4e, 0x47]);
        }
        other => panic!("expected media-pool sheet result, got {other:?}"),
    }
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::Image(image) if image.label == "media pool sheet page 1 of 2")
    ));
}

#[test]
fn engine_senses_are_read_only_filtered_and_outrank_host_collisions() {
    let (mut bridge, _, _, _) = fixture();
    bridge.sense_specs = vec![
        host_spec("media_frame"),
        host_spec("media_frame"),
        {
            let mut spec = host_spec("media_mutate");
            spec.tier = ToolTier::Workspace;
            spec
        },
        host_spec("invalid_name"),
    ];
    bridge
        .sense_outputs
        .push_back(Ok(ToolOutput::text("sandbox frame")));
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![("call_1", "media_frame", serde_json::json!({}))]),
        text_turn("Checked."),
    ]);
    let mut tool_host = ScriptedHost::new(
        vec![host_spec("media_frame")],
        vec![Ok(ToolOutput::text("wrong live frame"))],
    );

    let (outcome, _) = run_with(
        &provider,
        &mut bridge,
        &mut tool_host,
        &EditorContext::default(),
        "check the frame",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(bridge.sense_calls.len(), 1);
    assert!(tool_host.calls.is_empty(), "sandbox sense wins the name");
    let offered = &provider.tool_names()[0];
    assert_eq!(
        offered.iter().filter(|name| *name == "media_frame").count(),
        1
    );
    assert!(!offered.iter().any(|name| name == "media_mutate"));
    assert!(!offered.iter().any(|name| name == "invalid_name"));
}
