//! The project host: the one path that reads and edits the project.
//!
//! Every edit, from the editor, the assistant, or a plugin, goes through
//! [`ProjectHost`]. A command is proposed to every [`CommandProposed`] handler
//! with the engine locked, applied, and announced as a [`CommandApplied`] once
//! nothing can roll it back, together with the editor event its outcome
//! implies: [`MediaImported`], [`ProjectSaved`], and the rest. A group
//! ([`ProjectHost::group`]) or a transaction ([`ProjectHost::transact_with`])
//! applies any number of commands as one undo step.
//! One thread edits at a time, waiting up to [`ProjectHost::CONTENDED_WAIT`]
//! when another thread holds the project.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_commands::{Command, ProjectCommand};
use aria_engine::{ApplyOutcome, Engine, EngineError};
use aria_models::Project;
use parking_lot::{Condvar, Mutex};

use crate::context::PluginContext;
#[cfg(doc)]
use crate::editor_events::MediaImported;
#[cfg(doc)]
use crate::editor_events::ProjectSaved;
use crate::editor_events::{
    ExportFinished, ExportStarted, ProjectClosed, ProjectOpened, RenderResult,
};
use crate::events::{Event, EventBus, panic_message};
use crate::hooks::CommandProposed;

#[path = "project_host_inner.rs"]
mod inner;
use inner::*;
/// Who proposed a command.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A gesture, menu item, or key.
    Editor,
    /// The assistant, during a turn or replaying a plan.
    Assistant,
    /// A plugin, by id.
    Plugin(Arc<str>),
}

impl Origin {
    pub fn plugin(id: &str) -> Self {
        Self::Plugin(Arc::from(id))
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Editor => f.write_str("the editor"),
            Self::Assistant => f.write_str("the assistant"),
            Self::Plugin(id) => write!(f, "plugin {id}"),
        }
    }
}

/// A command that was applied and can no longer be rolled back by the group
/// it was part of. Posted after the outermost group commits.
#[derive(Debug, Clone)]
pub struct CommandApplied {
    pub command: Command,
    pub outcome: ApplyOutcome,
    /// The engine revision right after this command.
    pub revision: u64,
    pub origin: Origin,
}

impl Event for CommandApplied {}

/// Why a transaction did not apply. Nothing it applied remains.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransactError {
    /// Another thread held the project past the contended wait.
    #[error("{holder} is editing the project; try again when it finishes")]
    Busy { holder: Origin },
    /// The command replaces or writes the session rather than editing it, so it
    /// cannot be part of an undo step.
    #[error("{label} cannot run inside a group or a transaction: it is not an undoable edit")]
    NotUndoable { label: &'static str },
    /// A `CommandProposed` handler refused the command.
    #[error("{label} was refused: {reason}")]
    Refused { label: &'static str, reason: String },
    /// The engine rejected the command.
    #[error("{label} failed: {reason}")]
    Failed { label: &'static str, reason: String },
    /// The transaction body returned an error of its own.
    #[error("{0}")]
    Aborted(String),
    /// The transaction body panicked; the message is the panic payload.
    #[error("the transaction panicked: {0}")]
    Panicked(String),
    /// An edit was started from a `CommandProposed` handler, which runs with
    /// the engine locked.
    #[error(
        "an edit cannot start inside a CommandProposed handler; apply it after the handler returns"
    )]
    Reentrant,
}

/// What a committed transaction produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The engine revision after the transaction.
    pub revision: u64,
    /// One outcome per applied command, in order.
    pub outcomes: Vec<ApplyOutcome>,
}

/// The engine revisions a return to an earlier project spanned, for a cache
/// keyed by revision: at `left`, the project equals the one at `opened`, so
/// only the changes made between `opened` and `found` are changed back. A
/// rolled-back group, an undo and a redo each report one; a history jump
/// reports one for each step it takes. A return that changed nothing leaves
/// `found` and `left` equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reverted {
    /// The engine revision whose project the return restored.
    pub opened: u64,
    /// The engine revision just before the return.
    pub found: u64,
    /// The engine revision after the return.
    pub left: u64,
}

/// The project host. Registered by the engine plugin; the editor, the
/// assistant, and every plugin edit through the same value.
pub struct ProjectHost {
    engine: Arc<Mutex<Engine>>,
    events: Arc<EventBus>,
    sequence: Mutex<Sequence>,
    released: Condvar,
    storage_waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl fmt::Debug for ProjectHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectHost").finish_non_exhaustive()
    }
}

