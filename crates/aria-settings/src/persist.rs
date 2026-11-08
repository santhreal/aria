//! Replacing a file without ever leaving it unreadable.
//!
//! A save is a transaction, not a write: the new contents go to a temporary
//! beside the destination, are flushed and synced, and only then replace the
//! file. A crash, a full disk, or a permission fault leaves either the old file
//! or the new one, never a truncated mixture of both. The config file is the
//! only record of the API keys and cache locations; the editor's own state
//! files (window layout, keymaps, presets, recents) go through
//! [`replace_file`] for the same reason.
//!
//! The filesystem calls go through [`PersistenceFs`] so the failure paths
//! themselves are testable: a rename that fails on the third call is a test,
//! not a story.
//!
//! The rename that installs the file is recorded in its folder, which the OS
//! writes back on its own schedule, so the folder is synced after every
//! install. A failed folder sync is [`SaveError::Unsynced`]: the new file is
//! already in place and is not rolled back.

use std::ffi::OsString;
use std::fmt;
use std::fs::Permissions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use aria_core::io_reason;

pub(crate) const UNIQUE_PATH_ATTEMPTS: usize = 128;
pub(crate) static UNIQUE_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Who may read a replaced file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// Owner only (0600 on unix), whatever the replaced file's mode was.
    Private,
    /// The replaced file's permissions, or the umask default for a new file.
    Shared,
}

/// Why a save did not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// Nothing was installed: the destination holds what it held before.
    Failed(String),
    /// The new file is installed and reads back, but its folder could not be
    /// written to disk, so a crash before the OS writes it back may restore the
    /// previous file. The save is in force and is not rolled back.
    Unsynced(String),
}

impl SaveError {
    /// Whether the new file is in place despite the error.
    pub fn is_committed(&self) -> bool {
        matches!(self, Self::Unsynced(_))
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(reason) | Self::Unsynced(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<SaveError> for String {
    fn from(error: SaveError) -> Self {
        match error {
            SaveError::Failed(reason) | SaveError::Unsynced(reason) => reason,
        }
    }
}

pub(crate) trait PersistenceFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata>;
    /// Write the folder `path` to disk, so a rename recorded in it survives a
    /// crash.
    fn sync_dir(&self, path: &Path) -> io::Result<()>;
}

pub(crate) struct StdPersistenceFs;

impl PersistenceFs for StdPersistenceFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        std::fs::symlink_metadata(path)
    }

    #[cfg(unix)]
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }

    /// `std` opens no folder handle on Windows, and NTFS journals the rename
    /// that installs the file with the file's own metadata.
    #[cfg(not(unix))]
    fn sync_dir(&self, _: &Path) -> io::Result<()> {
        Ok(())
    }
}

/// Replace `destination` with `contents`, creating its directory when
/// missing. A crash or a failed write leaves the old file or the new one,
/// never a truncated file. The file keeps its permissions; a new file takes
/// the umask default. A symbolic link at `destination` stays a link: the file
/// it resolves to is replaced. A failed folder sync after the install is
/// reported, with the new file in place.
pub fn replace_file(destination: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = destination_parent(destination)?;
    std::fs::create_dir_all(parent).map_err(|e| {
        format!(
            "could not create {}: {e}",
            parent.display(),
            e = io_reason(&e)
        )
    })?;
    let destination = &link_target(destination)?;
    let permissions = existing_permissions(destination)?;
    let temporary = write_synced_temp(destination, contents, permissions, Access::Shared)?;
    install_temp_with_ops(destination, &temporary, &StdPersistenceFs).map_err(String::from)
}

/// Replace `destination` with `contents` readable by the owner only, in a
/// folder readable by the owner only.
pub(crate) fn persist_serialized(destination: &Path, contents: &[u8]) -> Result<(), SaveError> {
    let (destination, temporary) =
        private_temp(destination, contents).map_err(SaveError::Failed)?;
    install_temp_with_ops(&destination, &temporary, &StdPersistenceFs)
}

