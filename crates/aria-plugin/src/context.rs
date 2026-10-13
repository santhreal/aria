//! Plugin context — typed service registry and event-bus host.

use std::any::{Any, TypeId};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;

use crate::editor_events::{PluginActivated, PluginDeactivated};
use crate::error::PluginError;
use crate::events::EventBus;
use crate::plugin::{Plugin, PluginManifest};
use crate::registration::Registration;

pub(crate) type ServiceMap = RwLock<FxHashMap<TypeId, Box<dyn Any + Send + Sync>>>;

/// The shared context every plugin registers services and events into.
///
/// A clone shares the registry, the bus, and the ledgers. A clone made by
/// [`PluginContext::view`] also carries a capability scope: the plugin's id and
/// the services its manifest declared. Registration is never scoped — a plugin
/// providing a capability is not consuming one.
#[derive(Clone)]
pub struct PluginContext {
    services: Arc<ServiceMap>,
    events: Arc<EventBus>,
    plugins: Arc<RwLock<Vec<MountedPlugin>>>,
    pending: Arc<Mutex<Option<Vec<Registration>>>>,
    /// Mounts that failed, in the order they were attempted. A host reads this to
    /// tell the user which capability is missing and why, instead of a log line
    /// nobody sees.
    failures: Arc<RwLock<Vec<MountFailure>>>,
    /// Services asked for and refused, in the order they were asked for.
    denials: Arc<RwLock<Vec<CapabilityDenial>>>,
    /// `None` on the host's own context, which reads everything. `Some` on a
    /// plugin's view: its id, and the service names it declared.
    scope: Option<(&'static str, &'static [&'static str])>,
}

/// A service a plugin asked for and was refused, because its manifest does not
/// declare it.
///
/// Recorded rather than logged: a plugin that forgot a declaration otherwise
/// degrades silently, since a surface whose service is missing hides itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    pub plugin: &'static str,
    pub service: &'static str,
}

/// A mount that did not take, and the reason it gave.
///
/// Owned strings, not `&'static str`: an external plugin is identified by its
/// file name when the library will not even load, and there is no manifest to
/// borrow one from.
#[derive(Debug, Clone)]
pub struct MountFailure {
    pub id: String,
    pub name: String,
    pub reason: String,
}

struct MountedPlugin {
    manifest: PluginManifest,
    registrations: Vec<Registration>,
    mount: Duration,
}

/// A mounted plugin as a host sees it: what it says it is, what it actually
/// registered, and what mounting it cost.
#[derive(Debug, Clone)]
pub struct MountedPluginInfo {
    pub manifest: PluginManifest,
    /// Type names of the services this plugin registered while mounting.
    pub services: Vec<&'static str>,
    /// How long this plugin's own `mount` took.
    ///
    /// Measured here because this is what calls it: a start is the sum of these
    /// plus what the host does around them, and without the breakdown a slow
    /// start is one number with nothing to act on. The GPU device, the catalogs
    /// and the model directory are all mounted, so this is where the time is.
    pub mount: Duration,
}

/// Short Rust type name with module paths removed from the outer type and its
/// generic arguments.
///
/// One owner, because a manifest declares this spelling and a host lists it.
pub fn short_service_name(name: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = name.trim_start_matches("dyn ");
    if !trimmed.contains("::") {
        return std::borrow::Cow::Borrowed(trimmed);
    }

    let mut short = String::with_capacity(trimmed.len());
    let mut token_start = 0;
    for (index, ch) in trimmed.char_indices() {
        if matches!(ch, '<' | '>' | ',' | ' ') {
            let token = &trimmed[token_start..index];
            short.push_str(token.rsplit("::").next().unwrap_or(token));
            short.push(ch);
            token_start = index + ch.len_utf8();
        }
    }
    let token = &trimmed[token_start..];
    short.push_str(token.rsplit("::").next().unwrap_or(token));
    std::borrow::Cow::Owned(short)
}

impl PluginContext {
    /// Create an empty context.
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(FxHashMap::default())),
            events: Arc::new(EventBus::new()),
            plugins: Arc::new(RwLock::new(Vec::new())),
            pending: Arc::new(Mutex::new(None)),
            failures: Arc::new(RwLock::new(Vec::new())),
            denials: Arc::new(RwLock::new(Vec::new())),
            scope: None,
        }
    }

    /// The same context seen through one plugin's manifest: it resolves the
    /// services that manifest declares and nothing else.
    ///
    /// [`mount`](Self::mount) builds this for every plugin it mounts, so a plugin
    /// never holds the unscoped context. A clone of the view keeps the scope, which
    /// is what makes a handler registered during mount stay inside it.
    pub fn view(&self, plugin: &'static str, uses: &'static [&'static str]) -> Self {
        Self {
            scope: Some((plugin, uses)),
            ..self.clone()
        }
    }

    /// Services a plugin asked for and did not declare, in ask order.
    pub fn denials(&self) -> Vec<CapabilityDenial> {
        self.denials.read().clone()
    }

    /// Whether this context may read `T`, recording a denial when it may not.
    ///
    /// The declared name is the short one (`StorageService`), because the full
    /// path of a `dyn` trait object is a compiler detail no manifest should have
    /// to spell.
    fn allows<T: ?Sized + 'static>(&self) -> bool {
        let Some((plugin, uses)) = self.scope else {
            return true;
        };
        let full = std::any::type_name::<T>();
        let short = short_service_name(full);
        if uses.iter().any(|declared| *declared == short.as_ref()) {
            return true;
        }
        let denial = CapabilityDenial {
            plugin,
            service: full,
        };
        let mut denials = self.denials.write();
        if !denials.contains(&denial) {
            tracing::warn!(plugin = %plugin, service = %full, "undeclared service refused");
            denials.push(denial);
        }
        false
    }

    /// Register a service. Stores `Arc<T>` and returns a Drop guard.
    ///
    /// During [`mount`](Self::mount) the guard is retained by the context so
    /// the service stays registered for the plugin's lifetime.
    pub fn register<T: Send + Sync + 'static>(&self, service: T) -> Registration {
        self.register_arc(Arc::new(service))
    }

    /// Register an already-shared service.
    pub fn register_arc<T: ?Sized + Send + Sync + 'static>(&self, service: Arc<T>) -> Registration {
        let type_id = TypeId::of::<T>();
        self.services.write().insert(type_id, Box::new(service));
        self.retain_if_mounting(Registration::service(
            type_id,
            std::any::type_name::<T>(),
            &self.services,
        ))
    }

    fn retain_if_mounting(&self, registration: Registration) -> Registration {
        let mut pending = self.pending.lock();
        if let Some(list) = pending.as_mut() {
            list.push(registration);
            Registration::none()
        } else {
            registration
        }
    }

    /// Retrieve a service. Panics if it is missing or not declared.
    ///
    /// Fail closed: a plugin that reaches for a capability it did not declare is a
    /// bug in that plugin, and a panic during its own mount is recorded as a mount
    /// failure rather than taking the app down.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.try_get::<T>().unwrap_or_else(|| {
            panic!(
                "required plugin service unavailable: {}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Retrieve a service, or `None` if it is missing or not declared.
    pub fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        if !self.allows::<T>() {
            return None;
        }
        let map = self.services.read();
        let boxed = map.get(&TypeId::of::<T>())?;
        boxed.downcast_ref::<Arc<T>>().map(Arc::clone)
    }

    /// Whether a service of type `T` is registered and readable here.
    pub fn has<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.allows::<T>() && self.services.read().contains_key(&TypeId::of::<T>())
    }

    /// The event bus.
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// Shared handle to the event bus (for subscriptions that need `Arc`).
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.events
    }

    /// Subscribe a waterfall handler. During [`mount`](Self::mount) the guard is
    /// retained by the context, so a plugin's handler lives as long as the plugin
    /// — the same rule as a service.
    pub fn on_waterfall<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::WaterfallEvent + Send + Sync,
        F: Fn(&mut E) + Send + Sync + 'static,
    {
        let registration = self.events.waterfall::<E, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a serial handler, retained the same way.
    pub fn on_serial<E, R, F>(&self, handler: F) -> Registration
    where
        E: crate::events::SerialEvent + Send + Sync,
        R: Send + Sync + 'static,
        F: Fn(&E) -> Option<R> + Send + Sync + 'static,
    {
        let registration = self.events.on_serial::<E, R, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a notification handler on the bus's delivery thread, retained
    /// the same way.
    pub fn on_event<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let registration = self.events.subscribe::<E, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a notification handler on the host's main thread, retained the
    /// same way. In the editor that is the UI thread.
    pub fn on_event_main<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let registration = self.events.subscribe_main::<E, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Note a mount that did not take. One entry per plugin id: a retry replaces
    /// the previous reason rather than stacking another row.
    fn record_failure(&self, manifest: PluginManifest, err: &PluginError) {
        tracing::warn!(plugin = %manifest.id, error = %err, "plugin mount failed");
        let mut failures = self.failures.write();
        failures.retain(|f| f.id != manifest.id);
        failures.push(MountFailure {
            id: manifest.id.to_string(),
            name: manifest.name.to_string(),
            reason: err.to_string(),
        });
    }

    /// Note a plugin file that could not be loaded at all.
    ///
    /// An external plugin has no manifest until its library loads, so the file
    /// name stands in for the id. Returned as well as recorded, because the
    /// loader reports its own batch.
    pub(crate) fn record_external_failure(
        &self,
        id: String,
        name: String,
        reason: String,
    ) -> MountFailure {
        tracing::warn!(plugin = %id, reason = %reason, "external plugin failed to load");
        let failure = MountFailure { id, name, reason };
        let mut failures = self.failures.write();
        failures.retain(|held| held.id != failure.id);
        failures.push(failure.clone());
        failure
    }

    /// Mounts that failed, in attempt order.
    pub fn failures(&self) -> Vec<MountFailure> {
        self.failures.read().clone()
    }

    /// Mount a plugin in dependency order.
    ///
    /// Every failure is recorded in the failure ledger as well as returned, so a
    /// host that keeps going after one bad plugin can still tell the user which
    /// capability is missing. A later successful mount of the same id clears its
    /// entry.
    pub fn mount(&self, plugin: &dyn Plugin) -> Result<(), PluginError> {
        let manifest = *plugin.manifest();
        tracing::info!(plugin = %manifest.id, "mounting plugin");
        for dep in manifest.requires {
            let found = self.plugins.read().iter().any(|p| p.manifest.id == *dep);
            if !found {
                let err = PluginError::MissingDependency(manifest.id, dep);
                self.record_failure(manifest, &err);
                return Err(err);
            }
        }
        {
            let mut pending = self.pending.lock();
            if pending.is_some() {
                return Err(PluginError::MountFailed(
                    manifest.id,
                    "nested plugin mount is not supported".into(),
                ));
            }
            *pending = Some(Vec::new());
        }
        // The plugin never sees the host's own context: it gets the view its
        // manifest describes, and a handler it registers keeps that view because a
        // clone carries the scope.
        let scoped = self.view(manifest.id, manifest.uses);
        // Around the plugin's own work only: the dependency check and the
        // registration bookkeeping are the host's cost, and attributing them to
        // the plugin would make every mount look alike.
        let at = Instant::now();
        let caught =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plugin.mount(&scoped)));
        let elapsed = at.elapsed();
        let registrations = self.pending.lock().take().unwrap_or_default();
        let mount_result = match caught {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(PluginError::MountFailed(manifest.id, e.to_string())),
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "plugin panicked during mount".to_string()
                };
                Err(PluginError::MountFailed(
                    manifest.id,
                    format!("panic: {msg}"),
                ))
            }
        };
        if let Err(err) = mount_result {
            drop(registrations);
            self.record_failure(manifest, &err);
            return Err(err);
        }
        self.failures.write().retain(|f| f.id != manifest.id);
        self.plugins.write().push(MountedPlugin {
            manifest,
            registrations,
            mount: elapsed,
        });
        self.events.post(PluginActivated { id: manifest.id });
        Ok(())
    }

    /// Unmount a plugin by id.
    pub fn unmount(&self, plugin_id: &str) -> Result<(), PluginError> {
        let mut plugins = self.plugins.write();
        let idx = plugins
            .iter()
            .position(|p| p.manifest.id == plugin_id)
            .ok_or_else(|| {
                PluginError::UnmountFailed("unknown", format!("plugin not found: {plugin_id}"))
            })?;
        let removed = plugins.remove(idx);
        drop(plugins);
        drop(removed.registrations);
        self.denials.write().retain(|d| d.plugin != plugin_id);
        self.failures.write().retain(|f| f.id != plugin_id);
        tracing::info!(plugin = %removed.manifest.id, "unmounted plugin");
        self.events.post(PluginDeactivated {
            id: removed.manifest.id,
        });
        Ok(())
    }

    /// Mounted plugin ids, in mount order.
    pub fn plugins(&self) -> Vec<&'static str> {
        self.plugins.read().iter().map(|p| p.manifest.id).collect()
    }

    /// Every mounted plugin with the services it registered, in mount order.
    ///
    /// This is what a host lists: the manifest is the plugin's own description and
    /// the service names come from the registrations it made, so a plugin cannot
    /// claim a capability it did not register.
    pub fn mounted(&self) -> Vec<MountedPluginInfo> {
        self.plugins
            .read()
            .iter()
            .map(|plugin| MountedPluginInfo {
                manifest: plugin.manifest,
                services: plugin
                    .registrations
                    .iter()
                    .filter_map(Registration::service_name)
                    .collect(),
                mount: plugin.mount,
            })
            .collect()
    }
}

impl Default for PluginContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
