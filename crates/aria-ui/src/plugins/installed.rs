//! Installed plugin packages: the folders under `<data dir>/plugins`, and the
//! one development package `aria plugin dev` names.
//!
//! Every tier mounts through [`InstalledPlugins::mount`]: a native package from
//! a fresh copy of its library, a WASM package in its own instance, a data
//! package from its declared WGSL. A package with code that lists activation
//! events without `startup` is deferred instead, and mounts the same way on the
//! first of them. Enable, disable, reload, install and uninstall all go through
//! the same mount, deferral and unmount, so the Plugins list and the registry
//! cannot disagree about what is running.
//!
//! Disabling persists: the ids are listed in `<plugins>/.disabled`, one per
//! line, and a disabled package is read but not mounted at the next start.
//!
//! A WASM or data package reloads when its files change on disk. The
//! development package reloads on a change whatever its tier, and wins over an
//! installed package with the same id.
//!
//! WASM packages run under the fuel and memory limits in the config's
//! `[plugin_limits]` table, read when the folder is opened.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use aria_plugin::package::{self, DataPlugin, Package, PackageError, Staged, Tier};
use aria_plugin::{MountFailure, Plugin, PluginContext, PluginManifest};
use aria_plugin_wasm::Limits;
use aria_settings::PluginLimitSettings;

use super::external_plugin_dir;

/// The file in the plugin folder that lists disabled package ids. Hidden, so
/// discovery passes it over.
const DISABLED_FILE: &str = ".disabled";

/// The folder under the data dir that holds the copy of each native library
/// loaded this session.
const LIBRARY_CACHE: &str = "plugin-cache";

/// How long a changed file must stay unchanged before its package reloads, so
/// a file still being written is not read half way.
const SETTLE: Duration = Duration::from_millis(300);

const MIB: usize = 1024 * 1024;

/// The WASM host limits `settings` sets. The call deadline keeps its default.
pub fn wasm_limits(settings: PluginLimitSettings) -> Limits {
    Limits {
        fuel_per_call: settings.wasm_fuel_per_call,
        memory_bytes: usize::try_from(settings.wasm_memory_mib)
            .map_or(usize::MAX, |mib| mib.saturating_mul(MIB)),
        ..Limits::default()
    }
}

/// The WASM limits in the config under `data_dir`, or the defaults when the
/// config does not read.
fn configured_limits(data_dir: &Path) -> PluginLimitSettings {
    match aria_settings::load(&aria_settings::config_in(data_dir)) {
        Ok(settings) => settings.plugin_limits,
        Err(err) => {
            tracing::warn!(error = %err, "plugin limits: using the defaults");
            PluginLimitSettings::default()
        }
    }
}

/// The development package for this process, set once by the binary.
static DEVELOPING: OnceLock<PathBuf> = OnceLock::new();

/// Load the package in `dir` beside the installed ones for the life of the
/// process, over an installed package with the same id, and reload it when
/// its files change. Called once, before the first [`InstalledPlugins::open`].
pub fn develop(dir: PathBuf) -> Result<(), String> {
    DEVELOPING.set(dir).map_err(|dir| {
        format!(
            "{} cannot be developed: a development package is already set",
            dir.display()
        )
    })
}

/// The packages under one plugin folder, as last read, and which are disabled.
#[derive(Debug)]
pub struct InstalledPlugins {
    dir: PathBuf,
    data_root: PathBuf,
    limits: PluginLimitSettings,
    /// The development package directory, outside `dir`.
    dev: Option<PathBuf>,
    packages: Vec<Result<Package, PackageError>>,
    disabled: BTreeSet<String>,
    /// Each package directory's files as last mounted or read.
    stamps: BTreeMap<PathBuf, Stamp>,
}

/// A package that reloaded because its files changed, and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reloaded {
    /// The package id, or its folder name when it does not read.
    pub id: String,
    pub name: String,
    pub result: Result<(), String>,
}

