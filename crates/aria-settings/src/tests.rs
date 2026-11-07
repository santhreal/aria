//! Tests for this module.
//!
//! The cases live in sibling files, one per area, declared at the bottom. This
//! file holds only the shared fixtures; a child sees them through `use super::*`.

use std::cell::Cell;
use std::io;

use super::*;

#[derive(Default)]
struct FaultFs {
    rename_calls: Cell<usize>,
    remove_calls: Cell<usize>,
    failed_renames: Vec<usize>,
    failed_removals: Vec<usize>,
    /// Every folder synced, with the renames made before it, in order.
    synced_dirs: std::cell::RefCell<Vec<(PathBuf, usize)>>,
    fail_dir_sync: bool,
}

impl FaultFs {
    fn failing(failed_renames: &[usize], failed_removals: &[usize]) -> Self {
        Self {
            failed_renames: failed_renames.to_vec(),
            failed_removals: failed_removals.to_vec(),
            ..Self::default()
        }
    }

    fn next_call(counter: &Cell<usize>) -> usize {
        let call = counter.get() + 1;
        counter.set(call);
        call
    }
}

/// The reason of a save that installed nothing.
#[track_caller]
fn refused(result: Result<(), SaveError>) -> String {
    match result {
        Err(SaveError::Failed(reason)) => reason,
        other => panic!("expected a save that installed nothing, got {other:?}"),
    }
}

impl PersistenceFs for FaultFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let call = Self::next_call(&self.rename_calls);
        if self.failed_renames.contains(&call) {
            return Err(io::Error::other(format!("injected rename failure #{call}")));
        }
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let call = Self::next_call(&self.remove_calls);
        if self.failed_removals.contains(&call) {
            return Err(io::Error::other(format!(
                "injected removal failure #{call}"
            )));
        }
        std::fs::remove_file(path)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        std::fs::symlink_metadata(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.synced_dirs
            .borrow_mut()
            .push((path.to_path_buf(), self.rename_calls.get()));
        if self.fail_dir_sync {
            return Err(io::Error::other("injected folder sync failure"));
        }
        Ok(())
    }
}

fn transaction_artifacts(directory: &Path) -> Vec<PathBuf> {
    let mut artifacts: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.contains(".aria-tmp-") || name.contains(".aria-backup-")
            })
        })
        .collect();
    artifacts.sort();
    artifacts
}

fn assert_no_transaction_artifacts(directory: &Path) {
    let artifacts = transaction_artifacts(directory);
    assert!(
        artifacts.is_empty(),
        "unexpected transaction artifacts: {artifacts:?}"
    );
}

#[path = "tests_missing_file.rs"]
mod missing_file;

#[path = "tests_ai_protocol.rs"]
mod ai_protocol;

#[path = "tests_ai_source.rs"]
mod ai_source;
#[path = "tests_rejected.rs"]
mod rejected;
