//! Installing a package archive into a plugin folder, and removing one.
//!
//! [`stage`] unpacks an archive into a hidden folder beside the installed
//! packages and reads it; nothing in it runs. The caller shows the staged
//! package and what it asks for, then [`Staged::commit`] moves it into place,
//! replacing an installed package with the same id. Dropping a [`Staged`]
//! without committing deletes the unpacked files.

use std::fs;
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
    committed: bool,
}

/// Unpack `archive` into `plugins_dir` and read the package it holds.
///
/// The package must have a code or data tier this host reads, and an id
/// that no differently named folder in `plugins_dir` already takes.
pub fn stage(archive: &Path, plugins_dir: &Path) -> Result<Staged, PackageError> {
    fs::create_dir_all(plugins_dir)
        .map_err(|err| PackageError::new(plugins_dir, err.to_string()))?;
    let staging = plugins_dir.join(hidden_name("install"));
    let package = unpack(archive, &staging)?;
    let staged = Staged {
        package,
        previous: None,
        target: PathBuf::new(),
        asks: Permissions::default(),
        plugins_dir: plugins_dir.to_owned(),
        committed: false,
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
    /// install, and for an update that asks for more.
    pub fn needs_confirmation(&self) -> bool {
        self.previous.is_none() || !self.asks.is_empty()
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
    pub fn commit(mut self) -> Result<Package, PackageError> {
        let aside = self.plugins_dir.join(hidden_name("replaced"));
        let had_previous = self.previous.is_some();
        if had_previous {
            fs::rename(&self.target, &aside).map_err(|err| {
                PackageError::new(&self.target, format!("moving it aside: {err}"))
            })?;
        }
        if let Err(err) = fs::rename(&self.package.dir, &self.target) {
            if had_previous {
                let _ = fs::rename(&aside, &self.target);
            }
            return Err(PackageError::new(&self.target, err.to_string()));
        }
        self.committed = true;
        if had_previous {
            let _ = fs::remove_dir_all(&aside);
        }
        Package::open(&self.target)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.package.dir);
        }
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
    let aside = plugins_dir.join(hidden_name("removed"));
    fs::rename(&installed.dir, &aside)
        .map_err(|err| PackageError::new(&installed.dir, err.to_string()))?;
    fs::remove_dir_all(&aside).map_err(|err| PackageError::new(&aside, err.to_string()))?;
    Ok(installed.dir)
}

/// A folder name `discover` passes over and no other install in this process
/// or another one shares.
fn hidden_name(purpose: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        ".{purpose}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
