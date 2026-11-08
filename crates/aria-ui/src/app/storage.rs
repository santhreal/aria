//! The download cache: size against quota, clearing, and relocation.
//!
//! `aria-cloud`'s `DownloadCache` had no UI surface, so a cache with a quota
//! nothing could read or change was the only thing standing between the session and
//! a full disk. One cache instance lives on `AppState` for the session, because
//! protected paths and the current root are session state: rebuilding it per call
//! would forget both.
//!
//! The cache's directory is the layout's `download` entry: the storage root's
//! `downloads` folder, or the `[storage.paths]` override. The config file is
//! `data_dir/config.toml`: `~/.aria/config.toml` by default, the profile's own
//! under `ARIA_DATA_DIR`, and a temporary directory in a test.

use std::path::{Path, PathBuf};

use aria_cloud::cache::{DownloadCache, DownloadCacheClear, DownloadCacheSwitchError};
use aria_core::count::counted;
use aria_storage::{CacheId, NeverCancelled, StorageLayout, relocate_cache};

use super::AppState;
use super::picker::{Ask, FileSpec};

const MIB: u64 = 1024 * 1024;

/// The refusal of Clear cache and Move storage… while a download holds the
/// cache.
const DOWNLOAD_HELD: &str = "A download is in flight. Clear and Move run once it finishes";

