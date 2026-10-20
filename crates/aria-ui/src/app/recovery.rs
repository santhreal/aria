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
//!   project it shadows, so restoring writes back to the operator's own file
//!   rather than to the recovery copy.
//!
//! Time is a parameter, never `Instant::now()` read in here, so the interval is
//! testable without sleeping.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aria_commands::{Command, ProjectCommand};
use aria_engine::EngineError;

use super::AppState;

/// How long a dirty session may go without a recovery copy.
///
/// Twenty seconds is a compromise, not a measurement: long enough that a big
/// project is not being serialized constantly, short enough that a crash costs
/// less than an edit or two.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(20);

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

impl AppState {
    /// Write a recovery copy if one is due. Returns true when one was written.
    ///
    /// Called from the same half-second ticker that ages the status line: a
    /// dedicated timer would wake the app more often to do nothing.
    pub fn autosave_tick(&mut self, now: Instant) -> bool {
        if !self.store.is_dirty {
            return false;
        }
        if let Some(last) = self.last_autosave
            && now.duration_since(last) < AUTOSAVE_INTERVAL
        {
            return false;
        }
        self.last_autosave = Some(now);
        self.write_recovery().is_ok()
    }

    /// Write the recovery copy and its sidecar now, whatever the clock says.
    ///
    /// Separate from the tick so a test — and a future "save on suspend" — can
    /// ask for the copy without arranging a time.
    pub fn write_recovery(&mut self) -> Result<(), EngineError> {
        let file = recovery_path(&self.data_dir);
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.engine
            .lock()
            .project()
            .save_to_file(&file)
            .map_err(EngineError::Io)?;
        let origin = self
            .project_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let _ = std::fs::write(sidecar_path(&self.data_dir), origin);
        Ok(())
    }

    /// Forget any recovery copy. Called whenever the real file catches up with
    /// the session: a stale offer would hand back older work than the operator
    /// already has.
    pub fn clear_recovery(&mut self) {
        let _ = std::fs::remove_file(recovery_path(&self.data_dir));
        let _ = std::fs::remove_file(sidecar_path(&self.data_dir));
        self.last_autosave = None;
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
    /// afterwards because the operator's own file still holds the older content.
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
