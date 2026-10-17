//! The Plugins list: what is mounted, what it provides, and what failed.
//!
//! A row is built from facts the host already has: the built-in list (every
//! plugin that could be mounted), the installed packages under the plugin
//! folder, the context's mounted set (what took, what it registered, what
//! mounting it cost), and the context's failure ledger (why the rest did not).
//!
//! Enabling and disabling go through the same mount path as boot, so a disabled
//! plugin's services are gone from the registry rather than flagged: a surface that
//! asks `try_get` for them finds nothing and hides itself.

use std::path::Path;

use aria_plugin::package::PackageError;
use aria_plugin::{Activated, Activation, PluginManifest};

use crate::plugins::{builtin, builtin_by_id};
use crate::types::{UiCatalogRow, UiPluginRow};

use super::AppState;
use super::plugin_install::tier_label;
use gpui::SharedString;

/// The tier a built-in plugin's row names.
const BUILT_IN: &str = "built-in";

/// Plugins that must stay mounted: without them there is no editor to show a
/// Plugins list in, so they are listed but not offered a toggle.
const REQUIRED_PLUGINS: &[&str] = &["aria.storage", "aria.settings", "aria.engine"];

/// The last path segment of a service type name. `aria_plugin` owns the spelling,
/// because a manifest declares it and this list shows it.
fn short_service_name(name: &str) -> String {
    aria_plugin::short_service_name(name).to_string()
}

