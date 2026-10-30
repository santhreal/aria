//! `agent_eval` tests covering prompt edits (part 1). A `#[path]` child of the
//! `agent_eval` test binary; the tests reach their fixtures through `super::*`.

use super::*;

#[test]
fn cut_the_first_three_seconds() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "trim_clip",
            serde_json::json!({ "clip": clip, "start": 3.0, "duration": 7.0 }),
        )]),
        text_turn("Cut the first 3 seconds; the clip now runs 3.00s–10.00s."),
    ]);

    let context = EditorContext {
        selected_clips: vec![clip],
        ..Default::default()
    };
    let (outcome, events) = run(
        &provider,
        &mut host,
        &context,
        "cut the first 3 seconds of the selected clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("trimmed clip {clip} to 3.00s–10.00s")
    );
    assert!(outcome.text.contains("3.00s–10.00s"));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Action(_))));

    // The edit landed, frame-snapped, with the source in-point advanced.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(placed.timeline, TimeRange::at_rate(72, 168, R24));
    assert_eq!(placed.source_range().unwrap().start.value, 72);

    // One prompt = one history entry: a single undo restores everything.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(restored.timeline, TimeRange::at_rate(0, 240, R24));
    assert!(!host.engine.undo(), "exactly one history entry per prompt");

    // The system prompt carried the send-time selection.
    let first_request = &provider.requests()[0];
    match &first_request[0] {
        Message::System { content } => {
            assert!(content.contains(&format!("\"selected_clips\":[{clip}]")));
        }
        other => panic!("expected system message, got {other:?}"),
    }
}

#[test]
fn duplicate_selected_clip_and_one_prompt_undo_removes_the_copy() {
    let (mut host, _, track, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "duplicate_clip",
            serde_json::json!({
                "clip": clip,
                "to_track": track,
                "start": 12.0,
            }),
        )]),
        text_turn("Duplicated the selected clip at 12 seconds."),
    ]);
    let context = EditorContext {
        selected_clips: vec![clip],
        ..Default::default()
    };

    let (outcome, _) = run(
        &provider,
        &mut host,
        &context,
        "duplicate the selected clip at 12 seconds",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].command,
        WireCommand::DuplicateClip(aria_ai::wire::DuplicateClip {
            clip,
            to_track: track,
            start: 12.0,
        })
    );
    let duplicate = host
        .engine
        .project()
        .timeline()
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .find(|candidate| candidate.id.raw() != clip)
        .expect("duplicated clip")
        .clone();
    assert_eq!(duplicate.timeline, TimeRange::at_rate(288, 240, R24));
    assert_eq!(duplicate.link, None);
    assert_eq!(
        outcome.actions[0].description,
        format!(
            "duplicated clip {clip} onto track {track} at 12.00s (new clip {})",
            duplicate.id.raw()
        )
    );

    assert!(host.engine.undo(), "one prompt is one undo entry");
    assert!(host.engine.project().clip(duplicate.id).is_none());
    assert!(
        host.engine
            .project()
            .clip(aria_models::ClipId::from_raw(clip))
            .is_some()
    );
    assert_eq!(host.engine.project().timeline().clip_count(), 1);
    assert!(!host.engine.undo(), "the fixture itself created no history");
}

#[test]
fn unlink_one_member_commits_the_complete_group_as_one_phase() {
    let mut project = Project::new("eval-unlink", R24);
    let track = project.add_track(TrackKind::Sticker, "Overlays");
    let clips = [
        project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                TimeRange::at_rate(0, 24, R24),
            )
            .unwrap(),
        project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [0, 0, 255, 255],
                },
                TimeRange::at_rate(24, 24, R24),
            )
            .unwrap(),
    ];
    let link = LinkId::next();
    for clip in clips {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }
    let mut host = EngineHost::new(project);
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "unlink_clips",
            serde_json::json!({ "clips": [clips[1].raw()] }),
        )]),
        tool_turn(vec![("call_2", "commit_progress", serde_json::json!({}))]),
        text_turn("Unlinked the selected group."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext {
            selected_clips: vec![clips[1].raw()],
            ..Default::default()
        },
        "unlink the selected clip group and commit",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(outcome.phase_breaks, vec![1]);
    assert_eq!(
        outcome.actions[0].command,
        WireCommand::UnlinkClips(aria_ai::wire::UnlinkClips {
            clips: vec![clips[1].raw()]
        })
    );
    assert_eq!(
        outcome.actions[0].description,
        format!(
            "unlinked complete groups touched by clips {}",
            clips[1].raw()
        )
    );
    for clip in clips {
        assert_eq!(host.engine.project().clip(clip).unwrap().link, None);
    }

    assert!(host.engine.undo(), "the committed phase is one undo step");
    for clip in clips {
        assert_eq!(host.engine.project().clip(clip).unwrap().link, Some(link));
    }
    assert!(!host.engine.undo(), "the fixture itself created no history");
}

