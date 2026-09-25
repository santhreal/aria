//! `agent_eval` tests covering keyframes and effects 1. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `agent_eval` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn lower_music_volume_with_fades() {
    // Fixture with an audio lane holding one music clip.
    let mut project = Project::new("eval-volume", R24);
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
    let clip = project
        .add_clip(
            music,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap()
        .raw();
    let mut host = EngineHost::new(project);

    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_clip_audio",
            serde_json::json!({
                "clip": clip, "volume": 0.5, "fade_in": 1.0, "fade_out": 2.0,
            }),
        )]),
        text_turn("Lowered the music to 50% with fades."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "lower the music to half volume and fade it in and out",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("set clip {clip} volume 50%, fade in 1.00s, fade out 2.00s")
    );

    let clip_id = aria_models::ClipId::from_raw(clip);
    let c = host.engine.project().clip(clip_id).unwrap();
    assert_eq!(c.volume.constant(), Some(0.5));
    assert_eq!((c.fade_in, c.fade_out), (24, 48));
    // The summary the next prompt would see carries the new mix.
    let summary = summarize(host.engine.project());
    let summarized = &summary
        .tracks
        .iter()
        .find(|t| t.name == "Music")
        .unwrap()
        .clips[0];
    assert_eq!(summarized.volume, Some(0.5));
    assert_eq!(summarized.fade_in, Some(1.0));

    // One undo restores the default mix.
    assert!(host.engine.undo());
    assert!(
        !host
            .engine
            .project()
            .clip(clip_id)
            .unwrap()
            .has_custom_audio()
    );
}

#[test]
fn volume_envelope_with_keyframes() {
    // Fixture with an audio lane holding one music clip (10s at 24fps).
    let mut project = Project::new("eval-envelope", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/music.mp3",
        0,
        0,
        R24,
        120 * 24,
        true,
    ));
    let music = project.add_track(TrackKind::Audio, "Music");
    let clip = project
        .add_clip(
            music,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap()
        .raw();
    let mut host = EngineHost::new(project);

    // "duck the music down to 20% between 2s and 4s" → a volume envelope.
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "set_param_keyframe",
                serde_json::json!({ "clip": clip, "param": "volume", "at": 2.0, "value": 1.0 }),
            ),
            (
                "call_2",
                "set_param_keyframe",
                serde_json::json!({ "clip": clip, "param": "volume", "at": 3.0, "value": 0.2 }),
            ),
            (
                "call_3",
                "set_param_keyframe",
                serde_json::json!({ "clip": clip, "param": "volume", "at": 4.0, "value": 1.0 }),
            ),
        ]),
        text_turn("Ducked the music to 20% from 2 to 4 seconds."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "duck the music to 20% between 2 and 4 seconds",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 3);
    assert_eq!(
        outcome.actions[1].description,
        format!("keyframed clip {clip} volume = 20% at 3.00s")
    );

    // The envelope landed: a keyframed volume that dips at 3s (72 ticks).
    let clip_id = aria_models::ClipId::from_raw(clip);
    let c = host.engine.project().clip(clip_id).unwrap();
    assert!(c.has_volume_envelope());
    assert_eq!(c.volume.keyframes().len(), 3);
    assert_eq!(c.volume.sample(48), 1.0);
    assert_eq!(c.volume.sample(72), 0.2);
    assert_eq!(c.volume.sample(96), 1.0);

    // One prompt = one undo: the whole envelope disappears as a unit.
    assert!(host.engine.undo());
    assert!(
        !host
            .engine
            .project()
            .clip(clip_id)
            .unwrap()
            .has_volume_envelope()
    );
}

#[test]
fn pan_envelope_with_keyframes() {
    let mut project = Project::new("eval-pan", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/music.mp3",
        0,
        0,
        R24,
        120 * 24,
        true,
    ));
    let music = project.add_track(TrackKind::Audio, "Music");
    let clip = project
        .add_clip(
            music,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap()
        .raw();
    let mut host = EngineHost::new(project);

    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "set_param_keyframe",
                serde_json::json!({ "clip": clip, "param": "pan", "at": 0.0, "value": -1.0 }),
            ),
            (
                "call_2",
                "set_param_keyframe",
                serde_json::json!({ "clip": clip, "param": "pan", "at": 5.0, "value": 1.0 }),
            ),
        ]),
        text_turn("Swept the music from left to right."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "pan the music from left to right over five seconds",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert_eq!(
        outcome.actions[0].description,
        format!("keyframed clip {clip} pan = -1.00 at 0.00s")
    );

    let clip_id = aria_models::ClipId::from_raw(clip);
    let c = host.engine.project().clip(clip_id).unwrap();
    assert!(c.has_pan_envelope());
    assert_eq!(c.pan.sample(0), -1.0);
    assert_eq!(c.pan.sample(120), 1.0);
}

#[test]
fn fade_in_with_opacity_keyframes() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "set_param_keyframe",
                serde_json::json!({
                    "clip": clip, "param": "opacity", "at": 0.0,
                    "value": 0.0, "easing": "ease_in_out",
                }),
            ),
            (
                "call_2",
                "set_param_keyframe",
                serde_json::json!({
                    "clip": clip, "param": "opacity", "at": 1.0, "value": 1.0,
                }),
            ),
        ]),
        text_turn("Added a 1-second fade-in."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "fade the clip in over the first second",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert_eq!(
        outcome.actions[0].description,
        format!("keyframed clip {clip} opacity = 0% at 0.00s")
    );
    assert_eq!(
        outcome.actions[1].description,
        format!("keyframed clip {clip} opacity = 100% at 1.00s")
    );

    // The curve landed: 0 → 1 over the first 24 ticks, eased.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(placed.transform.is_animated());
    assert_eq!(placed.transform.opacity.keyframes().len(), 2);
    assert_eq!(placed.transform.sample(0).opacity, 0.0);
    assert_eq!(placed.transform.sample(24).opacity, 1.0);

    // One prompt = one undo: the animation disappears as a unit.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(!restored.transform.is_animated());
    assert!(!host.engine.undo());
}

#[test]
fn add_fade_in_animation_preset() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_clip_animation",
            serde_json::json!({
                "clip": clip,
                "slot": "in",
                "animation": "fade_in",
            }),
        )]),
        text_turn("Added a fade-in animation."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "add a fade in animation to the clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("set fade_in animation on clip {clip} (in slot)")
    );

    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(
        placed.animation_in.as_ref().map(|a| a.id.as_str()),
        Some("fade_in")
    );

    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(restored.animation_in.is_none());
    assert!(!host.engine.undo());
}

#[test]
fn speed_up_and_reverse_clip() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_clip_speed",
            serde_json::json!({ "clip": clip, "speed": 2.0, "reversed": true }),
        )]),
        text_turn("Doubled the speed and reversed it."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "play the clip backwards at double speed",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("set clip {clip} speed 2x, reversed")
    );

    // 10 s of source at 2x occupies 5 s of timeline (120 ticks @ 24 fps),
    // and the retiming shows up in the next describe() the model sees.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(placed.timeline.duration.value, 120);
    assert!(placed.reversed);
    let summary = summarize(host.engine.project());
    let described = &summary.tracks[0].clips[0];
    assert_eq!(described.speed, Some(2.0));
    assert_eq!(described.reversed, Some(true));

    // One undo restores the original 1x forward placement.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(restored.timeline.duration.value, 240);
    assert!(!restored.is_retimed());
}
