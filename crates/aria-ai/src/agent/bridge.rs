use std::sync::atomic::AtomicBool;

use aria_commands::EditOutcome;

use crate::describe::{ProjectSummary, SummaryRequest};
use crate::tools::HostToolSpec;
use crate::wire::WireCommand;

/// The loop's only view of the engine. The UI implements this over a
/// sandbox engine whose validated plan replays onto the live one
/// (`aria-ui/src/agent.rs`); tests implement it over a plain `Engine`.
pub trait EngineBridge {
    /// Fresh summary of the project as it stands, covering the slice
    /// `request` names. Implemented with [`crate::describe::summarize_for`],
    /// which decides whether the project fits one description.
    fn summary(&mut self, request: SummaryRequest) -> ProjectSummary;
    /// Read-only tools that inspect this exact engine state. Unlike ordinary
    /// host tools, these travel with the sandbox bridge so a screenshot taken
    /// after an edit observes the rehearsed project, not the live project.
    ///
    /// The loop accepts only [`crate::tools::ToolTier::ReadOnly`] specs here.
    fn sense_tools(&self) -> Vec<HostToolSpec> {
        Vec::new()
    }
    /// Execute one tool previously returned by [`EngineBridge::sense_tools`].
    /// Implementations must not mutate project state.
    fn sense(
        &mut self,
        name: &str,
        _arguments: &serde_json::Value,
        _cancel: &AtomicBool,
    ) -> Result<crate::tools::ToolOutput, String> {
        Err(format!("unknown engine sense '{name}'"))
    }
    /// Prepare for an ordinary registered [`crate::tools::ToolHost`] call.
    ///
    /// The loop invokes this after charging the host-call cap, but before
    /// authorization or dispatch. Returning `Err` rejects the call without
    /// invoking either [`crate::tools::ToolHost::authorize`],
    /// [`crate::tools::ToolHost::call`], or
    /// [`EngineBridge::after_host_call`]. Bridge-owned read-only senses do
    /// not pass through this hook.
    fn before_host_call(
        &mut self,
        _name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }
    /// Reconcile bridge state after an ordinary host dispatch was attempted.
    ///
    /// The loop invokes this exactly once after [`crate::tools::ToolHost::call`] returns,
    /// for both success and failure, and before exposing that result to the
    /// model. Authorization failures and pre-call rejections do not invoke
    /// it. `result` borrows the host result so implementations can inspect
    /// success or failure without cloning [`crate::tools::ToolOutput`].
    ///
    /// Host calls may have partial side effects even when they return `Err`.
    /// This hook is therefore the bridge's reconciliation boundary. A hook
    /// failure aborts the prompt and rolls back its sandbox edit group, but
    /// cannot promise to undo effects the host already performed.
    fn after_host_call(
        &mut self,
        _name: &str,
        _arguments: &serde_json::Value,
        _result: Result<&crate::tools::ToolOutput, &str>,
    ) -> Result<(), String> {
        Ok(())
    }
    /// Validate + apply one wire command. `Err` is a model-readable reason
    /// (validation rejection or engine error); state is unchanged on `Err`.
    fn apply(&mut self, command: &WireCommand) -> Result<Applied, String>;
    /// The dry-run path: validate, then apply into the open group so the next
    /// step is validated against the plan so far. The group is rolled back
    /// before the run returns, so nothing survives the rehearsal. Host work is
    /// never started, and `None` says so: a dry run must not spend an encoder,
    /// so what a render would have produced is not part of the projection.
    fn rehearse(&mut self, command: &WireCommand) -> Result<Option<EditOutcome>, String>;
    fn begin_group(&mut self);
    fn end_group(&mut self);
    fn rollback_group(&mut self);
}

/// What applying one wire command did.
///
/// Two shapes because lowering has two: an edit landed in the project, or work
/// was started that no turn can wait for. The queued arm carries the shell's
/// own line, because only the shell knows what it started — and reporting a
/// render as finished while the file is still being written is the one thing
/// this seam exists to prevent.
#[derive(Debug, Clone)]
pub enum Applied {
    /// The edit is in the project, and this is what the engine reported.
    Edited(EditOutcome),
    /// Work was started outside the turn. The line says what started.
    Queued(String),
}

impl Applied {
    /// What the engine reported, or `None` for work that was only started.
    pub fn edited(&self) -> Option<&EditOutcome> {
        match self {
            Self::Edited(outcome) => Some(outcome),
            Self::Queued(_) => None,
        }
    }
}
