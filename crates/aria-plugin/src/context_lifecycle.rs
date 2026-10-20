//! Mounting and unmounting plugins on a [`PluginContext`], and the failure,
//! fault, log, and cost records kept for each plugin.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use super::{MountFailure, MountedPlugin, MountedPluginInfo, PluginContext, PluginFault};
use crate::cost::PluginCost;
use crate::editor_events::{PluginActivated, PluginDeactivated};
use crate::error::PluginError;
use crate::plugin::{Plugin, PluginManifest};
use crate::plugin_log::{LogLevel, LogLine};
use crate::registration::Registration;

impl PluginContext {
    /// Note a mount that did not take. One entry per plugin id: a retry replaces
    /// the previous reason rather than stacking another row.
    fn record_failure(&self, manifest: &PluginManifest, err: &PluginError) {
        let reason = err.to_string();
        self.log_failure(&manifest.id, &reason);
        let mut failures = self.shared.failures.write();
        failures.retain(|f| f.id != manifest.id);
        failures.push(MountFailure {
            id: manifest.id.to_string(),
            name: manifest.name.to_string(),
            reason,
        });
    }

    /// Log why `plugin` stopped, with the backtrace of the panic `reason`
    /// reports when this thread caught one.
    fn log_failure(&self, plugin: &str, reason: &str) {
        match crate::plugin_log::take_panic_backtrace(reason) {
            Some(backtrace) => {
                self.shared
                    .logs
                    .push(plugin, LogLevel::Error, &format!("{reason}\n{backtrace}"));
            }
            None => self.shared.logs.push(plugin, LogLevel::Error, reason),
        }
    }

    /// Log `text` under this view's plugin: kept for the Plugins list and sent
    /// to `tracing`. The host's context belongs to no plugin, so its lines go
    /// to `tracing` only.
    pub fn log(&self, level: LogLevel, text: &str) {
        match &self.scope {
            Some(manifest) => self.shared.logs.push(&manifest.id, level, text),
            None => crate::plugin_log::trace("host", level, text),
        }
    }

    /// Log `text` under `plugin`, for a host that runs a plugin's code and
    /// receives its log calls, such as the WASM and native loaders.
    pub fn log_for(&self, plugin: &str, level: LogLevel, text: &str) {
        self.shared.logs.push(plugin, level, text);
    }

    /// The lines `plugin` logged this session, oldest first, at most
    /// [`LOG_LINES_PER_PLUGIN`](crate::LOG_LINES_PER_PLUGIN). They outlive an
    /// unmount, a failure and a reload.
    pub fn plugin_logs(&self, plugin: &str) -> Vec<LogLine> {
        self.shared.logs.lines(plugin)
    }

    /// How many lines [`plugin_logs`](Self::plugin_logs) holds for `plugin`.
    pub fn plugin_log_count(&self, plugin: &str) -> usize {
        self.shared.logs.count(plugin)
    }

    /// Drop `plugin`'s lines, for a plugin that was uninstalled. Returns
    /// whether it had any.
    pub fn clear_logs(&self, plugin: &str) -> bool {
        self.shared.logs.clear(plugin)
    }

    /// Moves whenever a line is logged or a plugin's lines are dropped.
    pub fn logs_revision(&self) -> u64 {
        self.shared.logs.revision()
    }

    /// Note a plugin that could not be loaded at all, such as a package whose
    /// code did not load or compile. Nothing is mounted, so `id` and `name`
    /// are whatever the loader has: the package's manifest, or the file name
    /// when the manifest did not read. Returned as well as recorded, because a
    /// loader reports its own batch.
    pub fn record_external_failure(
        &self,
        id: String,
        name: String,
        reason: String,
    ) -> MountFailure {
        self.log_failure(&id, &reason);
        let failure = MountFailure { id, name, reason };
        let mut failures = self.shared.failures.write();
        failures.retain(|held| held.id != failure.id);
        failures.push(failure.clone());
        failure
    }

    /// Plugins that failed to mount, or failed after it, in the order they
    /// failed.
    pub fn failures(&self) -> Vec<MountFailure> {
        self.shared.failures.read().clone()
    }

    /// Forget the recorded failure of `id`, for a plugin that was removed or
    /// is about to be loaded again. Returns whether there was one.
    pub fn clear_failure(&self, id: &str) -> bool {
        let mut failures = self.shared.failures.write();
        let before = failures.len();
        failures.retain(|failure| failure.id != id);
        failures.len() != before
    }

