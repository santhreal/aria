//! The cache registry as the Caches panel shows it: what each cache is, where it lives,
//! how much it holds, and how to clear, move, or redirect it.
//!
//! `aria-storage` owned the whole model — the registered caches, their tiers, their
//! resolved paths, the walk that measures one, the clear that empties one, and the
//! transactional relocation that moves one — and none of it had a surface. The one
//! cache the dialog could reach was the download cache, through `aria-cloud`. So a
//! disk filling up with proxies, analysis state, or model weights was invisible, and
//! the only path override anything could write was the storage root.
//!
//! Three rules this module keeps:
//!
//! - **The registry is the list.** Rows come from `CacheId::ALL`, so a cache added to
//!   `aria-storage` appears here without an edit. A memory cache is listed as memory
//!   rather than hidden: "why is Aria using 3 GiB of RAM" is a question the same panel
//!   should answer.
//! - **Measuring is an action, not a render.** `measure_disk_usage` walks the tree. The
//!   download-cache row already pays that per render for one directory; doing it for
//!   every cache would make opening Settings a disk scan. Usage is measured when asked
//!   and held until asked again.
//! - **An override is validated before it is written.** `validate_filesystem` runs on
//!   the candidate layout first, so a path that would collide with another cache or is
//!   not usable is refused by name instead of being persisted and failing at the next
//!   download.
//! - **The row's path is the writer's path.** Every writer resolves its directory
//!   through [`session_layout`], so Clear, Move, and Redirect act on the bytes the
//!   app writes, and an override moves the writes and not only the row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aria_core::count::counted;
use aria_settings::Settings;
use aria_storage::{
    CacheId, CacheKind, CacheTier, NeverCancelled, StorageLayout, clear_cache, measure_disk_usage,
    relocate_cache,
};

use super::AppState;
use super::picker::{Ask, FileSpec};

/// One registry row as the dialog needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiCache {
    /// The stable key, used for element ids and for the config table.
    pub key: &'static str,
    pub label: &'static str,
    /// "Memory" or "Disk".
    pub kind: &'static str,
    /// What is lost by clearing: "Regenerated locally", "Downloaded again", or "Kept".
    pub tier: &'static str,
    /// Which tier is evicted first, lowest first, so the order is visible.
    pub evicted: u8,
    /// The eviction order as a phrase, from the same row as `evicted`.
    pub eviction_order: &'static str,
    /// The resolved directory, absent for a memory cache.
    pub path: Option<PathBuf>,
    /// Whether `path` comes from an override the layout accepted.
    ///
    /// Read from the layout, never from the settings table: an override the
    /// layout refused is not in force, and a row that claimed it was pointed
    /// at a directory nothing writes to.
    pub overridden: bool,
    /// Why a configured override is not in force, when one was configured and
    /// refused.
    ///
    /// `Some` here with `overridden == false` is the state that used to be
    /// invisible: `config.toml` names a path, the layout would not take it, and
    /// every write still lands under the storage root.
    pub override_error: Option<String>,
    /// Whether **Redirect…** applies. The download cache holds files open
    /// projects read, so it changes directory only by moving them.
    pub redirectable: bool,
    /// Measured bytes and files, present only after a measurement.
    pub usage: Option<(u64, u64)>,
}

impl UiCache {
    /// Whether this row has a directory to clear, move, or redirect.
    pub fn on_disk(&self) -> bool {
        self.path.is_some()
    }
}

/// What a measurement produced, held between renders.
#[derive(Debug, Default, Clone)]
pub struct CacheForm {
    /// Bytes and file counts by cache key. Empty until the first measurement.
    pub usage: BTreeMap<&'static str, (u64, u64)>,
    /// Whether a measurement has been taken this session.
    pub measured: bool,
    /// Bytes the download cache occupies on disk, refreshed when Settings
    /// opens and whenever the download cache is modified.
    pub download_bytes: u64,
    /// Paths in `import_recents` that are currently missing on disk, refreshed
    /// when the File menu opens and whenever the Import Recent list changes.
    pub missing_imports: std::sync::Arc<[PathBuf]>,
}

/// Eviction order for one tier: rank, lowest first, and the phrase that names it.
///
/// Both come out of one exhaustive match so they cannot disagree. They used to
/// be two mappings a crate apart — a rank here and a `0 | 1 | _` in the
/// settings panel — and the wildcard meant a fourth tier would have been
/// silently labeled "never evicted" while sorting third.
fn eviction(tier: CacheTier) -> (u8, &'static str) {
    match tier {
        CacheTier::Disposable => (0, "evicted first"),
        CacheTier::Redownloadable => (1, "evicted next"),
        CacheTier::UserData => (2, "never evicted"),
    }
}

