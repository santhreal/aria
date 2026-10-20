//! AI tools: functions a plugin offers the assistant.
//!
//! A tool is a name, a description, a JSON Schema for its arguments, and a
//! handler. The assistant lists every registered tool beside its own and calls
//! one as `plugin_<name>`. The handler runs on the assistant's turn thread,
//! through the registering plugin's own view of the context, and returns text
//! and PNG images for the model. A handler that panics fails its plugin for the
//! session.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::listed::owner;
use crate::plugin::PluginManifest;
use crate::registration::{Contribution, ContributionKind, Registration};

/// The prefix of every plugin tool's name as the assistant calls it.
pub const AI_TOOL_PREFIX: &str = "plugin_";

/// Longest [`AiToolSpec::name`], in bytes. With [`AI_TOOL_PREFIX`] it stays
/// inside the 64 bytes model providers accept for a tool name.
pub const MAX_AI_TOOL_NAME: usize = 56;

/// Largest [`AiToolSpec::description`] plus the JSON text of
/// [`AiToolSpec::parameters`], in bytes. Every tool is sent with every request.
pub const MAX_AI_TOOL_SPEC: usize = 16 * 1024;

/// Widest and tallest image a tool may return, in pixels. A model request
/// carries every image a turn collected, so each one's size is bounded.
pub const MAX_AI_TOOL_IMAGE_SIDE: u32 = 2048;

/// The first eight bytes of every PNG file.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Width and height from a PNG's header chunk, or `None` for bytes that do
/// not start like a PNG.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.get(..24)?;
    if !header.starts_with(PNG_SIGNATURE) || &header[12..16] != b"IHDR" {
        return None;
    }
    let side = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    Some((side(16), side(20)))
}

/// How much trust a call needs before it runs, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AiToolTier {
    /// Reads state and changes nothing. Runs without asking.
    ReadOnly,
    /// Changes the editor or the project in a way an undo or another call
    /// reverses. Runs without asking.
    Workspace,
    /// Deletes data, writes outside the project, or reaches another system.
    /// Runs after the call is allowed on its confirmation card, unless the
    /// assistant's autonomy is Full.
    System,
}

impl AiToolTier {
    /// Every tier, least privileged first.
    pub const ALL: [Self; 3] = [Self::ReadOnly, Self::Workspace, Self::System];
}

/// What the assistant is told about one tool.
#[derive(Debug, Clone, PartialEq)]
pub struct AiToolSpec {
    /// Lowercase ASCII letters, digits, and `_`, at most [`MAX_AI_TOOL_NAME`]
    /// bytes, unique across the host. The assistant calls the tool as
    /// `plugin_<name>`.
    pub name: String,
    /// When to call the tool and what it returns, for the model.
    pub description: String,
    /// JSON Schema of the arguments: an object schema.
    pub parameters: serde_json::Value,
    pub tier: AiToolTier,
}

/// One PNG image a tool returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiToolImage {
    pub png: Vec<u8>,
    /// What the picture shows, in a few words.
    pub label: String,
}

/// What a call returns to the model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AiToolOutput {
    pub text: String,
    pub images: Vec<AiToolImage>,
}

impl AiToolOutput {
    /// Text and no images.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            images: Vec::new(),
        }
    }
}

/// A function the assistant can call.
pub trait AiTool: Send + Sync {
    /// Run the tool with `arguments`, an object matching the spec's schema.
    /// `ctx` is the registering plugin's view of the context. A long call
    /// returns an error once `cancel` is set. An `Err` is shown to the model
    /// as written.
    fn call(
        &self,
        ctx: &PluginContext,
        arguments: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<AiToolOutput, String>;
}

impl<F> AiTool for F
where
    F: Fn(&PluginContext, &serde_json::Value, &AtomicBool) -> Result<AiToolOutput, String>
        + Send
        + Sync,
{
    fn call(
        &self,
        ctx: &PluginContext,
        arguments: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<AiToolOutput, String> {
        self(ctx, arguments, cancel)
    }
}

/// One registered tool, as the assistant and the Plugins list read it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginAiTool {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    /// The registering plugin's name.
    pub plugin_name: Arc<str>,
    /// The name the assistant calls: `plugin_<name>`.
    pub call_name: String,
    pub spec: Arc<AiToolSpec>,
}

/// Why a call returned no output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiToolError {
    /// No tool is registered under this name.
    #[error("no plugin tool is registered as {0}")]
    Unknown(String),
    /// The tool returned this reason, or output the host does not accept.
    #[error("{0}")]
    Failed(String),
    /// The tool panicked with this message. Its plugin is failed.
    #[error("the tool panicked: {0}")]
    Panicked(String),
}