/// The size and modification time of each file a package mounts from: its
/// manifest, its component, this platform's library, each declared shader and
/// each declared texture image. `None` for a file that is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp(Vec<Option<(u64, SystemTime)>>);

impl Stamp {
    fn of(dir: &Path, package: Option<&Package>) -> Self {
        let mut files = vec![
            dir.join(package::MANIFEST_FILE),
            dir.join(package::COMPONENT_FILE),
            dir.join(package::LIBRARY_DIR).join(package::library_name()),
        ];
        if let Some(package) = package {
            let manifest = &package.manifest;
            let declared = manifest
                .effects
                .iter()
                .chain(&manifest.generators)
                .flat_map(|effect| effect.passes.iter().map(|pass| &pass.file))
                .chain(
                    manifest
                        .transitions
                        .iter()
                        .map(|transition| &transition.shader),
                )
                .chain(manifest.textures.iter().map(|texture| &texture.file));
            files.extend(declared.map(|file| dir.join(file)));
        }
        Self(
            files
                .iter()
                .map(|file| {
                    let meta = fs::metadata(file).ok()?;
                    Some((meta.len(), meta.modified().ok()?))
                })
                .collect(),
        )
    }

    /// Whether a file changed less than [`SETTLE`] before `now`. A time after
    /// `now`, from a clock that runs ahead, counts as settled: waiting on it
    /// could last until the clocks agree.
    fn settling(&self, now: SystemTime) -> bool {
        self.0
            .iter()
            .flatten()
            .any(|(_, modified)| now.duration_since(*modified).is_ok_and(|age| age < SETTLE))
    }
}

/// The package directory a read error is reported under.
fn error_dir(err: &PackageError) -> &Path {
    match err.path.file_name() {
        Some(name) if name == package::MANIFEST_FILE => err.path.parent().unwrap_or(&err.path),
        _ => &err.path,
    }
}

/// The directory of a package entry, and the package when it read.
fn entry_dir(entry: &Result<Package, PackageError>) -> (&Path, Option<&Package>) {
    match entry {
        Ok(package) => (&package.dir, Some(package)),
        Err(err) => (error_dir(err), None),
    }
}

/// Every package under `dir`, then the development package in place of an
/// installed one with its id.
fn read_packages(dir: &Path, dev: Option<&Path>) -> Vec<Result<Package, PackageError>> {
    let mut packages = package::discover(dir);
    if let Some(dev) = dev {
        let developed = Package::open(dev);
        if let Ok(package) = &developed {
            packages.retain(|entry| {
                !matches!(entry, Ok(installed) if installed.manifest.id == package.manifest.id)
            });
        }
        packages.push(developed);
    }
    packages
}

impl InstalledPlugins {
    /// Read the packages, the disabled list and the WASM limits under
    /// `data_dir`, with the development package set by [`develop`]. Nothing is
    /// mounted.
    pub fn open(data_dir: &Path) -> Self {
        Self::open_developing(data_dir, DEVELOPING.get().cloned())
    }

    /// [`Self::open`] with `dev` as the development package.
    pub(crate) fn open_developing(data_dir: &Path, dev: Option<PathBuf>) -> Self {
        let dir = external_plugin_dir(data_dir);
        let disabled = fs::read_to_string(dir.join(DISABLED_FILE))
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let mut installed = Self {
            dir,
            data_root: data_dir.to_owned(),
            limits: configured_limits(data_dir),
            dev,
            packages: Vec::new(),
            disabled,
            stamps: BTreeMap::new(),
        };
        installed.rescan();
        installed
    }

    /// The plugin folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The limits a WASM package mounts under.
    pub fn limits(&self) -> PluginLimitSettings {
        self.limits
    }

