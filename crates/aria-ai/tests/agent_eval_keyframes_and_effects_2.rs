//! `agent_eval` tests covering keyframes and effects (part 2). A `#[path]`
//! child of the `agent_eval` test binary; the tests reach their fixtures
//! through `super::*`.

use super::*;

#[test]
fn apply_and_clear_speed_ramp_on_clip() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_speed_curve",
            serde_json::json!({ "clip": clip, "preset": "montage" }),
        )]),
        text_turn("Added a montage speed ramp."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "give the clip a montage speed ramp",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("applied montage speed ramp to clip {clip}")
    );

    // The ramp lands, retimes the clip (montage averages faster than 1×, so
    // shorter than the 240-tick original), and surfaces in the next describe.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(placed.has_speed_curve() && placed.is_retimed());
    assert!(placed.timeline.duration.value < 240);
    let summary = summarize(host.engine.project());
    assert_eq!(summary.tracks[0].clips[0].speed_ramp, Some(true));

    // One undo restores the original constant-speed placement.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(restored.timeline.duration.value, 240);
    assert!(!restored.is_retimed());
}

#[test]
fn crop_to_center_and_mirror_clip() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_clip_crop",
            serde_json::json!({ "clip": clip, "left": 0.25, "right": 0.25, "flip_h": true }),
        )]),
        text_turn("Cropped to the middle half and mirrored it."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "crop to the center half and mirror it",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("set clip {clip} cropped left 25%, right 25%, flipped horizontally")
    );

    // The kept region and flip land on the model, and the next describe()
    // surfaces them so the model can reason about current framing.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(placed.crop.sample(0).x, 0.25);
    assert_eq!(placed.crop.sample(0).w, 0.5);
    assert!(placed.flip_h && !placed.flip_v);
    let summary = summarize(host.engine.project());
    let described = &summary.tracks[0].clips[0];
    assert_eq!(described.crop, Some([0.25, 0.0, 0.25, 0.0]));
    assert_eq!(described.flip_h, Some(true));
    assert_eq!(described.flip_v, None);

    // One undo restores the full frame.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(!restored.has_custom_crop());
}

#[test]
fn add_a_blur_to_the_clip() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "add_effect",
                serde_json::json!({ "clip": clip, "effect": "gaussian_blur" }),
            ),
            (
                "call_2",
                "set_effect_param",
                serde_json::json!({ "clip": clip, "index": 0, "param": "radius", "value": 6.0 }),
            ),
        ]),
        text_turn("Added a gaussian blur and set its radius to 6."),
    ]);

    let context = EditorContext {
        selected_clips: vec![clip],
        ..Default::default()
    };
    let (outcome, _) = run(
        &provider,
        &mut host,
        &context,
        "add a blur to the clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert_eq!(
        outcome.actions[0].description,
        format!("added gaussian_blur effect to clip {clip}")
    );

    // The effect landed and surfaces in the summary the next prompt sees,
    // with the index the model would address.
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert_eq!(placed.effects.len(), 1);
    assert_eq!(placed.effects[0].effect_id, "gaussian_blur");
    let summary = summarize(host.engine.project());
    let described = &summary.tracks[0].clips[0];
    assert_eq!(described.effects.len(), 1);
    assert_eq!(described.effects[0].effect, "gaussian_blur");
    assert_eq!(described.effects[0].params.get("radius"), Some(&6.0));

    // One prompt = one undo: the whole effect disappears as a unit.
    assert!(host.engine.undo());
    let restored = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .unwrap();
    assert!(restored.effects.is_empty());
    assert!(!host.engine.undo());
}

#[test]
fn crossfade_between_two_clips() {
    let mut project = Project::new("eval", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/eval.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let first = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 120, R24),
            RationalTime::new(0, R24),
        )
        .unwrap()
        .raw();
    let _second = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 120, R24),
            RationalTime::new(120, R24),
        )
        .unwrap();
    let mut host = EngineHost::new(project);

    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            (
                "call_1",
                "add_transition",
                serde_json::json!({ "clip": first, "transition": "crossfade" }),
            ),
            (
                "call_2",
                "set_transition",
                serde_json::json!({ "clip": first, "seconds": 0.5 }),
            ),
        ]),
        text_turn("Added a half-second crossfade between the two clips."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "crossfade between the two clips",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 2);
    assert_eq!(
        outcome.actions[0].description,
        format!("added crossfade transition after clip {first}")
    );

    // The junction landed and surfaces on the left clip in the next summary.
    let summary = summarize(host.engine.project());
    let described = &summary.tracks[0].clips[0];
    assert_eq!(described.transition.as_deref(), Some("crossfade"));

    // One prompt = one undo: the whole transition disappears as a unit.
    assert!(host.engine.undo());
    let summary = summarize(host.engine.project());
    assert_eq!(summary.tracks[0].clips[0].transition, None);
    assert!(!host.engine.undo());
}

#[test]
fn add_marker_at_playhead() {
    let (mut host, _, _, _) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "add_marker",
            serde_json::json!({ "at": 5.0, "name": "beat drop", "color": "red" }),
        )]),
        text_turn("Dropped a red marker at the beat."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "mark the beat drop at 5 seconds",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1);
    assert!(
        outcome.actions[0]
            .description
            .contains("added marker 'beat drop' at 5.00s"),
        "{}",
        outcome.actions[0].description
    );
    assert!(
        outcome.actions[0].description.contains("(red)"),
        "{}",
        outcome.actions[0].description
    );

    let markers = host.engine.project().timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].tick.value, 120);
    assert_eq!(markers[0].name, "beat drop");
    assert_eq!(markers[0].color, aria_models::MarkerColor::Red);

    let summary = summarize(host.engine.project());
    assert_eq!(summary.markers.len(), 1);
    assert_eq!(summary.markers[0].name, "beat drop");
    assert_eq!(summary.markers[0].color, "red");

    assert!(host.engine.undo());
    assert!(host.engine.project().timeline().markers().is_empty());
}

#[test]
fn keyframe_outside_clip_is_rejected_with_extent() {
    let (mut host, _, _, clip) = fixture();
    // First call misses the clip (it ends at 10 s); the model corrects.
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "set_param_keyframe",
            serde_json::json!({
                "clip": clip, "param": "scale", "at": 30.0, "value": 2.0,
            }),
        )]),
        tool_turn(vec![(
            "call_2",
            "set_param_keyframe",
            serde_json::json!({
                "clip": clip, "param": "scale", "at": 9.0, "value": 2.0,
            }),
        )]),
        text_turn("Keyframed the zoom at 9 seconds."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "zoom in at the end of the clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(outcome.actions.len(), 1, "only the corrected call applied");

    // The rejection named the clip's extent so the model could correct.
    let requests = provider.requests();
    let tool_results: Vec<&str> = requests
        .iter()
        .flat_map(|msgs| msgs.iter())
        .filter_map(|m| match m {
            Message::ToolResult { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        tool_results
            .iter()
            .any(|r| r.contains("outside clip") && r.contains("10.000s")),
        "rejection names the extent: {tool_results:?}"
    );
}
