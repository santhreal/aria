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
//! Each session writes to a slot of its own, `recovery.aria` for the first
//! and `recovery-N.aria` for the rest, and holds a lock on the slot for as
//! long as it lives. Two processes over one data directory used to share one
//! copy: each autosave replaced the other's, and a save in one deleted the
//! other's, so a crash of the second lost its work. A session now writes,
//! offers and clears only its own copy, and a copy whose session is gone is
//! offered on the next launch.
//!
//! Time is a parameter, never `Instant::now()` read in here, so the interval is
//! testable without sleeping.

use std::fs::{File, TryLockError};
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

/// The most recovery slots a data directory holds: one for each session open
/// at once, and one for each session that ended without saving its work.
const SLOTS: usize = 16;

/// The recovery copy in the first slot of `data_dir`, the one a lone session
/// writes.
pub fn recovery_path(data_dir: &Path) -> PathBuf {
    slot_file(data_dir, 0, "aria")
}

/// Slot `index`'s file with `extension`: the copy (`aria`), the sidecar naming
/// the project the copy shadows (`path`), and the lock a live session holds on
/// the slot (`lock`).
fn slot_file(data_dir: &Path, index: usize, extension: &str) -> PathBuf {
    if index == 0 {
        data_dir.join(format!("recovery.{extension}"))
    } else {
        data_dir.join(format!("recovery-{index}.{extension}"))
    }
}

/// Lock slot `index` of `data_dir` for as long as the returned file stays
/// open, or `None` while another session holds it. The operating system
/// releases the lock when the session's process ends, so a slot whose lock is
/// free holds no live session.
fn lock_slot(data_dir: &Path, index: usize) -> io::Result<Option<File>> {
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot_file(data_dir, index, "lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

/// Remove slot `index`'s copy and sidecar. The lock file stays: removing it
/// while another session opens it would let two sessions hold one slot.
fn remove_slot(data_dir: &Path, index: usize) {
    let _ = std::fs::remove_file(slot_file(data_dir, index, "aria"));
    let _ = std::fs::remove_file(slot_file(data_dir, index, "path"));
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
    /// The slot the copy is in.
    pub(crate) slot: usize,
}

/// The slot a session writes its recovery copy to, and the lock that marks the
/// slot as held while the session lives.
#[derive(Debug)]
struct Slot {
    index: usize,
    _lock: File,
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
    /// The session revision the last copy holds. `None` means no copy holds
    /// the session, so the next due tick writes one; a due tick at this
    /// revision writes nothing, since the copy on disk already holds it.
    copied: Option<u64>,
    /// The slot this session's copy goes to, locked at the first copy.
    slot: Option<Slot>,
}

impl Autosave {
    /// The slot this session holds, if it has written a copy.
    fn slot_index(&self) -> Option<usize> {
        self.slot.as_ref().map(|slot| slot.index)
    }

    /// The slot this session writes its copy to, locking the first free one
    /// on first use. A slot that holds a copy is passed over: the copy is
    /// another session's, live or ended, and stays until it is restored or
    /// discarded. Sessions in two processes over one data directory each
    /// write, offer and clear only their own copy.
    fn claim(&mut self, data_dir: &Path) -> io::Result<usize> {
        if let Some(index) = self.slot_index() {
            return Ok(index);
        }
        std::fs::create_dir_all(data_dir)?;
        for index in 0..SLOTS {
            let Some(lock) = lock_slot(data_dir, index)? else {
                continue;
            };
            // Checked under the lock: a session that held the slot may have
            // written its copy and ended before the lock was taken.
            if slot_file(data_dir, index, "aria").exists() {
                continue;
            }
            self.slot = Some(Slot { index, _lock: lock });
            return Ok(index);
        }
        Err(io::Error::other(format!(
            "each of the {SLOTS} recovery slots in {} holds a copy or a live session",
            data_dir.display()
        )))
    }

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
        revision: u64,
        write: impl FnOnce() -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        self.finish();
        self.last = Some(now);
        self.copied = Some(revision);
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
        let written = match writing.join() {
            Ok(Ok(())) => true,
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "the recovery copy was not written");
                false
            }
            Err(_) => {
                tracing::error!("the {THREAD_NAME} thread panicked writing the recovery copy");
                false
            }
        };
        // A copy that failed holds nothing, so the next due tick writes again.
        if !written {
            self.copied = None;
        }
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Write `project` as the copy in slot `index` of `data_dir`, then the sidecar
/// naming `origin`, the project file it shadows. The old sidecar is removed
/// first, so a copy is never read with a sidecar that names another project:
/// a copy without one is offered as untitled work.
fn write_copy(data_dir: &Path, index: usize, project: &Project, origin: &str) -> io::Result<()> {
    let sidecar = slot_file(data_dir, index, "path");
    match std::fs::remove_file(&sidecar) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }
    project.save_to_file(&slot_file(data_dir, index, "aria"))?;
    std::fs::write(sidecar, origin)
}

