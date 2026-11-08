//! Scratch paths a long write fills beside its destination, and the clearing
//! of those a dead writer left.
//!
//! A scratch path `p` has a lock file `p.lock` beside it, created before `p`
//! and locked exclusively by the writer until `p` is gone. The operating
//! system releases the lock when the writer exits, however it exits (a kill,
//! a crash, a process exit that skips destructors), so [`sweep`] finds exactly
//! the scratch paths whose writer is gone. A scratch path with no lock file is
//! never in use: its writer created the lock file first.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// The suffix of the lock file beside a scratch path.
pub const LOCK_SUFFIX: &str = ".lock";

/// The lock file of the scratch path `path`.
pub fn lock_path(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_owned();
    lock.push(LOCK_SUFFIX);
    PathBuf::from(lock)
}

/// The entries of one directory, as a claim and a sweep reach them: by path
/// ([`ScratchLock::claim`], [`sweep`]), or through a handle to a directory
/// the caller pinned, so a directory swapped for a link after the pin is
/// never written through.
pub trait ScratchDir {
    /// The names of the entries; a name that is not UTF-8 is left out.
    fn names(&self) -> io::Result<Vec<String>>;
    /// Open the entry `name` for reading and writing, creating it when
    /// missing and keeping its contents.
    fn open_lock(&self, name: &str) -> io::Result<File>;
    /// Whether the entry `name` exists, a link read as itself.
    fn contains(&self, name: &str) -> bool;
    /// Unlink the entry `name`.
    fn unlink(&self, name: &str) -> io::Result<()>;
}

impl ScratchDir for Path {
    fn names(&self) -> io::Result<Vec<String>> {
        Ok(fs::read_dir(self)?
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect())
    }

    fn open_lock(&self, name: &str) -> io::Result<File> {
        open_lock_file(&self.join(name))
    }

    fn contains(&self, name: &str) -> bool {
        fs::symlink_metadata(self.join(name)).is_ok()
    }

    fn unlink(&self, name: &str) -> io::Result<()> {
        fs::remove_file(self.join(name))
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// The claim on one scratch path: its lock file, locked until the value is
/// dropped. The drop of a [`Self::claim`] unlinks the lock file and then
/// releases the lock, so delete the scratch path before dropping the claim.
#[derive(Debug)]
pub struct ScratchLock {
    /// Unlinked by the drop; `None` for a [`Self::claim_in`], whose caller
    /// unlinks the lock file through its directory.
    lock_path: Option<PathBuf>,
    /// Locked for as long as the claim lives; never read.
    _held: File,
}

impl ScratchLock {
    /// Claim the scratch path `path`, which is not created. `Ok(None)` when a
    /// live writer holds the name, or a sweep unlinked the lock file between
    /// its open and the lock: claim another name. What a dead writer left at
    /// `path` is still there; the caller deletes or replaces it.
    pub fn claim(path: &Path) -> io::Result<Option<Self>> {
        let lock_path = lock_path(path);
        let Some(mut claim) = Self::hold(open_lock_file(&lock_path)?)? else {
            return Ok(None);
        };
        if !lock_path.exists() {
            return Ok(None);
        }
        claim.lock_path = Some(lock_path);
        Ok(Some(claim))
    }

    /// Claim the scratch entry `name` of `dir`, as [`Self::claim`] claims a
    /// path, opening its lock file through `dir`. The drop releases the lock
    /// and unlinks nothing: delete the scratch entry, then unlink the lock
    /// file `name.lock` through `dir`, then drop the claim.
    pub fn claim_in(dir: &(impl ScratchDir + ?Sized), name: &str) -> io::Result<Option<Self>> {
        let lock = format!("{name}{LOCK_SUFFIX}");
        let Some(claim) = Self::hold(dir.open_lock(&lock)?)? else {
            return Ok(None);
        };
        Ok(dir.contains(&lock).then_some(claim))
    }

    /// Lock `file`, or `Ok(None)` when another claim holds it.
    fn hold(file: File) -> io::Result<Option<Self>> {
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                lock_path: None,
                _held: file,
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    /// The lock file the drop unlinks: `Some` for a [`Self::claim`].
    pub fn lock_path(&self) -> Option<&Path> {
        self.lock_path.as_deref()
    }
}

impl Drop for ScratchLock {
    fn drop(&mut self) {
        // Unlinked while still locked, so a sweep that opened the file before
        // the unlink finds nothing to clear once it gets the lock.
        if let Some(lock_path) = &self.lock_path {
            let _ = fs::remove_file(lock_path);
        }
    }
}

/// Clear every scratch path in `dir` whose writer is gone, and its lock file.
///
/// `is_scratch` selects the scratch names; an entry is a scratch path or its
/// lock file, and every other entry is left alone, as is a scratch path whose
/// lock is held or cannot be opened. `clear` deletes or salvages each dead
/// scratch path; it runs with the lock held, so no writer claims the name
/// meanwhile.
pub fn sweep(dir: &Path, is_scratch: impl Fn(&str) -> bool, mut clear: impl FnMut(&Path)) {
    sweep_in(dir, is_scratch, |name| clear(&dir.join(name)));
}

/// [`sweep`] through the operations of `dir`; `clear` receives each dead
/// scratch entry's name.
pub fn sweep_in(
    dir: &(impl ScratchDir + ?Sized),
    is_scratch: impl Fn(&str) -> bool,
    mut clear: impl FnMut(&str),
) {
    let Ok(names) = dir.names() else {
        return;
    };
    for name in &names {
        let scratch = name.strip_suffix(LOCK_SUFFIX).unwrap_or(name);
        if !is_scratch(scratch) {
            continue;
        }
        // Locked the way a claim locks, so a writer claiming the name of a
        // leftover that has no lock file waits on this sweep or skips it.
        let lock = format!("{scratch}{LOCK_SUFFIX}");
        let Ok(held) = dir.open_lock(&lock) else {
            continue;
        };
        if held.try_lock().is_err() || !dir.contains(&lock) {
            continue;
        }
        if dir.contains(scratch) {
            clear(scratch);
        }
        let _ = dir.unlink(&lock);
        drop(held);
    }
}

/// Delete `path`, a directory with its contents or any other entry. A path
/// that does not exist is not an error.
pub fn remove(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;
