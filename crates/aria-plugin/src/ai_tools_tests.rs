//! Tests for plugin AI tools.
//!
//! Closes the class "a tool the assistant is offered cannot be called, outlives
//! its plugin, or takes the host down": every case runs through
//! `register_ai_tool` and `call_ai_tool`, the two entry points every tool
//! passes, whatever tier registered it. It does not cover how the assistant
//! lists and authorizes the tools; `aria-ui`'s host tool tests own that.

use std::sync::atomic::Ordering;

use serde_json::json;

use super::*;
use crate::plugin::Plugin;

/// The first 24 bytes of a PNG `width` by `height` pixels: what the host
/// reads of a tool's image.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes
}

fn spec(name: &str) -> AiToolSpec {
    AiToolSpec {
        name: name.to_owned(),
        description: "Echo a word back.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": { "word": { "type": "string" } },
            "required": ["word"],
        }),
        tier: AiToolTier::ReadOnly,
    }
}

/// Echoes `word`, names the plugin whose view it ran through, and returns
/// one picture.
fn echo(
    ctx: &PluginContext,
    arguments: &serde_json::Value,
    cancel: &AtomicBool,
) -> Result<AiToolOutput, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("stopped".into());
    }
    let word = arguments
        .get("word")
        .and_then(serde_json::Value::as_str)
        .ok_or("'word' is required")?;
    let scope = ctx
        .scope()
        .map_or_else(|| "host".to_owned(), |manifest| manifest.id.to_string());
    Ok(AiToolOutput {
        text: format!("{word} from {scope}"),
        images: vec![AiToolImage {
            png: png(4, 4),
            label: "a picture".into(),
        }],
    })
}

/// A plugin that offers one tool built by `make`.
struct WithTool {
    manifest: PluginManifest,
    name: &'static str,
    panics: bool,
}

impl Plugin for WithTool {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if self.panics {
            ctx.register_ai_tool(
                spec(self.name),
                |_: &PluginContext,
                 _: &serde_json::Value,
                 _: &AtomicBool|
                 -> Result<AiToolOutput, String> { panic!("boom") },
            )?;
        } else {
            ctx.register_ai_tool(spec(self.name), echo)?;
        }
        Ok(())
    }
}

fn plugin(id: &'static str, name: &'static str, panics: bool) -> WithTool {
    WithTool {
        manifest: PluginManifest::new(id, "Echoes", "0.1.0", &[], &[]),
        name,
        panics,
    }
}

fn idle() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn a_tool_runs_through_its_plugins_view_and_is_listed_by_call_name() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.echo", "echo", false))
        .expect("mounts");

    let tools = ctx.ai_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].call_name, "plugin_echo");
    assert_eq!(&*tools[0].plugin_id, "test.echo");
    assert_eq!(&*tools[0].plugin_name, "Echoes");
    assert_eq!(tools[0].spec.tier, AiToolTier::ReadOnly);

    let output = ctx
        .call_ai_tool(
            "plugin_echo",
            &json!({ "word": "hello" }),
            AiToolTier::ReadOnly,
            &idle(),
        )
        .expect("runs");
    assert_eq!(output.text, "hello from test.echo");
    assert_eq!(output.images[0].png, png(4, 4));

    // The tool's own error and the stop signal reach the caller as written.
    assert_eq!(
        ctx.call_ai_tool("plugin_echo", &json!({}), AiToolTier::ReadOnly, &idle()),
        Err(AiToolError::Failed("'word' is required".into()))
    );
    assert_eq!(
        ctx.call_ai_tool(
            "plugin_echo",
            &json!({ "word": "x" }),
            AiToolTier::ReadOnly,
            &AtomicBool::new(true)
        ),
        Err(AiToolError::Failed("stopped".into()))
    );
    // The name without its prefix is not the call name.
    assert_eq!(
        ctx.call_ai_tool(
            "echo",
            &json!({ "word": "x" }),
            AiToolTier::ReadOnly,
            &idle()
        ),
        Err(AiToolError::Unknown("echo".into()))
    );
}

