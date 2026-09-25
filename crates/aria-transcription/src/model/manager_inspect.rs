//! Reading what is on disk: status, verification, removal, and root capture.
//!
//! Split out of `manager.rs` so the installer and the inspector are separate
//! files rather than one 986-line one. Nothing here downloads: every function
//! answers a question about the model directory as it stands, under the same
//! pinned-root discipline the installer uses.

use super::manager_root::verify_model_file_unchanged;
use super::*;

impl ModelManager {
    pub(in crate::model) fn status_spec(
        &self,
        spec: &ModelSpec,
    ) -> Result<ModelStatus, ModelManagerError> {
        validate_spec(spec)?;
        let Some(snapshot) = self.capture_existing_root()? else {
            return Ok(ModelStatus::Missing);
        };
        let physical_path = snapshot.physical_model_path(spec);
        let lock = model_lock(&physical_path);
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        snapshot.verify_mapping()?;
        self.inspect_path(spec, &snapshot)
    }

    pub(super) fn verify_spec(&self, spec: &ModelSpec) -> Result<PathBuf, ModelManagerError> {
        match self.status_spec(spec)? {
            ModelStatus::Missing => Err(ModelManagerError::MissingModel {
                path: self.path_for_spec(spec)?,
            }),
            ModelStatus::Ready { path } => Ok(path),
            ModelStatus::Invalid { path, reason } => {
                Err(ModelManagerError::Integrity { path, reason })
            }
        }
    }

    pub(in crate::model) fn remove_spec(
        &self,
        spec: &ModelSpec,
    ) -> Result<bool, ModelManagerError> {
        validate_spec(spec)?;
        let Some(snapshot) = self.capture_existing_root()? else {
            return Ok(false);
        };
        let physical_path = snapshot.physical_model_path(spec);
        let lock = model_lock(&physical_path);
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        snapshot.verify_mapping()?;
        self.remove_from_snapshot(spec, &snapshot)
    }