/// One hold on the project by this thread, for a single command. Released on
/// drop, so an error or a panic cannot leave the project held.
struct Hold<'a> {
    host: &'a ProjectHost,
    released: bool,
}

impl Hold<'_> {
    /// Release the hold, queueing `announced` with the open group or returning
    /// it with everything else now ready to post.
    fn release(mut self, announced: impl IntoIterator<Item = Announcement>) -> Vec<Announcement> {
        self.released = true;
        self.host.leave(announced)
    }
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        if !self.released {
            // Only an error or a panic gets here, and neither applied anything
            // to announce.
            let _ = self.host.leave([]);
        }
    }
}

impl ProjectHost {
    /// How long an edit waits for another thread's edit to finish before it is
    /// refused. Long enough for any transaction of ordinary size; short enough
    /// that a refused gesture does not read as a frozen window.
    pub const CONTENDED_WAIT: Duration = Duration::from_millis(100);

    /// A host over `engine`, proposing and announcing on `ctx`'s event bus.
    ///
    /// The host keeps the bus, not the context, so registering the host in that
    /// context does not make the two own each other.
    pub fn new(engine: Arc<Mutex<Engine>>, ctx: &PluginContext) -> Self {
        Self {
            engine,
            events: Arc::clone(ctx.event_bus()),
            sequence: Mutex::new(Sequence::default()),
            released: Condvar::new(),
            storage_waker: Mutex::new(None),
        }
    }

