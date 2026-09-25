//! Transactional installation of catalogued Whisper models.
//!
//! This file owns the public surface and the install path. The rest of the
//! machinery lives in three siblings, because one file holding the installer,
//! the inspector, the pinned root and the scratch file was 986 lines and no
//! reader could hold it:
//!
//! - [`manager_root`] pins the model directory and revalidates the mapping.
//! - [`manager_temp`] owns the scratch file and the atomic rename.
//! - [`manager_inspect`] answers what is on disk: status, verify, remove.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use cap_std::ambient_authority;
use cap_std::fs::{
    Dir as CapabilityDir, File as CapabilityFile, OpenOptions as CapabilityOpenOptions,
};
use same_file::Handle as FileIdentity;
use sha2::{Digest, Sha256};

use super::catalog::{
    ModelIntegrityError, ModelManagerError, ModelSpec, ModelStatus, WhisperModel,
};
use super::download::{HttpDownloader, ModelDownloader};
use super::filesystem::{
    FileVersion, TEMP_FILE_SEQUENCE, check_begin_commit, check_cancelled, digest_to_hex,
    ensure_cap_regular_model_file, ensure_directory_root, ensure_regular_model_file,
    integrity_error, lock_model_with_cancellation, model_lock, never_cancelled, next_read_length,
    validate_root, validate_spec,
};

const DOWNLOAD_BUFFER_BYTES: usize = 64 * 1024;
const TEMP_FILE_ATTEMPTS: u64 = 256;

#[path = "manager_inspect.rs"]
mod manager_inspect;
#[path = "manager_root.rs"]
mod manager_root;
#[path = "manager_temp.rs"]
mod manager_temp;

// Re-exported at this level so the siblings' `use super::*` reaches them, and
// so `model`'s own test glob keeps naming `RootSnapshot`.
pub(in crate::model) use manager_root::RootSnapshot;
use manager_root::{ensure_safe_replace_target, sync_containing_directory};
use manager_temp::TemporaryDownload;

/// Safe, transactional storage for built-in Whisper models.
///
/// Every disk operation pins a canonical root directory handle, performs
/// model I/O relative to that handle, and revalidates that the configured root
/// still names the pinned directory before reporting success or mutating a
/// known model name.
#[derive(Debug, Clone)]
pub struct ModelManager {
    root: PathBuf,
}