fn tier_label(tier: CacheTier) -> &'static str {
    match tier {
        CacheTier::Disposable => "Regenerated locally",
        CacheTier::Redownloadable => "Downloaded again",
        CacheTier::UserData => "Your own data",
    }
}

/// The layout a session on `data_dir` resolves through, plus the configured
/// overrides it refused, by cache key.
///
/// The single definition: the live session reads it through
/// [`AppState::storage_layout_checked`], and the download cache is built from it
/// before the session exists. The root is the config's `[storage].root` when set
/// and `data_dir` otherwise, so a test never resolves into the real data tree.
/// Dropping a bad override is right, since the other caches still have to work,
/// and the refusal is returned so it is shown rather than silent.
pub(crate) fn session_layout(
    data_dir: &Path,
    settings: &Settings,
) -> (StorageLayout, BTreeMap<&'static str, String>) {
    // The configured root arrives absolute or not at all: a relative one is
    // refused while the config is loaded, and the loader records it, so
    // `storage_report` can show it. The profile root behind it is absolute
    // because `AppState::new` resolved it.
    let root = settings.storage.root.as_deref().unwrap_or(data_dir);
    let mut layout = StorageLayout::new(root).unwrap_or_else(|_| {
        StorageLayout::new(data_dir)
            .expect("AppState::new resolved the profile root to an absolute path")
    });
    let mut rejected = BTreeMap::new();
    for key in aria_settings::StoragePathOverrides::KEYS {
        let Some(path) = settings.storage.paths.get(key) else {
            continue;
        };
        if let Err(err) = layout.set_override_key(key, path) {
            rejected.insert(key, format!("{}: {err}", path.display()));
        }
    }
    (layout, rejected)
}

impl AppState {
    /// The storage layout this session resolves paths through.
    pub fn storage_layout(&self) -> StorageLayout {
        self.storage_layout_checked().0
    }

    /// The layout plus the configured overrides it refused, by cache key.
    ///
    /// One function builds the layout so the two answers cannot drift: whoever
    /// reports what is in force reads the same pass that decided it.
    pub fn storage_layout_checked(&self) -> (StorageLayout, BTreeMap<&'static str, String>) {
        session_layout(&self.data_dir, self.cloud_settings())
    }

    /// Point the writer of `id` at the directory the layout now resolves, and
    /// re-read what the session reports for it.
    ///
    /// Called after every clear, move, redirect, reset, and root move. The match
    /// has no wildcard, so a cache added to the registry does not compile until
    /// its writer is accounted for here.
    pub(super) fn after_cache_change(&mut self, id: CacheId) {
        match id {
            CacheId::RenderFiles => self.refresh_render_files(),
            CacheId::AiModels => self.refresh_models(),
            CacheId::Analysis => self.refresh_analysis_cache_size(),
            CacheId::Download => self.refresh_download_cache_size(),
            // Resolved through the layout at each use: nothing holds the path.
            CacheId::LibraryThumbnails | CacheId::Proxies | CacheId::Catalog | CacheId::Luts => {}
            CacheId::PreviewFrames | CacheId::TimelineFilmstrips | CacheId::TimelineWaveforms => {}
        }
    }

    /// Every registered cache, in registry order.
    pub fn cache_rows(&self) -> Vec<UiCache> {
        let (layout, rejected) = self.storage_layout_checked();
        CacheId::ALL
            .into_iter()
            .map(|id| {
                let descriptor = id.descriptor();
                let (evicted, eviction_order) = eviction(descriptor.tier);
                UiCache {
                    key: id.as_str(),
                    label: descriptor.label,
                    kind: match descriptor.kind {
                        CacheKind::Memory => "Memory",
                        CacheKind::Disk => "Disk",
                    },
                    tier: tier_label(descriptor.tier),
                    evicted,
                    eviction_order,
                    path: layout.resolve(id),
                    overridden: layout.override_for(id).is_some(),
                    override_error: rejected.get(id.as_str()).cloned(),
                    redirectable: id != CacheId::Download,
                    usage: self.caches.usage.get(id.as_str()).copied(),
                }
            })
            .collect()
    }

