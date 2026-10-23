//! Installing, reloading and removing plugin packages from the editor.
//!
//! An `.ariaplugin` reaches [`AppState::install_plugin_archives`] from a drop,
//! a path on the command line, or File > Install Plugin. Each archive is
//! unpacked beside the installed packages and shown in the install prompt with
//! its name, publisher, version, tier and the permissions it asks for. Nothing
//! in it runs until the prompt is confirmed. An update that asks for nothing
//! the installed version was not granted installs without the prompt.

use std::collections::{BTreeMap, VecDeque};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use aria_core::count::counted;
use aria_plugin::package::{self, Staged, Tier};
use aria_settings::PluginLimitSettings;

use super::picker::{Answer, Ask, FileSpec};
use super::{AppState, DialogKind};
use crate::plugins::installed::{InstalledPlugins, Reloaded};

/// The installed packages and the archives waiting to be installed.
#[derive(Debug)]
pub struct PluginInstalls {
    pub(crate) installed: InstalledPlugins,
    /// The archive the prompt is showing, unpacked.
    pending: Option<Staged>,
    /// Archives dropped together, shown one prompt at a time.
    queue: VecDeque<PathBuf>,
    /// The dialog the prompt opened over, shown again when it closes: an
    /// archive dropped on the Plugins section returns to it.
    return_to: DialogKind,
    /// The Log toggle's choice per plugin id in the Plugins list. A plugin
    /// without one shows its log when it has an error or a fault.
    pub(crate) log_open: BTreeMap<String, bool>,
}

impl PluginInstalls {
    /// The packages installed under `data_dir`, with nothing pending.
    pub fn open(data_dir: &Path) -> Self {
        Self {
            installed: InstalledPlugins::open(data_dir),
            pending: None,
            queue: VecDeque::new(),
            return_to: DialogKind::None,
            log_open: BTreeMap::new(),
        }
    }
}

/// What the install prompt shows for the pending package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPrompt {
    pub name: String,
    pub id: String,
    pub publisher: String,
    pub version: String,
    /// The version this replaces, for an update.
    pub replaces: Option<String>,
    pub tier: &'static str,
    /// One line per permission asked for: every permission for a new install,
    /// the added ones for an update.
    pub permissions: Vec<String>,
    /// Whether the package runs native code in the editor's process.
    pub native: bool,
    /// How many archives wait behind this one.
    pub waiting: usize,
}

/// The tier as the Plugins list and the install prompt name it.
pub fn tier_label(tier: &Tier) -> &'static str {
    match tier {
        Tier::Native { .. } => "native",
        Tier::Wasm { .. } => "WASM",
        Tier::Data => "data",
    }
}

/// Which WASM plugin limit a settings row sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmLimit {
    /// Instruction fuel for one call into a plugin.
    Fuel,
    /// Linear memory of one plugin instance, in MiB.
    Memory,
}

impl WasmLimit {
    /// The limits, as the settings rows, with each row's id and label.
    pub const ALL: [(Self, &'static str, &'static str); 2] = [
        (Self::Fuel, "wasm-limit-fuel", "WASM fuel per call"),
        (Self::Memory, "wasm-limit-memory", "WASM memory"),
    ];

    /// This limit in `limits`.
    pub fn value(self, limits: PluginLimitSettings) -> u64 {
        match self {
            Self::Fuel => limits.wasm_fuel_per_call,
            Self::Memory => limits.wasm_memory_mib,
        }
    }

    /// The values the config accepts for this limit.
    pub fn range(self) -> RangeInclusive<u64> {
        match self {
            Self::Fuel => {
                aria_settings::MIN_WASM_FUEL_PER_CALL..=aria_settings::MAX_WASM_FUEL_PER_CALL
            }
            Self::Memory => aria_settings::MIN_WASM_MEMORY_MIB..=aria_settings::MAX_WASM_MEMORY_MIB,
        }
    }

    /// `value` with its unit, digits grouped in threes.
    pub fn describe(self, value: u64) -> String {
        let digits = value.to_string();
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        match self {
            Self::Fuel => format!("{grouped} instructions"),
            Self::Memory => format!("{grouped} MiB"),
        }
    }
}

impl AppState {
    /// Take the plugin archives out of `paths` and start installing them.
    /// Returns the other paths, in order.
    pub fn take_plugin_archives(&mut self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let (archives, rest): (Vec<_>, Vec<_>) = paths
            .into_iter()
            .partition(|path| package::is_archive(path));
        if !archives.is_empty() {
            self.install_plugin_archives(archives);
        }
        rest
    }