/// Read the recovery offer in `data_dir` from disk: the first copy no live
/// session holds.
pub(crate) fn load_pending_recovery(data_dir: &Path) -> Option<RecoveryOffer> {
    offer_in(data_dir, None)
}

/// The first copy in `data_dir` that is in slot `own` or in a slot no live
/// session holds. A copy in another live session's slot is that session's
/// work in progress, not work a crash left behind.
fn offer_in(data_dir: &Path, own: Option<usize>) -> Option<RecoveryOffer> {
    (0..SLOTS).find_map(|slot| {
        let file = slot_file(data_dir, slot, "aria");
        if !file.is_file() || (own != Some(slot) && matches!(lock_slot(data_dir, slot), Ok(None))) {
            return None;
        }
        let project = std::fs::read_to_string(slot_file(data_dir, slot, "path"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .map(PathBuf::from);
        let name =
            super::session::project_name_of(project.as_deref(), "Untitled project").to_owned();
        Some(RecoveryOffer {
            file,
            project,
            name,
            slot,
        })
    })
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
    /// again. A session at the revision the last copy holds starts nothing:
    /// an idle dirty session would otherwise serialize and sync the same
    /// project every interval.
    pub fn autosave_tick(&mut self, now: Instant) -> bool {
        if !self.store.is_dirty
            && self
                .engine
                .try_lock()
                .is_some_and(|engine| engine.is_dirty())
        {
            self.sync_from_engine();
        }
        if !self.store.is_dirty || !self.autosave.due(now) {
            return false;
        }
        let Some((project, revision)) = self
            .engine
            .try_lock()
            .map(|engine| (engine.snapshot(), engine.revision()))
        else {
            return false;
        };
        if self.autosave.copied == Some(revision) {
            return false;
        }
        let write = match self.recovery_write(project) {
            Ok(write) => write,
            Err(err) => {
                // The next attempt waits out the interval, so a data directory
                // with no free slot is not retried at every tick.
                self.autosave.last = Some(now);
                tracing::warn!(error = %err, "the recovery copy has no slot to go to");
                return false;
            }
        };
        match self.autosave.start(now, revision, write) {
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
        self.refresh_pending_recovery();
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
        let (project, revision) = {
            let engine = self.engine.lock();
            (engine.snapshot(), engine.revision())
        };
        let write = self.recovery_write(project).map_err(EngineError::Io)?;
        self.autosave.copied = None;
        write().map_err(EngineError::Io)?;
        self.autosave.copied = Some(revision);
        self.refresh_pending_recovery();
        Ok(())
    }

    /// The write of a recovery copy of `project`, a snapshot of the session,
    /// into the slot this session holds.
    ///
    /// The snapshot costs a reference count; an edit made while the write
    /// still holds it copies the project once.
    fn recovery_write(
        &mut self,
        project: Arc<Project>,
    ) -> io::Result<impl FnOnce() -> io::Result<()> + Send + 'static> {
        let index = self.autosave.claim(&self.data_dir)?;
        let data_dir = self.data_dir.clone();
        let origin = self
            .project_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Ok(move || write_copy(&data_dir, index, &project, &origin))
    }

    /// Forget this session's recovery copy. Called whenever the real file
    /// catches up with the session: a stale offer would hand back older work
    /// than the file already holds. The copies of other sessions stay.
    pub fn clear_recovery(&mut self) {
        // A copy still in flight would land after the removal and offer it.
        self.autosave.finish();
        if let Some(index) = self.autosave.slot_index() {
            remove_slot(&self.data_dir, index);
        }
        self.autosave.last = None;
        self.autosave.copied = None;
        self.refresh_pending_recovery();
    }

    /// The recovery copy waiting to be offered, if any.
    ///
    /// Held on [`AppState`], refreshed when arriving at Home, and kept in step
    /// across every recovery write, clear, restore, and discard so a draw of
    /// Home does not read the disk.
    pub fn pending_recovery(&self) -> Option<RecoveryOffer> {
        self.pending_recovery.clone()
    }

    /// Re-read [`Self::pending_recovery`] from `data_dir`.
    pub fn refresh_pending_recovery(&mut self) {
        self.pending_recovery = offer_in(&self.data_dir, self.autosave.slot_index());
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
        self.engine
            .lock()
            .adopt_recovered_path(offer.project.clone());
        remove_slot(&self.data_dir, offer.slot);
        self.clear_recovery();
        self.enter_editor();
        self.sync_from_engine();
        self.status_message = Some(match &offer.project {
            Some(_) => format!(
                "Recovered unsaved work in {}. Save to keep it.",
                self.project_display_name()
            ),
            None => "Recovered unsaved work. Save it somewhere to keep it.".into(),
        });
        Ok(())
    }

    /// Throw the offered recovery copy away.
    pub fn discard_recovery(&mut self) {
        if let Some(offer) = self.pending_recovery.take() {
            remove_slot(&self.data_dir, offer.slot);
        }
        self.clear_recovery();
        self.clear_error();
        self.status_message = Some("Discarded the recovered work".into());
    }
}
