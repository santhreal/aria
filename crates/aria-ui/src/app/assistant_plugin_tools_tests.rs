//! A plugin's AI tools, as the desktop assistant lists, authorizes, and runs
//! them.
//!
//! The class this closes: a tool a plugin registers that the assistant is never
//! offered, is offered without its tier, runs without the question its tier
//! requires, or runs at a tier lower than the one its plugin registers now.
//! Every case goes through [`ShellHost`], the one tool host a turn uses, and the
//! end-to-end case through the shipped agent loop with a scripted provider. It
//! does not cover the registry's own refusals and panics; `aria-plugin`'s
//! `ai_tools_tests` own those.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aria_ai::agent::{AgentConfig, AgentEvent, PromptStatus, run_prompt_with_host};
use aria_ai::provider::{ChatTurn, FinishReason, ToolCall};
use aria_ai::providers::ScriptedProvider;
use aria_ai::tools::{ToolHost, ToolTier};
use aria_ai::{AgentExtensions, EditorContext, Message};
use aria_commands::{Command, EditCommand};
use aria_models::RationalTime;
use aria_plugin::{
    AiToolImage, AiToolOutput, AiToolSpec, AiToolTier, Origin, Plugin, PluginContext, PluginError,
    PluginManifest, ProjectHost,
};
use aria_settings::Autonomy;

use super::{Beat, SessionBridge};
use crate::app::AppState;
use crate::app::assistant_host::{ShellHost, host_tool_specs};

const PLUGIN: &str = "test.tools";

/// A 1 by 1 PNG header: what the host reads of a tool's image.
fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes
}

fn spec(name: &str, description: &str, tier: AiToolTier) -> AiToolSpec {
    AiToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
        }),
        tier,
    }
}

/// Offers `look` (reads the project), `mark` (adds a marker through the
/// project host), and `wipe` (a System tool that changes nothing).
struct Tools(PluginManifest);

impl Plugin for Tools {
    fn manifest(&self) -> &PluginManifest {
        &self.0
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_ai_tool(
            spec("look", "Count the markers.", AiToolTier::ReadOnly),
            |ctx: &PluginContext,
             _: &serde_json::Value,
             _: &AtomicBool|
             -> Result<AiToolOutput, String> {
                let host = ctx.try_get::<ProjectHost>().ok_or("no project")?;
                let markers = host.snapshot().timeline().markers().len();
                Ok(AiToolOutput {
                    text: format!("the project has {markers} marker(s)"),
                    images: vec![AiToolImage {
                        png: png(),
                        label: "the markers".into(),
                    }],
                })
            },
        )?;
        ctx.register_ai_tool(
            spec("mark", "Add a marker at the start.", AiToolTier::Workspace),
            |ctx: &PluginContext,
             arguments: &serde_json::Value,
             _: &AtomicBool|
             -> Result<AiToolOutput, String> {
                let host = ctx.try_get::<ProjectHost>().ok_or("no project")?;
                let name = arguments["name"].as_str().ok_or("'name' is required")?;
                let rate = host.snapshot().timeline().frame_rate;
                host.apply(
                    &Origin::plugin(PLUGIN),
                    Command::Edit(EditCommand::AddMarker {
                        at: RationalTime::zero(rate),
                        name: name.to_owned(),
                        color: None,
                    }),
                )
                .map_err(|err| err.to_string())?;
                Ok(AiToolOutput::text(format!("added {name}")))
            },
        )?;
        ctx.register_ai_tool(
            spec("wipe", "Pretend to delete everything.", AiToolTier::System),
            |_: &PluginContext,
             _: &serde_json::Value,
             _: &AtomicBool|
             -> Result<AiToolOutput, String> { Ok(AiToolOutput::text("wiped")) },
        )?;
        Ok(())
    }
}

fn tools() -> Tools {
    Tools(PluginManifest::new(
        PLUGIN,
        "Test tools",
        "0.1.0",
        &[],
        &["ProjectHost"],
    ))
}

/// A session with the tools plugin mounted on its context.
fn state() -> AppState {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("plugin-tools"),
    )
    .expect("state");
    state.new_project().expect("project");
    state.ctx.mount(&tools()).expect("mounts");
    state
}

