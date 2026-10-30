//! The pinned model root, and the checks that keep it pinned.
//!
//! [`RootSnapshot`] captures the configured directory once (its canonical
//! path, a capability handle, and the file identity of the directory itself),
//! and every operation revalidates that mapping before it reports success or
//! modifies a path.

use super::*;

pub(in crate::model) struct RootSnapshot {
    lexical_root: PathBuf,
    /// Read by [`TemporaryDownload`], which creates its scratch file beside the
    /// model inside the pinned root.
    pub(super) canonical_root: PathBuf,
    /// Read by the inspector and the temporary file: every model I/O goes
    /// through this handle rather than through a path resolved again.
    pub(super) dir: CapabilityDir,
    identity: FileIdentity,
}

impl RootSnapshot {
    pub(super) fn capture(lexical_root: &Path) -> Result<Self, ModelManagerError> {
        let metadata =
            fs::symlink_metadata(lexical_root).map_err(|source| ModelManagerError::Io {
                operation: "inspect model root",
                path: lexical_root.to_path_buf(),
                source,
            })?;
        ensure_directory_root(lexical_root, &metadata)?;

        let canonical_root =
            fs::canonicalize(lexical_root).map_err(|source| ModelManagerError::Io {
                operation: "canonicalize model root",
                path: lexical_root.to_path_buf(),
                source,
            })?;
        let dir = CapabilityDir::open_ambient_dir(&canonical_root, ambient_authority()).map_err(
            |source| ModelManagerError::Io {
                operation: "open canonical model root",
                path: canonical_root.clone(),
                source,
            },
        )?;
        let identity_file =
            dir.try_clone()
                .map(CapabilityDir::into_std_file)
                .map_err(|source| ModelManagerError::Io {
                    operation: "clone canonical model root handle",
                    path: canonical_root.clone(),
                    source,
                })?;
        let opened_metadata = identity_file
            .metadata()
            .map_err(|source| ModelManagerError::Io {
                operation: "inspect canonical model root handle",
                path: canonical_root.clone(),
                source,
            })?;
        ensure_directory_root(&canonical_root, &opened_metadata)?;
        let identity =
            FileIdentity::from_file(identity_file).map_err(|source| ModelManagerError::Io {
                operation: "identify canonical model root",
                path: canonical_root.clone(),
                source,
            })?;

        let snapshot = Self {
            lexical_root: lexical_root.to_path_buf(),
            canonical_root,
            dir,
            identity,
        };
        snapshot.verify_mapping()?;
        Ok(snapshot)
    }

    pub(in crate::model) fn physical_model_path(&self, spec: &ModelSpec) -> PathBuf {
        self.canonical_root.join(spec.filename())
    }

    pub(super) fn configured_model_path(&self, spec: &ModelSpec) -> PathBuf {
        self.lexical_root.join(spec.filename())
    }

    pub(super) fn verify_mapping(&self) -> Result<(), ModelManagerError> {
        let unchanged = || -> Option<bool> {
            let metadata = fs::symlink_metadata(&self.lexical_root).ok()?;
            if ensure_directory_root(&self.lexical_root, &metadata).is_err() {
                return Some(false);
            }

            let canonical = fs::canonicalize(&self.lexical_root).ok()?;
            if canonical != self.canonical_root {
                return Some(false);
            }

            let identity = FileIdentity::from_path(&self.lexical_root).ok()?;
            if identity != self.identity {
                return Some(false);
            }

            let final_metadata = fs::symlink_metadata(&self.lexical_root).ok()?;
            if ensure_directory_root(&self.lexical_root, &final_metadata).is_err() {
                return Some(false);
            }
            let final_canonical = fs::canonicalize(&self.lexical_root).ok()?;
            Some(final_canonical == self.canonical_root)
        };

        if unchanged() == Some(true) {
            Ok(())
        } else {
            Err(ModelManagerError::RootMappingChanged {
                root: self.lexical_root.clone(),
                pinned_root: self.canonical_root.clone(),
            })
        }
    }
}

pub(super) fn verify_model_file_unchanged(
    snapshot: &RootSnapshot,
    spec: &ModelSpec,
    expected_identity: &FileIdentity,
    expected_version: &FileVersion,
) -> Result<cap_std::fs::Metadata, ModelManagerError> {
    let path = snapshot.physical_model_path(spec);
    let metadata = snapshot
        .dir
        .symlink_metadata(spec.filename())
        .map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                ModelManagerError::FilesystemObjectChanged {
                    path: path.clone(),
                    object: "model file",
                }
            } else {
                ModelManagerError::Io {
                    operation: "reinspect model file",
                    path: path.clone(),
                    source,
                }
            }
        })?;
    ensure_cap_regular_model_file(&path, &metadata)?;

    let current_file = snapshot
        .dir
        .open(spec.filename())
        .map(CapabilityFile::into_std)
        .map_err(|source| ModelManagerError::Io {
            operation: "reopen model file for identity verification",
            path: path.clone(),
            source,
        })?;
    let current_metadata = current_file
        .metadata()
        .map_err(|source| ModelManagerError::Io {
            operation: "reinspect opened model file",
            path: path.clone(),
            source,
        })?;
    ensure_regular_model_file(&path, &current_metadata)?;
    let current_version = FileVersion::from_metadata(&current_metadata);
    let current_identity =
        FileIdentity::from_file(current_file).map_err(|source| ModelManagerError::Io {
            operation: "reidentify model file",
            path: path.clone(),
            source,
        })?;
    if &current_identity != expected_identity || &current_version != expected_version {
        return Err(ModelManagerError::FilesystemObjectChanged {
            path,
            object: "model file",
        });
    }

    snapshot.verify_mapping()?;
    Ok(metadata)
}

pub(super) fn ensure_safe_replace_target(
    snapshot: &RootSnapshot,
    spec: &ModelSpec,
) -> Result<bool, ModelManagerError> {
    // A first install has no replacement target, which is the common case and
    // the one that should allocate nothing.
    let metadata = match snapshot.dir.symlink_metadata(spec.filename()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(ModelManagerError::Io {
                operation: "inspect model replacement target",
                path: snapshot.physical_model_path(spec),
                source,
            });
        }
    };
    ensure_cap_regular_model_file(&snapshot.physical_model_path(spec), &metadata)?;
    Ok(true)
}

#[cfg(unix)]
pub(super) fn sync_containing_directory(
    snapshot: &RootSnapshot,
    installed_path: &Path,
) -> Result<(), ModelManagerError> {
    // cap-std opens directories with O_PATH on Linux, and fsync on an O_PATH
    // descriptor fails with EBADF, so reopen a regular read-only descriptor
    // and confirm it still names the pinned root before syncing through it.
    let sync_failed = |source| ModelManagerError::InstalledButDirectorySyncFailed {
        path: installed_path.to_path_buf(),
        source,
    };
    let directory = fs::File::open(&snapshot.canonical_root).map_err(sync_failed)?;
    let identity = FileIdentity::from_file(directory).map_err(sync_failed)?;
    if identity != snapshot.identity {
        return Err(ModelManagerError::RootMappingChanged {
            root: snapshot.lexical_root.clone(),
            pinned_root: snapshot.canonical_root.clone(),
        });
    }
    identity.as_file().sync_all().map_err(sync_failed)
}

#[cfg(not(unix))]
pub(super) fn sync_containing_directory(
    _snapshot: &RootSnapshot,
    _installed_path: &Path,
) -> Result<(), ModelManagerError> {
    Ok(())
}