#[test]
fn a_tool_leaves_with_its_plugin_and_its_name_is_released() {
    let ctx = PluginContext::new();
    let declared = plugin("test.leaving", "leaving", false);
    ctx.mount(&declared).expect("mounts");
    let before = ctx.ai_tools_revision();

    ctx.unmount("test.leaving").expect("unmounts");
    assert!(ctx.ai_tools().is_empty());
    assert_ne!(ctx.ai_tools_revision(), before);
    assert_eq!(
        ctx.call_ai_tool(
            "plugin_leaving",
            &json!({ "word": "x" }),
            AiToolTier::ReadOnly,
            &idle()
        ),
        Err(AiToolError::Unknown("plugin_leaving".into()))
    );

    ctx.mount(&declared).expect("mounts again");
    assert_eq!(ctx.ai_tools().len(), 1);
}

#[test]
fn a_panicking_tool_fails_its_plugin() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.panicky", "boom", true))
        .expect("mounts");

    assert_eq!(
        ctx.call_ai_tool(
            "plugin_boom",
            &json!({ "word": "x" }),
            AiToolTier::ReadOnly,
            &idle()
        ),
        Err(AiToolError::Panicked("boom".into()))
    );
    assert!(!ctx.is_mounted("test.panicky"));
    assert!(ctx.ai_tools().is_empty(), "the failed plugin's tool stays");
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].reason, "tool plugin_boom panicked: boom");
}

/// Every image passes one check on its way to the model: a PNG header whose
/// sides are within [`MAX_AI_TOOL_IMAGE_SIDE`]. Each case passes the image's
/// bytes as the `image` argument.
#[test]
fn an_image_is_returned_only_as_a_png_within_the_size_cap() {
    let ctx = PluginContext::new();
    let _held = ctx
        .register_ai_tool(
            AiToolSpec {
                parameters: json!({ "type": "object" }),
                ..spec("picture")
            },
            |_: &PluginContext,
             arguments: &serde_json::Value,
             _: &AtomicBool|
             -> Result<AiToolOutput, String> {
                let bytes: Vec<u8> = serde_json::from_value(arguments["image"].clone())
                    .map_err(|err| err.to_string())?;
                Ok(AiToolOutput {
                    text: String::new(),
                    images: vec![AiToolImage {
                        png: bytes,
                        label: "it".into(),
                    }],
                })
            },
        )
        .expect("registers");
    let call = |bytes: Vec<u8>| {
        ctx.call_ai_tool(
            "plugin_picture",
            &json!({ "image": bytes }),
            AiToolTier::ReadOnly,
            &idle(),
        )
        .map(|output| output.images.len())
    };
    let not_png = || {
        Err(AiToolError::Failed(
            "plugin_picture returned image \"it\", which is not a PNG".into(),
        ))
    };
    let too_big = |width: u32, height: u32| {
        Err(AiToolError::Failed(format!(
            "plugin_picture returned image \"it\" at {width}x{height}, over 2048 pixels on a side"
        )))
    };
    let side = MAX_AI_TOOL_IMAGE_SIDE;
    assert_eq!(call(png(side, side)), Ok(1));
    assert_eq!(call(png(1, 1)), Ok(1));
    assert_eq!(call(png(side + 1, 1)), too_big(side + 1, 1));
    assert_eq!(call(png(1, side + 1)), too_big(1, side + 1));
    assert_eq!(call(png(u32::MAX, u32::MAX)), too_big(u32::MAX, u32::MAX));
    assert_eq!(call(b"\xff\xd8\xff\xe0".to_vec()), not_png());
    assert_eq!(call(png(4, 4)[..23].to_vec()), not_png(), "a cut header");
    let mut not_header = png(4, 4);
    not_header[12..16].copy_from_slice(b"IDAT");
    assert_eq!(
        call(not_header),
        not_png(),
        "a first chunk that is not IHDR"
    );
    assert_eq!(call(Vec::new()), not_png());
}