    /// Mount WASM packages under `limits` from now on, remount the mounted
    /// ones so they run under them, and defer the waiting ones again so they
    /// load under them. Returns how each remount went, and each waiting
    /// package that could not be deferred again.
    pub fn set_limits(
        &mut self,
        ctx: &PluginContext,
        limits: PluginLimitSettings,
    ) -> Vec<Reloaded> {
        if self.limits == limits {
            return Vec::new();
        }
        self.limits = limits;
        let affected: Vec<Package> = self
            .packages
            .iter()
            .filter_map(|entry| entry.as_ref().ok())
            .filter(|package| {
                matches!(package.tier, Tier::Wasm { .. })
                    && (ctx.is_mounted(&package.manifest.id)
                        || ctx.is_waiting(&package.manifest.id))
            })
            .cloned()
            .collect();
        affected
            .into_iter()
            .filter_map(|package| {
                let result = if ctx.is_waiting(&package.manifest.id) {
                    match self.defer(ctx, &package) {
                        Ok(()) => return None,
                        Err(reason) => Err(reason),
                    }
                } else {
                    ctx.unmount(&package.manifest.id)
                        .map_err(|err| err.to_string())
                        .and_then(|()| self.mount(ctx, &package))
                };
                Some(Reloaded {
                    id: package.manifest.id,
                    name: package.manifest.name,
                    result,
                })
            })
            .collect()
    }

    /// Every package folder, in path order, then the development package, with
    /// the reason for one that did not read.
    pub fn packages(&self) -> &[Result<Package, PackageError>] {
        &self.packages
    }