struct Entry {
    serial: u64,
    tool: PluginAiTool,
    /// The registering plugin's manifest, or `None` for the host.
    scope: Option<Arc<PluginManifest>>,
    handler: Arc<dyn AiTool>,
}

/// The tools registered on one context, in registration order.
#[derive(Default)]
pub(crate) struct AiTools {
    entries: RwLock<Vec<Entry>>,
    serial: AtomicU64,
    revision: AtomicU64,
}

/// Removes one tool when its registration drops.
struct Unregister {
    registry: Weak<AiTools>,
    serial: u64,
}

impl Drop for Unregister {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        // The handler drops after the lock is released, so its drop code may
        // read the registry.
        let removed: Vec<_> = registry
            .entries
            .write()
            .extract_if(.., |entry| entry.serial == self.serial)
            .collect();
        if !removed.is_empty() {
            registry.revision.fetch_add(1, Ordering::Release);
        }
        drop(removed);
    }
}

fn refused(reason: String) -> PluginError {
    PluginError::AiToolRefused(reason)
}

/// Refuses a spec the model or a provider could not use.
fn check(spec: &AiToolSpec) -> Result<(), PluginError> {
    let name = &spec.name;
    if name.is_empty()
        || name.len() > MAX_AI_TOOL_NAME
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(refused(format!(
            "{name:?}: a tool name is 1 to {MAX_AI_TOOL_NAME} lowercase ASCII letters, digits, \
             and `_`"
        )));
    }
    if spec.description.trim().is_empty() {
        return Err(refused(format!("{name}: a tool needs a description")));
    }
    let Some(schema) = spec.parameters.as_object() else {
        return Err(refused(format!(
            "{name}: the parameters are not a JSON Schema object"
        )));
    };
    if schema.get("type").and_then(serde_json::Value::as_str) != Some("object") {
        return Err(refused(format!(
            "{name}: the parameters schema must have \"type\": \"object\""
        )));
    }
    let properties = match schema.get("properties") {
        None => None,
        Some(properties) => Some(properties.as_object().ok_or_else(|| {
            refused(format!(
                "{name}: \"properties\" in the parameters schema is not an object"
            ))
        })?),
    };
    if let Some(required) = schema.get("required") {
        let required = required.as_array().ok_or_else(|| {
            refused(format!(
                "{name}: \"required\" in the parameters schema is not an array"
            ))
        })?;
        for field in required {
            let declared = field
                .as_str()
                .is_some_and(|field| properties.is_some_and(|props| props.contains_key(field)));
            if !declared {
                return Err(refused(format!(
                    "{name}: required argument {field} is not in \"properties\""
                )));
            }
        }
    }
    let size = spec.description.len() + spec.parameters.to_string().len();
    if size > MAX_AI_TOOL_SPEC {
        return Err(refused(format!(
            "{name}: the description and schema are {size} bytes, over {MAX_AI_TOOL_SPEC}"
        )));
    }
    Ok(())
}

