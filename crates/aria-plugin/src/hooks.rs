//! Shipped extension points: the events the host actually emits.
//!
//! A service is one value per type, so anything many plugins must see in order is
//! an event rather than a service. Two shapes are used here:
//!
//! - [`CommandProposed`] is a waterfall. Every handler sees the command in mount
//!   order and may rewrite it or veto it; the first veto stops the chain and the
//!   host reports the reason instead of applying anything.
//! - [`ResolveDropped`] is serial. Handlers are asked by priority (highest
//!   first, ties in mount order) and the first one that claims the dropped path
//!   or URL wins, so a `.aria` file opens as a project instead of being
//!   imported as media by whichever plugin looked first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_commands::Command;
use aria_models::Project;

use crate::events::{SerialEvent, WaterfallEvent};
use crate::project_host::Origin;
use serde::{Deserialize, Serialize};

/// A command about to be applied.
///
/// A handler may edit `command` in place, or set `veto` to refuse it. The reason
/// is shown in the editor, so it states the lane, clip, or setting that caused
/// the refusal rather than the plugin that detected it.
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

/// A filesystem path or URL dropped onto the editor window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dropped {
    Path(PathBuf),
    Url(String),
}

impl Dropped {
    /// Borrow the dropped path when `self` is [`Dropped::Path`].
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path.as_path()),
            Self::Url(_) => None,
        }
    }

    /// Borrow the dropped URL when `self` is [`Dropped::Url`].
    pub fn as_url(&self) -> Option<&str> {
        match self {
            Self::Path(_) => None,
            Self::Url(url) => Some(url.as_str()),
        }
    }
}

/// A path or URL dropped on the app, asking which plugin claims it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveDropped {
    pub item: Dropped,
}

impl ResolveDropped {
    /// Build a [`ResolveDropped`] event for a filesystem path.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self {
            item: Dropped::Path(path.into()),
        }
    }

    /// Build a [`ResolveDropped`] event for a URL.
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            item: Dropped::Url(url.into()),
        }
    }
}

impl SerialEvent for ResolveDropped {}

/// What a plugin says should happen to a dropped path or URL.
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
    /// Read it into the open project through the importer registered for its
    /// extension.
    ImportTimeline,
    /// A plugin handled the dropped item directly.
    Handled,
}