impl ModelManager {
    /// Validates and stores an absolute, non-root model directory.
    ///
    /// This constructor performs lexical validation only and does not create
    /// the directory. The final root itself is rejected later if it is a
    /// symlink or non-directory object.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError`] for a relative path, filesystem root, or
    /// explicit lexical traversal.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ModelManagerError> {
        Ok(Self {
            root: validate_root(root.as_ref())?,
        })
    }

    /// Returns the validated model directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the fixed path for a built-in model without touching the disk.
    ///
    /// # Errors
    ///
    /// Returns an error only if a compiled catalog invariant is invalid.
    pub fn model_path(&self, model: WhisperModel) -> Result<PathBuf, ModelManagerError> {
        self.path_for_spec(model.spec())
    }

    /// Verifies the known file, returning missing, ready, or invalid status.
    ///
    /// Links, Windows reparse points, and non-regular model files are rejected
    /// as errors rather than represented as ordinary corruption.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError`] for unsafe filesystem objects or I/O
    /// failures.
    pub fn status(&self, model: WhisperModel) -> Result<ModelStatus, ModelManagerError> {
        self.status_spec(model.spec())
    }

    /// Requires exact size and SHA-256 verification and returns the model path.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError::MissingModel`], an integrity error, an
    /// unsafe-file error, or an I/O error.
    pub fn verify(&self, model: WhisperModel) -> Result<PathBuf, ModelManagerError> {
        self.verify_spec(model.spec())
    }

    /// Ensures a verified model using the bounded default HTTPS downloader.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError`] when verification, downloading, durable
    /// writing, or atomic installation fails.
    pub fn ensure(&self, model: WhisperModel) -> Result<PathBuf, ModelManagerError> {
        self.ensure_with(model, &HttpDownloader::default())
    }

    /// Ensures a verified model using an injected streaming downloader.
    ///
    /// Concurrent calls for the same canonical model path are serialized
    /// within the process, including calls through lexical directory aliases.
    /// Existing files are reused only after full verification.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError`] when verification, downloading, durable
    /// writing, or atomic installation fails.
    pub fn ensure_with(
        &self,
        model: WhisperModel,
        downloader: &dyn ModelDownloader,
    ) -> Result<PathBuf, ModelManagerError> {
        self.ensure_with_cancellation(model, downloader, &never_cancelled)
    }

    /// Ensures a verified model with cooperative cancellation.
    ///
    /// `cancelled` is borrowed for this call, so it may directly borrow a job
    /// context. It can be called many times and should be inexpensive. A panic
    /// from the callback is treated as a cancellation request.
    ///
    /// Cancellation is checked around root setup, lock acquisition, existing
    /// model verification, stream establishment, each download read, and
    /// durable temporary-file writes. A single blocking transport or reader
    /// call, and the existing-model hash pass, cannot be interrupted midway.
    ///
    /// The final cancellation check is used as this standalone API's commit
    /// decision. Once it passes, cancellation is no longer consulted:
    /// replacement, durability confirmation, and root-mapping checks finish
    /// and return their actual result. An outer job that must atomically
    /// coordinate cancellation with publication should instead use
    /// [`Self::ensure_with_cancellation_and_commit`] and bind its commit
    /// callback to the job owner's atomic commit handshake.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError::Cancelled`] when cancellation is observed
    /// before commit. Other failures retain the same meaning as
    /// [`Self::ensure_with`].
    pub fn ensure_with_cancellation(
        &self,
        model: WhisperModel,
        downloader: &dyn ModelDownloader,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PathBuf, ModelManagerError> {
        let begin_commit = || check_cancelled(cancelled).is_ok();
        self.ensure_with_cancellation_and_commit(model, downloader, cancelled, &begin_commit)
    }

    /// Ensures a verified model with cancellation and an atomic commit handshake.
    ///
    /// Both callbacks are borrowed for this call and may directly borrow an
    /// outer job context. `cancelled` has the same cooperative semantics as in
    /// [`Self::ensure_with_cancellation`]: it may be called many times before
    /// publication and a panic is treated as cancellation.
    ///
    /// `begin_commit` is not called when a verified existing model is reused.
    /// For a download or replacement, it is called exactly once, after the
    /// complete stream has passed exact length and SHA-256 verification, the
    /// temporary file has been flushed and synced, temporary-file identity and
    /// replacement-target safety have been checked, and the configured root
    /// mapping has been revalidated. The call occurs immediately before the
    /// first target removal or rename.
    ///
    /// Returning `false`, or panicking, fails closed with
    /// [`ModelManagerError::Cancelled`]. The temporary file is cleaned up and
    /// an existing target is left unchanged. Returning `true` is the point of
    /// no return: neither callback is consulted again, and replacement,
    /// directory durability, and final root-mapping checks return their actual
    /// result.
    ///
    /// An outer job manager can remove the cancellation/publication race by
    /// making `begin_commit` perform its atomic transition from cancellable to
    /// committing (for example, by calling its `try_begin_commit` operation).
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError::Cancelled`] when cancellation is observed
    /// before commit or the commit callback rejects or panics. Other failures
    /// retain the same meaning as [`Self::ensure_with`].
    pub fn ensure_with_cancellation_and_commit(
        &self,
        model: WhisperModel,
        downloader: &dyn ModelDownloader,
        cancelled: &dyn Fn() -> bool,
        begin_commit: &dyn Fn() -> bool,
    ) -> Result<PathBuf, ModelManagerError> {
        self.ensure_spec_with_cancellation_and_commit(
            model.spec(),
            downloader,
            cancelled,
            begin_commit,
        )
    }

    /// Removes only the known regular model file.
    ///
    /// Returns `true` when a file was removed and `false` when it was already
    /// absent. Directories are never removed. Link and Windows reparse-point
    /// model files are rejected and left untouched.
    ///
    /// # Errors
    ///
    /// Returns [`ModelManagerError`] for unsafe filesystem objects or I/O
    /// failures.
    pub fn remove(&self, model: WhisperModel) -> Result<bool, ModelManagerError> {
        self.remove_spec(model.spec())
    }

    #[cfg(test)]
    pub(super) fn ensure_spec(
        &self,
        spec: &ModelSpec,
        downloader: &dyn ModelDownloader,
    ) -> Result<PathBuf, ModelManagerError> {
        self.ensure_spec_with_cancellation(spec, downloader, &never_cancelled)
    }

    #[cfg(test)]
    pub(super) fn ensure_spec_with_cancellation(
        &self,
        spec: &ModelSpec,
        downloader: &dyn ModelDownloader,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PathBuf, ModelManagerError> {
        let begin_commit = || check_cancelled(cancelled).is_ok();
        self.ensure_spec_with_cancellation_and_commit(spec, downloader, cancelled, &begin_commit)
    }

    pub(super) fn ensure_spec_with_cancellation_and_commit(
        &self,
        spec: &ModelSpec,
        downloader: &dyn ModelDownloader,
        cancelled: &dyn Fn() -> bool,
        begin_commit: &dyn Fn() -> bool,
    ) -> Result<PathBuf, ModelManagerError> {
        validate_spec(spec)?;
        check_cancelled(cancelled)?;
        let snapshot = self.create_and_capture_root()?;
        let physical_path = snapshot.physical_model_path(spec);
        let reported_path = snapshot.configured_model_path(spec);
        let lock = model_lock(&physical_path);
        let _guard = lock_model_with_cancellation(lock.as_ref(), cancelled)?;
        snapshot.verify_mapping()?;

        check_cancelled(cancelled)?;
        let status = self.inspect_path(spec, &snapshot);
        check_cancelled(cancelled)?;
        if matches!(status?, ModelStatus::Ready { .. }) {
            return Ok(reported_path);
        }

        let mut temporary = TemporaryDownload::create(&snapshot, spec.filename())?;
        check_cancelled(cancelled)?;
        let download = downloader.download(spec);
        check_cancelled(cancelled)?;
        let mut reader = download?;
        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0_u8; DOWNLOAD_BUFFER_BYTES];

        loop {
            let read_length = next_read_length(spec.exact_bytes(), total, buffer.len());
            check_cancelled(cancelled)?;
            let read = reader.read(&mut buffer[..read_length]);
            check_cancelled(cancelled)?;
            let count = read.map_err(|source| ModelManagerError::Io {
                operation: "read model download",
                path: temporary.path.clone(),
                source,
            })?;
            if count == 0 {
                break;
            }
            let observed = total.saturating_add(count as u64);
            if observed > spec.exact_bytes() {
                return Err(ModelManagerError::DownloadTooLarge {
                    expected: spec.exact_bytes(),
                    observed,
                });
            }

            // The mutable borrow of `temporary` is released by the semicolon,
            // so the error path can read the path off it instead of a copy
            // hoisted before the loop. Downloads that succeed — every download
            // in the normal case — now allocate no path at all.
            let written = temporary.file_mut().write_all(&buffer[..count]);
            written.map_err(|source| ModelManagerError::Io {
                operation: "write model download",
                path: temporary.path.clone(),
                source,
            })?;
            hasher.update(&buffer[..count]);
            total = observed;
        }

        if total != spec.exact_bytes() {
            return Err(integrity_error(
                reported_path,
                ModelIntegrityError::WrongSize {
                    expected: spec.exact_bytes(),
                    actual: total,
                },
            ));
        }

        let actual_sha256 = digest_to_hex(&hasher.finalize());
        if actual_sha256 != spec.sha256() {
            return Err(integrity_error(
                reported_path,
                ModelIntegrityError::ChecksumMismatch {
                    expected: spec.sha256().to_owned(),
                    actual: actual_sha256,
                },
            ));
        }

        check_cancelled(cancelled)?;
        let flushed = temporary.file_mut().flush();
        flushed.map_err(|source| ModelManagerError::Io {
            operation: "flush model download",
            path: temporary.path.clone(),
            source,
        })?;
        check_cancelled(cancelled)?;
        let synced = temporary.file_mut().sync_all();
        synced.map_err(|source| ModelManagerError::Io {
            operation: "sync model download",
            path: temporary.path.clone(),
            source,
        })?;
        check_cancelled(cancelled)?;

        temporary.verify_current()?;
        let target_exists = ensure_safe_replace_target(&snapshot, spec)?;
        snapshot.verify_mapping()?;
        temporary.commit(spec.filename(), target_exists, begin_commit)?;
        sync_containing_directory(&snapshot, &physical_path)?;
        snapshot.verify_mapping()?;
        Ok(reported_path)
    }
}
