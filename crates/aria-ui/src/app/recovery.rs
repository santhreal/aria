//! Autosave and crash recovery.
//!
//! A crash, an OOM kill, or a power cut used to take everything since the last
//! explicit Save. While the session is dirty a copy is written beside the app's
//! own data every [`AUTOSAVE_INTERVAL`], and the next launch offers it back.
//!
//! Two rules keep this from becoming a second save path:
//!
//! - The copy is written straight from the project, not through
//!   `ProjectCommand::Save`, so the saved baseline never moves and an autosave
//!   never makes a dirty session look clean.
//! - The copy is a normal `.aria` file next to a one-line sidecar naming the
//!   project it shadows, so restoring writes back to the project file
//!   rather than to the recovery copy.
//!
//! The copy is written on a thread of its own, from a snapshot of the project
//! that costs a reference count: serializing and syncing `project-10k` takes
//! about 200 ms, which on the window's thread froze the editor every twenty
//! seconds of editing. At most one copy is in flight. Rewriting or removing the
//! copy, and dropping the session, first wait for the copy in flight, so a
//! late copy never replaces a newer one or lands after the offer was cleared.
//!
//! Time is a parameter, never `Instant::now()` read in here, so the interval is
//! testable without sleeping.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aria_commands::{Command, ProjectCommand};
use aria_engine::EngineError;
use aria_models::Project;

use super::AppState;

/// How long a dirty session may go without a recovery copy.
///
/// Twenty seconds is a compromise, not a measurement: long enough that a big
/// project is not being serialized constantly, short enough that a crash costs
/// less than an edit or two.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(20);

/// The thread that writes a recovery copy, as `/proc/<pid>/task/*/comm` shows
/// it.
const THREAD_NAME: &str = "aria-autosave";

/// The recovery copy for `data_dir`.
pub fn recovery_path(data_dir: &Path) -> PathBuf {
    data_dir.join("recovery.aria")
}

/// The sidecar naming the project the recovery copy shadows.
fn sidecar_path(data_dir: &Path) -> PathBuf {
    data_dir.join("recovery.path")
}

/// Work found on disk that no save accounts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryOffer {
    /// The recovery copy itself.
    pub file: PathBuf,
    /// The project it was a copy of, if that project had ever been saved.
    pub project: Option<PathBuf>,
    /// What to call it on screen.
    pub name: String,
}

/// When the last recovery copy started, and the one still being written.
///
/// Dropping it waits for the copy in flight. A session dropped mid-write would
/// otherwise leave a thread writing into a data directory the session no
/// longer holds, and a process ending mid-write leaves the temporary behind.
#[derive(Debug, Default)]
pub(crate) struct Autosave {
    /// When the last copy started. `None` means none since the file last caught
    /// up with the session, so the first dirty tick writes one.
    last: Option<Instant>,
    /// The copy being written, if one is.
    writing: Option<JoinHandle<io::Result<()>>>,
}

impl Autosave {
    /// Whether a copy may start at `now`: the interval since the last one has
    /// passed and no copy is still being written.
    pub(super) fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.duration_since(last) >= AUTOSAVE_INTERVAL)
            && self.writing.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Run `write` on the autosave thread, as the copy started at `now`.
    ///
    /// An error means the thread did not start and nothing was written.
    pub(super) fn start(
        &mut self,
        now: Instant,
        write: impl FnOnce() -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        self.finish();
        self.last = Some(now);
        let writing = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(write)?;
        self.writing = Some(writing);
        Ok(())
    }

    /// Wait for the copy being written, if one is, and log how it failed.
    pub(super) fn finish(&mut self) {
        let Some(writing) = self.writing.take() else {
            return;
        };
        match writing.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(error = %err, "the recovery copy was not written"),
            Err(_) => {
                tracing::error!("the {THREAD_NAME} thread panicked writing the recovery copy")
            }
        }
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Write `project` as the recovery copy in `data_dir`, then the sidecar naming
/// `origin`, the project file it shadows.
fn write_copy(data_dir: &Path, project: &Project, origin: &str) -> io::Result<()> {
    let _ = std::fs::create_dir_all(data_dir);
    project.save_to_file(&recovery_path(data_dir))?;
    let _ = std::fs::write(sidecar_path(data_dir), origin);
    Ok(())
}

