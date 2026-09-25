//! Replacing the config file without ever leaving it unreadable.
//!
//! A save is a transaction, not a write: the new contents go to a private
//! temporary beside the destination, are flushed and synced, and only then
//! replace the file. A crash, a full disk, or a permission fault leaves either
//! the old file or the new one, never a truncated mixture of both, because the
//! config is the only record of the operator's keys and cache locations.
//!
//! The filesystem calls go through [`PersistenceFs`] so the failure paths
//! themselves are testable: a rename that fails on the third call is a test,
//! not a story.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const UNIQUE_PATH_ATTEMPTS: usize = 128;
pub(crate) static UNIQUE_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) trait PersistenceFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata>;
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
}

pub(crate) fn persist_serialized(destination: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = destination_parent(destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder
            .create(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        let _ =
            std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }

    let permissions = match std::fs::metadata(destination) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(format!(
                "could not read permissions for {}: {e}",
                destination.display()
            ));
        }
    };

    let temporary = write_synced_temp(destination, contents, permissions)?;
    install_temp_with_ops(destination, &temporary, &StdPersistenceFs)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub(crate) fn write_synced_temp(
    destination: &Path,
    contents: &[u8],
    permissions: Option<std::fs::Permissions>,
) -> Result<PathBuf, String> {
    let (temporary, mut file) = create_unique_temp(destination)?;

    if let Err(e) = file.write_all(contents) {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not write temporary configuration for {}: {e}",
                destination.display()
            ),
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        let _ = permissions;
    }
    #[cfg(not(unix))]
    if let Some(permissions) = permissions
        && let Err(e) = file.set_permissions(permissions)
    {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not preserve permissions for {}: {e}",
                destination.display()
            ),
        ));
    }

    if let Err(e) = file.sync_all() {
        drop(file);
        return Err(cleanup_temp_after_error(
            &StdPersistenceFs,
            &temporary,
            format!(
                "could not sync temporary configuration for {}: {e}",
                destination.display()
            ),
        ));
    }
    drop(file);

    Ok(temporary)
}

pub(crate) fn create_unique_temp(destination: &Path) -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..UNIQUE_PATH_ATTEMPTS {
        let candidate = unique_sibling_path(destination, "tmp")?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "could not create a temporary configuration beside {}: {e}",
                    destination.display()
                ));
            }
        }
    }

    Err(format!(
        "could not allocate a unique temporary configuration beside {}",
        destination.display()
    ))
}

pub(crate) fn install_temp_with_ops(
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
                    "could not install configuration at {} with an atomic rename: {atomic_error}; \
                     the destination was absent, so no existing configuration required recovery",
                    destination.display()
                ),
            ));
        }
        Err(e) => {
            return Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install configuration at {} with an atomic rename: {atomic_error}; \
                     could not inspect the existing destination for recovery: {e}",
                    destination.display()
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
                    "could not replace configuration at {} after atomic rename failed \
                     ({atomic_error}): {e}; the existing configuration was left in place",
                    destination.display()
                ),
            ));
        }
    };

    if let Err(backup_error) = fs.rename(destination, &backup) {
        return Err(cleanup_temp_after_error(
            fs,
            temporary,
            format!(
                "could not replace configuration at {}: atomic rename failed ({atomic_error}); \
                 moving the existing configuration to a backup also failed: {backup_error}; \
                 the existing configuration was left in place",
                destination.display()
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
                    "could not install the new configuration at {} after backing up the existing \
                     file: {install_error}; the original configuration was restored",
                    destination.display()
                ),
            )),
            Err(rollback_error) => Err(cleanup_temp_after_error(
                fs,
                temporary,
                format!(
                    "could not install the new configuration at {} after backing up the existing \
                     file: {install_error}; rollback failed: {rollback_error}; the destination may \
                     be missing, and the original configuration backup was retained at {}",
                    destination.display(),
                    backup.display()
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
                    destination.display()
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
    let file_name = destination.file_name().ok_or_else(|| {
        format!(
            "configuration path {} has no file name",
            destination.display()
        )
    })?;
    let nonce = UNIQUE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".aria-{role}-{}-{nonce}", std::process::id()));
    Ok(parent.join(name))
}

pub(crate) fn destination_parent(destination: &Path) -> Result<&Path, String> {
    let parent = destination.parent().ok_or_else(|| {
        format!(
            "configuration path {} has no parent directory",
            destination.display()
        )
    })?;
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
            temporary.display()
        ),
    }
}