    /// Record a fault in `plugin`'s code that the host recovered from. One is
    /// kept per plugin, a newer fault replacing the older, until the plugin
    /// unmounts or fails. Ignored for a plugin that is not mounted.
    pub fn report_fault(&self, plugin: &str, message: impl Into<String>) {
        let Some(id) = self
            .shared
            .plugins
            .read()
            .iter()
            .find(|mounted| mounted.manifest.id == plugin)
            .map(|mounted| mounted.manifest.id.clone())
        else {
            return;
        };
        let message = message.into();
        self.shared.logs.push(&id, LogLevel::Warn, &message);
        let mut faults = self.shared.faults.write();
        faults.retain(|fault| fault.plugin != id);
        faults.push(PluginFault {
            plugin: id,
            message,
        });
    }

    /// The latest recovered fault of each mounted plugin, oldest first.
    pub fn faults(&self) -> Vec<PluginFault> {
        self.shared.faults.read().clone()
    }

    /// Mount a plugin in dependency order.
    ///
    /// Every failure is recorded in the failure ledger as well as returned, so a
    /// host that keeps going after one bad plugin can still report which
    /// capability is missing. A later successful mount of the same id clears its
    /// entry.
    pub fn mount(&self, plugin: &dyn Plugin) -> Result<(), PluginError> {
        let manifest = plugin.manifest().clone();
        tracing::info!(plugin = %manifest.id, "mounting plugin");
        for dep in manifest.requires.iter() {
            let found = self
                .shared
                .plugins
                .read()
                .iter()
                .any(|p| p.manifest.id == dep);
            if !found {
                let err = PluginError::MissingDependency(manifest.id.clone(), dep.to_owned());
                self.record_failure(&manifest, &err);
                return Err(err);
            }
        }
        {
            let mut pending = self.shared.pending.lock();
            if pending.is_some() {
                return Err(PluginError::MountFailed(
                    manifest.id.clone(),
                    "nested plugin mount is not supported".into(),
                ));
            }
            *pending = Some(Vec::new());
        }
        self.shared.jobs.hold(&manifest.id);
        // The plugin never sees the host's own context: it gets the view its
        // manifest describes, and a handler it registers keeps that view because a
        // clone carries the scope.
        let scoped = self.view(&manifest);
        let _ = self.shared.costs.reset(manifest.id.clone());
        // Around the plugin's own work only: the dependency check and the
        // registration bookkeeping are the host's cost, and attributing them to
        // the plugin would make every mount look alike.
        let at = Instant::now();
        let caught =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.mount(&scoped)));
        let elapsed = at.elapsed();
        let registrations = self.shared.pending.lock().take().unwrap_or_default();
        let mount_result = match caught {
            Ok(Ok(())) => Ok(()),
            // The plugin's own refusal already names it.
            Ok(Err(PluginError::MountFailed(id, reason))) if id == manifest.id => {
                Err(PluginError::MountFailed(id, reason))
            }
            Ok(Err(e)) => Err(PluginError::MountFailed(manifest.id.clone(), e.to_string())),
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "plugin panicked during mount".to_string()
                };
                Err(PluginError::MountFailed(
                    manifest.id.clone(),
                    format!("panic: {msg}"),
                ))
            }
        };
        if let Err(err) = mount_result {
            drop(registrations);
            self.shared
                .jobs
                .cancel_all(&manifest.id, self.job_manager().as_deref());
            self.shared.jobs.release();
            self.shared.costs.remove(&manifest.id);
            self.record_failure(&manifest, &err);
            return Err(err);
        }
        self.shared.failures.write().retain(|f| f.id != manifest.id);
        let id = manifest.id.clone();
        self.shared.plugins.write().push(MountedPlugin {
            manifest,
            registrations,
            mount: elapsed,
        });
        self.shared.jobs.release();
        self.shared.events.post(PluginActivated { id });
        Ok(())
    }

    /// Unmount a plugin by id.
    pub fn unmount(&self, plugin_id: &str) -> Result<(), PluginError> {
        let removed = self.take(plugin_id).ok_or_else(|| {
            PluginError::UnmountFailed(
                Cow::Borrowed("unknown"),
                format!("plugin not found: {plugin_id}"),
            )
        })?;
        self.shared.failures.write().retain(|f| f.id != plugin_id);
        tracing::info!(plugin = %removed.manifest.id, "unmounted plugin");
        self.retire(removed);
        Ok(())
    }

    /// Mark a mounted plugin failed for the session: drop every registration it
    /// made, record `reason` in [`failures`](Self::failures), and post
    /// [`PluginDeactivated`]. For a fault in the plugin's code after mount, such
    /// as a panic in a callback the host ran. A later [`mount`](Self::mount) of
    /// the plugin clears the entry. Returns whether `plugin` was mounted.
    pub fn fail(&self, plugin: &str, reason: impl Into<String>) -> bool {
        let Some(removed) = self.take(plugin) else {
            return false;
        };
        let reason = reason.into();
        self.log_failure(plugin, &reason);
        {
            let mut failures = self.shared.failures.write();
            failures.retain(|f| f.id != plugin);
            failures.push(MountFailure {
                id: plugin.to_owned(),
                name: removed.manifest.name.to_string(),
                reason,
            });
        }
        self.retire(removed);
        true
    }

    /// Take `plugin` out of the mounted set, with its denials and faults. The
    /// lock is released before anything of the plugin's drops, so drop code
    /// may read the context.
    fn take(&self, plugin: &str) -> Option<MountedPlugin> {
        let removed = {
            let mut plugins = self.shared.plugins.write();
            let index = plugins
                .iter()
                .position(|mounted| mounted.manifest.id == plugin)?;
            plugins.remove(index)
        };
        self.shared.denials.write().retain(|d| d.plugin != plugin);
        self.shared.faults.write().retain(|f| f.plugin != plugin);
        self.shared.costs.remove(plugin);
        Some(removed)
    }

    /// Drop a removed plugin's registrations and announce it is gone.
    fn retire(&self, removed: MountedPlugin) {
        drop(removed.registrations);
        self.shared
            .jobs
            .cancel_all(&removed.manifest.id, self.job_manager().as_deref());
        self.shared.events.post(PluginDeactivated {
            id: removed.manifest.id,
        });
    }

    /// Mounted plugin ids, in mount order.
    pub fn plugins(&self) -> Vec<Cow<'static, str>> {
        self.shared
            .plugins
            .read()
            .iter()
            .map(|p| p.manifest.id.clone())
            .collect()
    }

    /// Whether the plugin `id` is mounted.
    pub fn is_mounted(&self, id: &str) -> bool {
        self.shared
            .plugins
            .read()
            .iter()
            .any(|p| p.manifest.id == id)
    }

    /// Every mounted plugin with the services and contributions it registered,
    /// in mount order.
    ///
    /// This is what a host lists: the manifest is the plugin's own description and
    /// the service names come from the registrations it made, so a plugin cannot
    /// claim a capability it did not register.
    pub fn mounted(&self) -> Vec<MountedPluginInfo> {
        self.shared
            .plugins
            .read()
            .iter()
            .map(|plugin| {
                let (frame_cost, block_cost) = self.shared.costs.snapshot_for(&plugin.manifest.id);
                MountedPluginInfo {
                    manifest: plugin.manifest.clone(),
                    services: plugin
                        .registrations
                        .iter()
                        .filter_map(Registration::service_name)
                        .collect(),
                    contributions: plugin
                        .registrations
                        .iter()
                        .filter_map(Registration::contribution)
                        .cloned()
                        .collect(),
                    mount: plugin.mount,
                    frame_cost,
                    block_cost,
                }
            })
            .collect()
    }

    /// Measured per-frame and per-block runtime costs for every mounted plugin,
    /// in mount order.
    pub fn costs(&self) -> Vec<PluginCost> {
        self.shared
            .plugins
            .read()
            .iter()
            .map(|plugin| {
                let (frame, block) = self.shared.costs.snapshot_for(&plugin.manifest.id);
                PluginCost {
                    plugin: plugin.manifest.id.clone(),
                    frame,
                    block,
                }
            })
            .collect()
    }

    /// Measured runtime costs for mounted `plugin`, or `None` when `plugin` is
    /// not mounted.
    pub fn cost_of(&self, plugin: &str) -> Option<PluginCost> {
        if !self.is_mounted(plugin) {
            return None;
        }
        self.shared.costs.cost_of(plugin)
    }

    /// Record a per-frame render duration for `plugin` if it is mounted.
    pub fn record_frame_cost(&self, plugin: &str, duration: Duration) {
        if self.is_mounted(plugin) {
            self.shared.costs.record_frame(plugin, duration);
        }
    }

    /// Record a per-block audio duration for `plugin` if it is mounted.
    pub fn record_block_cost(&self, plugin: &str, duration: Duration) {
        if self.is_mounted(plugin) {
            self.shared.costs.record_block(plugin, duration);
        }
    }
}