impl AppState {
    /// Start a recovery copy if one is due. Returns true when one started.
    ///
    /// Called from the same half-second ticker that ages the status line: a
    /// dedicated timer would wake the app more often to do nothing. The copy
    /// is written on the autosave thread; a tick while it is still being
    /// written starts nothing, and the next tick after it finishes starts the
    /// next one. A preview decode holds the session for the whole decode, so a
    /// tick that finds it held starts nothing either, and the next tick tries
    /// again.
    pub fn autosave_tick(&mut self, now: Instant) -> bool {
        if !self.store.is_dirty || !self.autosave.due(now) {
            return false;
        }
        let Some(project) = self.engine.try_lock().map(|engine| engine.snapshot()) else {
            return false;
        };
        let write = self.recovery_write(project);
        match self.autosave.start(now, write) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "could not start the {THREAD_NAME} thread; writing the recovery copy on the window's thread"
                );
                self.write_recovery().is_ok()
            }
        }
    }

    /// Wait for the recovery copy being written, if one is.
    ///
    /// For a caller that reads the copy back, such as a test, or that times the
    /// tick apart from the write, such as a benchmark.
    pub fn finish_autosave(&mut self) {
        self.autosave.finish();
    }

    /// Write the recovery copy and its sidecar now, on this thread, whatever
    /// the clock says.
    ///
    /// Separate from the tick so a test — and a future "save on suspend" — can
    /// ask for the copy without arranging a time.
    pub fn write_recovery(&mut self) -> Result<(), EngineError> {
        // A copy still in flight would land after this one and replace it
        // with older work.
        self.autosave.finish();
        let project = self.engine.lock().snapshot();
        let write = self.recovery_write(project);
        write().map_err(EngineError::Io)
    }

    /// The write of a recovery copy of `project`, a snapshot of the session.
    ///
    /// The snapshot costs a reference count; an edit made while the write
    /// still holds it copies the project once.
    fn recovery_write(
        &self,
        project: Arc<Project>,
    ) -> impl FnOnce() -> io::Result<()> + Send + 'static {
        let data_dir = self.data_dir.clone();
        let origin = self
            .project_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        move || write_copy(&data_dir, &project, &origin)
    }

    /// Forget any recovery copy. Called whenever the real file catches up with
    /// the session: a stale offer would hand back older work than the file
    /// already holds.
    pub fn clear_recovery(&mut self) {
        // A copy still in flight would land after the removal and offer it.
        self.autosave.finish();
        let _ = std::fs::remove_file(recovery_path(&self.data_dir));
        let _ = std::fs::remove_file(sidecar_path(&self.data_dir));
        self.autosave.last = None;
    }

    /// The recovery copy waiting to be offered, if any.
    pub fn pending_recovery(&self) -> Option<RecoveryOffer> {
        let file = recovery_path(&self.data_dir);
        if !file.is_file() {
            return None;
        }
        let project = std::fs::read_to_string(sidecar_path(&self.data_dir))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .map(PathBuf::from);
        let name = project
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled project".into());
        Some(RecoveryOffer {
            file,
            project,
            name,
        })
    }

    /// Load the recovery copy, aimed back at the project it shadowed.
    ///
    /// `Load` rather than `Open`: a recovered session must come back even if a
    /// source file moved in the meantime, and a missing path is repaired by
    /// relinking, not by refusing the whole document. The session reads dirty
    /// afterwards because the project file still holds the older content.
    pub fn restore_recovery(&mut self) -> Result<(), EngineError> {
        let Some(offer) = self.pending_recovery() else {
            return Ok(());
        };
        self.apply_command(Command::Project(ProjectCommand::Load {
            path: offer.file.clone(),
        }))?;
        self.project_path = offer.project.clone();
        self.waveforms.clear();
        self.engine.lock().mark_unsaved();
        self.clear_recovery();
        self.enter_editor();
        self.sync_from_engine();
        self.status_message = Some(match &offer.project {
            Some(_) => format!("Recovered unsaved work in {}. Save to keep it.", offer.name),
            None => "Recovered unsaved work. Save it somewhere to keep it.".into(),
        });
        Ok(())
    }

    /// Throw the recovery copy away.
    pub fn discard_recovery(&mut self) {
        self.clear_recovery();
        self.status_message = Some("Discarded the recovered work.".into());
    }
}
