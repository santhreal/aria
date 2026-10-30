//! Measure, clear, and relocate cache directories.

use std::ffi::OsString;
use std::fs::{self, FileType, Metadata};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{ClearReport, DiskUsage, RelocationReport, StorageError, bounded_text};

/// Cooperative cancellation check used by filesystem operations.
pub trait Cancellation {
    /// Return `true` when the current operation should stop.
    fn is_cancelled(&self) -> bool;
}

impl<F> Cancellation for F
where
    F: Fn() -> bool,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

/// A cancellation check that never cancels.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeverCancelled;

impl Cancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Measure a cache tree without following directory symlinks.
///
/// A missing root has zero usage. A symlink root is rejected. Cancellation is
/// checked before each directory and entry; a single large-file metadata call
/// is not interruptible.
pub fn measure_disk_usage<C>(
    root: impl AsRef<Path>,
    cancellation: &C,
) -> Result<DiskUsage, StorageError>
where
    C: Cancellation + ?Sized,
{
    let root = normalize_absolute(root.as_ref(), "measurement root", false)?;
    check_cancelled(cancellation)?;

    let Some(metadata) = optional_symlink_metadata(&root, "inspect measurement root")? else {
        return Ok(DiskUsage::default());
    };
    reject_root_type(&metadata)?;

    let mut usage = DiskUsage::default();
    let mut stack = vec![(root, 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        if depth > 64 {
            return Err(StorageError::InvalidPath(
                "directory depth exceeds maximum limit of 64",
            ));
        }
        check_cancelled(cancellation)?;
        let entries =
            fs::read_dir(&directory).map_err(|error| io_error("read cache directory", error))?;
        for entry in entries {
            check_cancelled(cancellation)?;
            let entry = entry.map_err(|error| io_error("read cache entry", error))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| io_error("inspect cache entry", error))?;
            if metadata.file_type().is_dir() {
                if depth + 1 > 64 {
                    return Err(StorageError::InvalidPath(
                        "directory depth exceeds maximum limit of 64",
                    ));
                }
                stack.push((path, depth + 1));
            } else {
                add_usage(&mut usage, &metadata)?;
            }
        }
    }
    Ok(usage)
}

/// Remove a cache's contents while preserving its root directory.
///
/// A missing root is created as an empty directory. Empty paths, filesystem
/// roots, symlink roots, and non-directories are rejected. Nested symlinks are
/// removed as links and never followed. Cancellation can leave a partially
/// cleared tree, but the root itself is never intentionally removed.
pub fn clear_cache<C>(root: impl AsRef<Path>, cancellation: &C) -> Result<ClearReport, StorageError>
where
    C: Cancellation + ?Sized,
{
    let root = normalize_absolute(root.as_ref(), "clear root", false)?;
    check_cancelled(cancellation)?;

    let Some(metadata) = optional_symlink_metadata(&root, "inspect clear root")? else {
        fs::create_dir_all(&root).map_err(|error| io_error("create cache root", error))?;
        let created = fs::symlink_metadata(&root)
            .map_err(|error| io_error("inspect created cache root", error))?;
        reject_root_type(&created)?;
        return Ok(ClearReport::default());
    };
    reject_root_type(&metadata)?;

    let usage = remove_tree(&root, true, cancellation)?;
    fs::create_dir_all(&root).map_err(|error| io_error("recreate cache root", error))?;
    let recreated = fs::symlink_metadata(&root)
        .map_err(|error| io_error("inspect recreated cache root", error))?;
    reject_root_type(&recreated)?;
    Ok(ClearReport {
        removed_bytes: usage.bytes,
        removed_files: usage.files,
    })
}