fn host(state: &AppState, autonomy: Autonomy) -> (ShellHost, crate::handoff::Wakes<Beat>) {
    let (tx, rx) = crate::handoff::wakes();
    (
        ShellHost::new(
            Arc::clone(&state.engine),
            tx,
            autonomy,
            None,
            PluginContext::clone(&state.ctx),
        ),
        rx,
    )
}

fn markers(state: &AppState) -> Vec<String> {
    state
        .engine
        .lock()
        .project()
        .timeline()
        .markers()
        .iter()
        .map(|marker| marker.name.clone())
        .collect()
}

#[test]
fn a_mounted_plugins_tools_are_offered_with_their_tier_and_leave_with_it() {
    let state = state();
    let (host, _rx) = host(&state, Autonomy::Ask);
    let offered: Vec<(String, String, ToolTier)> = host
        .tools()
        .into_iter()
        .skip(host_tool_specs().len())
        .map(|spec| (spec.name, spec.description, spec.tier))
        .collect();
    assert_eq!(
        offered,
        [
            (
                "plugin_look".to_owned(),
                "Count the markers. (from the Test tools plugin)".to_owned(),
                ToolTier::ReadOnly
            ),
            (
                "plugin_mark".to_owned(),
                "Add a marker at the start. (from the Test tools plugin)".to_owned(),
                ToolTier::Workspace
            ),
            (
                "plugin_wipe".to_owned(),
                "Pretend to delete everything. (from the Test tools plugin)".to_owned(),
                ToolTier::System
            ),
        ]
    );
    for (name, _, _) in &offered {
        assert!(aria_ai::is_host_tool_name(name), "{name} would be dropped");
    }
    assert!(
        state.agent_capabilities().ends_with(", 3 plugin tool(s)"),
        "{}",
        state.agent_capabilities()
    );
    assert!(
        state.ai_tool_groups().contains(&(
            "from the Test tools plugin".into(),
            vec![
                "plugin_look".into(),
                "plugin_mark".into(),
                "plugin_wipe".into()
            ]
        )),
        "{:?}",
        state.ai_tool_groups()
    );

    state.ctx.unmount(PLUGIN).expect("unmounts");
    assert_eq!(host.tools().len(), host_tool_specs().len());
    assert!(state.agent_capabilities().ends_with(", 0 plugin tool(s)"));
    let mut host = host;
    assert_eq!(
        host.authorize(
            "plugin_look",
            &serde_json::json!({}),
            ToolTier::ReadOnly,
            &AtomicBool::new(false)
        ),
        Err("no plugin offers plugin_look any more".to_owned())
    );
}

fn tool_turn(calls: Vec<(&str, &str, serde_json::Value)>) -> ChatTurn {
    ChatTurn {
        text: String::new(),
        reasoning_summary: String::new(),
        tool_calls: calls
            .into_iter()
            .map(|(id, name, arguments)| ToolCall {
                id: id.to_owned(),
                name: name.to_owned(),
                arguments,
            })
            .collect(),
        finish: FinishReason::ToolCalls,
    }
}