    /// The installed or development package with `id`.
    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages
            .iter()
            .filter_map(|entry| entry.as_ref().ok())
            .find(|package| package.manifest.id == id)
    }

    /// Whether the package with `id` is the development package.
    pub fn is_development(&self, id: &str) -> bool {
        self.dev
            .as_deref()
            .is_some_and(|dev| self.package(id).is_some_and(|package| package.dir == dev))
    }

    /// Whether the package with `id` is disabled.
    pub fn is_disabled(&self, id: &str) -> bool {
        self.disabled.contains(id)
    }

    /// Read the plugin folder and the development package again. A package
    /// read for the first time is stamped as it is now; the others keep the
    /// stamp of their last mount, so a change not yet reloaded stays pending.
    pub fn rescan(&mut self) {
        self.packages = read_packages(&self.dir, self.dev.as_deref());
        for entry in &self.packages {
            let (dir, package) = entry_dir(entry);
            self.stamps
                .entry(dir.to_owned())
                .or_insert_with(|| Stamp::of(dir, package));
        }
    }

    /// Mount or defer every package that is not disabled, recording a package
    /// that would not read, load or defer in the context's failures. Returns
    /// the failures of this pass.
    ///
    /// A package that waits is deferred until its first
    /// activation event; the rest mount in the order their requirements set.
    /// Copies of native libraries left by an earlier session are removed first.
    pub fn mount_enabled(&self, ctx: &PluginContext) -> Vec<MountFailure> {
        // A library a running session still maps cannot be removed on
        // Windows; that copy stays until a start finds it unused.
        let _ = fs::remove_dir_all(self.data_root.join(LIBRARY_CACHE));
        let mut failures = Vec::new();
        let mut enabled: Vec<&Package> = Vec::with_capacity(self.packages.len());
        for entry in &self.packages {
            match entry {
                Ok(package) if self.disabled.contains(&package.manifest.id) => {}
                Ok(package) => enabled.push(package),
                Err(err) => {
                    let name = folder_name(&err.path);
                    failures.push(ctx.record_external_failure(
                        name.clone(),
                        name,
                        err.reason.clone(),
                    ));
                }
            }
        }
        let now = self.mounting_now();
        let manifests: Vec<PluginManifest> = enabled
            .iter()
            .map(|package| package.plugin_manifest())
            .collect();
        // A cycle mounts in folder order; each mount on the cycle fails naming
        // the requirement it lacks.
        let order = aria_plugin::load_order(&manifests).unwrap_or_else(|_| {
            manifests
                .iter()
                .map(|manifest| manifest.id.as_ref())
                .collect()
        });
        for id in order {
            let Some(package) = enabled.iter().find(|package| package.manifest.id == id) else {
                continue;
            };
            let result = if now.contains(id) {
                self.mount(ctx, package)
            } else {
                self.defer(ctx, package)
            };
            if let Err(reason) = result {
                failures.push(MountFailure {
                    id: package.manifest.id.clone(),
                    name: package.manifest.name.clone(),
                    reason,
                });
            }
        }
        failures
    }

    /// The ids of the enabled packages that mount at start: the development
    /// package, each that
    /// [loads at start](aria_plugin::package::PackageManifest::loads_at_start)
    /// or holds no code, and every enabled package one of them requires.
    fn mounting_now(&self) -> BTreeSet<&str> {
        let enabled: Vec<&Package> = self
            .packages
            .iter()
            .filter_map(|entry| entry.as_ref().ok())
            .filter(|package| !self.disabled.contains(&package.manifest.id))
            .collect();
        let mut now: Vec<&str> = enabled
            .iter()
            .copied()
            .filter(|package| {
                package.manifest.loads_at_start()
                    || package.tier == Tier::Data
                    || self.dev.as_deref() == Some(package.dir.as_path())
            })
            .map(|package| package.manifest.id.as_str())
            .collect();
        let mut next = 0;
        while let Some(id) = now.get(next).copied() {
            let required = enabled
                .iter()
                .copied()
                .find(|package| package.manifest.id == id);
            if let Some(package) = required {
                for requirement in &package.manifest.requires {
                    if !now.contains(&requirement.id.as_str())
                        && enabled
                            .iter()
                            .any(|held| held.manifest.id == requirement.id)
                    {
                        now.push(&requirement.id);
                    }
                }
            }
            next += 1;
        }
        now.into_iter().collect()
    }

    /// Whether `package` waits for its first activation event rather than
    /// mounting: it lists events without `startup`, holds code, is not the
    /// development package, and no package that mounts at start requires it.
    pub(crate) fn waits(&self, package: &Package) -> bool {
        !self.mounting_now().contains(package.manifest.id.as_str())
    }

    /// Mount `package` now, or defer it when it [waits](Self::waits).
    fn start(&self, ctx: &PluginContext, package: &Package) -> Result<(), String> {
        if self.waits(package) {
            self.defer(ctx, package)
        } else {
            self.mount(ctx, package)
        }
    }

    /// Hold `package` until its first activation event, then mount it as
    /// [`Self::mount`] would now. A package that cannot be deferred is
    /// recorded in the context's failures as a failed mount is.
    fn defer(&self, ctx: &PluginContext, package: &Package) -> Result<(), String> {
        let loader = self.loader();
        let deferred = package.clone();
        ctx.defer_package(package, move |ctx| loader.mount(ctx, &deferred))
            .inspect_err(|reason| {
                ctx.record_external_failure(
                    package.manifest.id.clone(),
                    package.manifest.name.clone(),
                    reason.clone(),
                );
            })
    }

    /// Load `package` by its tier and mount it. A load that fails is recorded
    /// in the context's failures as a failed mount is.
    pub fn mount(&self, ctx: &PluginContext, package: &Package) -> Result<(), String> {
        self.loader().mount(ctx, package)
    }

    fn loader(&self) -> Loader {
        Loader {
            data_root: self.data_root.clone(),
            limits: self.limits,
        }
    }

    /// Enable or disable the package with `id`, mounting, deferring or
    /// unmounting it, and persist the choice. Returns whether anything
    /// changed. An enabled package mounts, or waits for its first activation
    /// event, as it is on disk now, with what changed while it was disabled.
    pub fn set_enabled(
        &mut self,
        ctx: &PluginContext,
        id: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        if self.package(id).is_none() {
            return Err(format!("no installed plugin is {id}"));
        }
        let changed = if enabled {
            self.disabled.remove(id)
        } else {
            self.disabled.insert(id.to_owned())
        };
        if changed {
            self.save_disabled()?;
        }
        let mounted = ctx.is_mounted(id);
        if enabled && !mounted && !ctx.is_waiting(id) {
            ctx.clear_failure(id);
            self.rescan();
            let package = self
                .package(id)
                .cloned()
                .ok_or_else(|| format!("{id} is no longer in {}", self.dir.display()))?;
            self.stamp(&package);
            self.start(ctx, &package)?;
            return Ok(true);
        }
        if !enabled {
            let waited = ctx.cancel_activation(id);
            if mounted {
                ctx.unmount(id).map_err(|err| err.to_string())?;
            }
            // A disabled plugin is not failing; its old reason would read as
            // though it were.
            return Ok(ctx.clear_failure(id) || mounted || waited || changed);
        }
        Ok(changed)
    }

    /// Unmount the package with `id`, read it from disk again, and mount it:
    /// at once when it was mounted, otherwise as at start, so a package that
    /// waits for an activation event waits again.
    pub fn reload(&mut self, ctx: &PluginContext, id: &str) -> Result<(), String> {
        if self.is_disabled(id) {
            return Err(format!("{id} is disabled; enable it to load it"));
        }
        let was_mounted = ctx.is_mounted(id);
        if was_mounted {
            ctx.unmount(id).map_err(|err| err.to_string())?;
        }
        ctx.cancel_activation(id);
        ctx.clear_failure(id);
        self.rescan();
        let package = self
            .package(id)
            .cloned()
            .ok_or_else(|| format!("{id} is no longer in {}", self.dir.display()))?;
        self.stamp(&package);
        if was_mounted {
            self.mount(ctx, &package)
        } else {
            self.start(ctx, &package)
        }
    }

    /// Reload every package whose files changed since it was last mounted:
    /// the development package of any tier, and enabled WASM and data
    /// packages. A native package outside development keeps the library it
    /// loaded until it is reloaded by hand. A change younger than `SETTLE`
    /// at `now` waits for a later call, so a file still being written is not
    /// read.
    pub fn reload_changed(&mut self, ctx: &PluginContext, now: SystemTime) -> Vec<Reloaded> {
        let changed: Vec<(PathBuf, String)> = self
            .packages
            .iter()
            .filter_map(|entry| {
                let (dir, package) = entry_dir(entry);
                if let Some(package) = package {
                    let native = matches!(package.tier, Tier::Native { .. });
                    if self.disabled.contains(&package.manifest.id)
                        || (native && self.dev.as_deref() != Some(dir))
                    {
                        return None;
                    }
                }
                let stamp = Stamp::of(dir, package);
                if self.stamps.get(dir) == Some(&stamp) || stamp.settling(now) {
                    return None;
                }
                let key = package.map_or_else(|| folder_name(dir), |p| p.manifest.id.clone());
                Some((dir.to_owned(), key))
            })
            .collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let mut reloaded = Vec::with_capacity(changed.len());
        let mut unmounted = Vec::with_capacity(changed.len());
        for (dir, key) in &changed {
            let was_mounted = ctx.is_mounted(key);
            let result = if was_mounted {
                ctx.unmount(key).map_err(|err| err.to_string())
            } else {
                Ok(())
            };
            ctx.cancel_activation(key);
            ctx.clear_failure(key);
            unmounted.push((dir, key, was_mounted, result));
        }
        self.rescan();
        for (dir, key, was_mounted, unmount) in unmounted {
            let now_read = self
                .packages
                .iter()
                .find(|entry| entry_dir(entry).0 == dir.as_path());
            let outcome = match (unmount, now_read) {
                (Err(reason), _) => Reloaded {
                    id: key.clone(),
                    name: key.clone(),
                    result: Err(format!("did not unmount: {reason}")),
                },
                (Ok(()), Some(Ok(package))) => {
                    let package = package.clone();
                    let native = matches!(package.tier, Tier::Native { .. });
                    let result = if self.is_disabled(&package.manifest.id) {
                        Err(format!("{} is disabled", package.manifest.id))
                    } else if native && self.dev.as_deref() != Some(dir.as_path()) {
                        Err(
                            "a native package outside development loads only when it is \
                             reloaded from the Plugins list"
                                .to_owned(),
                        )
                    } else if was_mounted {
                        self.mount(ctx, &package)
                    } else {
                        self.start(ctx, &package)
                    };
                    self.stamp(&package);
                    Reloaded {
                        id: package.manifest.id,
                        name: package.manifest.name,
                        result,
                    }
                }
                (Ok(()), Some(Err(err))) => {
                    let name = folder_name(dir);
                    ctx.record_external_failure(name.clone(), name.clone(), err.reason.clone());
                    let reason = err.reason.clone();
                    self.stamps.insert(dir.clone(), Stamp::of(dir, None));
                    Reloaded {
                        id: name.clone(),
                        name,
                        result: Err(reason),
                    }
                }
                (Ok(()), None) => {
                    self.stamps.remove(dir.as_path());
                    Reloaded {
                        id: key.clone(),
                        name: key.clone(),
                        result: Err(format!("{} was removed", dir.display())),
                    }
                }
            };
            reloaded.push(outcome);
        }
        reloaded
    }

    /// Record `package`'s files as they are now: what its mount reads.
    fn stamp(&mut self, package: &Package) {
        self.stamps
            .insert(package.dir.clone(), Stamp::of(&package.dir, Some(package)));
    }

    /// Unpack `archive` beside the installed packages for the caller to
    /// confirm.
    pub fn stage(&self, archive: &Path) -> Result<Staged, PackageError> {
        package::stage(archive, &self.dir)
    }

    /// Install a staged package: unmount or stop deferring the version it
    /// replaces, move it into place, enable it and mount it: at once when the
    /// version it replaces was mounted, otherwise as at start. When the move
    /// fails the previous version is mounted or deferred again.
    pub fn commit(&mut self, ctx: &PluginContext, staged: Staged) -> Result<Package, String> {
        let id = staged.package().manifest.id.clone();
        if self.is_development(&id) {
            return Err(format!(
                "{id} is loaded for development from {}; end `aria plugin dev` to install it",
                self.dev.as_deref().unwrap_or(&self.dir).display()
            ));
        }
        let was_mounted = ctx.is_mounted(&id);
        if was_mounted {
            ctx.unmount(&id).map_err(|err| err.to_string())?;
        }
        let was_waiting = ctx.cancel_activation(&id);
        let installed = match staged.commit() {
            Ok(installed) => installed,
            Err(err) => {
                if let Some(previous) = self.package(&id).cloned() {
                    if was_mounted {
                        let _ = self.mount(ctx, &previous);
                    } else if was_waiting {
                        let _ = self.defer(ctx, &previous);
                    }
                }
                return Err(err.to_string());
            }
        };
        if self.disabled.remove(&id) {
            self.save_disabled()?;
        }
        ctx.clear_failure(&id);
        self.rescan();
        self.stamp(&installed);
        if was_mounted {
            self.mount(ctx, &installed)?;
        } else {
            self.start(ctx, &installed)?;
        }
        Ok(installed)
    }

    /// Unmount or stop deferring the package with `id`, delete its folder, and
    /// drop its log lines. Its storage under `plugin-data` stays.
    pub fn uninstall(&mut self, ctx: &PluginContext, id: &str) -> Result<PathBuf, String> {
        if self.is_development(id) {
            return Err(format!(
                "{id} is loaded for development, not installed; it has nothing to uninstall"
            ));
        }
        ctx.cancel_activation(id);
        if ctx.is_mounted(id) {
            ctx.unmount(id).map_err(|err| err.to_string())?;
        }
        let removed = package::uninstall(&self.dir, id).map_err(|err| err.to_string())?;
        ctx.clear_failure(id);
        ctx.clear_logs(id);
        if self.disabled.remove(id) {
            self.save_disabled()?;
        }
        self.stamps.remove(removed.as_path());
        self.rescan();
        Ok(removed)
    }

    fn save_disabled(&self) -> Result<(), String> {
        let path = self.dir.join(DISABLED_FILE);
        let mut text = String::new();
        for id in &self.disabled {
            text.push_str(id);
            text.push('\n');
        }
        let partial = self.dir.join(".disabled.partial");
        fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(&partial, text))
            .and_then(|()| fs::rename(&partial, &path))
            .map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// What mounts a package by its tier: the data dir its storage and library
