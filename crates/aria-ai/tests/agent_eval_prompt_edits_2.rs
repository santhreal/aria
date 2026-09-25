//! `agent_eval` tests covering prompt edits 2. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `agent_eval` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn delete_every_clip_on_the_music_track() {
    // Fixture with a second (audio) track holding three clips.
    let mut project = Project::new("eval-music", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/music.mp3",
        0,
        0,
        R24,
        120 * 24,
        true,
    ));
    project.add_track(TrackKind::Video, "V1");
    let music = project.add_track(TrackKind::Audio, "Music");
    let clips: Vec<u64> = (0..3)
        .map(|i| {
            project
                .add_clip(
                    music,
                    media,
                    TimeRange::at_rate(0, 120, R24),
                    RationalTime::new(i * 150, R24),
                )
                .unwrap()
                .raw()
        })
        .collect();
    let mut host = EngineHost::new(project);

    let provider = ScriptedProvider::new(vec![
        tool_turn(
            clips
                .iter()
                .enumerate()
                .map(|(i, clip)| {
                    (
                        match i {
                            0 => "call_1",
                            1 => "call_2",
                            _ => "call_3",
                        },
                        "remove_clip",
                        serde_json::json!({ "clip": clip }),
                    )
                })
                .collect(),
        ),
        text_turn("Cleared the music track."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "delete every clip on the music track",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 3);
    let summary = summarize(host.engine.project());
    let music_track = summary.tracks.iter().find(|t| t.name == "Music").unwrap();
    assert!(music_track.clips.is_empty());

    // One undo brings all three back.
    assert!(host.engine.undo());
    let summary = summarize(host.engine.project());
    let music_track = summary.tracks.iter().find(|t| t.name == "Music").unwrap();
    assert_eq!(music_track.clips.len(), 3);
}

#[test]
fn system_prompt_gains_host_tool_rules_only_when_host_tools_exist() {
    // Without host tools: no paragraph.
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn("Hi.")]);
    let _ = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "hello",
        &AgentConfig::default(),
    );
    match &provider.requests()[0][0] {
        Message::System { content } => assert!(!content.contains("Host tools:"), "{content}"),
        other => panic!("expected system message, got {other:?}"),
    }

    // With host tools: the paragraph and the spec both show up.
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![text_turn("Hi.")]);
    let mut tool_host = ScriptedHost::new(vec![host_spec("app_ping")], Vec::new());
    let _ = run_with(
        &provider,
        &mut host,
        &mut tool_host,
        &EditorContext::default(),
        "hello",
        &AgentConfig::default(),
    );
    match &provider.requests()[0][0] {
        Message::System { content } => {
            assert!(content.contains("Host tools:"), "{content}");
            assert!(content.contains("declined"), "{content}");
        }
        other => panic!("expected system message, got {other:?}"),
    }
    assert!(provider.tool_names()[0].iter().any(|n| n == "app_ping"));
}