impl PluginContext {
    /// Offer the assistant a tool.
    ///
    /// Held on the terms of [`register_command`](Self::register_command).
    /// Refused when the name is malformed or registered by any plugin, the
    /// description is empty, the parameters are not an object schema whose
    /// required arguments are declared, or the description and schema are
    /// over [`MAX_AI_TOOL_SPEC`] bytes.
    pub fn register_ai_tool(
        &self,
        spec: AiToolSpec,
        tool: impl AiTool + 'static,
    ) -> Result<Registration, PluginError> {
        check(&spec)?;
        let (plugin_id, plugin_name) = owner(self);
        let call_name = format!("{AI_TOOL_PREFIX}{}", spec.name);
        let registry = self.ai_tool_registry();
        let serial = {
            let mut entries = registry.entries.write();
            if let Some(held) = entries
                .iter()
                .find(|entry| entry.tool.call_name == call_name)
            {
                return Err(refused(format!(
                    "{} already offers a tool named {}",
                    held.tool.plugin_name, spec.name
                )));
            }
            let serial = registry.serial.fetch_add(1, Ordering::Relaxed);
            entries.push(Entry {
                serial,
                tool: PluginAiTool {
                    plugin_id,
                    plugin_name,
                    call_name: call_name.clone(),
                    spec: Arc::new(spec),
                },
                scope: self.scope(),
                handler: Arc::new(tool),
            });
            serial
        };
        registry.revision.fetch_add(1, Ordering::Release);
        let contribution = Contribution::new(ContributionKind::AiTool, call_name);
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                serial,
            },
        )))
    }

    /// Every registered tool, in registration order.
    pub fn ai_tools(&self) -> Vec<PluginAiTool> {
        self.ai_tool_registry()
            .entries
            .read()
            .iter()
            .map(|entry| entry.tool.clone())
            .collect()
    }

    /// Moves whenever a tool is registered or removed.
    pub fn ai_tools_revision(&self) -> u64 {
        self.ai_tool_registry().revision.load(Ordering::Acquire)
    }

    /// The tool the assistant calls `call_name`, if one is registered.
    pub fn ai_tool(&self, call_name: &str) -> Option<PluginAiTool> {
        self.ai_tool_registry()
            .entries
            .read()
            .iter()
            .find(|entry| entry.tool.call_name == call_name)
            .map(|entry| entry.tool.clone())
    }

    /// Run the tool the assistant calls `call_name`, which the caller allowed
    /// at `granted`.
    ///
    /// Fails without running when the tool registered now needs more trust
    /// than `granted`: a plugin reloaded between the caller's check and this
    /// call cannot raise its tier past it. The tool runs with no registry
    /// lock held. A panic fails the registering plugin for the session. An
    /// image that is not a PNG, or is over [`MAX_AI_TOOL_IMAGE_SIDE`] on a
    /// side, fails the call.
    pub fn call_ai_tool(
        &self,
        call_name: &str,
        arguments: &serde_json::Value,
        granted: AiToolTier,
        cancel: &AtomicBool,
    ) -> Result<AiToolOutput, AiToolError> {
        let found = self
            .ai_tool_registry()
            .entries
            .read()
            .iter()
            .find(|entry| entry.tool.call_name == call_name)
            .map(|entry| {
                (
                    entry.tool.spec.tier,
                    entry.scope.clone(),
                    Arc::clone(&entry.handler),
                )
            });
        let Some((tier, scope, handler)) = found else {
            return Err(AiToolError::Unknown(call_name.to_owned()));
        };
        if tier > granted {
            return Err(AiToolError::Failed(format!(
                "{call_name} changed since it was allowed and now needs {tier:?} trust; call it \
                 again"
            )));
        }
        let view = self.scoped(scope.clone());
        match catch_unwind(AssertUnwindSafe(|| handler.call(&view, arguments, cancel))) {
            Ok(Ok(output)) => {
                for image in &output.images {
                    match png_size(&image.png) {
                        None => {
                            return Err(AiToolError::Failed(format!(
                                "{call_name} returned image {:?}, which is not a PNG",
                                image.label
                            )));
                        }
                        Some((width, height))
                            if width > MAX_AI_TOOL_IMAGE_SIDE
                                || height > MAX_AI_TOOL_IMAGE_SIDE =>
                        {
                            return Err(AiToolError::Failed(format!(
                                "{call_name} returned image {:?} at {width}x{height}, over \
                                 {MAX_AI_TOOL_IMAGE_SIDE} pixels on a side",
                                image.label
                            )));
                        }
                        Some(_) => {}
                    }
                }
                Ok(output)
            }
            Ok(Err(reason)) => Err(AiToolError::Failed(reason)),
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                if let Some(manifest) = &scope {
                    self.fail(
                        &manifest.id,
                        format!("tool {call_name} panicked: {message}"),
                    );
                }
                Err(AiToolError::Panicked(message))
            }
        }
    }
}

#[cfg(test)]
#[path = "ai_tools_tests.rs"]
mod tests;