    pub(in crate::model) fn remove_from_snapshot(
        &self,
        spec: &ModelSpec,
        snapshot: &RootSnapshot,
    ) -> Result<bool, ModelManagerError> {
        // Allocated after the early return: removing a model that is already
        // absent is the repeat case, and it needs no path at all.
        let metadata = match snapshot.dir.symlink_metadata(spec.filename()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                snapshot.verify_mapping()?;
                return Ok(false);
            }
            Err(source) => {
                return Err(ModelManagerError::Io {
                    operation: "inspect model file",
                    path: snapshot.physical_model_path(spec),
                    source,
                });
            }
        };
        let path = snapshot.physical_model_path(spec);
        ensure_cap_regular_model_file(&path, &metadata)?;
        snapshot.verify_mapping()?;
        snapshot
            .dir
            .remove_file(spec.filename())
            .map_err(|source| ModelManagerError::Io {
                operation: "remove model file",
                path,
                source,
            })?;
        snapshot.verify_mapping()?;
        Ok(true)
    }

    pub(in crate::model) fn path_for_spec(
        &self,
        spec: &ModelSpec,
    ) -> Result<PathBuf, ModelManagerError> {
        validate_spec(spec)?;
        Ok(self.root.join(spec.filename()))
    }

    pub(in crate::model) fn inspect_path(
        &self,
        spec: &ModelSpec,
        snapshot: &RootSnapshot,
    ) -> Result<ModelStatus, ModelManagerError> {
        // Neither path is allocated until the file is known to exist. `status`
        // runs once per catalogued model on every Settings render, and the
        // answer for a model nobody installed is `Missing`.
        let metadata = match snapshot.dir.symlink_metadata(spec.filename()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                snapshot.verify_mapping()?;
                return Ok(ModelStatus::Missing);
            }
            Err(source) => {
                return Err(ModelManagerError::Io {
                    operation: "inspect model file",
                    path: snapshot.physical_model_path(spec),
                    source,
                });
            }
        };
        let path = snapshot.physical_model_path(spec);
        let reported_path = snapshot.configured_model_path(spec);
        ensure_cap_regular_model_file(&path, &metadata)?;
        if metadata.len() != spec.exact_bytes() {
            snapshot.verify_mapping()?;
            return Ok(ModelStatus::Invalid {
                path: reported_path,
                reason: ModelIntegrityError::WrongSize {
                    expected: spec.exact_bytes(),
                    actual: metadata.len(),
                },
            });
        }

        let mut file =
            snapshot
                .dir
                .open(spec.filename())
                .map_err(|source| ModelManagerError::Io {
                    operation: "open model file",
                    path: path.clone(),
                    source,
                })?;
        let identity_file = file
            .try_clone()
            .map(CapabilityFile::into_std)
            .map_err(|source| ModelManagerError::Io {
                operation: "clone opened model file",
                path: path.clone(),
                source,
            })?;
        let opened_metadata = identity_file
            .metadata()
            .map_err(|source| ModelManagerError::Io {
                operation: "inspect opened model file",
                path: path.clone(),
                source,
            })?;
        ensure_regular_model_file(&path, &opened_metadata)?;
        let opened_version = FileVersion::from_metadata(&opened_metadata);
        let opened_identity =
            FileIdentity::from_file(identity_file).map_err(|source| ModelManagerError::Io {
                operation: "identify opened model file",
                path: path.clone(),
                source,
            })?;

        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0_u8; DOWNLOAD_BUFFER_BYTES];
        loop {
            let read_length = next_read_length(spec.exact_bytes(), total, buffer.len());
            let count =
                file.read(&mut buffer[..read_length])
                    .map_err(|source| ModelManagerError::Io {
                        operation: "read model file",
                        path: path.clone(),
                        source,
                    })?;
            if count == 0 {
                break;
            }
            total = total.saturating_add(count as u64);
            if total > spec.exact_bytes() {
                verify_model_file_unchanged(snapshot, spec, &opened_identity, &opened_version)?;
                return Ok(ModelStatus::Invalid {
                    path: reported_path,
                    reason: ModelIntegrityError::WrongSize {
                        expected: spec.exact_bytes(),
                        actual: total,
                    },
                });
            }
            hasher.update(&buffer[..count]);
        }
        let actual_sha256 = digest_to_hex(&hasher.finalize());
        let final_metadata =
            verify_model_file_unchanged(snapshot, spec, &opened_identity, &opened_version)?;
        if total != spec.exact_bytes() {
            return Ok(ModelStatus::Invalid {
                path: reported_path,
                reason: ModelIntegrityError::WrongSize {
                    expected: spec.exact_bytes(),
                    actual: total,
                },
            });
        }

        if actual_sha256 != spec.sha256() {
            return Ok(ModelStatus::Invalid {
                path: reported_path,
                reason: ModelIntegrityError::ChecksumMismatch {
                    expected: spec.sha256().to_owned(),
                    actual: actual_sha256,
                },
            });
        }

        if final_metadata.len() != spec.exact_bytes() {
            return Ok(ModelStatus::Invalid {
                path: reported_path,
                reason: ModelIntegrityError::WrongSize {
                    expected: spec.exact_bytes(),
                    actual: final_metadata.len(),
                },
            });
        }

        snapshot.verify_mapping()?;
        Ok(ModelStatus::Ready {
            path: reported_path,
        })
    }

    pub(in crate::model) fn capture_existing_root(
        &self,
    ) -> Result<Option<RootSnapshot>, ModelManagerError> {
        let metadata = match fs::symlink_metadata(&self.root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ModelManagerError::Io {
                    operation: "inspect model root",
                    path: self.root.clone(),
                    source,
                });
            }
        };
        ensure_directory_root(&self.root, &metadata)?;
        RootSnapshot::capture(&self.root).map(Some)
    }

    pub(super) fn create_and_capture_root(&self) -> Result<RootSnapshot, ModelManagerError> {
        fs::create_dir_all(&self.root).map_err(|source| ModelManagerError::Io {
            operation: "create model root",
            path: self.root.clone(),
            source,
        })?;
        RootSnapshot::capture(&self.root)
    }
}