    pub(crate) fn set_storage_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.storage_waker.lock() = Some(Arc::new(waker));
    }

    /// The engine. Rendering and reading go through it directly; edits go
    /// through [`apply`](Self::apply), [`group`](Self::group), and
    /// [`transact_with`](Self::transact_with).
    pub fn engine(&self) -> &Arc<Mutex<Engine>> {
        &self.engine
    }

    /// The project as of the last applied command, as an immutable value.
    ///
    /// Inside a `CommandProposed` handler this is the project the proposed
    /// command applies to.
    pub fn snapshot(&self) -> Arc<Project> {
        match proposing() {
            Some((project, _)) => project,
            None => self.engine.lock().snapshot(),
        }
    }

    /// The engine revision: bumped by every change to the project.
    pub fn revision(&self) -> u64 {
        match proposing() {
            Some((_, revision)) => revision,
            None => self.engine.lock().revision(),
        }
    }

    /// The file path of the open project, or `None` when unsaved.
    pub fn project_path(&self) -> Option<PathBuf> {
        self.engine.lock().project_path().cloned()
    }

    /// Whether the open project has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.engine.lock().is_dirty()
    }

    /// Look up `id` across the open sequence, root timeline, and nested sequences.
    pub fn clip(&self, id: aria_models::ClipId) -> Option<crate::ClipRef> {
        crate::clip(&self.snapshot(), id)
    }

    /// Clips on `sequence` (`None` for the open sequence) overlapping the half-open
    /// `range`, in track order then timeline start order.
    pub fn clips_in_range(
        &self,
        sequence: Option<aria_models::SequenceId>,
        range: aria_core::TimeRange,
    ) -> Vec<crate::ClipRef> {
        crate::clips_in_range(&self.snapshot(), sequence, range)
    }

    /// Clips on `sequence` (`None` for the open sequence) covering `tick`,
    /// in track order then timeline start order.
    pub fn clips_at(
        &self,
        sequence: Option<aria_models::SequenceId>,
        tick: aria_core::RationalTime,
    ) -> Vec<crate::ClipRef> {
        crate::clips_at(&self.snapshot(), sequence, tick)
    }

    /// Resolve `selection` to [`ClipRef`](crate::ClipRef)s, dropping any clip
    /// ids no longer in the project.
    pub fn selected_clips(&self, selection: &[aria_models::ClipId]) -> Vec<crate::ClipRef> {
        crate::selected_clips(&self.snapshot(), selection)
    }

    /// Ruler markers on `sequence` (`None` for the open sequence) overlapping
    /// the half-open `range`, in timeline order.
    pub fn markers_in_range(
        &self,
        sequence: Option<aria_models::SequenceId>,
        range: aria_core::TimeRange,
    ) -> Vec<aria_models::Marker> {
        crate::markers_in_range(&self.snapshot(), sequence, range)
    }

    /// Who holds the project from another thread, if anyone.
    pub fn holder(&self) -> Option<Origin> {
        let me = std::thread::current().id();
        let sequence = self.sequence.lock();
        sequence
            .owner
            .as_ref()
            .filter(|(thread, _)| *thread != me)
            .map(|(_, origin)| origin.clone())
    }

    /// Take the project for this thread as `origin`, waiting up to
    /// [`CONTENDED_WAIT`](Self::CONTENDED_WAIT) for another thread to finish.
    /// On refusal, returns who holds it.
    fn enter(&self, origin: &Origin) -> Result<(), Origin> {
        let me = std::thread::current().id();
        let deadline = Instant::now() + Self::CONTENDED_WAIT;
        let mut sequence = self.sequence.lock();
        loop {
            let holder = match &sequence.owner {
                Some((thread, _)) if *thread == me => break,
                Some((_, holder)) => holder.clone(),
                None => {
                    sequence.owner = Some((me, origin.clone()));
                    break;
                }
            };
            if Instant::now() >= deadline {
                return Err(holder);
            }
            self.released.wait_until(&mut sequence, deadline);
        }
        sequence.holds += 1;
        Ok(())
    }

    /// Give back one hold. Queues `announced` while a group is open; otherwise
    /// returns it, with any announcements the closing group left, to post.
    fn leave(&self, announced: impl IntoIterator<Item = Announcement>) -> Vec<Announcement> {
        let mut sequence = self.sequence.lock();
        sequence.announced.extend(announced);
        sequence.holds -= 1;
        let ready = if sequence.groups == 0 {
            std::mem::take(&mut sequence.announced)
        } else {
            Vec::new()
        };
        if sequence.holds == 0 {
            sequence.owner = None;
            drop(sequence);
            self.released.notify_all();
        }
        ready
    }

    fn hold(&self, origin: &Origin) -> Result<Hold<'_>, Origin> {
        self.enter(origin)?;
        Ok(Hold {
            host: self,
            released: false,
        })
    }

    fn announce(&self, announced: Vec<Announcement>) {
        let mut saved_path = None;
        for announcement in announced {
            if let Announcement::Saved(saved) = &announcement {
                saved_path = Some(saved.path.clone());
            }
            announcement.post(&self.events);
        }
        if let Some(path) = saved_path
            && (self
                .events
                .has_handlers::<crate::editor_events::ProjectSaved>()
                || self.events.has_handlers::<CommandApplied>())
        {
            let engine = Arc::clone(&self.engine);
            self.events.defer(move || {
                let _ = engine.lock().finish_post_save(&path);
            });
            let _ = self.events.flush(Duration::from_secs(5));
        }
    }

    /// Propose `command` to every handler. The caller holds the project turn.
    fn propose(&self, command: Command, origin: &Origin) -> Result<Command, String> {
        let (project, revision) = {
            let engine = self.engine.lock();
            (engine.snapshot(), engine.revision())
        };
        let _proposing = Proposing::enter(Arc::clone(&project), revision);
        let mut proposed = CommandProposed::new(command, project, origin.clone());
        self.events.emit_waterfall(&mut proposed);
        {
            let mut engine = self.engine.lock();
            flush_proposing_storage_ops(&mut engine);
        }
        // The event and the stash drop when this returns, so the apply that
        // follows copies the project only if a handler kept a reference.
        match proposed.veto {
            Some(reason) => Err(reason),
            None => Ok(proposed.command),
        }
    }

    /// Propose and apply one command while holding the project turn. `probed`
    /// is the probe of the file an import or a relink names, taken by the
    /// caller; `None` takes it here.
    fn apply_locked<B, R>(
        &self,
        origin: &Origin,
        command: Command,
        probed: Option<aria_engine::Probed>,
        before: impl FnOnce(&Command, &Engine) -> B,
        after: impl FnOnce(B, &ApplyOutcome, &Engine) -> R,
    ) -> Result<(ApplyOutcome, R, Announced), Rejection> {
        let command = self.propose(command, origin).map_err(Rejection::Refused)?;
        // A probe opens the file, so it runs before the engine is locked.
        let probed = probed.or_else(|| {
            aria_engine::Probed::ahead(&command, |path| {
                self.engine
                    .lock()
                    .project()
                    .find_media_by_path(path)
                    .is_some()
            })
        });
        let mut engine = self.engine.lock();
        let engine = &mut *engine;
        let applied = self
            .events
            .has_handlers::<CommandApplied>()
            .then(|| command.clone());
        let closed = replaces_session(&command).then(|| ProjectClosed {
            path: engine.project_path().cloned(),
        });
        // An export cannot be part of a group, so nothing can roll it back and
        // its events are posted as it runs.
        let export = match &command {
            Command::Project(ProjectCommand::Export { path, .. }) => Some(path.clone()),
            _ => None,
        };
        if let Some(path) = &export {
            self.events.post(ExportStarted { path: path.clone() });
        }
        let observed = before(&command, engine);
        let outcome = engine.apply_probed(command, probed);
        if let Some(path) = export {
            let result = match &outcome {
                Ok(_) => RenderResult::Written,
                Err(err) => RenderResult::Failed(err.to_string()),
            };
            self.events.post(ExportFinished { path, result });
        }
        let outcome = outcome.map_err(Rejection::Engine)?;
        let result = after(observed, &outcome, engine);
        let applied = applied.map(|command| {
            Announcement::Applied(CommandApplied {
                command,
                outcome: outcome.clone(),
                revision: engine.revision(),
                origin: origin.clone(),
            })
        });
        let derived = derived(&outcome, engine, closed);
        crate::host_wire::record_session_outcome(
            &mut self.sequence.lock().session_changes,
            origin,
            &outcome,
            engine,
        );
        Ok((outcome, result, [applied, derived]))
    }

    /// Apply one command as its own undo step, or as part of this thread's
    /// open group.
    pub fn apply(&self, origin: &Origin, command: Command) -> Result<ApplyOutcome, EngineError> {
        self.apply_observed(origin, command, None, |_, _| (), |(), _, _| ())
            .map(|(outcome, ())| outcome)
    }

    /// [`apply`](Self::apply), with observers that run under the engine lock:
    /// `before` sees the command the handlers settled on and the engine it
    /// applies to, `after` sees the outcome and the engine after it.
    /// `probed` is the probe of the file an import or a relink names, taken
    /// by the caller ([`aria_engine::Probed::ahead_all`]); `None` takes it
    /// before the engine is locked.
    ///
    /// # Panics
    ///
    /// When called from a `CommandProposed` handler.
    pub fn apply_observed<B, R>(
        &self,
        origin: &Origin,
        command: Command,
        probed: Option<aria_engine::Probed>,
        before: impl FnOnce(&Command, &Engine) -> B,
        after: impl FnOnce(B, &ApplyOutcome, &Engine) -> R,
    ) -> Result<(ApplyOutcome, R), EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let hold = self.hold(origin).map_err(|holder| busy(&holder))?;
        // A command that clears history would take the open group with it.
        if self.sequence.lock().groups > 0 {
            transactional(&command).map_err(|err| EngineError::Refused(err.to_string()))?;
        }
        let applied = self.apply_locked(origin, command, probed, before, after);
        let (outcome, result, announced) = match applied {
            Ok(applied) => applied,
            Err(Rejection::Refused(reason)) => return Err(EngineError::Refused(reason)),
            Err(Rejection::Engine(err)) => return Err(err),
        };
        self.announce(hold.release(announced.into_iter().flatten()));
        Ok((outcome, result))
    }

    /// Open a group: every command this thread applies until it is committed
    /// becomes one undo step. A group opened inside another is a savepoint of
    /// it. Dropping the group without committing rolls it back.
    ///
    /// # Panics
    ///
    /// When called from a `CommandProposed` handler.
    pub fn group(self: &Arc<Self>, origin: &Origin) -> Result<EditGroup, EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let marks = self.open_group(origin).map_err(|holder| busy(&holder))?;
        Ok(EditGroup {
            host: Arc::clone(self),
            marks,
            open: true,
            _thread: PhantomData,
        })
    }

    fn open_group(&self, origin: &Origin) -> Result<Marks, Origin> {
        self.enter(origin)?;
        let mut engine = self.engine.lock();
        let mut sequence = self.sequence.lock();
        let steps = if sequence.groups == 0 {
            engine.begin_group();
            0
        } else {
            engine.group_len()
        };
        sequence.groups += 1;
        Ok(Marks {
            steps,
            announced: sequence.announced.len(),
            revision: engine.revision(),
        })
    }

    /// Close the open group by `finish`. Returns the engine revision before
    /// and after.
    fn close_group(&self, marks: Marks, finish: Finish) -> (u64, u64) {
        let revisions = {
            let mut engine = self.engine.lock();
            let mut sequence = self.sequence.lock();
            let found = engine.revision();
            let outermost = sequence.groups == 1;
            match (finish, outermost) {
                (Finish::Commit(Some(label)), true) => engine.commit_group_as(label),
                (Finish::Commit(None), true) => engine.commit_group(),
                (Finish::Commit(_), false) => {}
                (Finish::Rollback, outermost) => {
                    if outermost {
                        engine.rollback_group();
                    } else {
                        engine.rollback_group_to(marks.steps);
                    }
                    sequence.announced.truncate(marks.announced);
                }
            }
            sequence.groups -= 1;
            (found, engine.revision())
        };
        self.announce(self.leave([]));
        revisions
    }

    /// Run `body` as one transaction named `label`: its commands land as one
    /// undo step, or, when it fails, panics, or a command is refused, none of
    /// them remain. A plugin's step is named `id: label` in History.
    pub fn transact_with<T>(
        &self,
        origin: Origin,
        label: impl Into<Cow<'static, str>>,
        body: impl FnOnce(&mut Transaction<'_>) -> Result<T, TransactError>,
    ) -> Result<(T, Receipt), TransactError> {
        if in_handler() {
            return Err(TransactError::Reentrant);
        }
        let marks = self
            .open_group(&origin)
            .map_err(|holder| TransactError::Busy { holder })?;
        let label = match &origin {
            Origin::Plugin(id) => Cow::Owned(format!("{id}: {}", label.into())),
            Origin::Editor | Origin::Assistant => label.into(),
        };
        let mut tx = Transaction {
            host: self,
            origin,
            outcomes: Vec::new(),
        };
        let result = match catch_unwind(AssertUnwindSafe(|| body(&mut tx))) {
            Ok(result) => result,
            Err(payload) => Err(TransactError::Panicked(panic_message(payload.as_ref()))),
        };
        match result {
            Ok(value) => {
                let (_, revision) = self.close_group(marks, Finish::Commit(Some(label)));
                Ok((
                    value,
                    Receipt {
                        revision,
                        outcomes: tx.outcomes,
                    },
                ))
            }
            Err(err) => {
                self.close_group(marks, Finish::Rollback);
                Err(err)
            }
        }
    }

    /// Apply `commands` in order as one undo step named `label`.
    pub fn transact(
        &self,
        origin: Origin,
        label: impl Into<Cow<'static, str>>,
        commands: Vec<Command>,
    ) -> Result<Receipt, TransactError> {
        self.transact_with(origin, label, |tx| {
            for command in commands {
                tx.apply(command)?;
            }
            Ok(())
        })
        .map(|((), receipt)| receipt)
    }

    /// Undo one step. `None` when there is nothing to undo, the step failed,
    /// or this thread has a group open.
    pub fn undo(&self) -> Result<Option<Reverted>, EngineError> {
        self.walk_history(|engine| engine.history_depth().checked_sub(1))
            .map(|walked| walked.and_then(|steps| steps.into_iter().next()))
    }

    /// Redo one step. `None` when there is nothing to redo, the step failed,
    /// or this thread has a group open.
    pub fn redo(&self) -> Result<Option<Reverted>, EngineError> {
        self.walk_history(|engine| Some(engine.history_depth() + 1))
            .map(|walked| walked.and_then(|steps| steps.into_iter().next()))
    }

    /// Undo or redo one step at a time until `depth` steps are applied, and
    /// report each step in order. Every step leaves from the revision the one
    /// before it left, so a store that follows the steps links each revision a
    /// later step returns to. `None` when that point was not reached or this
    /// thread has a group open.
    pub fn history_jump(&self, depth: usize) -> Result<Option<Vec<Reverted>>, EngineError> {
        self.walk_history(|_| Some(depth))
    }

    /// Walk history one step at a time to the depth `target` names, reporting
    /// the revisions each step spanned when the walk reached that depth.
    fn walk_history(
        &self,
        target: impl FnOnce(&Engine) -> Option<usize>,
    ) -> Result<Option<Vec<Reverted>>, EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let hold = self.hold(&Origin::Editor).map_err(|holder| busy(&holder))?;
        let grouped = self.sequence.lock().groups > 0;
        let walked = if grouped {
            None
        } else {
            let mut engine = self.engine.lock();
            target(&engine).and_then(|depth| {
                let mut steps = Vec::new();
                loop {
                    let at = engine.history_depth();
                    let next = match at.cmp(&depth) {
                        std::cmp::Ordering::Equal => break Some(steps),
                        std::cmp::Ordering::Greater => at - 1,
                        std::cmp::Ordering::Less => at + 1,
                    };
                    let opened = engine.history_return(next)?;
                    let found = engine.revision();
                    if !engine.history_jump(next) {
                        break None;
                    }
                    steps.push(Reverted {
                        opened,
                        found,
                        left: engine.revision(),
                    });
                }
            })
        };
        self.announce(hold.release([]));
        Ok(walked)
    }

    pub(crate) fn take_session_changes(&self) -> Vec<crate::host_wire::SessionChange> {
        std::mem::take(&mut self.sequence.lock().session_changes)
    }

    /// Open the `.aria` project at `path` when the open project has no unsaved
    /// changes, posting [`ProjectClosed`] and [`ProjectOpened`].
    pub fn open(&self, origin: &Origin, path: impl Into<PathBuf>) -> Result<(), EngineError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(EngineError::Refused("project path cannot be empty".into()));
        }
        if self.is_dirty() {
            return Err(EngineError::Refused(
                "project has unsaved changes; save the project before opening another".into(),
            ));
        }
        self.apply(origin, Command::Project(ProjectCommand::Open { path }))
            .map(drop)
    }

    /// Save the open project to its current path without adding an undo step,
    /// posting [`ProjectSaved`].
    pub fn save(&self, origin: &Origin) -> Result<PathBuf, EngineError> {
        let Some(path) = self.project_path() else {
            return Err(EngineError::Refused(
                "the project has not been saved yet; choose a file to save it to".into(),
            ));
        };
        self.apply(
            origin,
            Command::Project(ProjectCommand::Save { path: path.clone() }),
        )?;
        Ok(self.project_path().unwrap_or(path))
    }

    /// Save the open project to `path` without adding an undo step, setting the
    /// project path and posting [`ProjectSaved`].
    pub fn save_as(
        &self,
        origin: &Origin,
        path: impl Into<PathBuf>,
    ) -> Result<PathBuf, EngineError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(EngineError::Refused("project path cannot be empty".into()));
        }
        let cmd = Command::Project(ProjectCommand::Save { path: path.clone() });
        self.apply(origin, cmd)?;
        Ok(self.project_path().unwrap_or(path))
    }

    /// Replace the session by running `f` on the engine while no edit or group
    /// is in flight on any thread, for a replacement that is not a command,
    /// such as starting a new project. Posts [`ProjectClosed`] for the session
    /// `f` replaced and [`ProjectOpened`] for the one it left.
    ///
    /// # Panics
    ///
    /// When called from a `CommandProposed` handler.
    pub fn replace_session<R>(
        &self,
        origin: &Origin,
        f: impl FnOnce(&mut Engine) -> R,
    ) -> Result<R, EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let hold = self.hold(origin).map_err(|holder| busy(&holder))?;
        if self.sequence.lock().groups > 0 {
            return Err(EngineError::Refused(
                "an edit group is still open; close it before replacing the session".into(),
            ));
        }
        let (result, replaced) = {
            let mut engine = self.engine.lock();
            let closed = ProjectClosed {
                path: engine.project_path().cloned(),
            };
            let result = f(&mut engine);
            let opened = ProjectOpened {
                path: engine.project_path().cloned(),
                project: engine.snapshot(),
            };
            (result, Announcement::Replaced(closed, opened))
        };
        self.announce(hold.release([replaced]));
        Ok(result)
    }

    /// Look up `key` in `plugin_id`'s per-project store.
    pub fn storage_get(&self, plugin_id: &str, key: &str) -> Option<Vec<u8>> {
        match proposing() {
            Some((project, _)) => project
                .plugin_storage_get(plugin_id, key)
                .map(<[u8]>::to_vec),
            None => self
                .engine
                .lock()
                .project()
                .plugin_storage_get(plugin_id, key)
                .map(<[u8]>::to_vec),
        }
    }

    /// Write `value` under `key` in `plugin_id`'s per-project store, returning
    /// `true` when the stored bytes changed.
    pub fn storage_set(&self, plugin_id: &str, key: &str, value: Vec<u8>) -> bool {
        if let Some(changed) = proposing_storage_set(plugin_id, key, value.clone()) {
            return changed;
        }
        let changed = self.engine.lock().set_plugin_storage(plugin_id, key, value);
        if changed && let Some(waker) = self.storage_waker.lock().clone() {
            waker();
        }
        changed
    }

    /// Remove `key` from `plugin_id`'s per-project store, returning `true`
    /// when an entry was removed.
    pub fn storage_delete(&self, plugin_id: &str, key: &str) -> bool {
        if let Some(removed) = proposing_storage_delete(plugin_id, key) {
            return removed;
        }
        let removed = self.engine.lock().delete_plugin_storage(plugin_id, key);
        if removed && let Some(waker) = self.storage_waker.lock().clone() {
            waker();
        }
        removed
    }

    /// All keys stored for `plugin_id` in the open project, in sorted order.
    pub fn storage_keys(&self, plugin_id: &str) -> Vec<String> {
        match proposing() {
            Some((project, _)) => project.plugin_storage_keys(plugin_id),
            None => self.engine.lock().project().plugin_storage_keys(plugin_id),
        }
    }
}

