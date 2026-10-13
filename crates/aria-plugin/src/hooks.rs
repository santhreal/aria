//! Shipped extension points: the events the host actually emits.
//!
//! A service is one value per type, so anything many plugins must see in order is
//! an event rather than a service. Two shapes are used here:
//!
//! - [`CommandProposed`] is a waterfall. Every handler sees the command in mount
//!   order and may rewrite it or veto it; the first veto stops the chain and the
//!   host reports the reason instead of applying anything.
//! - [`ResolveDroppedPath`] is serial. Handlers are asked in mount order and the
//!   first one that claims the path wins, so a `.aria` file opens as a project
//!   instead of being imported as media by whichever plugin looked first.

use std::path::PathBuf;
use std::sync::Arc;

use aria_commands::Command;
use aria_models::Project;

use crate::events::{SerialEvent, WaterfallEvent};
use crate::project_host::Origin;

/// A command about to be applied.
///
/// A handler may edit `command` in place, or set `veto` to refuse it. The reason
/// is shown to the user, so it names the lane, clip, or setting that caused the
/// refusal rather than the plugin that noticed.
///
/// `project` is the state the command will apply to. A handler reads it here
/// instead of locking the engine: the host emits this event with the engine
/// locked, so the project a handler sees cannot change before the command
/// lands, and locking the engine from a handler would deadlock.
pub struct CommandProposed {
    pub command: Command,
    /// The project the command applies to.
    pub project: Arc<Project>,
    /// Who proposed the command.
    pub origin: Origin,
    /// Why this command must not be applied. The first handler to set it ends the
    /// chain.
    pub veto: Option<String>,
}

impl CommandProposed {
    pub fn new(command: Command, project: Arc<Project>, origin: Origin) -> Self {
        Self {
            command,
            project,
            origin,
            veto: None,
        }
    }

    /// Refuse the command. Later handlers do not run, and the earlier ones have
    /// already had their say, so the first refusal is the one reported.
    pub fn refuse(&mut self, reason: impl Into<String>) {
        if self.veto.is_none() {
            self.veto = Some(reason.into());
        }
    }
}

impl WaterfallEvent for CommandProposed {
    fn should_stop(&self) -> bool {
        self.veto.is_some()
    }
}

/// A path dropped on the app, asking which plugin claims it.
pub struct ResolveDroppedPath {
    pub path: PathBuf,
}

impl SerialEvent for ResolveDroppedPath {}

/// What a plugin says should happen to a dropped path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAction {
    /// Open it as an Aria project.
    OpenProject,
    /// Register it in the media pool.
    ImportMedia,
    /// Load it as a 3D LUT.
    ApplyLut,
    /// Install it as an assistant skill.
    InstallSkill,
}