#[test]
fn malformed_and_duplicate_tools_are_refused() {
    let ctx = PluginContext::new();
    let longest = "a".repeat(MAX_AI_TOOL_NAME);
    let _longest = ctx
        .register_ai_tool(spec(&longest), echo)
        .expect("the longest name registers");
    let _first = ctx
        .register_ai_tool(spec("first"), echo)
        .expect("registers");

    let schema = |parameters: serde_json::Value| AiToolSpec {
        parameters,
        ..spec("schema")
    };
    let cases: Vec<(AiToolSpec, &str)> = vec![
        (spec(""), "a tool name is 1 to 56"),
        (spec(&format!("{longest}a")), "a tool name is 1 to 56"),
        (spec("Upper"), "a tool name is 1 to 56"),
        (spec("with-dash"), "a tool name is 1 to 56"),
        (spec("with space"), "a tool name is 1 to 56"),
        (
            AiToolSpec {
                description: " ".into(),
                ..spec("blank")
            },
            "blank: a tool needs a description",
        ),
        (schema(json!("object")), "are not a JSON Schema object"),
        (
            schema(json!({ "type": "string" })),
            "must have \"type\": \"object\"",
        ),
        (
            schema(json!({ "type": "object", "properties": [] })),
            "\"properties\" in the parameters schema is not an object",
        ),
        (
            schema(json!({ "type": "object", "required": "word" })),
            "\"required\" in the parameters schema is not an array",
        ),
        (
            schema(json!({ "type": "object", "required": ["word"] })),
            "required argument \"word\" is not in \"properties\"",
        ),
        (
            AiToolSpec {
                description: "x".repeat(MAX_AI_TOOL_SPEC),
                ..spec("huge")
            },
            "over 16384",
        ),
        (spec("first"), "Host already offers a tool named first"),
    ];
    for (bad, expected) in cases {
        let name = bad.name.clone();
        match ctx.register_ai_tool(bad, echo) {
            Err(PluginError::AiToolRefused(reason)) => assert!(
                reason.contains(expected),
                "{name:?}: {reason:?} does not contain {expected:?}"
            ),
            Err(other) => panic!("{name:?}: refused as {other}"),
            Ok(_) => panic!("{name:?} was registered"),
        }
    }
    assert_eq!(ctx.ai_tools().len(), 2, "a refused tool was listed");
}

#[test]
fn two_plugins_cannot_offer_one_name() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.first", "shared", false))
        .expect("mounts");
    // A different display name, so the error states the holder and not the
    // rejected plugin.
    let refused = ctx.mount(&WithTool {
        manifest: PluginManifest::new("test.second", "Mirrors", "0.1.0", &[], &[]),
        name: "shared",
        panics: false,
    });
    assert!(
        matches!(&refused, Err(PluginError::MountFailed(id, reason))
            if id == "test.second" && reason.contains("Echoes already offers a tool named shared")),
        "{refused:?}"
    );
    let output = ctx
        .call_ai_tool(
            "plugin_shared",
            &json!({ "word": "x" }),
            AiToolTier::ReadOnly,
            &idle(),
        )
        .expect("the first plugin's tool still runs");
    assert_eq!(output.text, "x from test.first");
}

/// The tier is read when the call runs: a tool re-registered with more trust
/// than the caller allowed does not run on the earlier allowance.
#[test]
fn a_call_allowed_at_a_lower_tier_does_not_run_a_more_trusted_tool() {
    let ctx = PluginContext::new();
    let held = ctx.register_ai_tool(spec("echo"), echo).expect("registers");
    assert_eq!(
        ctx.ai_tool("plugin_echo").map(|tool| tool.spec.tier),
        Some(AiToolTier::ReadOnly)
    );
    drop(held);
    assert_eq!(ctx.ai_tool("plugin_echo"), None);
    let _held = ctx
        .register_ai_tool(
            AiToolSpec {
                tier: AiToolTier::System,
                ..spec("echo")
            },
            echo,
        )
        .expect("registers again");

    assert_eq!(
        ctx.call_ai_tool(
            "plugin_echo",
            &json!({ "word": "x" }),
            AiToolTier::Workspace,
            &idle()
        ),
        Err(AiToolError::Failed(
            "plugin_echo changed since it was allowed and now needs System trust; call it again"
                .into()
        ))
    );
    assert_eq!(
        ctx.call_ai_tool(
            "plugin_echo",
            &json!({ "word": "x" }),
            AiToolTier::System,
            &idle()
        )
        .map(|output| output.text),
        Ok("x from host".to_owned())
    );
}
