//! Hot reloader for WebAssembly plugin packages on disk.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;
use aria_plugin::package::{COMPONENT_FILE, MANIFEST_FILE, Package, Tier, discover};
use rustc_hash::{FxHashMap, FxHasher};

use crate::{Limits, load, load_with_data_root};

/// Outcome of polling one WASM plugin package directory for changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// A package that was not mounted is now loaded and mounted.
    Mounted { id: String, dir: PathBuf },
    /// A mounted package whose `plugin.toml` or `plugin.wasm` changed was unmounted and remounted.
    Reloaded { id: String, dir: PathBuf },
    /// A mounted package whose directory was removed from disk was unmounted.
    Unmounted { id: String, dir: PathBuf },
    /// Loading or mounting a new or modified package failed.
    Failed {
        dir: PathBuf,
        id: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    manifest_hash: u64,
    manifest_len: u64,
    component_hash: u64,
    component_len: u64,
}

#[derive(Debug, Clone)]
struct TrackedPackage {
    id: Option<String>,
    fingerprint: Fingerprint,
}

/// Polls one or more plugin directories (or individual package directories) and
/// hot-reloads WASM packages when `plugin.toml` or `plugin.wasm` changes on disk.
#[derive(Debug)]
pub struct Reloader {
    watched: Vec<PathBuf>,
    limits: Limits,
    data_root: Option<PathBuf>,
    initial: FxHashMap<PathBuf, Fingerprint>,
    tracked: FxHashMap<PathBuf, TrackedPackage>,
}

impl Reloader {
    /// Create a reloader watching `dir` with `limits`.
    ///
    /// `dir` may be either a single package directory (holding `plugin.toml`) or a parent
    /// directory containing package subdirectories.
    pub fn new(dir: impl Into<PathBuf>, limits: Limits) -> Self {
        let mut reloader = Self {
            watched: Vec::new(),
            limits,
            data_root: None,
            initial: FxHashMap::default(),
            tracked: FxHashMap::default(),
        };
        reloader.watch(dir);
        reloader
    }

    /// Override the root directory under which package `data_dir` paths are resolved.
    #[must_use]
    pub fn with_data_root(mut self, data_root: impl Into<PathBuf>) -> Self {
        self.data_root = Some(data_root.into());
        self
    }

    /// Add another package directory or parent plugins directory to watch.
    pub fn watch(&mut self, dir: impl Into<PathBuf>) {
        let path = dir.into();
        for pkg_dir in candidate_package_dirs(&path) {
            if let Some(fp) = fingerprint_dir(&pkg_dir) {
                self.initial.entry(pkg_dir).or_insert(fp);
            }
        }
        if !self.watched.contains(&path) {
            self.watched.push(path);
        }
    }

