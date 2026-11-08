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
//! that process exits, however it exits, so [`sweep`] clears exactly the
//! folders whose process is gone: an install killed at its prompt and a
//! removal whose delete failed are deleted, and the previous version of an
//! update cut short is put back.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use aria_core::scratch::{self, ScratchLock};

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
    fs::create_dir_all(plugins_dir).map_err(|err| PackageError::io(plugins_dir, &err))?;
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
    /// installed. If it cannot be put back either, its folder is kept and the
    /// next [`sweep`] puts it back. Unmount the previous version before
    /// committing: on Windows a loaded library holds its folder.
    pub fn commit(self) -> Result<Package, PackageError> {
        let aside = match &self.previous {
            Some(_) => {
                let aside = WorkDir::claim(&self.plugins_dir, "replaced")?;
                fs::rename(&self.target, &aside.dir).map_err(|err| {
                    PackageError::new(
                        &self.target,
                        format!("moving it aside: {}", aria_models::io_reason(&err)),
                    )
                })?;
                Some(aside)
            }
            None => None,
        };
        if let Err(err) = fs::rename(&self.staging.dir, &self.target) {
            let mut reason = aria_models::io_reason(&err);
            if let Some(aside) = aside
                && let Err(back) = fs::rename(&aside.dir, &self.target)
            {
                reason = format!(
                    "{reason}; the previous version could not be put back ({}), so it is \
                     kept in {} and put back at the next start",
                    aria_models::io_reason(&back),
                    aside.dir.display()
                );
                aside.keep();
            }
            return Err(PackageError::new(&self.target, reason));
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
    // rather than a broken package. Once renamed the package is gone; a
    // delete that fails leaves the folder for the next sweep.
    let aside = WorkDir::claim(plugins_dir, "removed")?;
    fs::rename(&installed.dir, &aside.dir).map_err(|err| PackageError::io(&installed.dir, &err))?;
    drop(aside);
    Ok(installed.dir)
}

/// The prefixes of the hidden working folders: one per purpose.
const PURPOSES: [&str; 3] = ["install", "replaced", "removed"];

/// Clear every hidden working folder in `plugins_dir` that no live process
/// holds, with its lock file. A `.replaced-` folder holds the previous version
/// of an update. When no installed package has its id, the update never put
/// its package in place (a kill between the two moves, or a failed put-back):
/// the folder is moved back to the folder named by the id, or kept for the
/// next sweep while something else takes that name. Every other dead working
/// folder is deleted. A folder held by another running Aria, or one whose
/// lock cannot be read, is left alone; so is every entry whose name is not
/// `.<purpose>-<pid>-<n>` or that with `.lock` after it.
pub fn sweep(plugins_dir: &Path) {
    scratch::sweep(plugins_dir, is_work_folder, |dir| {
        let replaced = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".replaced-"));
        if !(replaced && restore_previous(plugins_dir, dir)) {
            let _ = scratch::remove(dir);
        }
    });
}

/// Whether the dead `.replaced-` folder `dir` holds a previous version that no
/// installed package supersedes. Such a folder is moved back to the folder
/// named by its id when that name is free; either way it must not be deleted.
fn restore_previous(plugins_dir: &Path, dir: &Path) -> bool {
    let Ok(previous) = Package::open(dir) else {
        return false;
    };
    let id = &previous.manifest.id;
    let superseded = discover(plugins_dir)
        .into_iter()
        .filter_map(Result::ok)
        .any(|package| package.manifest.id == *id);
    if superseded {
        return false;
    }
    let target = plugins_dir.join(id);
    if !target.exists() {
        let _ = fs::rename(dir, &target);
    }
    true
}

/// Whether `name` is a working folder's: `.<purpose>-<pid>-<n>`.
fn is_work_folder(name: &str) -> bool {
    let Some((purpose, numbers)) = name.strip_prefix('.').and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    PURPOSES.contains(&purpose)
        && numbers
            .split_once('-')
            .is_some_and(|(pid, n)| digits(pid) && digits(n))
}

/// A hidden working folder in a plugins folder, claimed by this process
/// through a [`ScratchLock`] for as long as the value lives. The drop deletes
/// the folder and then releases the claim.
#[derive(Debug)]
struct WorkDir {
    dir: PathBuf,
    claim: Option<ScratchLock>,
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
            // `None`: a process in another pid namespace has the same pid and
            // holds this name, or a sweep is clearing it: take the next.
            let Some(claim) =
                ScratchLock::claim(&dir).map_err(|err| PackageError::io(plugins_dir, &err))?
            else {
                continue;
            };
            // Left by a dead process that had this pid; the claim proves it
            // is no one's now.
            scratch::remove(&dir).map_err(|err| PackageError::io(&dir, &err))?;
            return Ok(Self {
                dir,
                claim: Some(claim),
            });
        }
    }

    /// Release the claim and leave the folder for the next [`sweep`].
    fn keep(mut self) {
        drop(self.claim.take());
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let Some(claim) = self.claim.take() else {
            return;
        };
        let _ = fs::remove_dir_all(&self.dir);
        drop(claim);
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
