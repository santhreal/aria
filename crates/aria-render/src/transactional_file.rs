//! Transactional publication for native file exports.
//!
//! Native encoders are allowed to remove or truncate the path they open. This
//! guard gives them a uniquely owned sibling directory and only moves the
//! completed regular file to the requested destination after it has been
//! finalized and synchronized.
//!
//! Each staging directory is a scratch path of [`aria_core::scratch`]: a
//! `<directory>.lock` file beside it, created before the directory, is locked
//! exclusively by the writer until the directory is gone. The operating
//! system releases the lock when the writer exits, however it exits, so every
//! new staging first deletes the staging directories in its folder whose lock
//! no live writer holds: what an export killed or crashed part way left,
//! unfinished file included.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use aria_core::scratch::{self, ScratchLock};

const STAGING_DIRECTORY_PREFIX: &str = ".aria-export-";
const MAX_ALLOCATION_ATTEMPTS: usize = 64;
const MAX_EXTENSION_UNITS: usize = 64;
#[cfg(test)]
const MAX_GENERATED_DIRECTORY_BYTES: usize = 80;

static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

/// A same-filesystem staging file that atomically replaces its destination
/// only when [`TransactionalFile::publish`] succeeds.
#[derive(Debug)]
pub(crate) struct TransactionalFile {
    destination: PathBuf,
    parent: PathBuf,
    staging_directory: PathBuf,
    staging_file: PathBuf,
    /// Held until the staging directory is gone; `None` after cleanup.
    claim: Option<ScratchLock>,
    cleanup_needed: bool,
}

impl TransactionalFile {
    /// Allocate a unique sibling staging directory and an empty file whose
    /// extension matches `destination`.
    pub(crate) fn new(destination: &Path) -> io::Result<Self> {
        let at = |error: io::Error| aria_models::path_error(destination, &error);
        validate_destination_path(destination).map_err(at)?;
        let destination = absolute_path(destination).map_err(at)?;
        validate_destination_shape(&destination)?;

        let parent = destination
            .parent()
            .ok_or_else(|| at(invalid_input("export destination has no parent directory")))?
            .to_owned();
        let staging_file_name = staging_file_name(&destination).map_err(at)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|error| error.duration())
            .as_nanos();

        scratch::sweep(&parent, is_staging_directory, |directory| {
            let _ = scratch::remove(directory);
        });
        for _ in 0..MAX_ALLOCATION_ATTEMPTS {
            let sequence = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
            let thread_hash = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::thread::current().id().hash(&mut hasher);
                hasher.finish()
            };
            let directory_name = format!(
                "{STAGING_DIRECTORY_PREFIX}{:08x}-{thread_hash:016x}-{timestamp:016x}-{sequence:016x}",
                std::process::id()
            );
            let staging_directory = parent.join(directory_name);
            // `None`: a sweep in another process is clearing this name.
            let claim = match ScratchLock::claim(&staging_directory) {
                Ok(Some(claim)) => claim,
                Ok(None) => continue,
                Err(error) => return Err(aria_models::unwritable_directory(&parent, &error)),
            };
            match fs::create_dir(&staging_directory) {
                Ok(()) => {}
                // A dead writer's leftover under the same name: the next
                // sweep deletes it once the claim's drop unlinks the lock.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(aria_models::unwritable_directory(&parent, &error)),
            }

