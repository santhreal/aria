//! Installing a package archive into a plugin folder, and removing one.
//!
//! [`stage`] unpacks an archive into a hidden folder beside the installed
//! packages and reads it; nothing in it runs. The caller shows the staged
//! package and what it asks for, then [`Staged::commit`] moves it into place,
//! replacing an installed package with the same id. Dropping a [`Staged`]
//! without committing deletes the unpacked files.
//!
//! Every hidden working folder (`.install-<pid>-<n>`, `.replaced-…`,
//! `.removed-…`) has a `<folder>.lock` file beside it, locked exclusively by
//! the process using the folder. The operating system releases the lock when
//! that process exits, however it exits, so [`sweep`] deletes exactly the
//! folders whose process is gone: an install killed at its prompt, a crash
//! part way through an update, a removal whose delete failed.

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::archive::unpack;
use super::manifest::{PackageManifest, Permissions};
use super::{Package, PackageError, Tier, discover};

/// An unpacked archive waiting to be committed.
#[derive(Debug)]
pub struct Staged {
    package: Package,
    previous: Option<Package>,
    target: PathBuf,
    asks: Permissions,
    plugins_dir: PathBuf,
    /// The folder `package` is unpacked in, deleted on drop. After a commit
    /// it no longer exists and the drop deletes only its lock.
    staging: WorkDir,
}

/// Unpack `archive` into `plugins_dir` and read the package it holds.
///
/// The package must have a code or data tier this host reads, and an id
/// that no differently named folder in `plugins_dir` already takes.
pub fn stage(archive: &Path, plugins_dir: &Path) -> Result<Staged, PackageError> {
    fs::create_dir_all(plugins_dir)
        .map_err(|err| PackageError::new(plugins_dir, err.to_string()))?;
    sweep(plugins_dir);
    let staging = WorkDir::claim(plugins_dir, "install")?;
    let package = unpack(archive, &staging.dir)?;
    let staged = Staged {
        package,
        previous: None,
        target: PathBuf::new(),
        asks: Permissions::default(),
        plugins_dir: plugins_dir.to_owned(),
        staging,
    };
    staged.locate()
}

impl Staged {
    /// Find the installed package this replaces and what the update asks for
    /// beyond what the installed version was granted.
    fn locate(mut self) -> Result<Self, PackageError> {
        let id = &self.package.manifest.id;
        let previous = discover(&self.plugins_dir)
            .into_iter()
            .filter_map(Result::ok)
            .find(|installed| installed.manifest.id == *id);
        self.target = match &previous {
            Some(installed) => installed.dir.clone(),
            None => {
                let target = self.plugins_dir.join(id);
                if target.exists() {
                    return Err(PackageError::new(
                        &target,
                        format!(
                            "is in the way of installing {id}: it holds no readable package; \
                             move or delete it"
                        ),
                    ));
                }
                target
            }
        };
        let permissions = &self.package.manifest.permissions;
        self.asks = match &previous {
            Some(installed) => permissions.beyond(&installed.manifest.permissions),
            None => permissions.clone(),
        };
        self.previous = previous;
        Ok(self)
    }

    /// The package as it will be installed. Its `dir` is the staging folder
    /// until committed.
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The manifest of the installed package this replaces.
    pub fn replaces(&self) -> Option<&PackageManifest> {
        self.previous.as_ref().map(|installed| &installed.manifest)
    }

    /// What this package asks for beyond what the installed version was
    /// granted: every permission for a new install.
    pub fn asks(&self) -> &Permissions {
        &self.asks
    }

    /// Whether the install needs a confirmation prompt: always for a new
    /// install, and for an update that asks for more or switches from data or
    /// WASM to native code.
    pub fn needs_confirmation(&self) -> bool {
        let switches_to_native = self.is_native()
            && self
                .previous
                .as_ref()
                .is_some_and(|installed| !matches!(installed.tier, Tier::Native { .. }));
        self.previous.is_none() || !self.asks.is_empty() || switches_to_native
    }

    /// Whether the package runs native code, which reaches the system beyond
    /// its declared permissions.
    pub fn is_native(&self) -> bool {
        matches!(self.package.tier, Tier::Native { .. })
    }

    /// Move the package into place and return it as installed.
    ///
    /// An installed package with the same id is moved aside first and put
    /// back if the move fails, so a failed commit leaves the previous version
    /// installed. Unmount the previous version before committing: on Windows
    /// a loaded library holds its folder.
    pub fn commit(self) -> Result<Package, PackageError> {
        let aside = match &self.previous {
            Some(_) => {
                let aside = WorkDir::claim(&self.plugins_dir, "replaced")?;
                fs::rename(&self.target, &aside.dir).map_err(|err| {
                    PackageError::new(&self.target, format!("moving it aside: {err}"))
                })?;
                Some(aside)
            }
            None => None,
        };
        if let Err(err) = fs::rename(&self.staging.dir, &self.target) {
            if let Some(aside) = &aside {
                let _ = fs::rename(&aside.dir, &self.target);
            }
            return Err(PackageError::new(&self.target, err.to_string()));
        }
        // Dropping `aside` deletes the previous version; one that fails to
        // delete is left for the next sweep.
        drop(aside);
        Package::open(&self.target)
    }
}