/// Create the private folder for `destination` and the synced temporary that
/// replaces the file it resolves to: (that file, the temporary).
fn private_temp(destination: &Path, contents: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let parent = destination_parent(destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder.create(parent).map_err(|e| {
            format!(
                "could not create {}: {e}",
                parent.display(),
                e = io_reason(&e)
            )
        })?;
        let _ =
            std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "could not create {}: {e}",
                parent.display(),
                e = io_reason(&e)
            )
        })?;
    }

    let destination = link_target(destination)?;
    let permissions = existing_permissions(&destination)?;
    let temporary = write_synced_temp(&destination, contents, permissions, Access::Private)?;
    Ok((destination, temporary))
}

/// Most symbolic links followed to a file's target, as `SYMLOOP_MAX` bounds
/// path resolution on Linux.
const MAX_LINK_HOPS: usize = 40;

/// The file a write to `destination` lands in: `destination` itself, or the
/// end of its chain of symbolic links. A configuration file linked from a
/// dotfiles checkout is replaced in the checkout, so the link survives the
/// save. The end of a dangling chain is a file the save creates.
fn link_target(destination: &Path) -> Result<PathBuf, String> {
    let mut current = destination.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = std::fs::read_link(&current).map_err(|e| {
                    format!(
                        "could not read the link {}: {e}",
                        current.display(),
                        e = io_reason(&e)
                    )
                })?;
                current = destination_parent(&current)?.join(link);
            }
            Ok(_) => return Ok(current),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(e) => {
                return Err(format!(
                    "could not inspect {}: {e}",
                    current.display(),
                    e = io_reason(&e)
                ));
            }
        }
    }
    Err(format!(
        "{} is a chain of more than {MAX_LINK_HOPS} symbolic links; point it at a file",
        destination.display()
    ))
}

fn existing_permissions(destination: &Path) -> Result<Option<Permissions>, String> {
    match std::fs::metadata(destination) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "could not read permissions for {}: {e}",
            destination.display(),
            e = io_reason(&e)
        )),
    }
}

pub(crate) fn write_synced_temp(
    destination: &Path,
    contents: &[u8],
    permissions: Option<Permissions>,
    access: Access,
) -> Result<PathBuf, String> {
    let (temporary, mut file) = create_unique_temp(destination, access)?;

    if let Err(e) = file.write_all(contents) {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not write a temporary file for {}: {e}",
                destination.display(),
                e = io_reason(&e)
            ),
        ));
    }

    #[cfg(unix)]
    let permissions = match access {
        Access::Private => Some(std::os::unix::fs::PermissionsExt::from_mode(0o600)),
        Access::Shared => permissions,
    };
    if let Some(permissions) = permissions
        && let Err(e) = file.set_permissions(permissions)
    {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not preserve permissions for {}: {e}",
                destination.display(),
                e = io_reason(&e)
            ),
        ));
    }

    if let Err(e) = file.sync_all() {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not sync a temporary file for {}: {e}",
                destination.display(),
                e = io_reason(&e)
            ),
        ));
    }
    drop(file);

    Ok(temporary)
}

pub(crate) fn create_unique_temp(
    destination: &Path,
    access: Access,
) -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..UNIQUE_PATH_ATTEMPTS {
        let candidate = unique_sibling_path(destination, "tmp")?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(match access {
                Access::Private => 0o600,
                Access::Shared => 0o666,
            });
        }
        #[cfg(not(unix))]
        let _ = access;
        match options.open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "could not create a temporary file beside {}: {e}",
                    destination.display(),
                    e = io_reason(&e)
                ));
            }
        }
    }

    Err(format!(
        "could not allocate a unique temporary file beside {}",
        destination.display()
    ))
}

/// Install `temporary` as `destination`, then sync the folder holding it.
pub(crate) fn install_temp_with_ops(
    destination: &Path,
    temporary: &Path,
    fs: &impl PersistenceFs,
) -> Result<(), SaveError> {
    let folder = destination_parent(destination)
        .map_err(|e| SaveError::Failed(cleanup_temp_after_error(fs, temporary, e)))?;
    install_temp(destination, temporary, fs).map_err(SaveError::Failed)?;
    fs.sync_dir(folder).map_err(|e| {
        SaveError::Unsynced(format!(
            "{} was saved, but its folder could not be written to disk: {e}; a crash \
             before the system writes it may bring back the previous version",
            destination.display(),
            e = io_reason(&e)
        ))
    })
}

