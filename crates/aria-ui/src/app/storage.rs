//! The download cache: size against quota, clearing, and relocation.
//!
//! `aria-cloud`'s `DownloadCache` had no UI surface, so a cache with a quota
//! nothing could read or change was the only thing standing between the session and
//! a full disk. One cache instance lives on `AppState` for the session, because
//! protected paths and the current root are session state: rebuilding it per call
//! would forget both.
//!
//! Every root here is derived from `data_dir`, and the config file is
//! `data_dir/config.toml` — the same path `aria_settings::default_config_path()`
//! resolves to in production, and a temporary directory in a test. Nothing in this
//! module touches an absolute path from outside that tree, so a test that clears the
//! cache cannot reach the real one.

use std::path::{Path, PathBuf};

use aria_cloud::cache::{DownloadCache, DownloadCacheClear, DownloadCacheSwitchError};

use super::AppState;

/// The directory holding downloaded assets, under the storage root.
pub const DOWNLOADS_DIR: &str = "downloads";

const MIB: u64 = 1024 * 1024;

/// Build the session's cache from the config beside `data_dir`.
///
/// A storage-root override redirects the cache; an absent or malformed config
/// yields the defaults rather than failing startup, because a cache that is not
/// on screen is still better than an editor that will not open.
pub(super) fn cache_for(data_dir: &Path) -> DownloadCache {
    let settings = aria_settings::load(&aria_settings::config_in(data_dir)).unwrap_or_default();
    let root = settings
        .storage
        .root
        .clone()
        .unwrap_or_else(|| data_dir.to_path_buf())
        .join(DOWNLOADS_DIR);
    DownloadCache::new(
        root,
        settings.storage.download_quota_mib.saturating_mul(MIB),
    )
}

impl AppState {
    /// The config file that owns the quota and the storage root.
    pub fn config_path(&self) -> PathBuf {
        aria_settings::config_in(&self.data_dir)
    }

    /// Where downloaded assets live.
    pub fn download_cache_root(&self) -> PathBuf {
        self.download_cache.root()
    }

    /// Bytes currently cached. Walks the tree, so callers show it, they do not poll
    /// it every frame.
    pub fn download_cache_size_bytes(&self) -> u64 {
        self.download_cache.size_bytes()
    }

    /// The quota in MiB, which is the unit the config and the dialog both use.
    pub fn download_quota_mib(&self) -> u64 {
        self.download_cache.quota_bytes() / MIB
    }

    /// How many cached files are pinned against eviction and clearing.
    pub fn protected_download_count(&self) -> usize {
        self.download_cache.protected_path_count()
    }

    /// Set the quota, persist it, and evict down to it.
    ///
    /// A value outside the range the config accepts is refused by name rather than
    /// silently clamped: a quota that is not the typed number is worse than a
    /// refusal.
    pub fn set_download_quota_mib(&mut self, mib: u64) -> Result<(), String> {
        if !aria_settings::StorageSettings::is_valid_download_quota_mib(mib) {
            return Err(format!(
                "download quota {mib} MiB is out of range ({}–{} MiB); pick a value inside it",
                aria_settings::MIN_DOWNLOAD_QUOTA_MIB,
                aria_settings::MAX_DOWNLOAD_QUOTA_MIB
            ));
        }
        let path = self.config_path();
        let mut settings = aria_settings::load(&path).map_err(|err| {
            format!(
                "{}: {err}; fix the file before changing the quota",
                path.display()
            )
        })?;
        settings.storage.download_quota_mib = mib;
        aria_settings::save(&path, &settings)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        self.download_cache.set_quota_bytes(mib.saturating_mul(MIB));
        self.download_cache.enforce_quota();
        self.status_message = Some(format!("Download quota set to {mib} MiB"));
        Ok(())
    }

    /// Whether the cache currently refuses destructive maintenance.
    ///
    /// True while a cloud download is in flight. Surfaced so a refused Clear reads as
    /// "busy", not "broken".
    pub fn download_cache_busy(&self) -> bool {
        self.download_cache.destructive_operations_blocked()
    }

