//! AI tools a component registers through `register-ai-tool`: the host side
//! of the world's `ai-tool-call` export.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aria_plugin::{AiTool, AiToolImage, AiToolOutput, AiToolSpec, AiToolTier, PluginContext};

use crate::bindings;
use crate::host::WasmPluginShared;

pub(crate) struct WasmAiTool {
    shared: Arc<WasmPluginShared>,
    /// The name the spec registered, without the `plugin_` prefix.
    name: String,
}

impl WasmAiTool {
    pub(crate) fn new(shared: Arc<WasmPluginShared>, name: String) -> Self {
        Self { shared, name }
    }
}

/// The host spec for the component's `spec`, or why its schema is not JSON.
pub(crate) fn host_spec(spec: bindings::AiToolSpec) -> Result<AiToolSpec, String> {
    let parameters = serde_json::from_str(&spec.parameters_json).map_err(|err| {
        format!(
            "the parameters of AI tool {} are not JSON: {err}",
            spec.name
        )
    })?;
    Ok(AiToolSpec {
        name: spec.name,
        description: spec.description,
        parameters,
        tier: match spec.tier {
            bindings::AiToolTier::ReadOnly => AiToolTier::ReadOnly,
            bindings::AiToolTier::Workspace => AiToolTier::Workspace,
            bindings::AiToolTier::System => AiToolTier::System,
        },
    })
}

impl AiTool for WasmAiTool {
    fn call(
        &self,
        ctx: &PluginContext,
        arguments: &serde_json::Value,
        _cancel: &AtomicBool,
    ) -> Result<AiToolOutput, String> {
        let output = self
            .shared
            .call_ai_tool(ctx, &self.name, &arguments.to_string())?;
        Ok(AiToolOutput {
            text: output.text,
            images: output
                .images
                .into_iter()
                .map(|image| AiToolImage {
                    png: image.png,
                    label: image.label,
                })
                .collect(),
        })
    }
}