    /// Queue `archives` for installing and show the first prompt.
    pub fn install_plugin_archives(&mut self, archives: Vec<PathBuf>) {
        self.plugin_installs.queue.extend(archives);
        if self.plugin_installs.pending.is_none() {
            self.stage_next_plugin();
        }
    }

    /// Ask for an `.ariaplugin` and install it.
    pub fn pick_plugin_archive(&mut self) {
        let spec =
            FileSpec::new("Install Plugin").filter("Aria plugin", &[package::ARCHIVE_EXTENSION]);
        self.request_pick(Ask::OpenFile(spec), |state, answer: Answer| {
            if let Some(path) = answer.one() {
                state.install_plugin_archives(vec![path]);
            }
        });
    }

    /// Unpack the next queued archive. One that needs no confirmation is
    /// installed at once; the first that does is shown in the prompt.
    fn stage_next_plugin(&mut self) {
        while let Some(archive) = self.plugin_installs.queue.pop_front() {
            let staged = match self.plugin_installs.installed.stage(&archive) {
                Ok(staged) => staged,
                Err(err) => {
                    self.report_error(format!("Could not install a plugin: {err}"));
                    continue;
                }
            };
            if staged.needs_confirmation() {
                if self.dialog != DialogKind::InstallPlugin {
                    self.plugin_installs.return_to = self.dialog;
                }
                self.plugin_installs.pending = Some(staged);
                self.open_dialog(DialogKind::InstallPlugin);
                return;
            }
            self.commit_plugin(staged);
        }
    }

    /// What the install prompt shows, or `None` with nothing pending.
    pub fn install_prompt(&self) -> Option<InstallPrompt> {
        let staged = self.plugin_installs.pending.as_ref()?;
        let manifest = &staged.package().manifest;
        Some(InstallPrompt {
            name: manifest.name.clone(),
            id: manifest.id.clone(),
            publisher: manifest.publisher.clone(),
            version: manifest.version.clone(),
            replaces: staged.replaces().map(|old| old.version.clone()),
            tier: tier_label(&staged.package().tier),
            permissions: staged.asks().describe(),
            native: staged.is_native(),
            waiting: self.plugin_installs.queue.len(),
        })
    }

    /// Install the package the prompt is showing, then show the next.
    pub fn confirm_plugin_install(&mut self) {
        let Some(staged) = self.plugin_installs.pending.take() else {
            return;
        };
        self.close_install_prompt();
        self.commit_plugin(staged);
        self.stage_next_plugin();
    }

    /// Drop the package the prompt is showing, then show the next.
    pub fn cancel_plugin_install(&mut self) {
        self.plugin_installs.pending = None;
        self.close_install_prompt();
        self.stage_next_plugin();
    }

    fn close_install_prompt(&mut self) {
        if self.dialog == DialogKind::InstallPlugin {
            self.dialog = std::mem::take(&mut self.plugin_installs.return_to);
        }
    }

    /// Drop the pending package and every queued archive: the prompt was left
    /// without an answer.
    pub(crate) fn abandon_plugin_installs(&mut self) {
        self.plugin_installs.pending = None;
        self.plugin_installs.queue.clear();
        self.plugin_installs.return_to = DialogKind::None;
    }

    fn commit_plugin(&mut self, staged: Staged) {
        let name = staged.package().manifest.name.clone();
        let version = staged.package().manifest.version.clone();
        let updated = staged.replaces().is_some();
        match self.plugin_installs.installed.commit(&self.ctx, staged) {
            Ok(_) => {
                self.refresh_render_files();
                self.status_message = Some(if updated {
                    format!("Updated {name} to {version}")
                } else {
                    format!("Installed {name} {version}")
                });
            }
            Err(err) => self.report_error(format!("{name} {version} did not install: {err}")),
        }
    }

    /// Unmount the installed package `id`, read it from disk again and mount
    /// it.
    pub fn reload_plugin(&mut self, id: &str) {
        match self.plugin_installs.installed.reload(&self.ctx, id) {
            Ok(()) => {
                self.refresh_render_files();
                self.status_message = Some(format!("Reloaded {id}"));
            }
            Err(err) => self.report_error(format!("{id} did not reload: {err}")),
        }
    }

    /// Reload every plugin whose files changed on disk, with each result on
    /// the status line. A reload that works clears the error an earlier
    /// reload of the same plugin left. Returns whether any reloaded or failed
    /// to, so the caller repaints only then.
    pub fn reload_changed_plugins(&mut self) -> bool {
        let reloaded = self
            .plugin_installs
            .installed
            .reload_changed(&self.ctx, SystemTime::now());
        for Reloaded { name, result, .. } in &reloaded {
            let failed = format!("{name} did not reload: ");
            match result {
                Ok(()) => {
                    if self
                        .last_error
                        .as_deref()
                        .is_some_and(|err| err.starts_with(&failed))
                    {
                        self.clear_error();
                    }
                    self.status_message = Some(format!("Reloaded {name}"));
                }
                Err(err) => self.report_error(format!("{failed}{err}")),
            }
        }
        if !reloaded.is_empty() {
            self.refresh_render_files();
        }
        !reloaded.is_empty()
    }