#[test]
fn questions_answer_without_editing() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn("The timeline is 10.00s long.")]);

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "how long is the timeline?",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert!(outcome.actions.is_empty());
    assert_eq!(outcome.text, "The timeline is 10.00s long.");
    assert!(events.iter().all(|e| matches!(e, AgentEvent::TextDelta(_))));
    assert!(
        !host.engine.undo(),
        "answering a question records no history"
    );
}

#[test]
fn answer_only_turn_in_dry_run_yields_no_plan() {
    // With the preview toggle on, the UI shows an Apply/Discard card only
    // for a non-empty plan; a question must come back as zero actions so
    // no empty card (and no "Applied 0 edits") ever renders.
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn("The timeline runs 10.00s.")]);

    let config = AgentConfig {
        dry_run: true,
        ..Default::default()
    };
    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "how long is the timeline?",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::DryRun);
    assert!(outcome.actions.is_empty());
    assert_eq!(outcome.text, "The timeline runs 10.00s.");
    assert!(events.iter().all(|e| matches!(e, AgentEvent::TextDelta(_))));
    assert!(!host.engine.undo(), "dry-run Q&A records no history");
}

#[test]
fn dry_run_collects_the_plan_without_touching_the_engine() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "trim_clip",
            serde_json::json!({ "clip": clip, "start": 3.0, "duration": 7.0 }),
        )]),
        text_turn("Planned one trim."),
    ]);

    let config = AgentConfig {
        dry_run: true,
        ..Default::default()
    };
    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "cut the first 3 seconds",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::DryRun);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].command,
        WireCommand::TrimClip(aria_ai::wire::TrimClip {
            clip,
            start: 3.0,
            duration: 7.0,
        })
    );

    // Untouched: original placement, no history.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(placed.timeline, TimeRange::at_rate(0, 240, R24));
    assert!(!host.engine.undo());
}

#[test]
fn add_a_title_that_says_intro() {
    let (mut host, _, _, _) = fixture();
    let (outcome, _) = run_with(
        &TitleAddingModel,
        &mut host,
        &mut NullToolHost,
        &EditorContext::default(),
        "add a title that says INTRO",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert!(
        outcome.actions[1]
            .description
            .starts_with("added text 'INTRO' at 0.00s for 3.00s"),
        "{}",
        outcome.actions[1].description
    );

    let summary = summarize(host.engine.project());
    let titles = summary
        .tracks
        .iter()
        .find(|t| t.name == "Titles")
        .expect("titles track");
    assert_eq!(titles.clips.len(), 1);

    // One undo removes both the clip and the track.
    assert!(host.engine.undo());
    assert!(
        summarize(host.engine.project())
            .tracks
            .iter()
            .all(|t| t.name != "Titles")
    );
    assert!(!host.engine.undo());
}

#[test]
fn extract_audio_after_creating_its_explicit_track_is_one_prompt_undo() {
    let (mut host, _, _, clip) = fixture();
    let model = AudioExtractingModel { clip };
    let (outcome, _) = run_with(
        &model,
        &mut host,
        &mut NullToolHost,
        &EditorContext {
            selected_clips: vec![clip],
            ..Default::default()
        },
        "extract the selected clip's audio",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert!(matches!(
        outcome.actions[1].command,
        WireCommand::ExtractAudio(_)
    ));
    assert!(
        outcome.actions[1]
            .description
            .starts_with(&format!("extracted audio from clip {clip} onto track"))
    );
    let audio = host
        .engine
        .project()
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == TrackKind::Audio)
        .and_then(|track| track.clips().next())
        .expect("extracted audio");
    assert_eq!(audio.audio_role, Some(aria_models::AudioRole::Extracted));
    assert!(
        !host
            .engine
            .project()
            .timeline()
            .carries_own_audio(aria_models::ClipId::from_raw(clip))
    );

    assert!(host.engine.undo(), "one prompt is one undo entry");
    assert!(
        host.engine
            .project()
            .timeline()
            .tracks_ordered()
            .all(|track| track.kind != TrackKind::Audio)
    );
    assert!(
        host.engine
            .project()
            .timeline()
            .carries_own_audio(aria_models::ClipId::from_raw(clip))
    );
    assert!(!host.engine.undo());
}
