//! Internal state and helpers for [`super::ProjectHost`].

use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::Arc;
use std::thread::ThreadId;

use aria_commands::{Command, ProjectCommand};
use aria_engine::{ApplyOutcome, Engine, EngineError};
use aria_models::Project;

use super::{CommandApplied, Origin, TransactError};
use crate::editor_events::{MediaImported, MediaRelinked, ProjectClosed, ProjectOpened, ProjectSaved};
use crate::events::EventBus;

/// A notification held with the open group and posted once nothing can roll it
/// back. A rolled-back group posts none of its own.
pub(super) enum Announcement {
    Applied(CommandApplied),
    Imported(MediaImported),
    Relinked(MediaRelinked),
    Saved(ProjectSaved),
    Replaced(ProjectClosed, ProjectOpened),
}

impl Announcement {
    pub(super) fn post(self, bus: &EventBus) {
        match self {
            Self::Applied(event) => bus.post(event),
            Self::Imported(event) => bus.post(event),
            Self::Relinked(event) => bus.post(event),
            Self::Saved(event) => bus.post(event),
            Self::Replaced(closed, opened) => {
                bus.post(closed);
                bus.post(opened);
            }
        }
    }
}

/// What one applied command announces: `CommandApplied` when anything reads
/// it, and the editor event its outcome implies, if any.
pub(super) type Announced = [Option<Announcement>; 2];

/// Who holds the project, and what they have open.
#[derive(Default)]
pub(super) struct Sequence {
    /// The editing thread and who it edits for, while `holds > 0`.
    pub(super) owner: Option<(ThreadId, Origin)>,
    /// Nested holds by the owner: one per open group plus one per command in
    /// flight.
    pub(super) holds: usize,
    /// Open groups, savepoints included.
    pub(super) groups: usize,
    /// Announcements from commands applied inside the open groups, posted when
    /// the outermost one commits and dropped when the group they belong to
    /// rolls back.
    pub(super) announced: Vec<Announcement>,
    pub(super) session_changes: Vec<crate::host_wire::SessionChange>,
}

pub(super) enum PendingStorageOp {
    Set {
        plugin_id: String,
        key: String,
        value: Vec<u8>,
    },
    Delete {
        plugin_id: String,
        key: String,
    },
}

struct ProposingState {
    project: Arc<Project>,
    revision: u64,
    storage_ops: Vec<PendingStorageOp>,
}

thread_local! {
    /// The project, revision, and any queued plugin storage writes for a
    /// `CommandProposed` dispatch on this thread while one runs. The engine is
    /// locked for the dispatch, so reads and plugin storage calls use this
    /// instead of locking the engine again.
    static PROPOSING: RefCell<Option<ProposingState>> = const { RefCell::new(None) };
}

/// Marks this thread as dispatching `CommandProposed` until dropped, including
/// when a handler panics.
pub(super) struct Proposing;

impl Proposing {
    pub(super) fn enter(project: Arc<Project>, revision: u64) -> Self {
        PROPOSING.with(|slot| {
            *slot.borrow_mut() = Some(ProposingState {
                project,
                revision,
                storage_ops: Vec::new(),
            });
        });
        Self
    }
}

impl Drop for Proposing {
    fn drop(&mut self) {
        PROPOSING.with(|slot| slot.borrow_mut().take());
    }
}

pub(super) fn proposing() -> Option<(Arc<Project>, u64)> {
    PROPOSING.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|state| (Arc::clone(&state.project), state.revision))
    })
}

pub(super) fn in_handler() -> bool {
    PROPOSING.with(|slot| slot.borrow().is_some())
}

pub(super) fn proposing_storage_set(plugin_id: &str, key: &str, value: Vec<u8>) -> Option<bool> {
    PROPOSING.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let state = borrow.as_mut()?;
        let changed = Arc::make_mut(&mut state.project).plugin_storage_set(
            plugin_id,
            key,
            value.clone(),
        );
        if changed {
            state.storage_ops.push(PendingStorageOp::Set {
                plugin_id: plugin_id.to_owned(),
                key: key.to_owned(),
                value,
            });
        }
        Some(changed)
    })
}

pub(super) fn proposing_storage_delete(plugin_id: &str, key: &str) -> Option<bool> {
    PROPOSING.with(|slot| {
        let mut borrow = slot.borrow_mut();
        let state = borrow.as_mut()?;
        let removed = Arc::make_mut(&mut state.project).plugin_storage_delete(plugin_id, key);
        if removed {
            state.storage_ops.push(PendingStorageOp::Delete {
                plugin_id: plugin_id.to_owned(),
                key: key.to_owned(),
            });
        }
        Some(removed)
    })
}