/// copies go under, and the WASM limits. A deferred package holds a copy, so
/// it loads as it would have when it was deferred.
#[derive(Debug, Clone)]
struct Loader {
    data_root: PathBuf,
    limits: PluginLimitSettings,
}

impl Loader {
    /// Load `package` by its tier and mount it. A load that fails is recorded
    /// in the context's failures as a failed mount is.
    fn mount(&self, ctx: &PluginContext, package: &Package) -> Result<(), String> {
        let loaded: Result<Box<dyn Plugin>, String> = match &package.tier {
            Tier::Native { library } => self.copy_library(package, library).and_then(|copy| {
                aria_plugin::external::load_library(package, &copy, Some(self.data_root.as_path()))
            }),
            Tier::Wasm { .. } => {
                let limits = wasm_limits(self.limits);
                aria_plugin_wasm::load_with_data_root(package, &limits, self.data_root.clone())
                    .map_err(|err| err.to_string())
            }
            Tier::Data => Ok(Box::new(DataPlugin::new(package.clone()))),
        };
        match loaded {
            Ok(plugin) => ctx.mount(plugin.as_ref()).map_err(|err| err.to_string()),
            Err(reason) => {
                ctx.record_external_failure(
                    package.manifest.id.clone(),
                    package.manifest.name.clone(),
                    reason.clone(),
                );
                Err(reason)
            }
        }
    }

