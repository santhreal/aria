//! The project host: the one path that reads and edits the project.
//!
//! Every edit, from the editor, the assistant, or a plugin, goes through
//! [`ProjectHost`]. A command is proposed to every [`CommandProposed`] handler
//! with the engine locked, applied, and announced as a [`CommandApplied`] once
//! nothing can roll it back, together with the editor event its outcome
//! implies: [`MediaImported`], [`ProjectSaved`], and the rest. A group
//! ([`ProjectHost::group`]) or a transaction ([`ProjectHost::transact_with`])
//! applies any number of commands as one undo step.
//!
//! One thread edits at a time. A thread that applies a command or opens a group
//! holds the project until the command lands or the outermost group closes;
//! edits from the same thread join in, and a group opened inside another is a
//! savepoint of it. An edit from another thread waits up to
//! [`ProjectHost::CONTENDED_WAIT`] and is then refused, naming the holder, so an
//! assistant turn that holds the project for its whole reply refuses an edit
//! from the window instead of freezing it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use aria_commands::{Command, ProjectCommand};
use aria_engine::{ApplyOutcome, Engine, EngineError};
use aria_models::Project;
use parking_lot::{Condvar, Mutex};

use crate::context::PluginContext;
use crate::editor_events::{
    ExportFinished, ExportStarted, MediaImported, MediaRelinked, ProjectClosed, ProjectOpened,
    ProjectSaved, RenderResult,
};
use crate::events::{Event, EventBus, panic_message};
use crate::hooks::CommandProposed;

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
pub struct CommandApplied {
    pub command: Command,
    pub outcome: ApplyOutcome,
    /// The engine revision right after this command.
    pub revision: u64,
    pub origin: Origin,
}

impl Event for CommandApplied {}

/// A notification held with the open group and posted once nothing can roll it
/// back. A rolled-back group posts none of its own.
enum Announcement {
    Applied(CommandApplied),
    Imported(MediaImported),
    Relinked(MediaRelinked),
    Saved(ProjectSaved),
    Replaced(ProjectClosed, ProjectOpened),
}