    /// Every resolved disk path, one per line, for a support report.
    ///
    /// Refused overrides are listed after the paths, then every value the
    /// config loader itself refused. A report that showed only where the caches
    /// resolve would look correct to someone whose config names a different
    /// directory entirely, which is exactly the situation that produces the
    /// support request.
    pub fn storage_report(&self) -> String {
        let (layout, rejected) = self.storage_layout_checked();
        let mut report = format!("storage root: {}\n", layout.root().display());
        for (id, path) in layout.resolved_disk_paths() {
            let marker = if layout.override_for(id).is_some() {
                " (override)"
            } else {
                ""
            };
            report.push_str(&format!("{}: {}{marker}\n", id.as_str(), path.display()));
        }
        for (key, reason) in &rejected {
            report.push_str(&format!("refused override for {key}: {reason}\n"));
        }
        for refusal in self.config_refusals() {
            report.push_str(&format!("refused in config.toml: {refusal}\n"));
        }
        report
    }

    /// Measure every disk cache and hold the result.
    ///
    /// Synchronous on purpose: this runs on request, and the walk is the
    /// requested answer. A cache directory that does not exist yet measures as
    /// zero rather than as an error — nothing has been written there.
    pub fn measure_caches(&mut self) -> Result<(), String> {
        let layout = self.storage_layout();
        let mut usage = BTreeMap::new();
        for (id, path) in layout.resolved_disk_paths() {
            if !path.exists() {
                usage.insert(id.as_str(), (0, 0));
                continue;
            }
            let measured = measure_disk_usage(&path, &NeverCancelled)
                .map_err(|err| format!("measuring {}: {err}", path.display()))?;
            usage.insert(id.as_str(), (measured.bytes, measured.files));
        }
        let total: u64 = usage.values().map(|(bytes, _)| *bytes).sum();
        self.caches.usage = usage;
        self.caches.measured = true;
        self.status_message = Some(format!(
            "Caches hold {}",
            super::storage::format_bytes(total)
        ));
        Ok(())
    }

    /// Empty one cache's directory.
    ///
    /// A memory cache is refused by name: nothing on disk belongs to it, and a button
    /// that silently did nothing would read as broken.
    pub fn clear_registered_cache(&mut self, key: &str) -> Result<(), String> {
        let id = CacheId::parse(key).map_err(|err| format!("{key}: {err}"))?;
        let Some(path) = self.storage_layout().resolve(id) else {
            return Err(format!(
                "{} lives in memory; there is nothing on disk to clear",
                id.descriptor().label
            ));
        };
        if id == CacheId::Download {
            // Through the live cache: a file an open project reads is pinned, and a
            // plain directory clear would delete it.
            self.clear_download_cache()?;
            self.caches.usage.remove(id.as_str());
            return Ok(());
        }
        if !path.exists() {
            self.status_message = Some(format!("{} is already empty", id.descriptor().label));
            return Ok(());
        }
        let report = clear_cache(&path, &NeverCancelled)
            .map_err(|err| format!("clearing {}: {err}", path.display()))?;
        // The measurement the panel is showing described the tree that just went away.
        self.caches.usage.insert(id.as_str(), (0, 0));
        self.status_message = Some(format!(
            "Cleared {}, {} from {}",
            counted(report.removed_files, "file", "files"),
            super::storage::format_bytes(report.removed_bytes),
            id.descriptor().label
        ));
        self.after_cache_change(id);
        Ok(())
    }