    /// Poll all watched directories and unmount/remount any WASM packages whose
    /// `plugin.toml` or `plugin.wasm` changed since the previous poll.
    pub fn poll(&mut self, ctx: &PluginContext) -> Vec<ReloadOutcome> {
        let mut outcomes = Vec::new();
        let mut seen_dirs = Vec::new();

        for watched in &self.watched {
            for dir in candidate_package_dirs(watched) {
                if !seen_dirs.contains(&dir) {
                    seen_dirs.push(dir);
                }
            }
        }
        seen_dirs.sort();

        let removed_dirs: Vec<PathBuf> = self
            .tracked
            .keys()
            .filter(|dir| !seen_dirs.contains(dir) && !dir.join(MANIFEST_FILE).is_file())
            .cloned()
            .collect();

        for dir in removed_dirs {
            if let Some(prev) = self.tracked.remove(&dir)
                && let Some(id) = prev.id
            {
                if ctx.is_mounted(&id) {
                    let _ = ctx.unmount(&id);
                }
                outcomes.push(ReloadOutcome::Unmounted { id, dir });
            }
        }

        for dir in seen_dirs {
            let Some(current_fp) = fingerprint_dir(&dir) else {
                continue;
            };

            if let Some(prev) = self.tracked.get(&dir).cloned() {
                let is_mounted = prev.id.as_deref().is_some_and(|id| ctx.is_mounted(id));
                if prev.fingerprint == current_fp && (is_mounted || prev.id.is_none()) {
                    continue;
                }
                if let Some(prev_id) = &prev.id
                    && ctx.is_mounted(prev_id)
                {
                    let _ = ctx.unmount(prev_id);
                }
                match self.mount_package(&dir, ctx) {
                    Ok(Some(id)) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: Some(id.clone()),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Reloaded { id, dir });
                    }
                    Ok(None) => {}
                    Err((opt_id, reason)) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: opt_id.clone(),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Failed {
                            dir,
                            id: opt_id,
                            reason,
                        });
                    }
                }
                continue;
            }

            let initial_fp = self.initial.remove(&dir);
            let pkg = match Package::open(&dir) {
                Ok(pkg) => pkg,
                Err(err) => {
                    self.tracked.insert(
                        dir.clone(),
                        TrackedPackage {
                            id: None,
                            fingerprint: current_fp,
                        },
                    );
                    outcomes.push(ReloadOutcome::Failed {
                        dir,
                        id: None,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            if !matches!(pkg.tier, Tier::Wasm { .. }) {
                continue;
            }

            let id = pkg.manifest.id.clone();
            if ctx.is_mounted(&id) {
                if initial_fp == Some(current_fp) {
                    self.tracked.insert(
                        dir,
                        TrackedPackage {
                            id: Some(id),
                            fingerprint: current_fp,
                        },
                    );
                    continue;
                }
                let _ = ctx.unmount(&id);
                match self.mount_opened_package(&pkg, ctx) {
                    Ok(()) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: Some(id.clone()),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Reloaded { id, dir });
                    }
                    Err(reason) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: Some(id.clone()),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Failed {
                            dir,
                            id: Some(id),
                            reason,
                        });
                    }
                }
            } else {
                match self.mount_opened_package(&pkg, ctx) {
                    Ok(()) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: Some(id.clone()),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Mounted { id, dir });
                    }
                    Err(reason) => {
                        self.tracked.insert(
                            dir.clone(),
                            TrackedPackage {
                                id: Some(id.clone()),
                                fingerprint: current_fp,
                            },
                        );
                        outcomes.push(ReloadOutcome::Failed {
                            dir,
                            id: Some(id),
                            reason,
                        });
                    }
                }
            }
        }

        outcomes
    }

    fn mount_package(
        &self,
        dir: &Path,
        ctx: &PluginContext,
    ) -> Result<Option<String>, (Option<String>, String)> {
        let pkg = Package::open(dir).map_err(|err| (None, err.to_string()))?;
        if !matches!(pkg.tier, Tier::Wasm { .. }) {
            return Ok(None);
        }
        let id = pkg.manifest.id.clone();
        self.mount_opened_package(&pkg, ctx)
            .map(|()| Some(id.clone()))
            .map_err(|reason| (Some(id), reason))
    }

    fn mount_opened_package(&self, pkg: &Package, ctx: &PluginContext) -> Result<(), String> {
        let plugin = match &self.data_root {
            Some(root) => load_with_data_root(pkg, &self.limits, root.clone()),
            None => load(pkg, &self.limits),
        }
        .map_err(|err| err.to_string())?;
        ctx.mount(plugin.as_ref()).map_err(|err| err.to_string())
    }
}

fn candidate_package_dirs(root: &Path) -> Vec<PathBuf> {
    if root.join(MANIFEST_FILE).is_file() {
        return vec![root.to_path_buf()];
    }
    let mut dirs = Vec::new();
    for entry in discover(root) {
        if let Ok(pkg) = entry
            && matches!(pkg.tier, Tier::Wasm { .. })
        {
            dirs.push(pkg.dir);
        }
    }
    if let Ok(read_dir) = fs::read_dir(root) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(COMPONENT_FILE).is_file() && !dirs.contains(&path) {
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    dirs
}

fn fingerprint_dir(dir: &Path) -> Option<Fingerprint> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let component_path = dir.join(COMPONENT_FILE);
    if !manifest_path.is_file() || !component_path.is_file() {
        return None;
    }
    let (manifest_hash, manifest_len) = hash_file(&manifest_path).ok()?;
    let (component_hash, component_len) = hash_file(&component_path).ok()?;
    Some(Fingerprint {
        manifest_hash,
        manifest_len,
        component_hash,
        component_len,
    })
}

fn hash_file(path: &Path) -> std::io::Result<(u64, u64)> {
    let bytes = fs::read(path)?;
    let mut hasher = FxHasher::default();
    bytes.hash(&mut hasher);
    Ok((hasher.finish(), bytes.len() as u64))
}