/// Build the session's cache from the startup layout and config.
///
/// The directory comes from [`session_layout`](super::caches::session_layout),
/// the same resolution the Caches panel shows, so a storage root or a
/// `download` override moves the writes and not only the row.
pub(super) fn cache_for(
    layout: &StorageLayout,
    settings: &aria_settings::Settings,
) -> DownloadCache {
    let root = layout
        .resolve(CacheId::Download)
        .expect("the download cache is a disk cache");
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

    /// Write `settings` to the config file at `path`.
    ///
    /// A save in force whose folder could not be written to disk
    /// ([`aria_settings::SaveError::Unsynced`]) is posted on the error line and
    /// returns `Ok`: the caller applies it as saved, and a cache move that
    /// commits with it does not move the data back under a config that already
    /// names the new path.
    pub(super) fn save_config(
        &mut self,
        path: &Path,
        settings: &aria_settings::Settings,
    ) -> Result<(), String> {
        match aria_settings::save(path, settings) {
            Err(err) if err.is_committed() => {
                self.report_error(String::from(err));
                Ok(())
            }
            saved => saved.map_err(String::from),
        }
    }

    /// Where downloaded assets live.
    pub fn download_cache_root(&self) -> PathBuf {
        self.download_cache.root()
    }

    /// Bytes currently cached, held on `CacheForm` and refreshed when
    /// Settings opens and whenever the download cache changes so a draw does
    /// not walk the cache directory per frame.
    pub fn download_cache_size_bytes(&self) -> u64 {
        self.caches.download_bytes
    }

    /// Re-measure [`Self::download_cache_size_bytes`] from the download cache
    /// directory on disk.
    pub fn refresh_download_cache_size(&mut self) {
        self.caches.download_bytes = self.download_cache.size_bytes();
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
        self.save_config(&path, &settings)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        self.download_cache.set_quota_bytes(mib.saturating_mul(MIB));
        self.download_cache.enforce_quota();
        self.refresh_cloud_settings();
        self.refresh_download_cache_size();
        self.status_message = Some(format!("Download quota set to {mib} MiB"));
        Ok(())
    }

    /// Why Clear cache and Move storage… cannot run, while they cannot.
    ///
    /// The app holds the cache's destructive-operation flag for one reason, a
    /// download in flight (`Self::hold_download_guard`), so every refusal and
    /// the Settings rows word the hold from here, before the cache is asked.
    pub fn download_cache_held(&self) -> Option<&'static str> {
        self.download_cache
            .destructive_operations_blocked()
            .then_some(DOWNLOAD_HELD)
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
        self.refresh_download_cache_size();
    }

    /// Delete every unpinned cached file.
    ///
    /// The status line names how many paths were kept, because a clear that silently
    /// leaves files behind looks like it failed, and one that silently deletes a file
    /// a project still needs looks like corruption.
    pub fn clear_download_cache(&mut self) -> Result<DownloadCacheClear, String> {
        if let Some(held) = self.download_cache_held() {
            return Err(held.into());
        }
        let cleared = self
            .download_cache
            .clear()
            .map_err(|err| format!("clearing the download cache: {err}"))?;
        self.refresh_download_cache_size();
        let removed = format!(
            "Cleared {}, {}",
            counted(cleared.removed_files, "file", "files"),
            format_bytes(cleared.removed_bytes)
        );
        self.status_message = Some(match cleared.retained_protected_files {
            0 => removed,
            kept => format!("{removed}; kept {kept} in use"),
        });
        Ok(cleared)
    }

    /// Move the storage root, taking every cache that follows it, and persist the
    /// new root.
    ///
    /// A cache follows the root unless `[storage.paths]` gives it a directory of
    /// its own. Every destination is checked before anything moves, so a refusal
    /// leaves the old root serving. The downloads move first, with their pins;
    /// the config is written next, and the downloads move back if it cannot be;
    /// the other caches follow one at a time. A cache whose move then fails stays
    /// where it was and is listed in the error, and is regenerated or downloaded
    /// again under the new root.
    pub fn relocate_storage_root(&mut self, root: impl AsRef<Path>) -> Result<(), String> {
        if let Some(held) = self.download_cache_held() {
            return Err(held.into());
        }
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(format!(
                "{} is not an absolute path; pick a full path for the storage root",
                root.display()
            ));
        }
        let before = self.storage_layout();
        if before.root() == root {
            return Err("the storage root is already there".into());
        }
        let after = layout_at(&before, root)?;
        let mut moves = Vec::new();
        for (id, from) in before.resolved_disk_paths() {
            if before.override_for(id).is_some() {
                continue;
            }
            let to = after.resolve(id).expect("a disk cache resolves");
            if to.exists() {
                return Err(format!(
                    "{} already exists; pick a root Aria can create its caches in",
                    to.display()
                ));
            }
            // The live download cache moves below even when it holds nothing:
            // it has to point at the new root either way.
            if id != CacheId::Download && from.exists() {
                moves.push((id, from, to));
            }
        }

        let download_from = self.download_cache_root();
        let download_to = before
            .override_for(CacheId::Download)
            .is_none()
            .then(|| after.resolve(CacheId::Download).expect("a disk cache"));
        if let Some(to) = &download_to {
            self.download_cache
                .switch_root(to, move_tree)
                .map_err(describe_switch_error)?;
        }
        if let Err(err) = self.write_storage_root(root) {
            if download_to.is_some()
                && let Err(back) = self.download_cache.switch_root(&download_from, move_tree)
            {
                return Err(format!(
                    "{err}; moving the downloads back failed too: {}",
                    describe_switch_error(back)
                ));
            }
            return Err(err);
        }
        let mut stayed = Vec::new();
        // A held connection would checkpoint into the file this move carries.
        self.analysis.index.release();
        for (id, from, to) in moves {
            if let Err(err) = relocate_cache(&from, &to, &NeverCancelled, |_| Ok(())) {
                stayed.push(format!(
                    "{} stayed in {}: {err}",
                    id.descriptor().label,
                    from.display()
                ));
            }
        }
        for id in CacheId::ALL {
            self.after_cache_change(id);
        }
        let moved = format!("Storage root is now {}", root.display());
        if stayed.is_empty() {
            self.status_message = Some(moved);
            Ok(())
        } else {
            Err(format!("{moved}, but {}", stayed.join("; ")))
        }
    }

    /// Record `root` as `[storage].root`.
    fn write_storage_root(&mut self, root: &Path) -> Result<(), String> {
        let path = self.config_path();
        let unchanged = |err: String| {
            format!(
                "{}: {err}; the storage root was not changed",
                path.display()
            )
        };
        let mut settings = aria_settings::load(&path).map_err(unchanged)?;
        settings.storage.root = Some(root.to_path_buf());
        self.save_config(&path, &settings).map_err(unchanged)?;
        self.refresh_cloud_settings();
        Ok(())
    }

    /// Move the download cache to `destination` with its files and pins, then
    /// record the override: **Move…** on the Downloads row.
    ///
    /// Through the live cache rather than a directory move under it, so the pins
    /// that keep a project's files from eviction move with the files.
    pub(super) fn relocate_download_cache(&mut self, destination: &Path) -> Result<(), String> {
        self.move_download_cache(destination, Some(destination.to_path_buf()))
    }

    /// Return the download cache to `default` with its files and pins, then drop
    /// the override: **Use default** on the Downloads row.
    pub(super) fn rehome_download_cache(&mut self, default: &Path) -> Result<(), String> {
        self.move_download_cache(default, None)
    }

    /// Move the live cache to `to` and write `recorded` as the `download`
    /// override. A config that cannot be written moves the files back.
    fn move_download_cache(&mut self, to: &Path, recorded: Option<PathBuf>) -> Result<(), String> {
        if let Some(held) = self.download_cache_held() {
            return Err(held.into());
        }
        let from = self.download_cache_root();
        self.download_cache
            .switch_root(to, move_tree)
            .map_err(describe_switch_error)?;
        if let Err(err) = self.write_override(CacheId::Download, recorded) {
            return Err(match self.download_cache.switch_root(&from, move_tree) {
                Ok(()) => err,
                Err(back) => format!(
                    "{err}; moving the downloads back failed too: {}",
                    describe_switch_error(back)
                ),
            });
        }
        self.after_cache_change(CacheId::Download);
        Ok(())
    }

    /// Ask for a folder, then move the storage root there: the Storage
    /// section's Move Storage button.
    pub fn pick_storage_root(&mut self) {
        if let Some(held) = self.download_cache_held() {
            self.report_error(held);
            return;
        }
        self.request_pick(
            Ask::Folder(FileSpec::new("Move Storage")),
            |state, answer| {
                let Some(picked) = answer.one() else {
                    return;
                };
                if let Err(err) = state.relocate_storage_root(&picked) {
                    state.report_error(err);
                }
            },
        );
    }
}

