//! The quota-managed download cache.
//!
//! Stock files, template bundles, and packs land here before import.
//! Unbounded data-dir growth is a real complaint generator, so the cache
//! enforces a byte quota with LRU eviction (by modification time — files
//! get re-touched on use). Legacy projects may still reference downloaded
//! source files directly; those paths are explicitly protected until import
//! promotion migrates every project to durable media storage.
//!
//! Settings surfaces a "clear download cache" action backed by
//! [`DownloadCache::clear`].

use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::error::CloudError;

/// Default quota: 2 GiB of downloaded stock/bundles before eviction.
pub const DEFAULT_QUOTA_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// A directory with a byte budget.
pub struct DownloadCache {
    root: RwLock<PathBuf>,
    quota_bytes: AtomicU64,
    protected_paths: RwLock<HashSet<PathBuf>>,
    destructive_operations_blocked: AtomicBool,
    activity: Mutex<CacheActivity>,
    activity_changed: Condvar,
}

impl std::fmt::Debug for DownloadCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadCache")
            .field("quota_bytes", &self.quota_bytes.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Exact accounting for an explicit download-cache clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadCacheClear {
    /// Logical bytes removed from non-directory entries.
    pub removed_bytes: u64,
    /// Number of non-directory entries removed.
    pub removed_files: u64,
    /// Existing protected files retained in the cache.
    pub retained_protected_files: u64,
}

/// Why an exclusive download-cache root switch failed.
#[derive(Debug, thiserror::Error)]
pub enum DownloadCacheSwitchError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// Project media inventory currently blocks destructive cache operations.
    #[error("download cache root switching is blocked until project media inventory completes")]
    DestructiveOperationsBlocked,
    /// Another maintenance operation or an active lease outlived the wait.
    #[error("download cache root switching timed out while cache files were in use")]
    MaintenanceBusy,
    /// The destination was not absolute.
    #[error("download cache root destination must be absolute")]
    RelativeDestination,
    /// The destination was a filesystem root such as `/`.
    #[error("download cache root destination cannot be a filesystem root")]
    FilesystemRootDestination,
    /// The destination was the current cache root.
    #[error("download cache root destination is already active")]
    SameDestination,
    /// The destination itself was a symbolic link.
    #[error("download cache root destination cannot be a symbolic link")]
    SymlinkDestination,
    /// A non-symlink filesystem entry already occupied the destination.
    #[error("download cache root destination already exists")]
    ExistingDestination,
    /// The destination could not be inspected safely.
    #[error("could not inspect download cache root destination: {0}")]
    DestinationIo(#[source] io::Error),
    /// A protected path did not belong to the current root.
    #[error("a protected download cache path is outside the current root")]
    ProtectedPathOutsideRoot,
    /// The caller's filesystem switch operation failed.
    #[error("download cache root migration failed: {0}")]
    Migration(#[source] E),
}

/// A cache path protected from clear or eviction for the lease's lifetime.
///
/// Keep the lease alive while reading, downloading, verifying, or promoting
/// the path. Drop it before calling [`DownloadCache::enforce_quota`] or
/// [`DownloadCache::clear`].
#[derive(Debug)]
pub struct DownloadCacheLease<'a> {
    cache: &'a DownloadCache,
    path: PathBuf,
}

impl DownloadCacheLease<'_> {
    /// Absolute path represented by this lease.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DownloadCacheLease<'_> {
    fn drop(&mut self) {
        self.cache.finish_access();
    }
}

#[derive(Debug, Default)]
struct CacheActivity {
    active_leases: usize,
    maintenance: bool,
}

struct MaintenanceGuard<'a> {
    cache: &'a DownloadCache,
}

impl Drop for MaintenanceGuard<'_> {
    fn drop(&mut self) {
        let mut activity = self.cache.activity.lock();
        activity.maintenance = false;
        self.cache.activity_changed.notify_all();
    }
}