    /// Ask, then unmount the installed package `id` and delete it. The
    /// development package is refused without asking: its folder is the
    /// developer's source.
    pub fn uninstall_plugin(&mut self, id: &str) {
        let installed = &self.plugin_installs.installed;
        let Some(package) = installed.package(id) else {
            self.report_error(format!("{id} is not an installed plugin"));
            return;
        };
        let name = package.manifest.name.clone();
        if installed.is_development(id) {
            self.report_error(format!(
                "{name} is loaded for development, not installed; it has nothing to uninstall"
            ));
            return;
        }
        let id = id.to_owned();
        let question = format!(
            "Remove {name}? Its files are deleted. What it stored for itself stays in plugin-data."
        );
        self.request_pick(
            Ask::Confirm {
                title: format!("Uninstall Plugin: {name}"),
                question,
            },
            move |state, answer| {
                if !answer.is_yes() {
                    return;
                }
                match state.plugin_installs.installed.uninstall(&state.ctx, &id) {
                    Ok(_) => {
                        state.refresh_render_files();
                        state.status_message = Some(format!("Uninstalled {name}"));
                    }
                    Err(err) => state.report_error(format!("{name} did not uninstall: {err}")),
                }
            },
        );
    }

    /// Show the folder of the installed package `id`, or the plugin folder
    /// itself for `None`.
    pub fn open_plugin_folder(&mut self, id: Option<&str>) {
        let installed = &self.plugin_installs.installed;
        let folder = match id {
            Some(id) => match installed.package(id) {
                Some(package) => package.dir.clone(),
                None => {
                    self.report_error(format!("{id} is not an installed plugin"));
                    return;
                }
            },
            None => installed.dir().to_owned(),
        };
        if let Err(err) = std::fs::create_dir_all(&folder)
            .map_err(|err| format!("{}: {err}", folder.display()))
            .and_then(|()| super::launch::open_folder(&folder))
        {
            self.report_error(err);
        }
    }

    /// The limits WASM plugins run under.
    pub fn wasm_limits(&self) -> PluginLimitSettings {
        self.plugin_installs.installed.limits()
    }

    /// Set one WASM plugin limit, clamped to the range the config accepts,
    /// save it, and remount the mounted WASM plugins under the new limits.
    ///
    /// Clamped rather than refused: the row steps by halving and doubling, and
    /// a step past the edge stops at it.
    pub fn set_wasm_limit(&mut self, limit: WasmLimit, value: u64) -> Result<(), String> {
        let path = self.config_path();
        let mut settings = aria_settings::load(&path)
            .map_err(|err| format!("{err}; fix the file before changing a plugin limit"))?;
        let range = limit.range();
        let clamped = value.clamp(*range.start(), *range.end());
        let slot = match limit {
            WasmLimit::Fuel => &mut settings.plugin_limits.wasm_fuel_per_call,
            WasmLimit::Memory => &mut settings.plugin_limits.wasm_memory_mib,
        };
        if *slot != clamped {
            *slot = clamped;
            aria_settings::save(&path, &settings)?;
        }
        let remounted = self
            .plugin_installs
            .installed
            .set_limits(&self.ctx, settings.plugin_limits);
        let failed: Vec<String> = remounted
            .iter()
            .filter_map(|Reloaded { name, result, .. }| {
                result.as_ref().err().map(|err| format!("{name}: {err}"))
            })
            .collect();
        if !failed.is_empty() {
            return Err(format!(
                "WASM limits are saved, but these plugins did not mount under them: {}",
                failed.join("; ")
            ));
        }
        if !remounted.is_empty() {
            self.refresh_render_files();
        }
        let label = WasmLimit::ALL
            .iter()
            .find_map(|(each, _, label)| (*each == limit).then_some(*label))
            .unwrap_or_default();
        self.status_message = Some(match remounted.len() {
            0 => format!("{label} set to {}", limit.describe(clamped)),
            count => format!(
                "{label} set to {}; remounted {}",
                limit.describe(clamped),
                counted(count, "plugin", "plugins")
            ),
        });
        Ok(())
    }
}

#[cfg(test)]
#[path = "plugin_install_tests.rs"]
mod tests;