/// `before`'s overrides on a new `root`, or why the new root cannot hold them.
fn layout_at(before: &StorageLayout, root: &Path) -> Result<StorageLayout, String> {
    let refused = |err: aria_storage::StorageError| format!("{}: {err}", root.display());
    let mut after = StorageLayout::new(root).map_err(refused)?;
    for (id, path) in before.overrides() {
        after.set_override(*id, path.clone()).map_err(refused)?;
    }
    after.validate_filesystem().map_err(refused)?;
    Ok(after)
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
            "the destination must be an absolute path".into()
        }
        DownloadCacheSwitchError::FilesystemRootDestination => {
            "the destination cannot be a filesystem root; pick a directory inside it".into()
        }
        DownloadCacheSwitchError::SameDestination => "the downloads are already there".into(),
        DownloadCacheSwitchError::ExistingDestination => {
            "that directory already exists; pick a path Aria can create".into()
        }
        DownloadCacheSwitchError::SymlinkDestination => {
            "that path is a symbolic link; pick a real directory".into()
        }
        DownloadCacheSwitchError::DestinationIo(err) => {
            format!("checking the destination: {err}")
        }
        DownloadCacheSwitchError::Migration(err) => format!("moving the downloads: {err}"),
        DownloadCacheSwitchError::MaintenanceBusy => {
            "a download is in flight; try again once it finishes".into()
        }
        DownloadCacheSwitchError::DestructiveOperationsBlocked => DOWNLOAD_HELD.into(),
        DownloadCacheSwitchError::ProtectedPathOutsideRoot => {
            "a pinned file sits outside the cache root; restart Aria and try again".into()
        }
    }
}

/// Bytes in display units. Kept here because the dialog and the status
/// line must agree on the unit.
pub(crate) fn format_bytes(bytes: u64) -> String {
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

/// A download's progress detail: what arrived of what the file holds, as
/// `12.0 MiB of 1.5 GiB`, or what arrived when the size is unknown.
pub(crate) fn download_detail(read: u64, total: u64) -> String {
    if total == 0 {
        format_bytes(read)
    } else {
        format!("{} of {}", format_bytes(read), format_bytes(total))
    }
}
