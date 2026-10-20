//! The controls pressed on the Home screen: the tiles that start or replace
//! a session, one recent project's two actions, and the recovery offer.
//!
//! `Load project` claimed this screen and nothing could press it. The recovery
//! banner's Restore is the only control in the product that sends that command,
//! and it lived in a `card_action` closure, so a banner wired to the discard
//! path would have thrown away the crashed session's work with nothing red. The
//! reach row also claimed a cloud panel action that does not exist: the cloud
//! panel loads catalogs, never a project.
//!
//! Every row here replaces or opens a session, which is why they belong
//! together: each one is either a [`crate::app::unsaved::PendingIntent`] or the
//! recovery offer, and both are answered before anything is lost.
//!
//! The rails, the Learn lines and the cloud template feed are not here. A rail
//! selects which pane is drawn, a Learn line is text, and a catalog feed is a
//! fetch: none of them touches the project.

use std::path::PathBuf;

use aria_engine::EngineError;

use crate::app::AppState;

/// One press on Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The New Project tile.
    NewProject,
    /// The Open tile, which asks for a `.aria` file.
    PickProject,
    /// A recent card's Open.
    OpenRecent(PathBuf),
    /// A recent card's Relink, drawn in place of Open when the file has moved.
    RelinkRecent(PathBuf),
    /// The bundled sample, so a first run is not an empty timeline.
    OpenSample,
    /// The Use Template tile, which asks for a `.ariat` bundle.
    UseTemplate,
    /// The recovery banner's Restore: take back what the crash interrupted.
    RestoreRecovery,
    /// The recovery banner's Discard: throw the copy away.
    DiscardRecovery,
    /// A recent card's Reveal, which hands the path to the desktop.
    Reveal(PathBuf),
}

impl Pick {
    /// The pressed control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::NewProject => "a new project started",
            Self::PickProject => "a project chosen from the file dialog",
            Self::OpenRecent(_) => "a recent project opened",
            Self::RelinkRecent(_) => "a moved recent project relinked",
            Self::OpenSample => "the bundled sample opened",
            Self::UseTemplate => "a template bundle applied",
            Self::RestoreRecovery => "the recovered work restored",
            Self::DiscardRecovery => "the recovered work discarded",
            Self::Reveal(_) => "a project shown in the file manager",
        }
    }

    /// What pressing this row lands.
    ///
    /// A row is not required to send a command: starting a new project resets
    /// the engine's session rather than editing a project, so there is nothing
    /// to send and nothing to undo, and discarding the recovery copy only
    /// deletes a file beside the app's data. Declared here so a census asserts
    /// the right thing per row instead of holding every row to the loudest one.
    pub fn lands(&self) -> Lands {
        match self {
            Self::NewProject => Lands::Session,
            Self::PickProject
            | Self::OpenRecent(_)
            | Self::RelinkRecent(_)
            | Self::OpenSample
            | Self::UseTemplate
            | Self::RestoreRecovery => Lands::Command,
            Self::DiscardRecovery => Lands::Recovery,
            Self::Reveal(_) => Lands::Outside,
        }
    }

    /// Whether pressing this leaves the editor for another application.
    ///
    /// Declared at the row rather than listed in the census, so the default for
    /// a new row is to be measured. A suite that pressed [`Pick::Reveal`] would
    /// open a file manager window on whoever ran it, which is the one thing a
    /// test here must never do.
    pub fn leaves_the_editor(&self) -> bool {
        match self.lands() {
            Lands::Outside => true,
            Lands::Command | Lands::Session | Lands::Recovery => false,
        }
    }
}

/// Where a row's effect shows up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lands {
    /// A command the open project receives.
    Command,
    /// A fresh session: the engine is reset, so no command is sent and no undo
    /// entry exists to go back through.
    Session,
    /// The recovery copy on disk, and nothing in the project.
    Recovery,
    /// Another application, which is why a test must not press it.
    Outside,
}

/// Press one row.
///
/// Everything but the restore returns nothing to refuse: an intent is either run
/// or parked behind the unsaved-changes prompt, and a file dialog answers later.
/// The restore replaces the session, so it can fail the way opening a project
/// fails, and the caller reports that rather than each row doing it.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::NewProject => state.request_new_project(),
        Pick::PickProject => state.request_pick_project(),
        Pick::OpenRecent(path) => state.request_open_project(path),
        Pick::RelinkRecent(path) => crate::app::recents::relink_recent(state, &path),
        Pick::OpenSample => state.request_open_sample(),
        Pick::UseTemplate => state.pick_and_apply_template(),
        Pick::RestoreRecovery => return state.restore_recovery(),
        Pick::DiscardRecovery => state.discard_recovery(),
        Pick::Reveal(path) => crate::app::recents::reveal_in_folder(&path),
    }
    Ok(())
}