/// An open group on a [`ProjectHost`]. Dropping it without committing rolls it
/// back.
///
/// Not `Send`: the project is held by the thread that opened the group, and
/// only that thread can close it.
pub struct EditGroup {
    host: Arc<ProjectHost>,
    marks: Marks,
    open: bool,
    _thread: PhantomData<*const ()>,
}

impl fmt::Debug for EditGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditGroup")
            .field("open", &self.open)
            .field("opened_revision", &self.marks.revision)
            .finish_non_exhaustive()
    }
}

impl EditGroup {
    /// Commit, naming the undo step after its first command. Returns the engine
    /// revision.
    pub fn commit(mut self) -> u64 {
        self.open = false;
        self.host.close_group(self.marks, Finish::Commit(None)).1
    }

    /// Commit, naming the undo step `label`. Returns the engine revision.
    pub fn commit_as(mut self, label: impl Into<Cow<'static, str>>) -> u64 {
        self.open = false;
        self.host
            .close_group(self.marks, Finish::Commit(Some(label.into())))
            .1
    }

    /// Revert every command applied since the group opened. Returns the
    /// revisions the group spanned.
    pub fn rollback(mut self) -> Reverted {
        self.open = false;
        let (found, left) = self.host.close_group(self.marks, Finish::Rollback);
        Reverted {
            opened: self.marks.revision,
            found,
            left,
        }
    }
}

