//! The scratch file a download is written to, and the atomic install.
//!
//! A download never writes the model's own name. It writes a uniquely named
//! temporary beside it inside the pinned root, verifies that the file it is
//! about to publish is still the one it wrote, and renames only after
//! `begin_commit` agrees. Dropping without committing removes the scratch file,
//! so a cancelled or failed install leaves the directory as it found it.
//!
//! The scratch file is a scratch path of [`aria_core::scratch`], claimed
//! through its lock file until the download commits or drops. Every new
//! download first deletes the scratch files in the model folder whose lock no
//! live download holds: what a download killed part way left. The claim, the
//! sweep and the unlink of the lock file all go through the pinned root, so a
//! model folder swapped for a link mid-download is never written through and
//! the lock file is unlinked from the folder it was made in.

use aria_core::scratch::{self, ScratchDir, ScratchLock};

use super::*;

pub(super) struct TemporaryDownload {
    root: CapabilityDir,
    name: PathBuf,
    /// The claim's lock file, unlinked through `root` by [`Drop::drop`].
    lock_name: PathBuf,
    /// Named by the installer's I/O errors, which report the file the write
    /// failed on rather than the model's final name.
    pub(super) path: PathBuf,
    file: Option<CapabilityFile>,
    identity: Option<FileIdentity>,
    committed: bool,
    /// Dropped after [`Drop::drop`] deletes an uncommitted file and the lock
    /// file.
    _claim: ScratchLock,
}

impl TemporaryDownload {
    pub(super) fn create(
        snapshot: &RootSnapshot,
        model_filename: &str,
    ) -> Result<Self, ModelManagerError> {
        let root = snapshot
            .dir
            .try_clone()
            .map_err(|source| ModelManagerError::Io {
                operation: "clone pinned model root",
                path: snapshot.canonical_root.clone(),
                source,
            })?;
        scratch::sweep_in(&Pinned(&root), is_download_scratch, |name| {
            let _ = root.remove_file(name);
        });
        for _ in 0..TEMP_FILE_ATTEMPTS {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                ".{model_filename}.download-{}-{sequence}.tmp",
                std::process::id()
            );
            let path = snapshot.canonical_root.join(&name);
            // `None`: a download in another process is using this name, or a
            // sweep is clearing it.
            let claim = match ScratchLock::claim_in(&Pinned(&root), &name) {
                Ok(Some(claim)) => claim,
                Ok(None) => continue,
                Err(source) => {
                    return Err(ModelManagerError::Io {
                        operation: "lock temporary model file",
                        path: scratch::lock_path(&path),
                        source,
                    });
                }
            };
            let name = PathBuf::from(name);
            let mut options = CapabilityOpenOptions::new();
            options.write(true).create_new(true);
            match root.open_with(&name, &options) {
                Ok(file) => {
                    let mut temporary = Self {
                        lock_name: scratch::lock_path(&name),
                        root,
                        name,
                        path,
                        file: Some(file),
                        identity: None,
                        committed: false,
                        _claim: claim,
                    };
                    let identity_file = temporary
                        .file
                        .as_ref()
                        .expect("new temporary file is open")
                        .try_clone()
                        .map(CapabilityFile::into_std)
                        .map_err(|source| ModelManagerError::Io {
                            operation: "clone temporary model file",
                            path: temporary.path.clone(),
                            source,
                        })?;
                    let metadata =
                        identity_file
                            .metadata()
                            .map_err(|source| ModelManagerError::Io {
                                operation: "inspect temporary model file",
                                path: temporary.path.clone(),
                                source,
                            })?;
                    ensure_regular_model_file(&temporary.path, &metadata)?;
                    temporary.identity =
                        Some(FileIdentity::from_file(identity_file).map_err(|source| {
                            ModelManagerError::Io {
                                operation: "identify temporary model file",
                                path: temporary.path.clone(),
                                source,
                            }
                        })?);
                    return Ok(temporary);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = root.remove_file(scratch::lock_path(&name));
                }
                Err(source) => {
                    let _ = root.remove_file(scratch::lock_path(&name));
                    return Err(ModelManagerError::Io {
                        operation: "create temporary model file",
                        path,
                        source,
                    });
                }
            }
        }

