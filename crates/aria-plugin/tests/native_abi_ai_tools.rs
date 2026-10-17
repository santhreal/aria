//! Native C ABI AI tools. An `aria-sdk` plugin registers a tool with
//! `Host::register_ai_tool`, and the host lists it under `plugin_<name>` and
//! runs it through `ai_tool_call`: the arguments reach the plugin as JSON, the
//! text and PNG images come back, a refusal reaches the model as written, a
//! call allowed at a lower tier does not run, a panic fails the plugin and
//! takes its tools off the list, and a plugin table without the
//! `ai_tool_call` entry cannot register a tool.
//!
//! Not covered: a plugin built as a separate `cdylib`; the in-process table
//! has the same layout `aria_sdk::export!` builds.

#![allow(unsafe_code)]

use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;

use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin::{AiToolError, AiToolImage, AiToolOutput, AiToolTier, PluginContext};
use aria_plugin_abi::{ARIA_API_MAJOR, ARIA_PLUGIN_V1_DECODERS_SIZE};
use aria_sdk::{Host, Plugin as SdkPlugin, PluginRuntime};
use serde_json::{Value, json};
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

/// A 1x1 RGBA PNG.
const DOT: [u8; 70] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 208, 170, 191, 242, 31, 0,
    4, 209, 2, 125, 61, 100, 161, 106, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn echo_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "word": { "type": "string" } },
        "required": ["word"],
    })
}

/// Offers `native_echo`, which repeats a word and returns [`DOT`], and
/// `native_boom`, which panics.
struct Guest;

impl SdkPlugin for Guest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_ai_tool(&aria_sdk::AiToolSpec {
            name: "native_echo".to_owned(),
            description: "Repeat a word twice.".to_owned(),
            parameters: echo_schema(),
            tier: aria_sdk::AiToolTier::Workspace,
        })?;
        host.register_ai_tool(&aria_sdk::AiToolSpec {
            name: "native_boom".to_owned(),
            description: "Panic.".to_owned(),
            parameters: json!({ "type": "object" }),
            tier: aria_sdk::AiToolTier::ReadOnly,
        })
    }

    fn ai_tool_call(
        &self,
        _host: &Host,
        name: &str,
        arguments: &Value,
    ) -> Result<aria_sdk::AiToolOutput, String> {
        match name {
            "native_echo" => {
                let word = arguments["word"].as_str().ok_or("word must be a string")?;
                Ok(aria_sdk::AiToolOutput {
                    text: format!("{word} {word}"),
                    images: vec![aria_sdk::AiToolImage {
                        label: "dot".to_owned(),
                        png: DOT.to_vec(),
                    }],
                })
            }
            "native_boom" => panic!("boom"),
            other => Err(format!("no AI tool {other}")),
        }
    }
}

fn bound() -> PluginRuntime<Guest> {
    let runtime = PluginRuntime::new(Guest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

/// The plugin `table` as package `id`, loaded on a fresh context.
fn loaded(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Box<dyn aria_plugin::Plugin>) {
    let dir = temp.path().join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native AI Tools"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI AI tool test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    let pkg = Package::open(&dir).expect("open package");
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    (PluginContext::new(), plugin)
}

fn call(
    ctx: &PluginContext,
    name: &str,
    arguments: &Value,
    granted: AiToolTier,
) -> Result<AiToolOutput, AiToolError> {
    ctx.call_ai_tool(name, arguments, granted, &AtomicBool::new(false))
}

#[test]
fn a_native_plugin_offers_its_tools_and_answers_calls_with_text_and_images() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativeaitools.full", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");

    let tools = ctx.ai_tools();
    let listed: Vec<_> = tools
        .iter()
        .map(|tool| {
            (
                tool.call_name.as_str(),
                &*tool.plugin_id,
                &*tool.plugin_name,
                tool.spec.tier,
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                "plugin_native_echo",
                "test.nativeaitools.full",
                "Native AI Tools",
                AiToolTier::Workspace
            ),
            (
                "plugin_native_boom",
                "test.nativeaitools.full",
                "Native AI Tools",
                AiToolTier::ReadOnly
            ),
        ]
    );
    assert_eq!(tools[0].spec.description, "Repeat a word twice.");
    assert_eq!(tools[0].spec.parameters, echo_schema());

    let output = call(
        &ctx,
        "plugin_native_echo",
        &json!({ "word": "take" }),
        AiToolTier::Workspace,
    )
    .expect("echo runs");
    assert_eq!(
        output,
        AiToolOutput {
            text: "take take".to_owned(),
            images: vec![AiToolImage {
                png: DOT.to_vec(),
                label: "dot".to_owned(),
            }],
        }
    );

    assert_eq!(
        call(
            &ctx,
            "plugin_native_echo",
            &json!({ "word": 5 }),
            AiToolTier::Workspace
        ),
        Err(AiToolError::Failed("word must be a string".to_owned()))
    );
    let refused = call(
        &ctx,
        "plugin_native_echo",
        &json!({ "word": "take" }),
        AiToolTier::ReadOnly,
    )
    .expect_err("a ReadOnly grant does not run a Workspace tool");
    assert!(
        refused.to_string().contains("needs Workspace trust"),
        "{refused}"
    );
    assert!(ctx.failures().is_empty(), "{:?}", ctx.failures());
}

#[test]
fn a_panicking_native_tool_fails_its_plugin_and_leaves_the_list() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativeaitools.boom", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");

    let err = call(&ctx, "plugin_native_boom", &json!({}), AiToolTier::System)
        .expect_err("a panic fails the call");
    assert_eq!(
        err,
        AiToolError::Failed("AI tool native_boom panicked: boom".to_owned())
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("native_boom panicked: boom"),
        "{}",
        failures[0].reason
    );
    assert!(
        ctx.ai_tools().is_empty(),
        "a failed plugin's tools are listed"
    );
    assert_eq!(
        call(
            &ctx,
            "plugin_native_echo",
            &json!({ "word": "take" }),
            AiToolTier::System
        ),
        Err(AiToolError::Unknown("plugin_native_echo".to_owned()))
    );
}

#[test]
fn a_plugin_table_without_the_ai_tool_entry_cannot_register_a_tool() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(bound);
    let mut table = plugin_table!(RT);
    table.struct_size = ARIA_PLUGIN_V1_DECODERS_SIZE;
    assert!(table.has_decoders() && !table.has_ai_tools());
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativeaitools.short", &table);

    let err = ctx
        .mount(plugin.as_ref())
        .expect_err("register_ai_tool is refused")
        .to_string();
    assert!(err.contains("ai_tool_call"), "{err}");
    assert!(ctx.ai_tools().is_empty());
}