/// The shipped loop offers the tools, dispatches the model's calls to them,
/// and returns their text and pictures; a tool's edit is one undo step.
#[test]
fn a_turn_calls_plugin_tools_that_read_and_edit_the_project() {
    let state = state();
    let provider = ScriptedProvider::new(vec![
        tool_turn(vec![
            ("c1", "plugin_look", serde_json::json!({})),
            (
                "c2",
                "plugin_mark",
                serde_json::json!({ "name": "From a plugin" }),
            ),
        ]),
        ChatTurn {
            text: "Done.".to_owned(),
            reasoning_summary: String::new(),
            tool_calls: Vec::new(),
            finish: FinishReason::Stop,
        },
    ]);
    let (beats, _wakes) = crate::handoff::wakes::<Beat>();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut bridge = SessionBridge::new(
        Arc::clone(state.host()),
        crate::handoff::Wake::clone(&beats),
        Arc::clone(&cancel),
    );
    let (mut host, _rx) = host(&state, Autonomy::Ask);
    let mut seen = |_: AgentEvent| {};
    let outcome = run_prompt_with_host(
        &provider,
        &mut bridge,
        &mut host,
        &EditorContext::default(),
        &AgentExtensions::default(),
        &[],
        "mark the start",
        &AgentConfig::default(),
        &cancel,
        &mut seen,
    );
    assert_eq!(outcome.status, PromptStatus::Completed);

    let offered = &provider.tool_names()[0];
    assert!(
        offered.iter().any(|name| name == "plugin_mark"),
        "{offered:?}"
    );

    let results: Vec<(String, String, usize)> = provider.requests()[1]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id,
                content,
                images,
            } => Some((call_id.clone(), content.clone(), images.len())),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [
            ("c1".to_owned(), "the project has 0 marker(s)".to_owned(), 1),
            ("c2".to_owned(), "added From a plugin".to_owned(), 0),
        ]
    );
    assert_eq!(markers(&state), ["From a plugin"]);
    assert!(state.engine.lock().undo());
    assert!(markers(&state).is_empty());
}

/// A System tool asks first under Ask, names the plugin in the question, and
/// does not run when refused.
#[test]
fn a_system_plugin_tool_runs_only_after_you_allow_it() {
    let state = state();
    for allow in [true, false] {
        let (mut host, rx) = host(&state, Autonomy::Ask);
        let answer = std::thread::spawn(move || {
            let Beat::Ask(ask) = pollster::block_on(rx.next()).expect("the host asks") else {
                panic!("a System tool sent something other than a question");
            };
            assert_eq!(ask.name, "plugin_wipe");
            assert_eq!(ask.summary, "run the Test tools plugin's tool with {}");
            ask.reply.send(allow).expect("the host is still waiting");
        });
        let authorized = host.authorize(
            "plugin_wipe",
            &serde_json::json!({}),
            ToolTier::System,
            &AtomicBool::new(false),
        );
        answer.join().expect("the answering thread");
        if allow {
            assert_eq!(authorized, Ok(()));
            assert_eq!(
                host.call(
                    "plugin_wipe",
                    &serde_json::json!({}),
                    &AtomicBool::new(false)
                )
                .map(|output| output.text),
                Ok("wiped".to_owned())
            );
        } else {
            assert_eq!(authorized, Err("you did not allow this".to_owned()));
        }
    }
}

/// The tier the plugin registers now wins over the one the turn listed: a
/// tool listed read-only and re-registered as System asks, and a call that
/// skipped authorization runs at read-only trust only.
#[test]
fn a_plugin_tool_is_authorized_at_its_current_tier() {
    let state = state();
    let (mut host, rx) = host(&state, Autonomy::Ask);
    let question = std::thread::spawn(move || {
        let Beat::Ask(ask) = pollster::block_on(rx.next()).expect("the host asks") else {
            panic!("expected a question");
        };
        assert_eq!(ask.name, "plugin_wipe");
        ask.reply.send(false).expect("the host is still waiting");
    });
    assert_eq!(
        host.authorize(
            "plugin_wipe",
            &serde_json::json!({}),
            ToolTier::ReadOnly,
            &AtomicBool::new(false)
        ),
        Err("you did not allow this".to_owned())
    );
    question.join().expect("the answering thread");

    assert_eq!(
        host.call(
            "plugin_mark",
            &serde_json::json!({ "name": "x" }),
            &AtomicBool::new(false)
        ),
        Err(
            "plugin_mark changed since it was allowed and now needs Workspace trust; call it \
             again"
                .to_owned()
        )
    );
    assert!(markers(&state).is_empty(), "the unauthorized call ran");

    host.authorize(
        "plugin_mark",
        &serde_json::json!({ "name": "x" }),
        ToolTier::Workspace,
        &AtomicBool::new(false),
    )
    .expect("a Workspace tool does not ask");
    host.call(
        "plugin_mark",
        &serde_json::json!({ "name": "x" }),
        &AtomicBool::new(false),
    )
    .expect("runs once allowed");
    assert_eq!(markers(&state), ["x"]);
}