impl DownloadCache {
    pub fn new(root: PathBuf, quota_bytes: u64) -> Self {
        Self {
            root: RwLock::new(root),
            quota_bytes: AtomicU64::new(quota_bytes),
            protected_paths: RwLock::new(HashSet::new()),
            destructive_operations_blocked: AtomicBool::new(false),
            activity: Mutex::new(CacheActivity::default()),
            activity_changed: Condvar::new(),
        }
    }

    /// Snapshots the currently active cache root.
    ///
    /// A switch that follows this call does not alter the returned path.
    #[must_use]
    pub fn root(&self) -> PathBuf {
        self.root.read().clone()
    }

    /// Exclusively migrate the cache to a new root and publish it on success.
    ///
    /// `destination` must be absolute, absent, different from the current
    /// root, and not a filesystem root or symbolic link. Existing leases are
    /// allowed to finish before `migrate` runs, and new leases wait until this
    /// method returns. The closure receives the old and new roots while cache
    /// maintenance and root access are exclusive. It must use those arguments
    /// instead of calling root-dependent methods on this cache.
    ///
    /// The in-memory root and every protected path are changed together only
    /// after `migrate` succeeds. A closure error leaves both unchanged.
    pub fn switch_root<D, F, E>(
        &self,
        destination: D,
        migrate: F,
    ) -> Result<(), DownloadCacheSwitchError<E>>
    where
        D: AsRef<Path>,
        F: FnOnce(&Path, &Path) -> Result<(), E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        if self.destructive_operations_blocked() {
            return Err(DownloadCacheSwitchError::DestructiveOperationsBlocked);
        }
        let _maintenance = self
            .begin_maintenance(MAINTENANCE_WAIT_TIMEOUT)
            .map_err(|_| DownloadCacheSwitchError::MaintenanceBusy)?;
        if self.destructive_operations_blocked() {
            return Err(DownloadCacheSwitchError::DestructiveOperationsBlocked);
        }