/// Delete the installed package with `id` from `plugins_dir` and return the
/// folder it was in. The plugin's storage under `plugin-data` is kept.
/// Unmount the plugin first.
pub fn uninstall(plugins_dir: &Path, id: &str) -> Result<PathBuf, PackageError> {
    let installed = discover(plugins_dir)
        .into_iter()
        .filter_map(Result::ok)
        .find(|package| package.manifest.id == id)
        .ok_or_else(|| PackageError::new(plugins_dir, format!("no installed package is {id}")))?;
    // Renamed first, so a removal that fails part way leaves a hidden folder
    // rather than a broken package.
    let aside = WorkDir::claim(plugins_dir, "removed")?;
    fs::rename(&installed.dir, &aside.dir)
        .map_err(|err| PackageError::new(&installed.dir, err.to_string()))?;
    fs::remove_dir_all(&aside.dir).map_err(|err| PackageError::new(&aside.dir, err.to_string()))?;
    Ok(installed.dir)
}

/// The prefixes of the hidden working folders: one per purpose.
const PURPOSES: [&str; 3] = ["install", "replaced", "removed"];

/// The suffix of the lock file beside a working folder.
const LOCK_SUFFIX: &str = ".lock";

/// Delete every hidden working folder in `plugins_dir` that no live process
/// holds, with its lock file. A folder held by another running Aria, or one
/// whose lock cannot be read, is left alone; so is every entry whose name is
/// not `.<purpose>-<pid>-<n>` or that with `.lock` after it.
pub fn sweep(plugins_dir: &Path) {
    let Ok(entries) = fs::read_dir(plugins_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(folder) = name.to_str().and_then(work_folder_name) else {
            continue;
        };
        let dir = plugins_dir.join(folder);
        let lock = lock_path(&dir);
        let held = match File::options().read(true).write(true).open(&lock) {
            Ok(file) => match file.try_lock() {
                Ok(()) => Some(file),
                Err(TryLockError::WouldBlock | TryLockError::Error(_)) => continue,
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        let _ = fs::remove_dir_all(&dir);
        // Unlinked while still locked, so a claim that opened this lock file
        // before the unlink finds it gone once it gets the lock.
        let _ = fs::remove_file(&lock);
        drop(held);
    }
}

/// The working folder `name` is, or is the lock file of: `None` for every
/// other entry.
fn work_folder_name(name: &str) -> Option<&str> {
    let folder = name.strip_suffix(LOCK_SUFFIX).unwrap_or(name);
    let rest = folder.strip_prefix('.')?;
    let (purpose, numbers) = rest.split_once('-')?;
    let (pid, n) = numbers.split_once('-')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (PURPOSES.contains(&purpose) && digits(pid) && digits(n)).then_some(folder)
}

fn lock_path(dir: &Path) -> PathBuf {
    let mut name = dir.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// A hidden working folder in a plugins folder, claimed by this process: its
/// lock file is locked exclusively for as long as the value lives, and the
/// drop deletes the folder and then the lock file.
#[derive(Debug)]
struct WorkDir {
    dir: PathBuf,
    lock: PathBuf,
    held: Option<File>,
}

impl WorkDir {
    /// Claim a fresh `.<purpose>-<pid>-<n>` name in `plugins_dir`. The folder
    /// itself is not created.
    fn claim(plugins_dir: &Path, purpose: &str) -> Result<Self, PackageError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let name = format!(
                ".{purpose}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let dir = plugins_dir.join(name);
            let lock = lock_path(&dir);
            let file = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock)
                .map_err(|err| PackageError::new(&lock, err.to_string()))?;
            match file.try_lock() {
                Ok(()) => {}
                // A process in another pid namespace has the same pid and
                // holds this name, or a sweep is deleting it: take the next.
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(err)) => {
                    return Err(PackageError::new(&lock, format!("locking it: {err}")));
                }
            }
            if !lock.exists() {
                // A sweep unlinked the file between the open and the lock.
                continue;
            }
            // Left by a dead process that had this pid; the lock proves it is
            // no one's now.
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|err| PackageError::new(&dir, err.to_string()))?;
            }
            return Ok(Self {
                dir,
                lock,
                held: Some(file),
            });
        }
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        let _ = fs::remove_file(&self.lock);
        drop(self.held.take());
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
