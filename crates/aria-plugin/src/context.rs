//! Plugin context — typed service registry and event-bus host.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;

use crate::activation::Activations;
use crate::ai_tools::AiTools;
use crate::analyzers::Analyzers;
use crate::commands::Commands;
use crate::cost::{CostLedger, CostMetric};
use crate::decoders::Decoders;
use crate::drivers::DriverLanguages;
use crate::error::PluginError;
use crate::events::EventBus;
use crate::interchange::Interchange;
use crate::jobs::PluginJobs;
use crate::panels::Panels;
use crate::plugin::PluginManifest;
use crate::plugin_log::PluginLogs;
use crate::presets::Presets;
use crate::registration::{Contribution, ContributionKind, Registration};
use crate::scopes::Scopes;
use crate::themes::Themes;
use crate::viewer_overlay::Overlays;
use crate::workspaces::Workspaces;

pub(crate) type ServiceMap = RwLock<FxHashMap<TypeId, Box<dyn Any + Send + Sync>>>;

/// The shared context every plugin registers services and events into.
///
/// A clone shares the registry, the bus, and the ledgers. A clone made by
/// [`PluginContext::view`] also carries a capability scope: the plugin's id and
/// the services its manifest declared. Registration is never scoped — a plugin
/// providing a capability is not consuming one.
#[derive(Clone)]
pub struct PluginContext {
    shared: Arc<Shared>,
    /// `None` on the host's own context, which reads everything. `Some` on a
    /// plugin's view: the manifest whose id and service names scope it.
    scope: Option<Arc<PluginManifest>>,
}

impl std::fmt::Debug for PluginContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginContext")
            .field("scope", &self.scope.as_ref().map(|m| m.id.as_ref()))
            .finish_non_exhaustive()
    }
}

/// A context that does not keep its registries alive, for a service the
/// context itself holds: [`upgrade`](Self::upgrade) yields the host's context
/// while any other clone is alive.
#[derive(Clone)]
pub struct WeakContext {
    shared: Weak<Shared>,
}

impl WeakContext {
    /// The host's context, or `None` once every clone has dropped.
    pub fn upgrade(&self) -> Option<PluginContext> {
        self.shared.upgrade().map(|shared| PluginContext {
            shared,
            scope: None,
        })
    }
}

impl std::fmt::Debug for WeakContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeakContext")
            .field("alive", &(self.shared.strong_count() > 0))
            .finish()
    }
}

/// What every clone of one context shares.
struct Shared {
    services: Arc<ServiceMap>,
    events: Arc<EventBus>,
    plugins: RwLock<Vec<MountedPlugin>>,
    /// Moves each time a plugin joins or leaves `plugins`, under its write
    /// lock.
    mounted_revision: AtomicU64,
    pending: Mutex<Option<Vec<Registration>>>,
    /// Plugins that failed, in the order they failed. A host reads this to
    /// report which capability is missing and why in the editor, instead of in
    /// a log line.
    failures: RwLock<Vec<MountFailure>>,
    /// Services asked for and refused, in the order they were asked for.
    denials: RwLock<Vec<CapabilityDenial>>,
    /// The latest recovered fault of each mounted plugin.
    faults: RwLock<Vec<PluginFault>>,
    /// Per-plugin runtime frame and audio-block costs.
    costs: CostLedger,
    /// The importers and exporters plugins registered.
    interchange: Arc<Interchange>,
    /// The palette commands plugins registered.
    commands: Arc<Commands>,
    /// The declared panels plugins registered.
    panels: Arc<Panels>,
    /// The viewer overlays plugins registered.
    overlays: Arc<Overlays>,
    /// The animation presets plugins registered.
    presets: Arc<Presets>,
    /// The themes plugins registered.
    themes: Arc<Themes>,
    /// The decoders plugins registered.
    decoders: Arc<Decoders>,
    /// The driver languages plugins registered.
    driver_languages: Arc<DriverLanguages>,
    /// The tools plugins offer the assistant.
    ai_tools: Arc<AiTools>,
    /// The audio analyzers plugins registered.
    analyzers: Arc<Analyzers>,
    /// The scopes plugins registered.
    scopes: Arc<Scopes>,
    /// The workspaces plugins registered.
    workspaces: Arc<Workspaces>,
    /// The background jobs plugins started.
    jobs: PluginJobs,
    /// The packages waiting for their first activation event.
    activations: Arc<Activations>,
    /// The lines each plugin logged, by plugin id.
    logs: PluginLogs,
}