/// Relocate a cache directory and persist the completed destination.
///
/// The destination must be absolute, absent, non-root, and disjoint from the
/// source. The source must be an absolute, non-symlink directory. The function
/// measures first, then prefers an atomic rename. If rename reports a
/// cross-device move, it copies into a unique temporary sibling and atomically
/// renames that sibling into place.
///
/// `persist` runs only after the destination is complete. On callback failure
/// or panic, rollback is attempted. Copy mode leaves the source untouched
/// until persistence succeeds; after that commit point cleanup runs to
/// completion even if cancellation is requested. Callers receive an explicit
/// error if rollback or post-commit source cleanup fails.
pub fn relocate_cache<C, F>(
    old_path: impl AsRef<Path>,
    new_path: impl AsRef<Path>,
    cancellation: &C,
    persist: F,
) -> Result<RelocationReport, StorageError>
where
    C: Cancellation + ?Sized,
    F: FnOnce(&Path) -> Result<(), String>,
{
    relocate_with_strategy(
        old_path.as_ref(),
        new_path.as_ref(),
        cancellation,
        persist,
        RelocationStrategy::Auto,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelocationStrategy {
    Auto,
    #[cfg(test)]
    ForceCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferKind {
    Renamed,
    Copied,
}

pub(crate) fn relocate_with_strategy<C, F>(
    old_path: &Path,
    new_path: &Path,
    cancellation: &C,
    persist: F,
    strategy: RelocationStrategy,
) -> Result<RelocationReport, StorageError>
where
    C: Cancellation + ?Sized,
    F: FnOnce(&Path) -> Result<(), String>,
{
    check_cancelled(cancellation)?;
    let (old_path, new_path) = prepare_relocation(old_path, new_path)?;
    check_cancelled(cancellation)?;
    let usage = measure_disk_usage(&old_path, cancellation)?;
    check_cancelled(cancellation)?;

    let transfer = match strategy {
        RelocationStrategy::Auto => {
            ensure_destination_absent(&new_path)?;
            match fs::rename(&old_path, &new_path) {
                Ok(()) => TransferKind::Renamed,
                Err(error) if is_cross_device(&error) => {
                    copy_into_destination(&old_path, &new_path, cancellation)?;
                    TransferKind::Copied
                }
                Err(error) => return Err(io_error("rename cache directory", error)),
            }
        }
        #[cfg(test)]
        RelocationStrategy::ForceCopy => {
            copy_into_destination(&old_path, &new_path, cancellation)?;
            TransferKind::Copied
        }
    };

    let persistence_failure = match catch_unwind(AssertUnwindSafe(|| persist(&new_path))) {
        Ok(Ok(())) => None,
        Ok(Err(message)) => Some(bounded_text(message)),
        Err(_) => Some(String::from("persistence callback panicked")),
    };

    if let Some(persistence) = persistence_failure {
        let rollback = match transfer {
            TransferKind::Renamed => rollback_rename(&new_path, &old_path),
            TransferKind::Copied => remove_path_if_exists(&new_path),
        };
        return match rollback {
            Ok(()) => Err(StorageError::PersistenceFailed {
                message: persistence,
            }),
            Err(error) => Err(StorageError::RollbackFailed {
                persistence,
                rollback: bounded_text(error.to_string()),
            }),
        };
    }

    let report = RelocationReport {
        bytes: usage.bytes,
        files: usage.files,
        used_copy_fallback: transfer == TransferKind::Copied,
    };
    if transfer == TransferKind::Copied {
        remove_path_if_exists(&old_path).map_err(|error| StorageError::CommittedCleanupFailed {
            message: bounded_text(error.to_string()),
            report,
        })?;
    }

    Ok(report)
}

fn prepare_relocation(
    old_path: &Path,
    new_path: &Path,
) -> Result<(PathBuf, PathBuf), StorageError> {
    let old_path = normalize_absolute(old_path, "relocation source", false)?;
    let new_path = normalize_absolute(new_path, "relocation destination", false)?;
    if paths_overlap(&old_path, &new_path) {
        return Err(StorageError::PathsOverlap);
    }

    let source = optional_symlink_metadata(&old_path, "inspect relocation source")?
        .ok_or(StorageError::SourceMissing)?;
    reject_root_type(&source)?;
    ensure_destination_absent(&new_path)?;

    let canonical_old = fs::canonicalize(&old_path)
        .map_err(|error| io_error("canonicalize relocation source", error))?;
    let canonical_new = canonicalize_candidate(&new_path)?;
    if paths_overlap(&canonical_old, &canonical_new) {
        return Err(StorageError::PathsOverlap);
    }

    let parent = new_path
        .parent()
        .ok_or(StorageError::DangerousPath("destination has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create relocation parent directory", error))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| io_error("inspect relocation parent directory", error))?;
    if !parent_metadata.file_type().is_dir() {
        return Err(StorageError::NotDirectory);
    }

    let canonical_new = canonicalize_candidate(&new_path)?;
    if paths_overlap(&canonical_old, &canonical_new) {
        return Err(StorageError::PathsOverlap);
    }
    ensure_destination_absent(&new_path)?;
    Ok((old_path, new_path))
}

fn copy_into_destination<C>(
    old_path: &Path,
    new_path: &Path,
    cancellation: &C,
) -> Result<(), StorageError>
where
    C: Cancellation + ?Sized,
{
    check_cancelled(cancellation)?;
    ensure_destination_absent(new_path)?;
    let parent = new_path
        .parent()
        .ok_or(StorageError::DangerousPath("destination has no parent"))?;
    let temporary = create_temporary_sibling(parent)?;

    let copy_result = copy_tree_contents(old_path, &temporary, cancellation)
        .and_then(|()| check_cancelled(cancellation))
        .and_then(|()| ensure_destination_absent(new_path))
        .and_then(|()| {
            fs::rename(&temporary, new_path)
                .map_err(|error| io_error("install copied cache directory", error))
        });

    match copy_result {
        Ok(()) => Ok(()),
        Err(original) => match remove_path_if_exists(&temporary) {
            Ok(()) => Err(original),
            Err(cleanup) => Err(StorageError::TemporaryCleanupFailed {
                original: bounded_text(original.to_string()),
                cleanup: bounded_text(cleanup.to_string()),
            }),
        },
    }
}

fn copy_tree_contents<C>(
    source_root: &Path,
    destination_root: &Path,
    cancellation: &C,
) -> Result<(), StorageError>
where
    C: Cancellation + ?Sized,
{
    let mut stack = vec![(
        source_root.to_path_buf(),
        destination_root.to_path_buf(),
        0usize,
    )];
    while let Some((source, destination, depth)) = stack.pop() {
        if depth > 64 {
            return Err(StorageError::InvalidPath(
                "directory depth exceeds maximum limit of 64",
            ));
        }
        check_cancelled(cancellation)?;
        let source_metadata = fs::symlink_metadata(&source)
            .map_err(|error| io_error("inspect copy source directory", error))?;
        if !source_metadata.file_type().is_dir() {
            return Err(StorageError::NotDirectory);
        }

        let entries = fs::read_dir(&source).map_err(|error| io_error("read copy source", error))?;
        for entry in entries {
            check_cancelled(cancellation)?;
            let entry = entry.map_err(|error| io_error("read copy entry", error))?;
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path)
                .map_err(|error| io_error("inspect copy entry", error))?;
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                if depth + 1 > 64 {
                    return Err(StorageError::InvalidPath(
                        "directory depth exceeds maximum limit of 64",
                    ));
                }
                fs::create_dir(&destination_path)
                    .map_err(|error| io_error("create copied directory", error))?;
                stack.push((source_path, destination_path, depth + 1));
            } else if file_type.is_file() {
                fs::copy(&source_path, &destination_path)
                    .map_err(|error| io_error("copy cache file", error))?;
            } else if file_type.is_symlink() {
                copy_symlink(&source_path, &destination_path, &file_type)?;
            } else {
                return Err(StorageError::UnsupportedFileType);
            }
        }
    }
    Ok(())
}

