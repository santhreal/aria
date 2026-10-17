//! Tools a plugin offers the Aria assistant: the spec it registers with
//! [`Host::register_ai_tool`](crate::Host::register_ai_tool) and the output
//! [`Plugin::ai_tool_call`](crate::Plugin::ai_tool_call) returns.

use serde::{Deserialize, Serialize};

/// The trust a tool needs before the assistant runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiToolTier {
    /// Reads state and changes nothing. Runs without asking.
    ReadOnly,
    /// Changes the project or the editor in a way an undo reverses. Runs
    /// without asking.
    Workspace,
    /// Deletes data, writes outside the project, or reaches another system.
    /// Runs after the operator allows the call, unless autonomy is Full.
    System,
}

/// One tool, as the assistant lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiToolSpec {
    /// 1 to 56 bytes of lowercase ASCII letters, digits, and `_`. The
    /// assistant calls the tool `plugin_<name>`.
    pub name: String,
    /// What the tool does, for the model.
    pub description: String,
    /// A JSON Schema object describing the arguments.
    pub parameters: serde_json::Value,
    /// The trust a call needs.
    pub tier: AiToolTier,
}

/// A PNG image a tool returns to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiToolImage {
    /// A few words naming the image.
    pub label: String,
    /// PNG bytes, at most 2048 pixels on a side.
    pub png: Vec<u8>,
}

/// What a tool returns to the model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiToolOutput {
    /// The text the model reads.
    pub text: String,
    /// Images the model sees after the text.
    pub images: Vec<AiToolImage>,
}

impl AiToolOutput {
    /// Output of `text` and no images.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            images: Vec::new(),
        }
    }
}