impl Drop for EditGroup {
    fn drop(&mut self) {
        if self.open {
            self.host.close_group(self.marks, Finish::Rollback);
        }
    }
}

/// An open transaction, handed to the body of
/// [`ProjectHost::transact_with`].
#[derive(Debug)]
pub struct Transaction<'a> {
    host: &'a ProjectHost,
    origin: Origin,
    outcomes: Vec<ApplyOutcome>,
}

impl Transaction<'_> {
    /// The project with every command applied so far. Drop it before the next
    /// [`apply`](Self::apply): a held snapshot makes that apply copy the
    /// project.
    pub fn project(&self) -> Arc<Project> {
        self.host.engine.lock().snapshot()
    }

    /// Who the transaction's commands are proposed as.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Propose and apply one command.
    pub fn apply(&mut self, command: Command) -> Result<ApplyOutcome, TransactError> {
        transactional(&command)?;
        let label = command.label();
        // The transaction's group holds the project, so this never waits.
        let hold = self
            .host
            .hold(&self.origin)
            .map_err(|holder| TransactError::Busy { holder })?;
        let applied = self
            .host
            .apply_locked(&self.origin, command, None, |_, _| (), |(), _, _| ());
        let (outcome, (), announced) = applied.map_err(|rejection| match rejection {
            Rejection::Refused(reason) => TransactError::Refused { label, reason },
            Rejection::Engine(err) => TransactError::Failed {
                label,
                reason: err.to_string(),
            },
        })?;
        self.host
            .announce(hold.release(announced.into_iter().flatten()));
        self.outcomes.push(outcome.clone());
        Ok(outcome)
    }
}

#[cfg(test)]
#[path = "project_host_tests.rs"]
mod tests;