        let destination = destination.as_ref();
        let mut root = self.root.write();
        if !destination.is_absolute() {
            return Err(DownloadCacheSwitchError::RelativeDestination);
        }
        if is_filesystem_root(destination) {
            return Err(DownloadCacheSwitchError::FilesystemRootDestination);
        }
        if destination == root.as_path() {
            return Err(DownloadCacheSwitchError::SameDestination);
        }
        match std::fs::symlink_metadata(destination) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DownloadCacheSwitchError::SymlinkDestination);
            }
            Ok(_) => return Err(DownloadCacheSwitchError::ExistingDestination),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(DownloadCacheSwitchError::DestinationIo(error)),
        }

        // Root readers are excluded before this lock is acquired, and every
        // protected-path writer takes the root lock first. Holding both locks
        // makes the remap and root publication one observable transition.
        let mut protected = self.protected_paths.write();
        let remapped = protected
            .iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(root.as_path())
                    .map_err(|_| DownloadCacheSwitchError::ProtectedPathOutsideRoot)?;
                if relative.as_os_str().is_empty()
                    || !relative
                        .components()
                        .all(|component| matches!(component, Component::Normal(_)))
                {
                    return Err(DownloadCacheSwitchError::ProtectedPathOutsideRoot);
                }
                Ok(destination.join(relative))
            })
            .collect::<Result<HashSet<_>, DownloadCacheSwitchError<E>>>()?;

        migrate(root.as_path(), destination).map_err(DownloadCacheSwitchError::Migration)?;

        *protected = remapped;
        *root = destination.to_path_buf();
        // Publish the new root before protected-path readers resume. Any
        // operation that needs both always acquires root first.
        drop(root);
        drop(protected);
        Ok(())
    }

    /// Returns the current byte quota.
    ///
    /// Quota enforcement snapshots this value once per invocation, so a
    /// concurrent update takes effect on a subsequent invocation.
    #[must_use]
    pub fn quota_bytes(&self) -> u64 {
        self.quota_bytes.load(Ordering::Relaxed)
    }

    /// Replaces the byte quota used by subsequent quota enforcement.
    ///
    /// An enforcement already in progress continues using the quota it
    /// snapshotted when it started.
    pub fn set_quota_bytes(&self, new: u64) {
        self.quota_bytes.store(new, Ordering::Relaxed);
    }

    /// Keep a cache-owned source file available while a project references it.
    ///
    /// Missing paths may be protected so a later re-download cannot be
    /// evicted before the project is migrated. Paths outside this cache, the
    /// cache root itself, and paths containing traversal components fail
    /// closed.
    pub fn protect_path(&self, path: &Path) -> Result<(), CloudError> {
        let root = self.root.read();
        let path = contained_cache_path(root.as_path(), path)?;
        if let Ok(metadata) = std::fs::symlink_metadata(&path)
            && !metadata.file_type().is_file()
        {
            return Err(invalid_cache_path());
        }
        self.protected_paths.write().insert(path);
        Ok(())
    }

    /// Number of project-referenced cache paths currently protected.
    #[must_use]
    pub fn protected_path_count(&self) -> usize {
        self.protected_paths.read().len()
    }

    /// Fail closed while project references have not been fully inventoried.
    ///
    /// Reads, writes, and explicit protection continue to work. Quota
    /// enforcement becomes a no-op and explicit clearing returns an error.
    pub fn block_destructive_operations(&self) {
        self.destructive_operations_blocked
            .store(true, Ordering::Release);
    }

    /// Re-enable clear and eviction after every project reference was scanned.
    pub fn allow_destructive_operations(&self) {
        self.destructive_operations_blocked
            .store(false, Ordering::Release);
    }

    /// Whether clear and quota eviction currently fail closed.
    #[must_use]
    pub fn destructive_operations_blocked(&self) -> bool {
        self.destructive_operations_blocked.load(Ordering::Acquire)
    }

    /// Reserve one safe cache path for a complete read or write operation.
    ///
    /// Concurrent leases are allowed. Clear and eviction wait for every lease
    /// to drop, while new leases wait for maintenance to finish.
    pub fn lease(&self, key: &str) -> Result<DownloadCacheLease<'_>, CloudError> {
        self.begin_access()?;
        match self.path_for(key) {
            Ok(path) => Ok(DownloadCacheLease { cache: self, path }),
            Err(error) => {
                self.finish_access();
                Err(error)
            }
        }
    }

    /// Where a download for `key` (e.g. `stock/pexels-857191-hd.mp4`)
    /// should land. Creates parent directories.
    pub fn path_for(&self, key: &str) -> Result<PathBuf, CloudError> {
        let key = validated_key(key)?;
        let root = self.root.read();
        ensure_cache_root(root.as_path())?;
        let parent = key.parent().unwrap_or_else(|| Path::new(""));
        ensure_safe_subdirectories(root.as_path(), parent)?;
        let path = root.join(key);
        reject_symlink_or_directory(&path)?;
        Ok(path)
    }

    /// Whether `key` is already downloaded (touches it for LRU).
    pub fn hit(&self, key: &str) -> Option<PathBuf> {
        let key = validated_key(key).ok()?;
        let root = self.root.read();
        if !safe_existing_root(root.as_path()) {
            return None;
        }
        ensure_existing_safe_subdirectories(
            root.as_path(),
            key.parent().unwrap_or_else(|| Path::new("")),
        )
        .ok()?;
        let path = root.join(key);
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_file() {
            // Re-touch so recently-used files survive eviction.
            match filetime_touch(&path) {
                Ok(()) => Some(path),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(_) => Some(path),
            }
        } else {
            None
        }
    }

    /// Total bytes currently cached.
    pub fn size_bytes(&self) -> u64 {
        let root = self.root.read();
        saturating_total_bytes(
            walk_files(root.as_path())
                .into_iter()
                .filter_map(|p| std::fs::symlink_metadata(p).ok())
                .map(|m| m.len()),
        )
    }

    /// Evict least-recently-modified files until under quota. Called after
    /// each completed download; cheap when under budget.
    pub fn enforce_quota(&self) {
        if self.destructive_operations_blocked() {
            tracing::debug!("download cache eviction blocked pending project inventory");
            return;
        }
        let Ok(_maintenance) = self.begin_maintenance(MAINTENANCE_WAIT_TIMEOUT) else {
            tracing::debug!("download cache eviction skipped while files remain in use");
            return;
        };
        if self.destructive_operations_blocked() {
            return;
        }
        let quota_bytes = self.quota_bytes();
        let root = self.root.read();
        let protected = self.protected_paths.read().clone();
        let mut files: Vec<(PathBuf, u64, SystemTime)> = walk_files(root.as_path())
            .into_iter()
            .filter(|path| !protected.contains(path))
            .filter_map(|p| {
                let meta = std::fs::symlink_metadata(&p).ok()?;
                let mtime = meta.modified().ok()?;
                Some((p, meta.len(), mtime))
            })
            .collect();
        let mut total = saturating_total_bytes(files.iter().map(|(_, len, _)| *len));
        if total <= quota_bytes {
            return;
        }
        // Oldest first.
        files.sort_by_key(|(_, _, mtime)| *mtime);
        for (path, len, _) in files {
            if total <= quota_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                tracing::debug!("cache evicted {}", path.display());
                total = total.saturating_sub(len);
            }
        }
    }

    /// Delete everything (the settings "clear download cache" action).
    /// The root itself stays so in-flight paths remain valid.
    pub fn clear(&self) -> Result<DownloadCacheClear, CloudError> {
        if self.destructive_operations_blocked() {
            return Err(cache_inventory_incomplete());
        }
        let _maintenance = self.begin_maintenance(MAINTENANCE_WAIT_TIMEOUT)?;
        if self.destructive_operations_blocked() {
            return Err(cache_inventory_incomplete());
        }
        let root = self.root.read();
        if safe_root_entries(root.as_path())?.is_none() {
            return Ok(DownloadCacheClear {
                removed_bytes: 0,
                removed_files: 0,
                retained_protected_files: 0,
            });
        }
        let protected = self.protected_paths.read().clone();
        let (files, mut directories) = walk_tree(root.as_path());
        let mut removed_bytes = 0_u64;
        let mut removed_files = 0_u64;
        let mut retained_protected_files = 0_u64;
        for path in files {
            if protected.contains(&path) {
                retained_protected_files = retained_protected_files.saturating_add(1);
                continue;
            }
            let bytes = std::fs::symlink_metadata(&path)?.len();
            std::fs::remove_file(path)?;
            removed_bytes = removed_bytes.saturating_add(bytes);
            removed_files = removed_files.saturating_add(1);
        }
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for path in directories {
            match std::fs::remove_dir(path) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::NotFound
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(DownloadCacheClear {
            removed_bytes,
            removed_files,
            retained_protected_files,
        })
    }

    fn begin_access(&self) -> Result<(), CloudError> {
        let mut activity = self.activity.lock();
        while activity.maintenance {
            self.activity_changed.wait(&mut activity);
        }
        activity.active_leases = activity
            .active_leases
            .checked_add(1)
            .ok_or_else(cache_activity_exhausted)?;
        Ok(())
    }

    fn finish_access(&self) {
        let mut activity = self.activity.lock();
        activity.active_leases = activity.active_leases.saturating_sub(1);
        if activity.active_leases == 0 {
            self.activity_changed.notify_all();
        }
    }

    fn begin_maintenance(&self, timeout: Duration) -> Result<MaintenanceGuard<'_>, CloudError> {
        let deadline = Instant::now() + timeout;
        let mut activity = self.activity.lock();
        while activity.maintenance || activity.active_leases != 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(cache_busy());
            };
            let result = self.activity_changed.wait_for(&mut activity, remaining);
            if result.timed_out() && (activity.maintenance || activity.active_leases != 0) {
                return Err(cache_busy());
            }
        }
        activity.maintenance = true;
        Ok(MaintenanceGuard { cache: self })
    }
}