fn install_temp(
    destination: &Path,
    temporary: &Path,
    fs: &impl PersistenceFs,
) -> Result<(), String> {
    let atomic_error = match fs.rename(temporary, destination) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    match fs.symlink_metadata(destination) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install {} with an atomic rename: {atomic_error}; \
                     the destination was absent, so no existing file required recovery",
                    destination.display(),
                    atomic_error = io_reason(&atomic_error)
                ),
            ));
        }
        Err(e) => {
            return Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install {} with an atomic rename: {atomic_error}; \
                     could not inspect the existing destination for recovery: {e}",
                    destination.display(),
                    atomic_error = io_reason(&atomic_error),
                    e = io_reason(&e)
                ),
            ));
        }
    }

    let backup = match vacant_backup_path(destination, fs) {
        Ok(path) => path,
        Err(e) => {
            return Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not replace {} after atomic rename failed \
                     ({atomic_error}): {e}; the existing file was left in place",
                    destination.display(),
                    atomic_error = io_reason(&atomic_error)
                ),
            ));
        }
    };

    if let Err(backup_error) = fs.rename(destination, &backup) {
        return Err(cleanup_temp_after_error(
            fs,
            temporary,
            format!(
                "could not replace {}: atomic rename failed ({atomic_error}); \
                 moving the existing file to a backup also failed: {backup_error}; \
                 the existing file was left in place",
                destination.display(),
                atomic_error = io_reason(&atomic_error),
                backup_error = io_reason(&backup_error)
            ),
        ));
    }

    match fs.rename(temporary, destination) {
        Ok(()) => {
            // Installation is the commit point. A stale backup is preferable
            // to reporting failure after callers have already persisted the
            // new state; relocation transactions use `save` as their commit
            // callback and would otherwise roll data back out from under the
            // newly installed configuration.
            let _ = fs.remove_file(&backup);
            Ok(())
        }
        Err(install_error) => match fs.rename(&backup, destination) {
            Ok(()) => Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install the new {} after backing up the existing \
                     file: {install_error}; the original file was restored",
                    destination.display(),
                    install_error = io_reason(&install_error)
                ),
            )),
            Err(rollback_error) => Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install the new {} after backing up the existing \
                     file: {install_error}; rollback failed: {rollback_error}; the destination may \
                     be missing, and the original file's backup was retained at {}",
                    destination.display(),
                    backup.display(),
                    install_error = io_reason(&install_error),
                    rollback_error = io_reason(&rollback_error)
                ),
            )),
        },
    }
}

pub(crate) fn vacant_backup_path(
    destination: &Path,
    fs: &impl PersistenceFs,
) -> Result<PathBuf, String> {
    for _ in 0..UNIQUE_PATH_ATTEMPTS {
        let candidate = unique_sibling_path(destination, "backup")?;
        match fs.symlink_metadata(&candidate) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(e) => {
                return Err(format!(
                    "could not inspect a backup path beside {}: {e}",
                    destination.display(),
                    e = io_reason(&e)
                ));
            }
        }
    }

    Err(format!(
        "could not allocate a unique backup beside {}",
        destination.display()
    ))
}

pub(crate) fn unique_sibling_path(destination: &Path, role: &str) -> Result<PathBuf, String> {
    let parent = destination_parent(destination)?;
    let file_name = destination
        .file_name()
        .ok_or_else(|| format!("path {} has no file name", destination.display()))?;
    let nonce = UNIQUE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".aria-{role}-{}-{nonce}", std::process::id()));
    Ok(parent.join(name))
}

pub(crate) fn destination_parent(destination: &Path) -> Result<&Path, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("path {} has no parent directory", destination.display()))?;
    if parent.as_os_str().is_empty() {
        Ok(Path::new("."))
    } else {
        Ok(parent)
    }
}

pub(crate) fn cleanup_temp_after_error(
    fs: &impl PersistenceFs,
    temporary: &Path,
    primary_error: String,
) -> String {
    match fs.remove_file(temporary) {
        Ok(()) => primary_error,
        Err(e) if e.kind() == io::ErrorKind::NotFound => primary_error,
        Err(e) => format!(
            "{primary_error}; temporary-file cleanup also failed for {}: {e}; \
             the temporary file may remain",
            temporary.display(),
            e = io_reason(&e)
        ),
    }
}