impl AppState {
    /// Every plugin the host knows about: the built-ins in mount order, then the
    /// installed packages in folder order and the development package, then any
    /// other plugin the failure ledger names.
    ///
    /// A package that does not read has no manifest, so its row is the folder
    /// name and the reason: the one place a user finds out a plugin they
    /// installed is not running.
    pub fn plugin_rows(&self) -> Vec<UiPluginRow> {
        let mounted = self.ctx.mounted();
        let waiting = self.ctx.waiting_plugins();
        let failures = self.ctx.failures();
        let denials = self.ctx.denials();
        let faults = self.ctx.faults();
        let fault_of = |id: &str| -> Option<SharedString> {
            faults
                .iter()
                .find(|fault| fault.plugin == id)
                .map(|fault| fault.message.clone().into())
        };
        let denied_by = |id: &str| -> Vec<SharedString> {
            denials
                .iter()
                .filter(|denial| denial.plugin == id)
                .map(|denial| SharedString::from(short_service_name(denial.service)))
                .collect()
        };
        let row = |manifest: &PluginManifest,
                   tier: &'static str,
                   permissions: Vec<String>,
                   development: bool| {
            let live = mounted.iter().find(|info| info.manifest.id == manifest.id);
            let waiting_for: Vec<SharedString> = waiting
                .iter()
                .find(|plugin| plugin.id == manifest.id)
                .map(|plugin| {
                    plugin
                        .events
                        .iter()
                        .map(|event| SharedString::from(event.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let installed = tier != BUILT_IN;
            let error: Option<SharedString> = failures
                .iter()
                .find(|failure| failure.id == manifest.id)
                .map(|failure| failure.reason.clone().into());
            let fault = fault_of(&manifest.id);
            UiPluginRow {
                id: manifest.id.clone().into(),
                name: manifest.name.clone().into(),
                version: manifest.version.clone().into(),
                tier: tier.into(),
                installed,
                development,
                can_toggle: installed || !REQUIRED_PLUGINS.contains(&manifest.id.as_ref()),
                requires: manifest.requires.iter().map(SharedString::from).collect(),
                services: live
                    .map(|info| {
                        info.services
                            .iter()
                            .map(|name| SharedString::from(short_service_name(name)))
                            .collect()
                    })
                    .unwrap_or_default(),
                contributions: live
                    .map(|info| {
                        info.contributions
                            .iter()
                            .map(|contribution| SharedString::from(contribution.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                permissions: permissions.into_iter().map(SharedString::from).collect(),
                denied: denied_by(&manifest.id),
                enabled: live.is_some() || !waiting_for.is_empty(),
                waiting_for,
                mount_ms: live.map(|info| info.mount.as_secs_f32() * 1_000.0),
                frame_ms: live.and_then(|info| info.frame_cost.mean_ms()),
                frame_max_ms: live.and_then(|info| info.frame_cost.max_ms()),
                block_ms: live.and_then(|info| info.block_cost.mean_ms()),
                block_max_ms: live.and_then(|info| info.block_cost.max_ms()),
                log_lines: self.ctx.plugin_log_count(&manifest.id),
                log_open: self.log_open(&manifest.id, error.is_some() || fault.is_some()),
                error,
                fault,
            }
        };
        let unread = |id: String, name: String, reason: String| UiPluginRow {
            log_lines: self.ctx.plugin_log_count(&id),
            log_open: self.log_open(&id, true),
            id: id.into(),
            name: name.into(),
            version: SharedString::default(),
            tier: SharedString::default(),
            installed: false,
            development: false,
            can_toggle: false,
            requires: Vec::new(),
            services: Vec::new(),
            contributions: Vec::new(),
            permissions: Vec::new(),
            denied: Vec::new(),
            enabled: false,
            waiting_for: Vec::new(),
            mount_ms: None,
            frame_ms: None,
            frame_max_ms: None,
            block_ms: None,
            block_max_ms: None,
            error: Some(reason.into()),
            fault: None,
        };

        let builtins = builtin(&self.data_dir);
        let mut rows: Vec<UiPluginRow> = builtins
            .iter()
            .map(|plugin| row(plugin.manifest(), BUILT_IN, Vec::new(), false))
            .collect();
        let installs = &self.plugin_installs.installed;
        for entry in installs.packages() {
            rows.push(match entry {
                Ok(package) => row(
                    &package.plugin_manifest(),
                    tier_label(&package.tier),
                    package.manifest.permissions.describe(),
                    installs.is_development(&package.manifest.id),
                ),
                Err(PackageError { path, reason }) => {
                    let folder = crate::plugins::installed::folder_name(path);
                    unread(folder.clone(), folder, reason.clone())
                }
            });
        }
        // A failure the rows above do not name: a plugin mounted by a caller
        // outside this list.
        let named: Vec<SharedString> = rows.iter().map(|row| row.id.clone()).collect();
        rows.extend(
            failures
                .iter()
                .filter(|failure| !named.iter().any(|id| id.as_str() == failure.id))
                .map(|failure| {
                    unread(
                        failure.id.clone(),
                        failure.name.clone(),
                        failure.reason.clone(),
                    )
                }),
        );
        rows
    }

    /// Whether this plugin may be turned off. The three the shell cannot run
    /// without are listed without a toggle rather than offered one that refuses.
    /// An installed package can always be turned off and on: a native one loads
    /// from a fresh copy of its library each time.
    pub fn plugin_can_toggle(&self, id: &str) -> bool {
        if builtin(&self.data_dir)
            .iter()
            .any(|p| p.manifest().id == id)
        {
            return !REQUIRED_PLUGINS.contains(&id);
        }
        self.plugin_installs.installed.package(id).is_some()
    }

    /// Every catalog and where it came from, in the order the loader read them.
    ///
    /// A catalog is data a user can replace, so the one thing the app must be able
    /// to answer is which set is actually in effect. A file that was refused shows
    /// its reason here; without this a bad override looks exactly like no override.
    pub fn catalog_rows(&self) -> Vec<UiCatalogRow> {
        aria_models::catalog::status()
            .into_iter()
            .map(|status| UiCatalogRow {
                name: status.name.into(),
                entries: status.entries,
                overridden: status.overridden,
                problem: status.problem.map(Into::into),
            })
            .collect()
    }

    /// Mount or unmount one plugin. Returns whether the list changed, so a
    /// caller repaints only on a real change.
    ///
    /// A failed mount is not an error the caller has to handle: it lands in the
    /// context's ledger and the row shows the reason. An installed package's
    /// choice is kept for the next start.
    pub fn set_plugin_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if !self.plugin_can_toggle(id) {
            self.status_message = Some(format!("{id} cannot be turned off"));
            return false;
        }
        if builtin_by_id(id, &self.data_dir).is_none() {
            let verb = if enabled { "enabled" } else { "disabled" };
            return match self
                .plugin_installs
                .installed
                .set_enabled(&self.ctx, id, enabled)
            {
                Ok(changed) => {
                    if changed {
                        self.status_message = Some(format!("{id} {verb}"));
                        self.refresh_render_files();
                    }
                    changed
                }
                Err(err) => {
                    self.status_message = Some(format!("{id} was not {verb}: {err}"));
                    // The choice was kept and the ledger holds the reason, so
                    // the row changed even though nothing mounted.
                    true
                }
            };
        }
        let mounted = self.ctx.is_mounted(id);
        if mounted == enabled {
            return false;
        }
        if enabled {
            let Some(plugin) = builtin_by_id(id, &self.data_dir) else {
                self.status_message = Some(format!("{id} is not a built-in plugin"));
                return false;
            };
            match self.ctx.mount(plugin.as_ref()) {
                Ok(()) => {
                    self.status_message = Some(format!("{id} enabled"));
                    self.refresh_render_files();
                    true
                }
                Err(err) => {
                    self.status_message = Some(format!("{id} did not mount: {err}"));
                    // The ledger carries the reason for the row, so the list stays
                    // truthful even though nothing mounted.
                    true
                }
            }
        } else {
            match self.ctx.unmount(id) {
                Ok(()) => {
                    self.status_message = Some(format!("{id} disabled"));
                    self.refresh_render_files();
                    true
                }
                Err(err) => {
                    self.status_message = Some(format!("{id} did not unmount: {err}"));
                    false
                }
            }
        }
    }

    /// Whether the Plugins list shows `id`'s log: the Log toggle's choice, or
    /// `troubled` (an error or a fault) before one.
    fn log_open(&self, id: &str, troubled: bool) -> bool {
        self.plugin_installs
            .log_open
            .get(id)
            .copied()
            .unwrap_or(troubled)
    }

    /// Open or close `id`'s log in the Plugins list.
    pub fn toggle_plugin_log(&mut self, id: &str) {
        let troubled = self.ctx.failures().iter().any(|failure| failure.id == id)
            || self.ctx.faults().iter().any(|fault| fault.plugin == id);
        let open = !self.log_open(id, troubled);
        self.plugin_installs.log_open.insert(id.to_owned(), open);
    }

    /// `id`'s log as Copy puts it on the clipboard: one `level text` line per
    /// logged line, oldest first.
    pub fn plugin_log_text(&self, id: &str) -> String {
        let mut text = String::new();
        for line in self.ctx.plugin_logs(id) {
            text.push_str(line.level.as_str());
            text.push(' ');
            text.push_str(&line.text);
            text.push('\n');
        }
        text
    }

    /// Load the waiting packages that read `path`'s file type. Call it before
    /// the path is classified, imported or read, so the package that reads it
    /// is mounted first.
    pub(crate) fn load_plugins_for_file(&mut self, path: &Path) {
        let activated = self.ctx.activate_for_file(path);
        self.report_activations(activated);
    }

    /// Load the waiting packages that wait for `event`.
    pub(crate) fn load_plugins_for(&mut self, event: &Activation) {
        let activated = self.ctx.activate(event);
        self.report_activations(activated);
    }

    /// Put each package an activation event failed to load in the status
    /// strip. Its row in the Plugins list holds the reason too.
    pub(crate) fn report_activations(&mut self, activated: Vec<Activated>) {
        if !activated.is_empty() {
            self.refresh_render_files();
        }
        let failed: Vec<String> = activated
            .into_iter()
            .filter_map(|one| {
                let reason = one.result.err()?;
                Some(format!("{} did not load: {reason}", one.name))
            })
            .collect();
        if !failed.is_empty() {
            self.status_message = Some(failed.join("; "));
        }
    }
}
