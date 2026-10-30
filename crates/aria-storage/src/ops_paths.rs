//! Path primitives and the platform edges of a relocation.
//!
//! Path normalization, overlap checks, physical-root resolution, and
//! per-platform symlink and non-directory removal. Tree traversal and
//! cancellation reporting are in `ops.rs`.

use super::*;

pub(crate) fn normalize_absolute(
    path: &Path,
    role: &'static str,
    allow_filesystem_root: bool,
) -> Result<PathBuf, StorageError> {
    if path.as_os_str().is_empty() {
        return Err(StorageError::DangerousPath("path is empty"));
    }
    if !path.is_absolute() {
        return Err(StorageError::PathNotAbsolute(role));
    }

    let mut prefix: Option<OsString> = None;
    let mut has_root = false;
    let mut parts: Vec<OsString> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value.as_os_str().to_os_string()),
            Component::RootDir => has_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(value) => parts.push(value.to_os_string()),
        }
    }

    if !allow_filesystem_root && parts.is_empty() {
        return Err(StorageError::DangerousPath(
            "filesystem root is not allowed",
        ));
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if has_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in parts {
        normalized.push(part);
    }
    Ok(normalized)
}

pub(crate) fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

pub(crate) fn resolve_physical_cache_root(path: &Path) -> Result<PathBuf, StorageError> {
    let mut cursor = normalize_absolute(path, "resolved cache root", false)?;
    let mut missing_components = Vec::new();

    loop {
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                let file_type = metadata.file_type();
                if missing_components.is_empty() {
                    reject_root_type(&metadata)?;
                } else if !file_type.is_dir() && !file_type.is_symlink() {
                    return Err(StorageError::NotDirectory);
                }

                let mut physical = fs::canonicalize(&cursor)
                    .map_err(|error| io_error("canonicalize cache layout ancestor", error))?;
                let ancestor_metadata = fs::metadata(&physical)
                    .map_err(|error| io_error("inspect canonical cache ancestor", error))?;
                if !ancestor_metadata.is_dir() {
                    return Err(StorageError::NotDirectory);
                }

                for component in missing_components.iter().rev() {
                    physical.push(component);
                }
                return normalize_absolute(&physical, "physical cache root", false);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let component = match cursor.components().next_back() {
                    Some(Component::Normal(component)) => component.to_os_string(),
                    _ => {
                        return Err(StorageError::DangerousPath(
                            "cache root has no existing ancestor",
                        ));
                    }
                };
                let parent = cursor.parent().ok_or(StorageError::DangerousPath(
                    "cache root has no existing ancestor",
                ))?;
                missing_components.push(component);
                cursor = parent.to_path_buf();
            }
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                return Err(StorageError::NotDirectory);
            }
            Err(error) => return Err(io_error("inspect cache layout path", error)),
        }
    }
}

pub(crate) fn canonicalize_candidate(path: &Path) -> Result<PathBuf, StorageError> {
    let mut cursor = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(&cursor) {
            Ok(mut canonical) => {
                for part in suffix.iter().rev() {
                    canonical.push(part);
                }
                return normalize_absolute(&canonical, "canonical path", true);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let file_name = cursor
                    .file_name()
                    .ok_or(StorageError::DangerousPath("path has no existing ancestor"))?;
                suffix.push(file_name.to_os_string());
                cursor = cursor
                    .parent()
                    .ok_or(StorageError::DangerousPath("path has no existing ancestor"))?
                    .to_path_buf();
            }
            Err(error) => return Err(io_error("canonicalize relocation destination", error)),
        }
    }
}

pub(super) fn create_temporary_sibling(parent: &Path) -> Result<PathBuf, StorageError> {
    static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);
    const MAX_ATTEMPTS: usize = 128;

    for _ in 0..MAX_ATTEMPTS {
        let id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!(".aria-storage-relocation-{}-{id}.tmp", std::process::id());
        let candidate = parent.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create temporary relocation directory", error)),
        }
    }
    Err(StorageError::TemporaryNameExhausted)
}

pub(super) fn is_cross_device(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
}

pub(super) fn io_error(operation: &'static str, source: io::Error) -> StorageError {
    StorageError::Io { operation, source }
}

#[cfg(unix)]
pub(super) fn copy_symlink(
    source: &Path,
    destination: &Path,
    _: &FileType,
) -> Result<(), StorageError> {
    let target =
        fs::read_link(source).map_err(|error| io_error("read cache symbolic link", error))?;
    std::os::unix::fs::symlink(target, destination)
        .map_err(|error| io_error("copy cache symbolic link", error))
}

#[cfg(windows)]
pub(super) fn copy_symlink(
    source: &Path,
    destination: &Path,
    file_type: &FileType,
) -> Result<(), StorageError> {
    use std::os::windows::fs::FileTypeExt;

    let target =
        fs::read_link(source).map_err(|error| io_error("read cache symbolic link", error))?;
    if file_type.is_symlink_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
            .map_err(|error| io_error("copy cache directory symbolic link", error))
    } else {
        std::os::windows::fs::symlink_file(target, destination)
            .map_err(|error| io_error("copy cache file symbolic link", error))
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn copy_symlink(_: &Path, _: &Path, _: &FileType) -> Result<(), StorageError> {
    Err(StorageError::UnsupportedFileType)
}

#[cfg(windows)]
pub(super) fn remove_non_directory(path: &Path, file_type: &FileType) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt;

    if file_type.is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(not(windows))]
pub(super) fn remove_non_directory(path: &Path, _: &FileType) -> io::Result<()> {
    fs::remove_file(path)
}