            let staging_file = staging_directory.join(&staging_file_name);
            let guard = Self {
                destination: destination.clone(),
                parent: parent.clone(),
                staging_directory,
                staging_file,
                claim: Some(claim),
                cleanup_needed: true,
            };

            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&guard.staging_file)
            {
                Ok(file) => {
                    drop(file);
                    return Ok(guard);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    drop(guard);
                }
                Err(error) => {
                    drop(guard);
                    return Err(aria_models::unwritable_directory(&parent, &error));
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "could not allocate a unique export staging directory after {MAX_ALLOCATION_ATTEMPTS} attempts in {}",
                parent.display()
            ),
        ))
    }

    pub(crate) fn staging_path(&self) -> &Path {
        &self.staging_file
    }

    /// Synchronize and atomically move the staged regular file into place.
    ///
    /// Once the atomic replacement commits, later directory-sync or staging
    /// cleanup errors are warnings rather than ordinary failures: returning an
    /// error at that point would incorrectly tell callers that publication did
    /// not happen.
    pub(crate) fn publish(mut self) -> io::Result<()> {
        validate_destination_shape(&self.destination)?;
        let at = |error: io::Error| aria_models::path_error(&self.destination, &error);

        // Inspect the directory entry itself before opening it. In particular,
        // never follow a symlink left at the encoder's output path.
        let metadata = fs::symlink_metadata(&self.staging_file).map_err(at)?;
        if !metadata.file_type().is_file() {
            return Err(at(invalid_input(
                "export staging path is not a regular file after encoding",
            )));
        }

        // `sync_all` flushes file buffers, which needs write access on Windows
        // (a read-only handle denies it); read-only would be enough on unix.
        let staged_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.staging_file)
            .map_err(at)?;
        if !staged_file.metadata().map_err(at)?.is_file() {
            return Err(at(invalid_input(
                "opened export staging artifact is not a regular file",
            )));
        }
        sync_file(&staged_file, &self.staging_file).map_err(at)?;
        drop(staged_file);

        // The staging directory is not synchronized: until the destination
        // directory is, a crash leaves the destination as it was, and the next
        // staging in this folder deletes what is left of this one.
        let staged_path =
            tempfile::TempPath::try_from_path(self.staging_file.clone()).map_err(at)?;
        if let Err(error) = staged_path.persist(&self.destination) {
            return Err(at(error.error));
        }

        // The destination now names the complete file. Everything after this
        // point is best-effort housekeeping/durability reporting.
        if let Err(error) = self.cleanup() {
            tracing::warn!(
                error_kind = ?error.kind(),
                raw_os_error = ?error.raw_os_error(),
                "export committed but its staging directory could not be removed"
            );
        }
        if let Err(error) = sync_directory_if_supported(&self.parent) {
            tracing::warn!(
                error_kind = ?error.kind(),
                raw_os_error = ?error.raw_os_error(),
                "export committed but its destination directory could not be synchronized"
            );
        }

        Ok(())
    }

    fn cleanup(&mut self) -> io::Result<()> {
        if !self.cleanup_needed {
            return Ok(());
        }
        match fs::remove_dir_all(&self.staging_directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.cleanup_needed = false;
        drop(self.claim.take());
        Ok(())
    }
}

/// Whether `name` is a staging directory's: the prefix and four hex fields.
fn is_staging_directory(name: &str) -> bool {
    let hex = |field: &str| !field.is_empty() && field.bytes().all(|b| b.is_ascii_hexdigit());
    name.strip_prefix(STAGING_DIRECTORY_PREFIX)
        .is_some_and(|fields| fields.split('-').count() == 4 && fields.split('-').all(hex))
}

impl Drop for TransactionalFile {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(
                error_kind = ?error.kind(),
                raw_os_error = ?error.raw_os_error(),
                "export staging cleanup failed"
            );
        }
    }
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn validate_destination_path(destination: &Path) -> io::Result<()> {
    let file_name = destination
        .file_name()
        .ok_or_else(|| invalid_input("export destination has no file name"))?;
    let mut components = Path::new(file_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(invalid_input(
            "export destination file name is not a single path component",
        ));
    }
    Ok(())
}

fn validate_destination_shape(destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(metadata) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is {}, not a regular file; export to a file name of its own",
                destination.display(),
                if metadata.file_type().is_symlink() {
                    "a symbolic link"
                } else if metadata.is_dir() {
                    "a folder"
                } else {
                    "a special file"
                }
            ),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(aria_models::path_error(destination, &error)),
    }
}

