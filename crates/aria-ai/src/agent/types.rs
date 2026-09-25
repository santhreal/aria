use aria_commands::EditOutcome;

use crate::provider::{ImagePart, Message};
use crate::wire::WireCommand;

/// Guardrail knobs.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Hard cap on edit-tool calls per prompt (the runaway-loop fuse).
    /// Reaching it does not fail the prompt: over-cap edits are refused
    /// and the run completes keeping the edits already applied.
    pub max_tool_calls: usize,
    /// Hard cap on host-tool calls per prompt. A separate fuse: senses
    /// and app control must not starve editing, nor the reverse.
    pub max_host_calls: usize,
    /// Hard cap on provider turns per prompt.
    pub max_turns: usize,
    /// Hard cap on images carried by one request, newest kept. Screenshot
    /// tools bound each image's dimensions, so count × bounded size caps
    /// the whole vision payload.
    pub max_images: usize,
    /// Hard cap on total encoded image bytes carried by one request. This
    /// protects the provider boundary even when an extensible host tool does
    /// not honor the normal screenshot dimension limits.
    pub max_image_bytes: usize,
    /// Validate and collect the plan without applying anything.
    pub dry_run: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_calls: 1000,
            max_host_calls: 200,
            max_turns: 200,
            max_images: 25,
            max_image_bytes: 24 * 1024 * 1024,
            dry_run: false,
        }
    }
}

/// One command the agent ran (or, in dry-run, plans to run).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionLogEntry {
    pub command: WireCommand,
    /// Human-readable line for the transcript / undo tooltip / eval
    /// assertions, e.g. `split clip 7 at 12.40s (new clip 21)`.
    pub description: String,
    /// The entity the step produced, when it produced one. A plan replays
    /// against a project where the rehearsal's entities no longer exist, so
    /// this is what a later step's reference to one is rewritten from.
    pub outcome: Option<EditOutcome>,
}

/// Streamed progress for the chat panel.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// Assistant text, as it streams.
    TextDelta(String),
    /// A provider-generated reasoning summary, kept out of model history.
    ReasoningDelta(String),
    /// An edit was applied (or validated, in dry-run).
    Action(ActionLogEntry),
    /// A host tool ran; `summary` is the first line of its output.
    HostAction { name: String, summary: String },
    /// One image returned by a successful host/sense tool, after runtime
    /// payload limits. Embedders can render it inline while the same encoded
    /// bytes continue to the model.
    Image(ImagePart),
}

/// How the prompt ended.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptStatus {
    /// Edits applied (possibly none) and recorded as one history entry.
    Completed,
    /// Dry-run: the plan in `actions` validated but nothing was applied.
    DryRun,
    /// This prompt's sandbox edits rolled back. Ordinary host effects may
    /// remain when a host dispatch was attempted; the string says why the
    /// prompt stopped.
    Aborted(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptOutcome {
    /// The model's final text answer (empty if it only edited).
    pub text: String,
    pub actions: Vec<ActionLogEntry>,
    /// Indices into `actions` where a committed phase ends (exclusive),
    /// from `commit_progress`. The tail past the last break is the final,
    /// implicit phase — never listed; empty means one phase. Callers group
    /// live replay by these; rehearsal and rollback stay one group per
    /// prompt.
    pub phase_breaks: Vec<usize>,
    pub status: PromptStatus,
    /// This turn's conversation, ready to append to the session history so
    /// the next prompt remembers it: the user message, every assistant
    /// turn and tool result the loop produced, and the final text answer.
    /// Empty when the prompt aborted (no conversational memory trace is
    /// retained, even though an already-dispatched host effect may remain).
    /// `describe_project` results are collapsed to a short placeholder —
    /// they're large and the fresh system snapshot supersedes them.
    pub turn_messages: Vec<Message>,
}
