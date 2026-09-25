//! `agent_eval` tests covering work the loop hands over instead of applying: a
//! render, which takes longer to write than a turn lasts.
//!
//! The class these close: a turn that reports work as finished because the tool
//! call returned. A render is the only call in the vocabulary whose file does
//! not exist when the call comes back, so a loop that described it like an edit
//! would tell the model the clip had been replaced while the encoder was still
//! on frame one — and the model's next call would be made against a project
//! that does not exist yet.
//!
//! What they do not catch: the encode, and the shell that runs it. That is
//! `aria-ui`'s bake coverage.

use super::*;

/// A render is checked, handed over, and reported as started. Nothing about the
/// clip changes in the turn, and the line the model reads says so.
#[test]
fn a_render_is_handed_over_and_reported_as_started() {
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
                "render_and_replace",
                serde_json::json!({ "clip": clip }),
            ),
        ]),
        text_turn("Started the render."),
    ]);

    let (outcome, events) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "bake that clip",
        &AgentConfig::default(),
    );

    assert_eq!(outcome.status, PromptStatus::Completed);
    assert_eq!(
        host.queued,
        vec![QueuedWork::RenderAndReplace {
            clip: aria_models::ClipId::from_raw(clip)
        }],
        "the render was not handed to the shell"
    );

    // Two actions, and the second is the render: the plan carries it, so an
    // Apply of a dry run replays the render rather than dropping it.
    assert_eq!(outcome.actions.len(), 2);
    let render = &outcome.actions[1];
    assert_eq!(
        render.command,
        WireCommand::RenderAndReplace(aria_ai::wire::RenderAndReplace { clip })
    );
    assert!(
        render.description.starts_with("started rendering"),
        "the transcript claims more than started: {}",
        render.description
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentEvent::Action(entry) if entry.description == render.description
        )),
        "the queued render never reached the transcript"
    );

    // What the model reads back says the render began, and the clip is
    // untouched: the loop reported the ask, not an outcome it cannot know.
    let requests = provider.requests();
    let result = requests[1]
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::ToolResult {
                call_id, content, ..
            } if call_id == "call_2" => Some(content),
            _ => None,
        })
        .expect("the render's tool result");
    assert!(result.starts_with("ok: started rendering"), "{result}");
    assert!(
        !result.contains("replaced the clip"),
        "the result claims the swap already happened: {result}"
    );
    let placed = host
        .engine
        .project()
        .clip(aria_models::ClipId::from_raw(clip))
        .expect("the clip");
    assert!(
        !placed.is_baked(),
        "the clip was replaced by a file nothing has written yet"
    );
    assert_eq!(
        placed.effects.len(),
        1,
        "the edit before the render was lost"
    );
}

/// A dry run validates a render and starts nothing. The plan step says a render
/// begins, because that is all Apply can promise.
///
/// The clip is dressed before the prompt rather than in it: a dry run applies
/// nothing, so every call in a planned turn is checked against the project as it
/// stands, and a render only makes sense on a clip that already has something on
/// it.
#[test]
fn a_dry_run_plans_a_render_without_starting_it() {
    let (mut host, _, _, clip) = fixture();
    host.engine
        .apply(aria_commands::Command::Edit(
            aria_commands::EditCommand::AddEffect {
                clip: aria_models::ClipId::from_raw(clip),
                effect_id: "vignette".into(),
            },
        ))
        .expect("something for a render to fold in");
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "render_and_replace",
            serde_json::json!({ "clip": clip }),
        )]),
        text_turn("Here is the plan."),
    ]);

    let config = AgentConfig {
        dry_run: true,
        ..AgentConfig::default()
    };
    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "plan a bake",
        &config,
    );

    assert_eq!(outcome.status, PromptStatus::DryRun);
    assert!(
        host.queued.is_empty(),
        "a dry run started a render: {:?}",
        host.queued
    );
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("start rendering clip {clip} and decode that file in its place")
    );
}

/// A render nothing would gain is refused in the turn, by the model's own rule,
/// and the shell is never asked. The fixture clip carries nothing, so rendering
/// it would re-encode the file it already decodes.
#[test]
fn a_render_worth_nothing_is_refused_before_the_shell_hears_of_it() {
    let (mut host, _, _, clip) = fixture();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "render_and_replace",
            serde_json::json!({ "clip": clip }),
        )]),
        text_turn("That clip has nothing to render."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "bake that clip",
        &AgentConfig::default(),
    );

    assert!(
        host.queued.is_empty(),
        "a refused render still reached the shell"
    );
    assert!(
        outcome.actions.is_empty(),
        "a refused render was logged as an action"
    );
    let requests = provider.requests();
    let result = requests[1]
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::ToolResult {
                call_id, content, ..
            } if call_id == "call_1" => Some(content),
            _ => None,
        })
        .expect("the render's tool result");
    assert!(
        result.contains("would only re-encode it"),
        "the refusal does not say why: {result}"
    );
}

/// Restoring is an ordinary edit, so it is applied in the turn like any other.
/// Both halves of the pair are covered here on purpose: the class this closes is
/// one call being reported like the other.
#[test]
fn restoring_a_rendered_clip_is_an_edit_the_turn_applies() {
    let (mut host, media, _, clip) = fixture();
    let id = aria_models::ClipId::from_raw(clip);
    for edit in [
        aria_commands::EditCommand::AddEffect {
            clip: id,
            effect_id: "gaussian_blur".into(),
        },
        aria_commands::EditCommand::BakeClip {
            clip: id,
            media: aria_models::MediaId::from_raw(media),
        },
    ] {
        host.engine
            .apply(aria_commands::Command::Edit(edit))
            .expect("the fixture media covers the clip");
    }
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![(
            "call_1",
            "restore_unrendered",
            serde_json::json!({ "clip": clip }),
        )]),
        text_turn("Put it back."),
    ]);

    let (outcome, _) = run(
        &provider,
        &mut host,
        &EditorContext::default(),
        "restore that clip",
        &AgentConfig::default(),
    );

    assert!(
        host.queued.is_empty(),
        "a restore was handed to the shell instead of applied"
    );
    assert_eq!(outcome.actions.len(), 1);
    assert_eq!(
        outcome.actions[0].description,
        format!("put clip {clip} back as it was before it was rendered")
    );
    assert!(
        !host.engine.project().clip(id).expect("the clip").is_baked(),
        "the restore was reported but not applied"
    );
}