    /// Move one cache's contents to `destination` and persist the override.
    ///
    /// The move happens first and the config is written after it succeeds, so a failed
    /// move leaves the old path serving and the config still naming it. The reverse
    /// order would point the next launch at a directory the data never reached.
    pub fn relocate_registered_cache(
        &mut self,
        key: &str,
        destination: impl AsRef<Path>,
    ) -> Result<(), String> {
        let id = CacheId::parse(key).map_err(|err| format!("{key}: {err}"))?;
        let destination = destination.as_ref();
        let source = self.storage_layout().resolve(id).ok_or_else(|| {
            format!(
                "{} lives in memory; there is nothing on disk to move",
                id.descriptor().label
            )
        })?;
        self.validate_override(id, destination)?;
        if source == destination {
            self.write_override(id, Some(destination.to_path_buf()))?;
            self.status_message = Some(format!(
                "{} already resolves to {}",
                id.descriptor().label,
                destination.display()
            ));
            return Ok(());
        }
        if id == CacheId::Download {
            self.relocate_download_cache(destination)?;
            self.caches.usage.remove(id.as_str());
            self.status_message = Some(format!(
                "{} is now at {}",
                id.descriptor().label,
                destination.display()
            ));
            return Ok(());
        }
        if source.exists() {
            let target = destination.to_path_buf();
            let this = &mut *self;
            // `relocate_cache` calls this only once the destination is complete, and
            // rolls back if it fails: the config never names a path the data did not
            // reach.
            let report = relocate_cache(&source, destination, &NeverCancelled, |_| {
                this.write_override(id, Some(target))
            })
            .map_err(|err| {
                format!(
                    "moving {} to {}: {err}",
                    source.display(),
                    destination.display()
                )
            })?;
            self.caches
                .usage
                .insert(id.as_str(), (report.bytes, report.files));
        } else {
            // Nothing written yet, so there is nothing to carry: the move is the
            // new path alone.
            self.write_override(id, Some(destination.to_path_buf()))?;
            self.caches.usage.insert(id.as_str(), (0, 0));
        }
        self.status_message = Some(format!(
            "{} is now at {}",
            id.descriptor().label,
            destination.display()
        ));
        self.after_cache_change(id);
        Ok(())
    }

    /// Point one cache at `destination` without moving what is already there.
    ///
    /// Separate from relocation because the two are different intentions: an override
    /// aims a cache at a bigger disk from now on, a relocation carries the existing data
    /// with it.
    pub fn set_cache_override(
        &mut self,
        key: &str,
        destination: impl AsRef<Path>,
    ) -> Result<(), String> {
        let id = CacheId::parse(key).map_err(|err| format!("{key}: {err}"))?;
        let destination = destination.as_ref();
        if id == CacheId::Download {
            return Err(format!(
                "{} holds files open projects read, so it changes directory only by moving \
                 them; use Move… instead",
                id.descriptor().label
            ));
        }
        self.validate_override(id, destination)?;
        self.write_override(id, Some(destination.to_path_buf()))?;
        self.status_message = Some(format!(
            "{} resolves to {}",
            id.descriptor().label,
            destination.display()
        ));
        self.after_cache_change(id);
        Ok(())
    }

    /// Ask for a folder, then move cache `key` there with its contents: the
    /// cache row's Move button.
    pub fn pick_cache_move(&mut self, key: &'static str) {
        self.request_pick(
            Ask::Folder(FileSpec::new("Move Cache")),
            move |state, answer| {
                let Some(picked) = answer.one() else {
                    return;
                };
                if let Err(err) = state.relocate_registered_cache(key, &picked) {
                    state.report_error(err);
                }
            },
        );
    }

    /// Ask for a folder, then point cache `key` at it, leaving what it holds
    /// where it is: the cache row's Redirect button.
    pub fn pick_cache_redirect(&mut self, key: &'static str) {
        self.request_pick(
            Ask::Folder(FileSpec::new("Redirect Cache")),
            move |state, answer| {
                let Some(picked) = answer.one() else {
                    return;
                };
                if let Err(err) = state.set_cache_override(key, &picked) {
                    state.report_error(err);
                }
            },
        );
    }

    /// Drop one cache's override, returning it to the storage root.
    ///
    /// The message names the directory the cache lands back on, computed by
    /// removing the override from a candidate layout rather than by rebuilding
    /// the default path here. "Follows the storage root" is not an answer to
    /// "so where will my proxies be written now"; a cache's default is a
    /// registry-owned relative path and the layout is the only thing that knows
    /// it.
    pub fn clear_cache_override(&mut self, key: &str) -> Result<(), String> {
        let id = CacheId::parse(key).map_err(|err| format!("{key}: {err}"))?;
        let mut candidate = self.storage_layout();
        let removed = candidate.remove_override(id);
        let configured = self
            .cloud_settings()
            .storage
            .paths
            .get(id.as_str())
            .is_some();
        let landing = candidate.resolve(id);
        // A configured override the layout refused is still worth clearing: the
        // config keeps naming a path nothing writes to until it is gone.
        if removed.is_none() && !configured {
            self.status_message = Some(match landing {
                Some(path) => format!(
                    "{} already resolves to {}",
                    id.descriptor().label,
                    path.display()
                ),
                None => format!(
                    "{} lives in memory and has no path to reset",
                    id.descriptor().label
                ),
            });
            return Ok(());
        }
        match (id, removed, &landing) {
            // The live download cache is on the override: bring its files and pins
            // home with it, so the default it returns to holds what it held.
            (CacheId::Download, Some(_), Some(default))
                if *default != self.download_cache_root() =>
            {
                self.rehome_download_cache(default).map_err(|err| {
                    format!(
                        "{} cannot return to {}: {err}",
                        id.descriptor().label,
                        default.display()
                    )
                })?;
            }
            _ => self.write_override(id, None)?,
        }
        self.status_message = Some(match landing {
            Some(path) => format!(
                "{} now resolves to {}",
                id.descriptor().label,
                path.display()
            ),
            None => format!("{} follows the storage root again", id.descriptor().label),
        });
        self.after_cache_change(id);
        Ok(())
    }