pub(super) fn flush_proposing_storage_ops(engine: &mut Engine) {
    let ops = PROPOSING.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .map(|state| std::mem::take(&mut state.storage_ops))
            .unwrap_or_default()
    });
    for op in ops {
        match op {
            PendingStorageOp::Set {
                plugin_id,
                key,
                value,
            } => {
                engine.set_plugin_storage(&plugin_id, &key, value);
            }
            PendingStorageOp::Delete { plugin_id, key } => {
                engine.delete_plugin_storage(&plugin_id, &key);
            }
        }
    }
}

/// Whether `command` edits the session in a way history can undo.
///
/// Exhaustive over project commands, so a new one does not compile until it
/// is classified.
pub(super) fn transactional(command: &Command) -> Result<(), TransactError> {
    let Command::Project(project) = command else {
        return Ok(());
    };
    match project {
        ProjectCommand::Import { .. }
        | ProjectCommand::RemoveMedia { .. }
        | ProjectCommand::SetMediaLabel { .. } => Ok(()),
        ProjectCommand::Save { .. }
        | ProjectCommand::Open { .. }
        | ProjectCommand::Load { .. }
        | ProjectCommand::RelinkMedia { .. }
        | ProjectCommand::Export { .. }
        | ProjectCommand::SaveTemplate { .. }
        | ProjectCommand::ApplyTemplate { .. } => Err(TransactError::NotUndoable {
            label: command.label(),
        }),
    }
}

/// Whether `command` replaces the session, so its outcome closes one project
/// and opens another.
pub(super) fn replaces_session(command: &Command) -> bool {
    matches!(
        command,
        Command::Project(
            ProjectCommand::Open { .. }
                | ProjectCommand::Load { .. }
                | ProjectCommand::ApplyTemplate { .. }
        )
    )
}

/// The editor event `outcome` implies, read from the engine right after it.
/// `closed` is the session the command replaced, when it replaced one.
///
/// Exhaustive over outcomes, so a new one does not compile until it is
/// classified.
pub(super) fn derived(
    outcome: &ApplyOutcome,
    engine: &Engine,
    closed: Option<ProjectClosed>,
) -> Option<Announcement> {
    let media_path = |media| {
        engine
            .project()
            .media(media)
            .map(|source| source.path.clone())
    };
    match outcome {
        ApplyOutcome::Imported { media } => media_path(*media).map(|path| {
            Announcement::Imported(MediaImported {
                media: *media,
                path,
            })
        }),
        ApplyOutcome::Relinked { media } => media_path(*media).map(|path| {
            Announcement::Relinked(MediaRelinked {
                media: *media,
                path,
            })
        }),
        ApplyOutcome::Saved => engine
            .project_path()
            .map(|path| Announcement::Saved(ProjectSaved { path: path.clone() })),
        ApplyOutcome::Opened | ApplyOutcome::Loaded | ApplyOutcome::AppliedTemplate => {
            Some(Announcement::Replaced(
                closed.unwrap_or(ProjectClosed { path: None }),
                ProjectOpened {
                    path: engine.project_path().cloned(),
                    project: engine.snapshot(),
                },
            ))
        }
        ApplyOutcome::RemovedMedia { .. }
        | ApplyOutcome::LabeledMedia { .. }
        | ApplyOutcome::Exported { .. }
        | ApplyOutcome::SavedTemplate
        | ApplyOutcome::Edited(_) => None,
    }
}

/// Why one proposed-and-applied command did not land.
pub(super) enum Rejection {
    Refused(String),
    Engine(EngineError),
}

/// The refusal an edit gets while another thread holds the project.
pub(super) fn busy(holder: &Origin) -> EngineError {
    EngineError::Refused(
        TransactError::Busy {
            holder: holder.clone(),
        }
        .to_string(),
    )
}

/// How far an open group reaches back, to roll back to.
#[derive(Clone, Copy)]
pub(super) struct Marks {
    /// Steps the enclosing group held when this one opened; zero when outermost.
    pub(super) steps: usize,
    /// Announcements queued when this one opened.
    pub(super) announced: usize,
    /// The engine revision when this one opened.
    pub(super) revision: u64,
}

pub(super) enum Finish {
    Commit(Option<Cow<'static, str>>),
    Rollback,
}