    /// Take the guard for the duration of a background download.
    ///
    /// A Clear or a Move that lands mid-download deletes or moves the partial file the
    /// download is still writing, and the import that follows then references a path
    /// that is gone. The guard is not a preference: it is held by the work
    /// that would be corrupted, and released when that work ends.
    pub(crate) fn hold_download_guard(&mut self) {
        self.download_cache.block_destructive_operations();
    }

    pub(crate) fn release_download_guard(&mut self) {
        self.download_cache.allow_destructive_operations();
    }

    /// Delete every unpinned cached file.
    ///
    /// The status line names how many paths were kept, because a clear that silently
    /// leaves files behind looks like it failed, and one that silently deletes a file
    /// a project still needs looks like corruption.
    pub fn clear_download_cache(&mut self) -> Result<DownloadCacheClear, String> {
        let cleared = self
            .download_cache
            .clear()
            .map_err(|err| format!("clearing the download cache: {err}"))?;
        let removed = format!(
            "Cleared {} file(s), {}",
            cleared.removed_files,
            format_bytes(cleared.removed_bytes)
        );
        self.status_message = Some(match cleared.retained_protected_files {
            0 => removed,
            kept => format!("{removed}; kept {kept} in use"),
        });
        Ok(cleared)
    }

    /// Move the storage root, taking the cache with it, and persist the new root.
    ///
    /// The move happens before anything is published, so a failed move leaves the old
    /// root serving. The config is written only after the move succeeds, because a
    /// config pointing at a root that does not exist is a broken next launch.
    pub fn relocate_storage_root(&mut self, root: impl AsRef<Path>) -> Result<(), String> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(format!(
                "{} is not an absolute path; pick a full path for the storage root",
                root.display()
            ));
        }
        let destination = root.join(DOWNLOADS_DIR);
        self.download_cache
            .switch_root(&destination, move_tree)
            .map_err(describe_switch_error)?;

        let path = self.config_path();
        let mut settings = aria_settings::load(&path).map_err(|err| {
            format!(
                "{}: {err}; the cache moved but the config was not updated",
                path.display()
            )
        })?;
        settings.storage.root = Some(root.to_path_buf());
        aria_settings::save(&path, &settings).map_err(|err| {
            format!(
                "{}: {err}; the cache moved but the config was not updated",
                path.display()
            )
        })?;
        self.status_message = Some(format!("Storage root is now {}", root.display()));
        self.refresh_render_files();
        Ok(())
    }
}

/// Move `from` to `to`, falling back to copy-then-delete across filesystems.
///
/// A missing source is success: relocating before the first download has nothing to
/// carry, and refusing that would make the setting unusable on a fresh install.
fn move_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.exists() {
        return std::fs::create_dir_all(to);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        // Across devices rename fails; copy the tree, then drop the original.
        Err(_) => {
            copy_tree(from, to)?;
            std::fs::remove_dir_all(from)
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Turn a switch failure into a sentence naming the corrective action.
fn describe_switch_error(err: DownloadCacheSwitchError<std::io::Error>) -> String {
    match err {
        DownloadCacheSwitchError::RelativeDestination => {
            "the storage root must be an absolute path".into()
        }
        DownloadCacheSwitchError::FilesystemRootDestination => {
            "the storage root cannot be a filesystem root; pick a directory inside it".into()
        }
        DownloadCacheSwitchError::SameDestination => "the storage root is already there".into(),
        DownloadCacheSwitchError::ExistingDestination => {
            "that directory already exists; pick a path Aria can create".into()
        }
        DownloadCacheSwitchError::SymlinkDestination => {
            "that path is a symbolic link; pick a real directory".into()
        }
        DownloadCacheSwitchError::DestinationIo(err) => {
            format!("checking the destination: {err}")
        }
        DownloadCacheSwitchError::Migration(err) => format!("moving the cache: {err}"),
        DownloadCacheSwitchError::MaintenanceBusy => {
            "a download is in flight; try again once it finishes".into()
        }
        DownloadCacheSwitchError::DestructiveOperationsBlocked => {
            "the cache is locked while the project inventory is scanned".into()
        }
        DownloadCacheSwitchError::ProtectedPathOutsideRoot => {
            "a pinned file sits outside the cache root; restart Aria and try again".into()
        }
    }
}

/// Bytes in display units. Kept here because the dialog and the status
/// line must agree on the unit.
pub fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * MIB;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}
