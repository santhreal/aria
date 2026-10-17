//! AI tools a native plugin registers through `register_ai_tool`: the host
//! side of [`aria_plugin_abi::AriaPluginV1::ai_tool_call`].

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aria_plugin_abi::{ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaStr};
use serde::Deserialize;

use super::{HostCallContext, NativePluginState};
use crate::ai_tools::{AiTool, AiToolImage, AiToolOutput, AiToolSpec, AiToolTier};
use crate::context::PluginContext;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawTier {
    ReadOnly,
    Workspace,
    System,
}

#[derive(Deserialize)]
struct RawSpec {
    name: String,
    description: String,
    parameters: serde_json::Value,
    tier: RawTier,
}

/// The `ai_tool_call` value: an [`AiToolOutput`] as JSON.
#[derive(Deserialize)]
struct RawOutput {
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawImage {
    label: String,
    png: Vec<u8>,
}

/// Register the tool `spec_json` states for the plugin `ctx` is mounting.
pub(super) fn register(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    let raw: RawSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid AI tool spec JSON: {err}"))?;
    let tool = NativeAiTool {
        name: raw.name.clone(),
        state: Arc::clone(ctx.state),
    };
    ctx.scoped_ctx
        .register_ai_tool(
            AiToolSpec {
                name: raw.name,
                description: raw.description,
                parameters: raw.parameters,
                tier: match raw.tier {
                    RawTier::ReadOnly => AiToolTier::ReadOnly,
                    RawTier::Workspace => AiToolTier::Workspace,
                    RawTier::System => AiToolTier::System,
                },
            },
            tool,
        )
        .map(drop)
        .map_err(|err| err.to_string())
}

struct NativeAiTool {
    /// The name the spec registered, without the `plugin_` prefix.
    name: String,
    state: Arc<NativePluginState>,
}

impl AiTool for NativeAiTool {
    fn call(
        &self,
        ctx: &PluginContext,
        arguments: &serde_json::Value,
        _cancel: &AtomicBool,
    ) -> Result<AiToolOutput, String> {
        if self.state.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.state.id));
        }
        let arguments = arguments.to_string();
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: ctx,
            unscoped_ctx: ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, the name, and the arguments outlive `ai_tool_call`.
        let result = unsafe {
            (self.state.table.ai_tool_call)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(&self.name),
                AriaStr::from_str(&arguments),
            )
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => {
                let raw: RawOutput = serde_json::from_str(&text)
                    .map_err(|err| format!("{} returned invalid output JSON: {err}", self.name))?;
                Ok(AiToolOutput {
                    text: raw.text,
                    images: raw
                        .images
                        .into_iter()
                        .map(|image| AiToolImage {
                            png: image.png,
                            label: image.label,
                        })
                        .collect(),
                })
            }
            ARIA_STATUS_PANIC => {
                let reason = format!("AI tool {} panicked: {text}", self.name);
                self.state.mark_faulted(ctx, reason.clone());
                Err(reason)
            }
            _ => Err(text),
        }
    }
}