const MAX_CACHE_KEY_BYTES: usize = 1_024;
const MAINTENANCE_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

fn validated_key(key: &str) -> Result<&Path, CloudError> {
    let path = Path::new(key);
    let valid = !key.is_empty()
        && key.len() <= MAX_CACHE_KEY_BYTES
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(path)
    } else {
        Err(CloudError::Protocol(
            "invalid download cache key".to_owned(),
        ))
    }
}

fn contained_cache_path(root: &Path, path: &Path) -> Result<PathBuf, CloudError> {
    if !root.is_absolute() || !path.is_absolute() {
        return Err(invalid_cache_path());
    }
    let relative = path.strip_prefix(root).map_err(|_| invalid_cache_path())?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid_cache_path());
    }
    Ok(root.join(relative))
}

fn cache_activity_exhausted() -> CloudError {
    io::Error::other("download cache has too many active operations").into()
}

fn cache_busy() -> CloudError {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "download cache files are still in use",
    )
    .into()
}

fn cache_inventory_incomplete() -> CloudError {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "download cache is protected until project media inventory completes",
    )
    .into()
}

fn is_filesystem_root(path: &Path) -> bool {
    path.is_absolute() && path.parent().is_none()
}

fn ensure_cache_root(root: &Path) -> Result<(), CloudError> {
    if !root.is_absolute() {
        return Err(invalid_cache_root());
    }
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(invalid_cache_root()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(root)?;
            let metadata = std::fs::symlink_metadata(root)?;
            if metadata.file_type().is_dir() {
                Ok(())
            } else {
                Err(invalid_cache_root())
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn safe_existing_root(root: &Path) -> bool {
    root.is_absolute()
        && std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn ensure_safe_subdirectories(root: &Path, relative: &Path) -> Result<(), CloudError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid_cache_path());
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(invalid_cache_path()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
                let metadata = std::fs::symlink_metadata(&current)?;
                if !metadata.file_type().is_dir() {
                    return Err(invalid_cache_path());
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_existing_safe_subdirectories(root: &Path, relative: &Path) -> Result<(), ()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        current.push(name);
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| ())?;
        if !metadata.file_type().is_dir() {
            return Err(());
        }
    }
    Ok(())
}

fn reject_symlink_or_directory(path: &Path) -> Result<(), CloudError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(invalid_cache_path()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn safe_root_entries(root: &Path) -> Result<Option<std::fs::ReadDir>, CloudError> {
    if !root.is_absolute() {
        return Err(invalid_cache_root());
    }
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            std::fs::read_dir(root).map(Some).map_err(Into::into)
        }
        Ok(_) => Err(invalid_cache_root()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn invalid_cache_root() -> CloudError {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "download cache root must be an absolute, real directory",
    )
    .into()
}

fn invalid_cache_path() -> CloudError {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "download cache path crosses an unsafe filesystem entry",
    )
    .into()
}

fn saturating_total_bytes(lengths: impl IntoIterator<Item = u64>) -> u64 {
    lengths
        .into_iter()
        .fold(0, |total, len| total.saturating_add(len))
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    walk_tree(root).0
}

fn walk_tree(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    if !safe_existing_root(root) {
        return (Vec::new(), Vec::new());
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 64 {
            continue;
        }
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                directories.push(path.clone());
                stack.push((path, depth + 1));
            } else {
                files.push(path);
            }
        }
    }
    (files, directories)
}

/// Bump a file's mtime to now (LRU touch). `File::set_times` needs no
/// extra dependency.
fn filetime_touch(path: &Path) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new().append(true).open(path)?;
    let now = std::fs::FileTimes::new().set_modified(SystemTime::now());
    file.set_times(now)
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