    /// Copy a native library to a path no earlier load used, so a reload maps
    /// the library on disk now rather than the one the process already holds,
    /// and the installed file is never held open.
    ///
    /// Earlier copies of the same package from this process are deleted
    /// first. A mapped library stays mapped once its file is deleted; on
    /// Windows the delete fails while it is mapped and the copy waits for the
    /// next start.
    fn copy_library(&self, package: &Package, library: &Path) -> Result<PathBuf, String> {
        static LOADS: AtomicU64 = AtomicU64::new(0);
        let cache = self.data_root.join(LIBRARY_CACHE);
        fs::create_dir_all(&cache).map_err(|err| format!("{}: {err}", cache.display()))?;
        let prefix = format!("{}-{}-", package.manifest.id, std::process::id());
        if let Ok(entries) = fs::read_dir(&cache) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let copy = cache.join(format!(
            "{prefix}{}-{}",
            LOADS.fetch_add(1, Ordering::Relaxed),
            package::library_name()
        ));
        fs::copy(library, &copy).map_err(|err| format!("{}: {err}", library.display()))?;
        Ok(copy)
    }
}

/// The folder or file name a package error is reported under, when there is
/// no manifest to name it. An error in `plugin.toml` names its package folder.
pub(crate) fn folder_name(path: &Path) -> String {
    let named = match path.file_name() {
        Some(name) if name == aria_plugin::package::MANIFEST_FILE => path.parent().unwrap_or(path),
        _ => path,
    };
    named
        .file_name()
        .unwrap_or(named.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
#[path = "installed_tests.rs"]
mod tests;
