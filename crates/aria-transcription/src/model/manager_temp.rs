//! The scratch file a download is written to, and the atomic install.
//!
//! A download never writes the model's own name. It writes a uniquely named
//! temporary beside it inside the pinned root, verifies that the file it is
//! about to publish is still the one it wrote, and renames only after
//! `begin_commit` agrees. Dropping without committing removes the scratch file,
//! so a cancelled or failed install leaves the directory as it found it.

use super::*;

pub(super) struct TemporaryDownload {
    root: CapabilityDir,
    name: PathBuf,
    /// Named by the installer's I/O errors, which report the file the write
    /// failed on rather than the model's final name.
    pub(super) path: PathBuf,
    file: Option<CapabilityFile>,
    identity: Option<FileIdentity>,
    committed: bool,
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
        for _ in 0..TEMP_FILE_ATTEMPTS {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = PathBuf::from(format!(
                ".{model_filename}.download-{}-{sequence}.tmp",
                std::process::id()
            ));
            let path = snapshot.canonical_root.join(&name);
            let mut options = CapabilityOpenOptions::new();
            options.write(true).create_new(true);
            match root.open_with(&name, &options) {
                Ok(file) => {
                    let mut temporary = Self {
                        root,
                        name,
                        path,
                        file: Some(file),
                        identity: None,
                        committed: false,
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
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => {
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

impl Drop for TemporaryDownload {
    fn drop(&mut self) {
        if !self.committed {
            drop(self.identity.take());
            drop(self.file.take());
            let _ = self.root.remove_file(&self.name);
        }
    }
}