/// A service a plugin asked for and was refused, because its manifest does not
/// declare it.
///
/// Recorded rather than logged: a plugin that forgot a declaration otherwise
/// degrades silently, since a surface whose service is missing hides itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    pub plugin: Cow<'static, str>,
    pub service: &'static str,
}

/// A plugin that is not running, and the reason: its mount did not take, or
/// its code faulted after mount and [`PluginContext::fail`] dropped it.
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

/// A fault in a mounted plugin's code that the host recovered from, such as an
/// audio processor bypassed for missing its playback deadlines. The plugin
/// stays mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFault {
    pub plugin: Cow<'static, str>,
    pub message: String,
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
    /// The commands, panels, importers, exporters, effects, transitions and
    /// audio effects this plugin registered while mounting, in registration
    /// order.
    pub contributions: Vec<Contribution>,
    /// How long this plugin's own `mount` took.
    ///
    /// Measured here because this is what calls it: a start is the sum of these
    /// plus what the host does around them, and without the breakdown a slow
    /// start is one number with nothing to act on. The GPU device, the catalogs
    /// and the model directory are all mounted, so this is where the time is.
    pub mount: Duration,
    /// Measured per-frame cost across the effects, transitions, and generators
    /// this plugin registered.
    pub frame_cost: CostMetric,
    /// Measured per-block cost across the audio processors this plugin
    /// registered.
    pub block_cost: CostMetric,
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
        crate::plugin_log::capture_panics();
        Self {
            shared: Arc::new(Shared {
                services: Arc::new(RwLock::new(FxHashMap::default())),
                events: Arc::new(EventBus::new()),
                plugins: RwLock::new(Vec::new()),
                mounted_revision: AtomicU64::new(0),
                pending: Mutex::new(None),
                failures: RwLock::new(Vec::new()),
                denials: RwLock::new(Vec::new()),
                faults: RwLock::new(Vec::new()),
                costs: CostLedger::new(),
                interchange: Arc::default(),
                commands: Arc::default(),
                panels: Arc::default(),
                overlays: Arc::default(),
                presets: Arc::default(),
                themes: Arc::default(),
                decoders: Arc::default(),
                driver_languages: Arc::default(),
                ai_tools: Arc::default(),
                scopes: Arc::default(),
                analyzers: Arc::default(),
                workspaces: Arc::default(),
                jobs: PluginJobs::default(),
                activations: Arc::default(),
                logs: PluginLogs::default(),
            }),
            scope: None,
        }
    }

    /// The same context seen through one plugin's manifest: it resolves the
    /// services that manifest declares and nothing else.
    ///
    /// [`mount`](Self::mount) builds this for every plugin it mounts, so a plugin
    /// never holds the unscoped context. A clone of the view keeps the scope, which
    /// is what makes a handler registered during mount stay inside it.
    pub fn view(&self, manifest: &PluginManifest) -> Self {
        self.scoped(Some(Arc::new(manifest.clone())))
    }

    /// The same context under `scope`: a plugin's view, or the host's own
    /// context for `None`.
    pub(crate) fn scoped(&self, scope: Option<Arc<PluginManifest>>) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            scope,
        }
    }

    /// The manifest this view is scoped to, or `None` on the host's context.
    pub(crate) fn scope(&self) -> Option<Arc<PluginManifest>> {
        self.scope.clone()
    }

    /// A handle that does not keep this context alive.
    pub fn downgrade(&self) -> WeakContext {
        WeakContext {
            shared: Arc::downgrade(&self.shared),
        }
    }

    /// Whether a plugin's mount is running, on any thread. Mounts do not nest.
    pub(crate) fn mounting(&self) -> bool {
        self.shared.pending.lock().is_some()
    }

    /// The plugin this view belongs to, or `None` on the host's context.
    pub(crate) fn plugin_scope(&self) -> Option<Cow<'static, str>> {
        self.scope.as_ref().map(|manifest| manifest.id.clone())
    }

    /// The importers and exporters registered on this context.
    pub(crate) fn interchange(&self) -> &Arc<Interchange> {
        &self.shared.interchange
    }

    /// The palette commands registered on this context.
    pub(crate) fn command_registry(&self) -> &Arc<Commands> {
        &self.shared.commands
    }

    /// The declared panels registered on this context.
    pub(crate) fn panel_registry(&self) -> &Arc<Panels> {
        &self.shared.panels
    }

    /// The viewer overlays registered on this context.
    pub(crate) fn overlay_registry(&self) -> &Arc<Overlays> {
        &self.shared.overlays
    }

    /// The animation presets registered on this context.
    pub(crate) fn preset_registry(&self) -> &Arc<Presets> {
        &self.shared.presets
    }

    /// The themes registered on this context.
    pub(crate) fn theme_registry(&self) -> &Arc<Themes> {
        &self.shared.themes
    }

    /// The decoders registered on this context.
    pub(crate) fn decoder_registry(&self) -> &Arc<Decoders> {
        &self.shared.decoders
    }

    /// The driver languages registered on this context.
    pub(crate) fn driver_language_registry(&self) -> &Arc<DriverLanguages> {
        &self.shared.driver_languages
    }

    /// The tools plugins offer the assistant on this context.
    pub(crate) fn ai_tool_registry(&self) -> &Arc<AiTools> {
        &self.shared.ai_tools
    }

    /// The audio analyzers registered on this context.
    pub(crate) fn analyzer_registry(&self) -> &Arc<Analyzers> {
        &self.shared.analyzers
    }

    /// The scopes registered on this context.
    pub(crate) fn scope_registry(&self) -> &Arc<Scopes> {
        &self.shared.scopes
    }

    /// The workspaces registered on this context.
    pub(crate) fn workspace_registry(&self) -> &Arc<Workspaces> {
        &self.shared.workspaces
    }

    /// The jobs plugins started on this context.
    pub(crate) fn job_registry(&self) -> &PluginJobs {
        &self.shared.jobs
    }

    /// The packages waiting for their first activation event.
    pub(crate) fn activation_registry(&self) -> &Arc<Activations> {
        &self.shared.activations
    }

    /// Services a plugin asked for and did not declare, in ask order.
    pub fn denials(&self) -> Vec<CapabilityDenial> {
        self.shared.denials.read().clone()
    }

    /// Whether this context may read `T`, recording a denial when it may not.
    ///
    /// The declared name is the short one (`StorageService`), because the full
    /// path of a `dyn` trait object is a compiler detail no manifest should have
    /// to spell.
    fn allows<T: ?Sized + 'static>(&self) -> bool {
        let Some(scope) = &self.scope else {
            return true;
        };
        let full = std::any::type_name::<T>();
        let short = short_service_name(full);
        if scope.uses.contains(short.as_ref()) {
            return true;
        }
        let denial = CapabilityDenial {
            plugin: scope.id.clone(),
            service: full,
        };
        let mut denials = self.shared.denials.write();
        if !denials.contains(&denial) {
            tracing::warn!(plugin = %denial.plugin, service = %full, "undeclared service refused");
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
        self.shared
            .services
            .write()
            .insert(type_id, Box::new(service));
        self.retain_if_mounting(Registration::service(
            type_id,
            std::any::type_name::<T>(),
            &self.shared.services,
        ))
    }

    /// Add an effect or generator drawn by WGSL passes
    /// ([`aria_compositor::EffectDefinition`]) or a CPU callback
    /// ([`aria_compositor::CpuEffectDefinition`]) to the effect catalog.
    ///
    /// The entry is listed, applied, resolved and rendered like a shipped one
    /// until the returned guard drops; during [`mount`](Self::mount) the context
    /// retains the guard for the plugin's lifetime. Refused when a pass does not
    /// check against the host prelude or the spec breaks a catalog rule; see
    /// [`aria_compositor::register_effect`]. A CPU callback that returns `Err`
    /// leaves the frame as passthrough and reports a fault in
    /// [`faults`](Self::faults); one that panics leaves the frame as passthrough
    /// and fails this plugin for the session through [`fail`](Self::fail).
    pub fn register_effect(
        &self,
        definition: impl Into<aria_compositor::EffectRender>,
    ) -> Result<Registration, PluginError> {
        let definition = definition.into();
        let contribution =
            Contribution::new(ContributionKind::Effect, definition.spec().id.clone());
        let guard = aria_compositor::register_effect_with_sinks(
            definition,
            Some(self.effect_fault_sink()),
            self.frame_cost_sink(),
        )?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Add a CPU-rendered effect or generator (`spec.generator == true`) to the
    /// effect catalog, on the terms of [`register_effect`](Self::register_effect).
    pub fn register_cpu_effect(
        &self,
        spec: aria_models::EffectSpec,
        effect: impl aria_compositor::CpuEffect + 'static,
    ) -> Result<Registration, PluginError> {
        self.register_effect(aria_compositor::CpuEffectDefinition::new(spec, effect))
    }
    /// Add a transition drawn by the definition's WGSL to the transition
    /// catalog, on the terms of [`register_effect`](Self::register_effect).
    pub fn register_transition(
        &self,
        definition: aria_compositor::TransitionDefinition,
    ) -> Result<Registration, PluginError> {
        let contribution =
            Contribution::new(ContributionKind::Transition, definition.spec.id.clone());
        let guard = aria_compositor::register_transition_with_sinks(
            definition,
            Some(self.effect_fault_sink()),
            self.frame_cost_sink(),
        )?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Add `image`, straight-alpha RGBA8, as the plugin texture `id`, which
    /// an effect pass reads with [`aria_models::PassInput::Texture`]. Held on
    /// the terms of [`register_effect`](Self::register_effect); an effect
    /// registered while the texture lives keeps drawing with it after the
    /// guard drops. Refused on the terms of
    /// [`aria_compositor::register_texture`].
    pub fn register_texture(
        &self,
        id: impl Into<String>,
        image: &aria_core::RgbaImage,
    ) -> Result<Registration, PluginError> {
        let id = id.into();
        let contribution = Contribution::new(ContributionKind::Texture, id.as_str());
        let guard = aria_compositor::register_texture(id, image)?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Add an audio effect: `spec` to the audio effect catalog, where the
    /// inspector and the mixer list it, and `factory` behind it, whose
    /// processors run it in every clip, lane and master chain that holds it,
    /// in playback and in export.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect).
    /// Refused when the catalog refuses the spec (see
    /// [`aria_models::register_audio_effect`]) or a processor is already
    /// registered under its id. A processor that panics is bypassed and fails
    /// this plugin for the session through [`fail`](Self::fail); one that
    /// misses three playback deadlines in a row is bypassed and reported in
    /// [`faults`](Self::faults).
    pub fn register_audio_effect(
        &self,
        spec: aria_models::AudioEffectSpec,
        factory: impl aria_render::AudioProcessorFactory + 'static,
    ) -> Result<Registration, PluginError> {
        let id = spec.id.clone();
        let contribution = Contribution::new(ContributionKind::AudioEffect, id.clone());
        let (entry, _) =
            aria_models::register_audio_effect(spec).map_err(PluginError::AudioEffectRefused)?;
        let processor = aria_render::register_audio_processor_with_cost(
            id,
            Arc::new(factory),
            Some(self.fault_sink()),
            self.audio_cost_sink(),
        )
        .map_err(PluginError::AudioEffectRefused)?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, (entry, processor))))
    }

    /// Set the Effect menu and Effects browser category of an audio or video
    /// effect registered by this plugin.
    pub fn set_effect_category(&self, id: &str, category: impl Into<String>) -> Result<(), String> {
        let is_own_effect = |regs: &[Registration]| {
            regs.iter().any(|reg| {
                reg.contribution().is_some_and(|c| {
                    c.id == id
                        && matches!(
                            c.kind,
                            ContributionKind::AudioEffect | ContributionKind::Effect
                        )
                })
            })
        };
        let owned = {
            let pending = self.shared.pending.lock();
            if let Some(list) = pending.as_ref() {
                is_own_effect(list)
            } else if let Some(plugin_id) = self.plugin_scope() {
                self.shared
                    .plugins
                    .read()
                    .iter()
                    .find(|m| m.manifest.id == plugin_id)
                    .is_some_and(|m| is_own_effect(&m.registrations))
            } else {
                false
            }
        };
        if !owned {
            return Err(format!("effect `{id}` is not registered by this plugin"));
        }
        aria_models::set_registered_effect_category(id, category)
    }

    pub(crate) fn effect_fault_sink(&self) -> aria_compositor::EffectFaultSink {
        let plugin = self.plugin_scope();
        let shared = Arc::downgrade(&self.shared);
        Arc::new(move |fault: &aria_compositor::EffectFault| {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let Some(plugin) = plugin.as_deref() else {
                tracing::warn!(%fault, "host cpu effect fault");
                return;
            };
            let context = Self {
                shared,
                scope: None,
            };
            match &fault.kind {
                aria_compositor::EffectFaultKind::Panicked(_) => {
                    context.fail(plugin, fault.to_string());
                }
                aria_compositor::EffectFaultKind::Failed(_)
                | aria_compositor::EffectFaultKind::MissedDeadline { .. } => {
                    context.report_fault(plugin, fault.to_string());
                }
            }
        })
    }

    /// Where the per-frame render timings of an effect or transition this
    /// context registers go. Captures a weak handle to the plugin's atomic
    /// cost slot so the render thread records without locking the context.
    pub(crate) fn frame_cost_sink(&self) -> Option<aria_compositor::EffectCostSink> {
        let plugin = self.plugin_scope()?;
        let slot = self.shared.costs.weak_slot(plugin);
        Some(Arc::new(move |took: Duration| {
            if let Some(slot) = slot.upgrade() {
                slot.record_frame(took);
            }
        }))
    }

    /// Where the per-block timings of an audio processor this context registers
    /// go. Captures a weak handle to the plugin's atomic cost slot so the audio
    /// thread records without locking the context.
    fn audio_cost_sink(&self) -> Option<aria_render::CostSink> {
        let plugin = self.plugin_scope()?;
        let slot = self.shared.costs.weak_slot(plugin);
        Some(Arc::new(move |took: Duration| {
            if let Some(slot) = slot.upgrade() {
                slot.record_block(took);
            }
        }))
    }

    /// Where the faults of a processor this context registers go. Holds the
    /// context weakly: the processor registry is process-wide, and a strong
    /// hold would keep a dropped context's plugins registered.
    fn fault_sink(&self) -> aria_render::FaultSink {
        let plugin = self.plugin_scope();
        let shared = Arc::downgrade(&self.shared);
        Arc::new(move |fault: &aria_render::ProcessorFault| {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let Some(plugin) = plugin.as_deref() else {
                tracing::warn!(%fault, "host audio processor fault");
                return;
            };
            let context = Self {
                shared,
                scope: None,
            };
            match &fault.kind {
                aria_render::FaultKind::Panicked(_) => {
                    // Dropping a plugin runs its registrations' drop code,
                    // which has no place on the audio thread.
                    let weak = Arc::downgrade(&context.shared);
                    let reason = fault.to_string();
                    let plugin = plugin.to_owned();
                    context.shared.events.defer(move || {
                        if let Some(shared) = weak.upgrade() {
                            let context = Self {
                                shared,
                                scope: None,
                            };
                            context.fail(&plugin, reason);
                        }
                    });
                }
                aria_render::FaultKind::MissedDeadlines { .. } => {
                    context.report_fault(plugin, fault.to_string());
                }
            }
        })
    }

    pub(crate) fn retain_if_mounting(&self, registration: Registration) -> Registration {
        let mut pending = self.shared.pending.lock();
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
        let map = self.shared.services.read();
        let boxed = map.get(&TypeId::of::<T>())?;
        boxed.downcast_ref::<Arc<T>>().map(Arc::clone)
    }

    /// Whether a service of type `T` is registered and readable here.
    pub fn has<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.allows::<T>() && self.shared.services.read().contains_key(&TypeId::of::<T>())
    }

    fn allows_project_read(&self) -> bool {
        if let Some(scope) = &self.scope
            && (scope.uses.contains("ProjectHost") || scope.uses.contains("EditorView"))
        {
            return true;
        }
        self.allows::<crate::ProjectHost>()
    }

    fn unscoped_editor_view(&self) -> Option<Arc<crate::EditorView>> {
        let map = self.shared.services.read();
        let boxed = map.get(&TypeId::of::<crate::EditorView>())?;
        boxed
            .downcast_ref::<Arc<crate::EditorView>>()
            .map(Arc::clone)
    }

    /// Look up `id` across the project's sequences. Requires `ProjectHost` read access.
    pub fn clip(&self, id: crate::ClipId) -> Option<crate::ClipRef> {
        let host = self.try_get::<crate::ProjectHost>()?;
        host.clip(id)
    }

    /// Clips on `sequence` (`None` for the open sequence) overlapping the half-open
    /// `range`, in track order then timeline start order. Requires `ProjectHost` read access.
    pub fn clips_in_range(
        &self,
        sequence: Option<crate::SequenceId>,
        range: crate::TimeRange,
    ) -> Vec<crate::ClipRef> {
        let Some(host) = self.try_get::<crate::ProjectHost>() else {
            return Vec::new();
        };
        host.clips_in_range(sequence, range)
    }

    /// Clips on `sequence` (`None` for the open sequence) covering `tick`,
    /// in track order then timeline start order. Requires `ProjectHost` read access.
    pub fn clips_at(
        &self,
        sequence: Option<crate::SequenceId>,
        tick: crate::RationalTime,
    ) -> Vec<crate::ClipRef> {
        let Some(host) = self.try_get::<crate::ProjectHost>() else {
            return Vec::new();
        };
        host.clips_at(sequence, tick)
    }

    /// Resolve the current [`EditorView`](crate::EditorView) selection to
    /// [`ClipRef`](crate::ClipRef)s, dropping any ids no longer in the project.
    /// Requires `ProjectHost` read access.
    pub fn selected_clips(&self) -> Vec<crate::ClipRef> {
        let Some(host) = self.try_get::<crate::ProjectHost>() else {
            return Vec::new();
        };
        let selection = self
            .unscoped_editor_view()
            .and_then(|view| view.current())
            .map(|state| state.selection)
            .unwrap_or_default();
        host.selected_clips(&selection)
    }

    /// Ruler markers on `sequence` (`None` for the open sequence) overlapping
    /// the half-open `range`, in timeline order. Requires `ProjectHost` read access.
    pub fn markers_in_range(
        &self,
        sequence: Option<crate::SequenceId>,
        range: crate::TimeRange,
    ) -> Vec<crate::Marker> {
        let Some(host) = self.try_get::<crate::ProjectHost>() else {
            return Vec::new();
        };
        host.markers_in_range(sequence, range)
    }

    /// Sequence In and Out points from [`EditorView`](crate::EditorView).
    /// Requires `project` read access (`ProjectHost` or `EditorView`).
    pub fn in_out(&self) -> Option<crate::InOut> {
        if !self.allows_project_read() {
            return None;
        }
        let state = self.unscoped_editor_view().and_then(|view| view.current());
        Some(state.as_ref().map(crate::in_out).unwrap_or_default())
    }

    /// The event bus.
    pub fn events(&self) -> &EventBus {
        &self.shared.events
    }

    /// Shared handle to the event bus (for subscriptions that need `Arc`).
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.shared.events
    }

    /// Subscribe a waterfall handler. During [`mount`](Self::mount) the guard is
    /// retained by the context, so a plugin's handler lives as long as the plugin
    /// — the same rule as a service.
    pub fn on_waterfall<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::WaterfallEvent + Send + Sync,
        F: Fn(&mut E) + Send + Sync + 'static,
    {
        let registration = self.shared.events.waterfall::<E, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a serial handler, retained the same way.
    pub fn on_serial<E, R, F>(&self, handler: F) -> Registration
    where
        E: crate::events::SerialEvent + Send + Sync,
        R: Send + Sync + 'static,
        F: Fn(&E) -> Option<R> + Send + Sync + 'static,
    {
        let registration = self.shared.events.on_serial::<E, R, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a serial handler with explicit `priority` (higher runs first;
    /// ties run in mount order), retained the same way as [`on_serial`](Self::on_serial).
    pub fn on_serial_with_priority<E, R, F>(&self, priority: i32, handler: F) -> Registration
    where
        E: crate::events::SerialEvent + Send + Sync,
        R: Send + Sync + 'static,
        F: Fn(&E) -> Option<R> + Send + Sync + 'static,
    {
        let registration = self
            .shared
            .events
            .on_serial_with_priority::<E, R, F>(priority, handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a notification handler on the bus's delivery thread, retained
    /// the same way.
    pub fn on_event<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let registration = self.shared.events.subscribe::<E, F>(handler);
        self.retain_if_mounting(registration)
    }

    /// Subscribe a notification handler on the host's main thread, retained the
    /// same way. In the editor that is the UI thread.
    pub fn on_event_main<E, F>(&self, handler: F) -> Registration
    where
        E: crate::events::Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let registration = self.shared.events.subscribe_main::<E, F>(handler);
        self.retain_if_mounting(registration)
    }
}

impl Default for PluginContext {
    fn default() -> Self {
        Self::new()
    }
}

#[path = "context_lifecycle.rs"]
mod lifecycle;

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