        Err(ModelManagerError::TemporaryNameExhausted {
            root: snapshot.canonical_root.clone(),
        })
    }

    pub(super) fn file_mut(&mut self) -> &mut CapabilityFile {
        self.file
            .as_mut()
            .expect("temporary model file is open until commit")
    }

    pub(super) fn verify_current(&self) -> Result<(), ModelManagerError> {
        let metadata =
            self.root
                .symlink_metadata(&self.name)
                .map_err(|source| ModelManagerError::Io {
                    operation: "reinspect temporary model file",
                    path: self.path.clone(),
                    source,
                })?;
        ensure_cap_regular_model_file(&self.path, &metadata)?;

        let current_file = self
            .root
            .open(&self.name)
            .map(CapabilityFile::into_std)
            .map_err(|source| ModelManagerError::Io {
                operation: "reopen temporary model file",
                path: self.path.clone(),
                source,
            })?;
        let current_metadata = current_file
            .metadata()
            .map_err(|source| ModelManagerError::Io {
                operation: "reinspect opened temporary model file",
                path: self.path.clone(),
                source,
            })?;
        ensure_regular_model_file(&self.path, &current_metadata)?;
        let current_identity =
            FileIdentity::from_file(current_file).map_err(|source| ModelManagerError::Io {
                operation: "reidentify temporary model file",
                path: self.path.clone(),
                source,
            })?;
        if self.identity.as_ref() != Some(&current_identity) {
            return Err(ModelManagerError::FilesystemObjectChanged {
                path: self.path.clone(),
                object: "temporary model file",
            });
        }
        Ok(())
    }

    pub(super) fn commit(
        mut self,
        target_name: &str,
        target_exists: bool,
        begin_commit: &dyn Fn() -> bool,
    ) -> Result<(), ModelManagerError> {
        drop(self.identity.take());
        drop(self.file.take());

        check_begin_commit(begin_commit)?;

        #[cfg(windows)]
        if target_exists {
            self.root
                .remove_file(target_name)
                .map_err(|source| ModelManagerError::Io {
                    operation: "remove invalid model before replacement",
                    path: self.root_path(target_name),
                    source,
                })?;
        }
        #[cfg(not(windows))]
        let _ = target_exists;

        self.root
            .rename(&self.name, &self.root, target_name)
            .map_err(|source| ModelManagerError::Io {
                operation: "atomically install model file",
                path: self.root_path(target_name),
                source,
            })?;
        self.committed = true;
        Ok(())
    }

    fn root_path(&self, name: &str) -> PathBuf {
        self.path
            .parent()
            .expect("temporary model path has a parent")
            .join(name)
    }
}

/// The pinned model folder, reached through its handle.
struct Pinned<'a>(&'a CapabilityDir);

impl ScratchDir for Pinned<'_> {
    fn names(&self) -> io::Result<Vec<String>> {
        Ok(self
            .0
            .entries()?
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect())
    }

    fn open_lock(&self, name: &str) -> io::Result<fs::File> {
        let mut options = CapabilityOpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        self.0
            .open_with(name, &options)
            .map(CapabilityFile::into_std)
    }

    fn contains(&self, name: &str) -> bool {
        self.0.symlink_metadata(name).is_ok()
    }

    fn unlink(&self, name: &str) -> io::Result<()> {
        self.0.remove_file(name)
    }
}

/// Whether `name` is a download's scratch file:
/// `.<model>.download-<pid>-<n>.tmp`.
fn is_download_scratch(name: &str) -> bool {
    let Some((model, numbers)) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .and_then(|rest| rest.rsplit_once(".download-"))
    else {
        return false;
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    !model.is_empty()
        && numbers
            .split_once('-')
            .is_some_and(|(pid, n)| digits(pid) && digits(n))
}

impl Drop for TemporaryDownload {
    fn drop(&mut self) {
        if !self.committed {
            drop(self.identity.take());
            drop(self.file.take());
            let _ = self.root.remove_file(&self.name);
        }
        let _ = self.root.remove_file(&self.lock_name);
    }
}