fn rollback_rename(destination: &Path, source: &Path) -> Result<(), StorageError> {
    if optional_symlink_metadata(source, "inspect rollback source")?.is_some() {
        return Err(StorageError::DestinationExists);
    }
    let destination_metadata =
        optional_symlink_metadata(destination, "inspect rollback destination")?
            .ok_or(StorageError::SourceMissing)?;
    reject_root_type(&destination_metadata)?;
    fs::rename(destination, source).map_err(|error| io_error("roll back cache rename", error))
}

fn remove_path_if_exists(path: &Path) -> Result<(), StorageError> {
    let Some(metadata) = optional_symlink_metadata(path, "inspect cleanup root")? else {
        return Ok(());
    };
    if metadata.file_type().is_dir() {
        remove_tree(path, false, &NeverCancelled)?;
    } else {
        remove_non_directory(path, &metadata.file_type())
            .map_err(|error| io_error("remove cleanup entry", error))?;
    }
    Ok(())
}

fn remove_tree<C>(
    root: &Path,
    preserve_root: bool,
    cancellation: &C,
) -> Result<DiskUsage, StorageError>
where
    C: Cancellation + ?Sized,
{
    enum Step {
        Visit(PathBuf, usize),
        RemoveDirectory(PathBuf),
    }

    let mut usage = DiskUsage::default();
    let mut stack = vec![Step::Visit(root.to_path_buf(), 0usize)];
    while let Some(step) = stack.pop() {
        check_cancelled(cancellation)?;
        match step {
            Step::Visit(path, depth) => {
                if depth > 64 {
                    return Err(StorageError::InvalidPath(
                        "directory depth exceeds maximum limit of 64",
                    ));
                }
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|error| io_error("inspect removal entry", error))?;
                if metadata.file_type().is_dir() {
                    let entries = fs::read_dir(&path)
                        .map_err(|error| io_error("read removal directory", error))?;
                    stack.push(Step::RemoveDirectory(path));
                    for entry in entries {
                        check_cancelled(cancellation)?;
                        let entry = entry.map_err(|error| io_error("read removal entry", error))?;
                        if depth + 1 > 64 {
                            return Err(StorageError::InvalidPath(
                                "directory depth exceeds maximum limit of 64",
                            ));
                        }
                        stack.push(Step::Visit(entry.path(), depth + 1));
                    }
                } else {
                    add_usage(&mut usage, &metadata)?;
                    remove_non_directory(&path, &metadata.file_type())
                        .map_err(|error| io_error("remove cache entry", error))?;
                }
            }
            Step::RemoveDirectory(path) => {
                if preserve_root && path == root {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|error| io_error("inspect removal directory", error))?;
                if metadata.file_type().is_dir() {
                    fs::remove_dir(&path)
                        .map_err(|error| io_error("remove cache directory", error))?;
                } else {
                    add_usage(&mut usage, &metadata)?;
                    remove_non_directory(&path, &metadata.file_type())
                        .map_err(|error| io_error("remove replaced cache entry", error))?;
                }
            }
        }
    }
    Ok(usage)
}

