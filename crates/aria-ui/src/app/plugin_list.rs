//! The Plugins list: what is mounted, what it provides, and what failed.
//!
//! Nothing showed the plugin host at all, so a plugin that did not mount was a log
//! line and a capability that silently did nothing. A row is built from three
//! facts the host already has: the built-in list (every plugin that could be
//! mounted), the context's mounted set (what took), and the context's failure
//! ledger (why the rest did not).
//!
//! Enabling and disabling go through the same mount path as boot, so a disabled
//! plugin's services are gone from the registry rather than flagged: a surface that
//! asks `try_get` for them finds nothing and hides itself.

use crate::plugins::{builtin, builtin_by_id};
use crate::types::{UiCatalogRow, UiPluginRow};

use super::AppState;
use gpui::SharedString;

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
    /// external libraries that loaded, then any plugin file that would not load
    /// at all.
    ///
    /// A file that failed to load has no manifest, so its row is the file name and
    /// the reason — the one place a user finds out a plugin they installed is not
    /// running.
    pub fn plugin_rows(&self) -> Vec<UiPluginRow> {
        let mounted = self.ctx.mounted();
        let failures = self.ctx.failures();
        let denials = self.ctx.denials();
        let denied_by = |id: &str| -> Vec<SharedString> {
            denials
                .iter()
                .filter(|denial| denial.plugin == id)
                .map(|denial| SharedString::from(short_service_name(denial.service)))
                .collect()
        };
        builtin(&self.data_dir)
            .iter()
            .map(|plugin| {
                let manifest = *plugin.manifest();
                let live = mounted.iter().find(|info| info.manifest.id == manifest.id);
                UiPluginRow {
                    id: manifest.id.into(),
                    name: manifest.name.into(),
                    version: manifest.version.into(),
                    requires: manifest.requires.iter().map(|d| (*d).into()).collect(),
                    services: live
                        .map(|info| {
                            info.services
                                .iter()
                                .map(|name| SharedString::from(short_service_name(name)))
                                .collect()
                        })
                        .unwrap_or_default(),
                    denied: denied_by(manifest.id),
                    enabled: live.is_some(),
                    mount_ms: live.map(|info| info.mount.as_secs_f32() * 1_000.0),
                    error: failures
                        .iter()
                        .find(|failure| failure.id == manifest.id)
                        .map(|failure| failure.reason.clone().into()),
                }
            })
            .chain(aria_plugin::external::loaded().into_iter().map(|manifest| {
                let live = mounted.iter().find(|info| info.manifest.id == manifest.id);
                UiPluginRow {
                    id: manifest.id.into(),
                    name: manifest.name.into(),
                    version: manifest.version.into(),
                    requires: manifest.requires.iter().map(|d| (*d).into()).collect(),
                    services: live
                        .map(|info| {
                            info.services
                                .iter()
                                .map(|name| SharedString::from(short_service_name(name)))
                                .collect()
                        })
                        .unwrap_or_default(),
                    denied: denied_by(manifest.id),
                    enabled: live.is_some(),
                    mount_ms: live.map(|info| info.mount.as_secs_f32() * 1_000.0),
                    error: failures
                        .iter()
                        .find(|failure| failure.id == manifest.id)
                        .map(|failure| failure.reason.clone().into()),
                }
            }))
            .chain(
                failures
                    .iter()
                    .filter(|failure| {
                        let known = |id: &str| {
                            builtin(&self.data_dir)
                                .iter()
                                .any(|p| p.manifest().id == id)
                                || aria_plugin::external::loaded().iter().any(|m| m.id == id)
                        };
                        !known(&failure.id)
                    })
                    .map(|failure| UiPluginRow {
                        id: failure.id.clone().into(),
                        name: failure.name.clone().into(),
                        version: SharedString::default(),
                        requires: Vec::new(),
                        services: Vec::new(),
                        denied: Vec::new(),
                        enabled: false,
                        mount_ms: None,
                        error: Some(failure.reason.clone().into()),
                    })
                    .collect::<Vec<_>>(),
            )
            .collect()
    }

    /// Whether this plugin may be turned off. The three the shell cannot run
    /// without are listed without a toggle rather than offered one that refuses,
    /// and an external plugin has none either: re-mounting it would mean loading
    /// its library again, and the loader maps a library once per process.
    pub fn plugin_can_toggle(&self, id: &str) -> bool {
        !REQUIRED_PLUGINS.contains(&id)
            && builtin(&self.data_dir)
                .iter()
                .any(|p| p.manifest().id == id)
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

    /// Mount or unmount one built-in plugin. Returns whether the mounted set
    /// changed, so a caller repaints only on a real change.
    ///
    /// A failed mount is not an error the caller has to handle: it lands in the
    /// context's ledger and the row shows the reason.
    pub fn set_plugin_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if !self.plugin_can_toggle(id) {
            self.status_message = Some(format!("{id} cannot be turned off"));
            return false;
        }
        let mounted = self.ctx.plugins().iter().any(|live| *live == id);
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
                    true
                }
                Err(err) => {
                    self.status_message = Some(format!("{id} did not unmount: {err}"));
                    false
                }
            }
        }
    }
}