impl Announcement {
    fn post(self, bus: &EventBus) {
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
type Announced = [Option<Announcement>; 2];

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

/// Who holds the project, and what they have open.
#[derive(Default)]
struct Sequence {
    /// The editing thread and who it edits for, while `holds > 0`.
    owner: Option<(ThreadId, Origin)>,
    /// Nested holds by the owner: one per open group plus one per command in
    /// flight.
    holds: usize,
    /// Open groups, savepoints included.
    groups: usize,
    /// Announcements from commands applied inside the open groups, posted when
    /// the outermost one commits and dropped when the group they belong to
    /// rolls back.
    announced: Vec<Announcement>,
}

thread_local! {
    /// The project and revision a `CommandProposed` dispatch on this thread is
    /// about, while one runs. The engine is locked for the dispatch, so reads
    /// return this instead of locking it again.
    static PROPOSING: RefCell<Option<(Arc<Project>, u64)>> = const { RefCell::new(None) };
}

/// Marks this thread as dispatching `CommandProposed` until dropped, including
/// when a handler panics.
struct Proposing;

impl Proposing {
    fn enter(project: Arc<Project>, revision: u64) -> Self {
        PROPOSING.with(|slot| *slot.borrow_mut() = Some((project, revision)));
        Self
    }
}

impl Drop for Proposing {
    fn drop(&mut self) {
        PROPOSING.with(|slot| slot.borrow_mut().take());
    }
}

fn proposing() -> Option<(Arc<Project>, u64)> {
    PROPOSING.with(|slot| slot.borrow().clone())
}

fn in_handler() -> bool {
    PROPOSING.with(|slot| slot.borrow().is_some())
}

/// Whether `command` edits the session in a way history can undo.
///
/// Exhaustive over project commands, so a new one does not compile until it
/// is classified.
fn transactional(command: &Command) -> Result<(), TransactError> {
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
fn replaces_session(command: &Command) -> bool {
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
fn derived(
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
enum Rejection {
    Refused(String),
    Engine(EngineError),
}

/// The refusal an edit gets while another thread holds the project.
fn busy(holder: &Origin) -> EngineError {
    EngineError::Refused(TransactError::Busy {
        holder: holder.clone(),
    }
    .to_string())
}

/// How far an open group reaches back, to roll back to.
#[derive(Clone, Copy)]
struct Marks {
    /// Steps the enclosing group held when this one opened; zero when outermost.
    steps: usize,
    /// Announcements queued when this one opened.
    announced: usize,
}

enum Finish {
    Commit(Option<Cow<'static, str>>),
    Rollback,
}

/// The project host. Registered by the engine plugin; the editor, the
/// assistant, and every plugin edit through the same value.
pub struct ProjectHost {
    engine: Arc<Mutex<Engine>>,
    events: Arc<EventBus>,
    sequence: Mutex<Sequence>,
    released: Condvar,
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
        }
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
        for announcement in announced {
            announcement.post(&self.events);
        }
    }

    /// Propose `command` to every handler. The caller holds the engine lock.
    fn propose(&self, engine: &Engine, command: Command, origin: &Origin) -> Result<Command, String> {
        let project = engine.snapshot();
        let _proposing = Proposing::enter(Arc::clone(&project), engine.revision());
        let mut proposed = CommandProposed::new(command, project, origin.clone());
        self.events.emit_waterfall(&mut proposed);
        // The event and the stash drop when this returns, so the apply that
        // follows copies the project only if a handler kept a reference.
        match proposed.veto {
            Some(reason) => Err(reason),
            None => Ok(proposed.command),
        }
    }

    /// Propose and apply one command on a locked engine.
    fn apply_locked<B, R>(
        &self,
        engine: &mut Engine,
        origin: &Origin,
        command: Command,
        before: impl FnOnce(&Command, &Project) -> B,
        after: impl FnOnce(B, &ApplyOutcome, &Engine) -> R,
    ) -> Result<(ApplyOutcome, R, Announced), Rejection> {
        let command = self
            .propose(engine, command, origin)
            .map_err(Rejection::Refused)?;
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
        let observed = before(&command, engine.project());
        let outcome = engine.apply(command);
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
        Ok((outcome, result, [applied, derived]))
    }

    /// Apply one command as its own undo step, or as part of this thread's
    /// open group.
    pub fn apply(&self, origin: &Origin, command: Command) -> Result<ApplyOutcome, EngineError> {
        self.apply_observed(origin, command, |_, _| (), |(), _, _| ())
            .map(|(outcome, ())| outcome)
    }

    /// [`apply`](Self::apply), with observers that run under the engine lock:
    /// `before` sees the command the handlers settled on and the project it
    /// applies to, `after` sees the outcome and the engine after it.
    ///
    /// # Panics
    ///
    /// When called from a `CommandProposed` handler.
    pub fn apply_observed<B, R>(
        &self,
        origin: &Origin,
        command: Command,
        before: impl FnOnce(&Command, &Project) -> B,
        after: impl FnOnce(B, &ApplyOutcome, &Engine) -> R,
    ) -> Result<(ApplyOutcome, R), EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let hold = self.hold(origin).map_err(|holder| busy(&holder))?;
        // A command that clears history would take the open group with it.
        if self.sequence.lock().groups > 0 {
            transactional(&command).map_err(|err| EngineError::Refused(err.to_string()))?;
        }
        let applied = {
            let mut engine = self.engine.lock();
            self.apply_locked(&mut engine, origin, command, before, after)
        };
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
        })
    }

    fn close_group(&self, marks: Marks, finish: Finish) -> u64 {
        let revision = {
            let mut engine = self.engine.lock();
            let mut sequence = self.sequence.lock();
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
            engine.revision()
        };
        self.announce(self.leave([]));
        revision
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
                let revision = self.close_group(marks, Finish::Commit(Some(label)));
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

    /// Undo one step. `Ok(false)` when there is nothing to undo or this thread
    /// has a group open.
    pub fn undo(&self) -> Result<bool, EngineError> {
        self.step_history(Engine::undo)
    }

    /// Redo one step. `Ok(false)` when there is nothing to redo or this thread
    /// has a group open.
    pub fn redo(&self) -> Result<bool, EngineError> {
        self.step_history(Engine::redo)
    }

    /// Undo or redo until `depth` steps are applied. `Ok(false)` when that
    /// point was not reached or this thread has a group open.
    pub fn history_jump(&self, depth: usize) -> Result<bool, EngineError> {
        self.step_history(|engine| engine.history_jump(depth))
    }

    fn step_history(&self, step: impl FnOnce(&mut Engine) -> bool) -> Result<bool, EngineError> {
        assert!(!in_handler(), "{}", TransactError::Reentrant);
        let hold = self
            .hold(&Origin::Editor)
            .map_err(|holder| busy(&holder))?;
        let grouped = self.sequence.lock().groups > 0;
        let stepped = !grouped && step(&mut self.engine.lock());
        self.announce(hold.release([]));
        Ok(stepped)
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

impl EditGroup {
    /// Commit, naming the undo step after its first command. Returns the engine
    /// revision.
    pub fn commit(mut self) -> u64 {
        self.open = false;
        self.host.close_group(self.marks, Finish::Commit(None))
    }

    /// Commit, naming the undo step `label`. Returns the engine revision.
    pub fn commit_as(mut self, label: impl Into<Cow<'static, str>>) -> u64 {
        self.open = false;
        self.host
            .close_group(self.marks, Finish::Commit(Some(label.into())))
    }

    /// Revert every command applied since the group opened.
    pub fn rollback(mut self) {
        self.open = false;
        self.host.close_group(self.marks, Finish::Rollback);
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
        let applied = {
            let mut engine = self.host.engine.lock();
            self.host
                .apply_locked(&mut engine, &self.origin, command, |_, _| (), |(), _, _| ())
        };
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