fn staging_file_name(destination: &Path) -> io::Result<OsString> {
    let mut name = OsString::from("export");
    if let Some(extension) = destination.extension() {
        if os_str_units(extension) > MAX_EXTENSION_UNITS {
            return Err(invalid_input("export destination extension is too long"));
        }
        name.push(".");
        name.push(extension);
    }

    let mut components = Path::new(&name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(invalid_input(
            "export destination extension is not a safe path component",
        ));
    }
    Ok(name)
}

#[cfg(unix)]
fn os_str_units(value: &OsStr) -> usize {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().len()
}

#[cfg(windows)]
fn os_str_units(value: &OsStr) -> usize {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().count()
}

#[cfg(not(any(unix, windows)))]
fn os_str_units(value: &OsStr) -> usize {
    value.to_string_lossy().len()
}

// The paths [`sync_file`] and [`sync_directory_if_supported`] synchronized on
// this thread, in order.
#[cfg(test)]
thread_local! {
    static SYNCED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_sync(path: &Path) {
    SYNCED.with_borrow_mut(|synced| synced.push(path.to_owned()));
}

#[cfg(not(test))]
fn record_sync(_: &Path) {}

/// Flush `file`, opened at `path`, to the device.
fn sync_file(file: &fs::File, path: &Path) -> io::Result<()> {
    record_sync(path);
    file.sync_all()
}

#[cfg(unix)]
fn sync_directory_if_supported(path: &Path) -> io::Result<()> {
    record_sync(path);
    match fs::File::open(path).and_then(|directory| directory.sync_all()) {
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

#[cfg(not(unix))]
fn sync_directory_if_supported(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn assert_no_staging_directories(parent: &Path) {
        let leaked: Vec<_> = fs::read_dir(parent)
            .expect("read parent")
            .map(|entry| entry.expect("directory entry").file_name())
            .filter(|name| name.to_string_lossy().starts_with(STAGING_DIRECTORY_PREFIX))
            .collect();
        assert!(leaked.is_empty(), "leaked staging directories: {leaked:?}");
    }

    #[test]
    fn abandoned_and_unwound_staging_preserve_old_destination() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        let sibling = root.path().join("keep.txt");
        fs::write(&destination, b"old export").expect("seed destination");
        fs::write(&sibling, b"unrelated").expect("seed sibling");

        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        fs::write(staged.staging_path(), b"incomplete export").expect("write staging");
        drop(staged);

        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"old export"
        );
        assert_eq!(fs::read(&sibling).expect("read sibling"), b"unrelated");
        assert_no_staging_directories(root.path());

        let unwind = std::panic::catch_unwind(|| {
            let staged = TransactionalFile::new(&destination).expect("allocate staging");
            fs::write(staged.staging_path(), b"panicked export").expect("write staging");
            panic!("simulate encoder panic");
        });
        assert!(unwind.is_err());
        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"old export"
        );
        assert_eq!(fs::read(&sibling).expect("read sibling"), b"unrelated");
        assert_no_staging_directories(root.path());
    }

    #[test]
    fn abandoned_staging_leaves_absent_destination_absent() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");

        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        fs::write(staged.staging_path(), b"incomplete export").expect("write staging");
        drop(staged);

        assert!(!destination.exists());
        assert_no_staging_directories(root.path());
    }

    /// The names in `dir`, sorted, hidden ones included.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read dir")
            .map(|entry| {
                let name = entry.expect("directory entry").file_name();
                name.to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// WHY: an export killed or crashed part way (SIGKILL, a power cut, a
    /// process exit that skips destructors) left its staging directory,
    /// unfinished file included, hidden beside the destination for good. The
    /// next staging in that folder deletes every staging directory no live
    /// writer holds, with its lock file, and leaves a live one and every
    /// other entry alone. Not caught: a live writer from a build that made no
    /// lock file, whose directory reads as dead.
    #[test]
    fn the_next_export_deletes_what_a_killed_export_left() {
        let root = tempfile::tempdir().expect("tempdir");
        let parent = root.path();
        let live = TransactionalFile::new(&parent.join("other.mp4")).expect("a live export");
        fs::write(live.staging_path(), b"in progress").expect("write live");

        // Dead: a directory beside an unheld lock, one with no lock file (left
        // before lock files existed), and a lock file whose directory is gone.
        let name = |pid: u32| format!("{STAGING_DIRECTORY_PREFIX}{pid:08x}-00000001-0002-0003");
        for (pid, locked) in [(7, true), (8, false)] {
            let dead = parent.join(name(pid));
            fs::create_dir(&dead).expect("mkdir");
            fs::write(dead.join("export.mp4"), b"unfinished").expect("write");
            if locked {
                File::create(scratch::lock_path(&dead)).expect("lock file");
            }
        }
        File::create(scratch::lock_path(&parent.join(name(9)))).expect("orphan lock file");
        // Not staging directories: a name off the pattern and a file.
        fs::create_dir(parent.join(".aria-export-notes")).expect("mkdir");
        fs::write(parent.join("keep.txt"), b"unrelated").expect("write");

        let staged = TransactionalFile::new(&parent.join("movie.mp4")).expect("allocate");
        fs::write(staged.staging_path(), b"complete export").expect("write staging");
        staged.publish().expect("publish");

        let live_name = |path: &Path| {
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned()
        };
        let mut kept = vec![
            live_name(&live.staging_directory),
            live_name(
                live.claim
                    .as_ref()
                    .and_then(ScratchLock::lock_path)
                    .expect("held by path"),
            ),
            ".aria-export-notes".to_owned(),
            "keep.txt".to_owned(),
            "movie.mp4".to_owned(),
        ];
        kept.sort();
        assert_eq!(entries(parent), kept);
        assert_eq!(
            fs::read(live.staging_path()).expect("live export kept"),
            b"in progress"
        );
        drop(live);
        assert_eq!(
            entries(parent),
            vec![".aria-export-notes", "keep.txt", "movie.mp4"]
        );
    }

    #[test]
    fn successful_publication_replaces_old_file_and_cleans_staging() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        let sibling = root.path().join("keep.txt");
        fs::write(&destination, b"old export").expect("seed destination");
        fs::write(&sibling, b"unrelated").expect("seed sibling");

        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        let staging_directory = staged.staging_directory.clone();
        fs::write(staged.staging_path(), b"complete export").expect("write staging");
        staged.publish().expect("publish");

        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"complete export"
        );
        assert_eq!(fs::read(&sibling).expect("read sibling"), b"unrelated");
        assert!(!staging_directory.exists());
        assert_no_staging_directories(root.path());
    }

    #[test]
    fn destination_directory_during_publication_is_preserved() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        fs::write(staged.staging_path(), b"complete export").expect("write staging");

        fs::create_dir(&destination).expect("create destination directory");
        fs::write(destination.join("marker"), b"keep").expect("seed destination directory");
        let error = staged
            .publish()
            .expect_err("directory must reject publication");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(destination.join("marker")).expect("read marker"),
            b"keep"
        );
        assert_no_staging_directories(root.path());
    }

    #[test]
    fn staging_is_a_sibling_and_preserves_the_extension() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("cut.final.MP4");
        let staged = TransactionalFile::new(&destination).expect("allocate staging");

        assert_eq!(
            staged.staging_directory.parent(),
            destination.parent(),
            "staging directory must use the exact destination parent"
        );
        assert_eq!(
            staged.staging_file.parent(),
            Some(staged.staging_directory.as_path())
        );
        assert_eq!(staged.staging_file.extension(), destination.extension());
        assert!(
            staged
                .staging_directory
                .file_name()
                .expect("staging name")
                .to_string_lossy()
                .len()
                <= MAX_GENERATED_DIRECTORY_BYTES
        );

        drop(staged);
        assert_no_staging_directories(root.path());
    }

    #[cfg(unix)]
    #[test]
    fn staging_symlink_is_rejected_without_following_it() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        let outside = root.path().join("outside.mp4");
        fs::write(&destination, b"old export").expect("seed destination");
        fs::write(&outside, b"outside").expect("seed outside");

        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        fs::remove_file(staged.staging_path()).expect("remove placeholder");
        symlink(&outside, staged.staging_path()).expect("replace staging with symlink");
        let error = staged.publish().expect_err("symlink must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"old export"
        );
        assert_eq!(fs::read(&outside).expect("read outside"), b"outside");
        assert_no_staging_directories(root.path());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_destination_name_does_not_require_utf8_conversion() {
        use std::os::unix::ffi::OsStringExt;

        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join(OsString::from_vec(vec![
            b'v', 0xff, b'i', b'd', b'.', b'm', b'p', b'4',
        ]));
        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        assert_eq!(staged.staging_file.extension(), Some(OsStr::new("mp4")));
        fs::write(staged.staging_path(), b"complete export").expect("write staging");
        drop(staged);

        assert!(!destination.exists());
        assert_no_staging_directories(root.path());
    }

    /// WHY: `aria frame` and `aria export` into a folder that cannot be
    /// written failed as "i/o error: Permission denied (os error 13)", naming
    /// no path. The staging allocation states the folder and the reason and
    /// keeps the kind. Not caught when the test runs with the right to write
    /// anywhere (as root), which the probe write detects.
    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_written_is_stated_by_path() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("tempdir");
        let locked = root.path().join("locked");
        fs::create_dir(&locked).expect("make a folder");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).expect("lock");
        let writable = fs::write(locked.join("probe"), b"").is_ok();
        let result = TransactionalFile::new(&locked.join("movie.mp4"));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlock");
        if writable {
            return;
        }
        let error = result.expect_err("a staging folder in a locked folder");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.to_string(),
            format!(
                "cannot write in \"{}\": permission denied",
                locked.display()
            )
        );
        assert_no_staging_directories(&locked);
    }

    /// WHY: publication synchronized the staging directory before the rename,
    /// a directory deleted right after it, so the sync made nothing durable
    /// and cost a journal commit: 0.33 s of a 4 s `aria frame` on a rotational
    /// disk. Publication now synchronizes the staged file before the rename
    /// and the destination folder after it, and nothing else. Not caught: a
    /// sync recorded but not made, or one made outside `publish`.
    #[test]
    fn publication_syncs_the_file_and_then_the_destination_folder_only() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        for existing in [false, true] {
            if existing {
                fs::write(&destination, b"old export").expect("seed destination");
            }
            let staged = TransactionalFile::new(&destination).expect("allocate staging");
            fs::write(staged.staging_path(), b"complete export").expect("write staging");
            let mut expected = vec![staged.staging_file.clone()];
            if cfg!(unix) {
                expected.push(staged.parent.clone());
            }
            SYNCED.with_borrow_mut(Vec::clear);
            staged.publish().expect("publish");
            assert_eq!(
                SYNCED.with_borrow(Clone::clone),
                expected,
                "existing {existing}"
            );
            assert_eq!(
                fs::read(&destination).expect("read destination"),
                b"complete export"
            );
            assert_no_staging_directories(root.path());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_publication_replaces_an_existing_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let destination = root.path().join("movie.mp4");
        fs::write(&destination, b"old export").expect("seed destination");

        let staged = TransactionalFile::new(&destination).expect("allocate staging");
        fs::write(staged.staging_path(), b"complete export").expect("write staging");
        staged.publish().expect("replace existing destination");

        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"complete export"
        );
        assert_no_staging_directories(root.path());
    }
}