fn add_usage(usage: &mut DiskUsage, metadata: &Metadata) -> Result<(), StorageError> {
    usage.bytes = usage
        .bytes
        .checked_add(metadata.len())
        .ok_or(StorageError::AccountingOverflow)?;
    usage.files = usage
        .files
        .checked_add(1)
        .ok_or(StorageError::AccountingOverflow)?;
    Ok(())
}

fn check_cancelled<C>(cancellation: &C) -> Result<(), StorageError>
where
    C: Cancellation + ?Sized,
{
    if cancellation.is_cancelled() {
        Err(StorageError::Cancelled)
    } else {
        Ok(())
    }
}

fn reject_root_type(metadata: &Metadata) -> Result<(), StorageError> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        Err(StorageError::SymlinkRoot)
    } else if !file_type.is_dir() {
        Err(StorageError::NotDirectory)
    } else {
        Ok(())
    }
}

fn optional_symlink_metadata(
    path: &Path,
    operation: &'static str,
) -> Result<Option<Metadata>, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(operation, error)),
    }
}

fn ensure_destination_absent(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::SymlinkRoot),
        Ok(_) => Err(StorageError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect relocation destination", error)),
    }
}
// Path primitives and platform-specific file operations are defined in
// `ops_paths.rs` and re-exported here as needed.
#[path = "ops_paths.rs"]
mod paths;

use paths::{
    canonicalize_candidate, copy_symlink, create_temporary_sibling, io_error, is_cross_device,
    remove_non_directory,
};
pub(crate) use paths::{normalize_absolute, paths_overlap, resolve_physical_cache_root};