    /// Refuse an override the layout could not live with, before it is persisted.
    pub(super) fn validate_override(&self, id: CacheId, destination: &Path) -> Result<(), String> {
        if !destination.is_absolute() {
            return Err(format!(
                "{} is not an absolute path; a cache override must name a full path",
                destination.display()
            ));
        }
        let mut candidate = self.storage_layout();
        candidate
            .set_override(id, destination.to_path_buf())
            .map_err(|err| format!("{}: {err}", destination.display()))?;
        candidate
            .validate_filesystem()
            .map_err(|err| format!("{}: {err}", destination.display()))
    }

    /// Persist one override into `[storage.paths]`.
    pub(super) fn write_override(
        &mut self,
        id: CacheId,
        value: Option<PathBuf>,
    ) -> Result<(), String> {
        let path = self.config_path();
        let mut settings = aria_settings::load(&path).map_err(|err| {
            format!(
                "{}: {err}; fix the file before changing a cache path",
                path.display()
            )
        })?;
        let slot = settings
            .storage
            .paths
            .slot(id.as_str())
            .ok_or_else(|| format!("{} has no path setting", id.as_str()))?;
        *slot = value;
        aria_settings::save(&path, &settings)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        self.refresh_cloud_settings();
        Ok(())
    }

    /// Whether `media` is currently decoded from a proxy rather than its own file.
    ///
    /// The renderer holds the substitution, not the project, so it is not in the
    /// saved file — but the projection carries it anyway: the pass that builds
    /// the pool rows reads it from the engine it already has (see
    /// `Snapshot::from_engine`). Reading the row is therefore the same answer as
    /// asking the session, without a lock per row per frame.
    pub fn media_has_proxy(&self, media: aria_models::MediaId) -> bool {
        self.store
            .media
            .iter()
            .any(|item| item.id == media.raw() && item.proxied)
    }

    /// Return one pool item to decoding its own file.
    ///
    /// The proxy cache could be emptied from Settings, but a single item could not
    /// be un-proxied: the substitution outlives the file on disk, so a stale or
    /// wrong proxy meant a lane that previewed the wrong picture with no way back
    /// short of reopening the project. Inert on an item with no proxy, so the row
    /// can be offered without asking first.
    pub fn clear_media_proxy(&mut self, media: aria_models::MediaId) {
        {
            let mut engine = self.engine.lock();
            if engine.media_proxy(media).is_none() {
                return;
            }
            engine.clear_media_proxy(media);
        }
        self.status_message = Some("Decoding from the original file".into());
        // The projection does not carry the substitution, but the pool pictures and
        // the open decoders were rendered through it; syncing drops what the
        // renderer no longer stands behind.
        self.sync_from_engine();
    }

    /// Return every pool item to its own file.
    ///
    /// One item at a time through the engine rather than the renderer's own
    /// `clear_proxies`, because the engine is the only handle the UI holds and the
    /// set of proxied items is exactly the pool: the loop cannot miss one that a
    /// project references.
    pub fn clear_all_media_proxies(&mut self) {
        let ids: Vec<u64> = self.store.media.iter().map(|item| item.id).collect();
        let mut cleared = 0_usize;
        {
            let mut engine = self.engine.lock();
            for id in ids {
                let media = aria_models::MediaId::from_raw(id);
                if engine.media_proxy(media).is_some() {
                    engine.clear_media_proxy(media);
                    cleared += 1;
                }
            }
        }
        if cleared == 0 {
            self.status_message = Some("No proxies in use".into());
            return;
        }
        self.status_message = Some(format!("Cleared {cleared} proxies"));
        self.sync_from_engine();
    }
}
