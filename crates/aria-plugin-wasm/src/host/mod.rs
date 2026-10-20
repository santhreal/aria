//! Host implementation of `world aria:plugin/plugin@1.0.0` and `aria_plugin::Plugin`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use aria_plugin::package::{Access, Package, register_declared};
use aria_plugin::{Names, Plugin, PluginContext, PluginError, PluginManifest};
use parking_lot::Mutex;
use wasmtime::component::{Component as WasmComponent, HasSelf, Linker};
use wasmtime::{Store, StoreLimits, StoreLimitsBuilder};

use crate::bindings;
use crate::events::UnmountSentinel;
use crate::legacy_bindings;
use crate::runtime::{SharedEngine, classify_trap, deadline_ticks};
use crate::{Limits, LoadError, WORLD};

mod media;
mod panels;
mod project;
mod registration;
mod storage;

#[cfg(test)]
pub(crate) use self::storage::sanitize_relative_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallPhase {
    Idle,
    Mount,
    Unmount,
    Command,
    Event,
    Import,
    Export,
    Audio,
    Panel,
    Overlay,
    Encode,
    Job,
    Decode,
    AiTool,
    Scope,
    CpuEffect,
    Driver,
    Analyzer,
}

#[derive(Default)]
pub(super) struct ActiveExport {
    pub(super) path: String,
    pub(super) bytes: Vec<u8>,
}

pub(crate) struct HostState {
    pub(super) shared: Weak<WasmPluginShared>,
    pub(super) package: Package,
    pub(super) data_root: PathBuf,
    pub(super) limiter: StoreLimits,
    pub(super) phase: CallPhase,
    pub(super) active_ctx: Option<PluginContext>,
    /// The file an import or decoder call reads, readable for that call.
    pub(super) active_read_path: Option<PathBuf>,
    pub(super) active_export: Option<ActiveExport>,
    pub(super) active_encode_path: Option<PathBuf>,
}

pub(super) enum PluginPreKind {
    Current(bindings::PluginPre<HostState>),
    Legacy(legacy_bindings::LegacyPluginPre<HostState>),
}

pub(super) enum PluginKind {
    Current(bindings::Plugin),
    Legacy(legacy_bindings::LegacyPlugin),
}

impl PluginKind {
    pub(super) fn current(&self) -> wasmtime::Result<&bindings::Plugin> {
        match self {
            Self::Current(plugin) => Ok(plugin),
            Self::Legacy(_) => Err(wasmtime::Error::msg(
                "legacy @1.0.0 component does not export this function",
            )),
        }
    }
}

pub(super) struct LiveInstance {
    pub(super) store: Store<HostState>,
    pub(super) plugin: PluginKind,
}

pub(super) enum InstanceState {
    Unmounted,
    Mounted(LiveInstance),
    Faulted(String),
}

pub(crate) struct WasmPluginShared {
    pub(super) package: Package,
    pub(super) manifest: PluginManifest,
    pub(super) limits: Limits,
    pub(super) data_root: PathBuf,
    pub(super) engine: &'static SharedEngine,
    /// The component, linked against the host and type-checked against the
    /// world at load, so a mismatch fails the load before any guest code runs.
    pub(super) pre: PluginPreKind,
    pub(super) state: Mutex<InstanceState>,
    pub(super) active_thread: Mutex<Option<ThreadId>>,
    /// The id the next `start-job` hands the guest.
    pub(super) next_job: AtomicU64,
}

pub(super) struct ActiveThreadGuard<'a> {
    slot: &'a Mutex<Option<ThreadId>>,
}

impl<'a> ActiveThreadGuard<'a> {
    pub(super) fn enter(slot: &'a Mutex<Option<ThreadId>>) -> Self {
        *slot.lock() = Some(std::thread::current().id());
        Self { slot }
    }
}

impl Drop for ActiveThreadGuard<'_> {
    fn drop(&mut self) {
        *self.slot.lock() = None;
    }
}

struct UnmountOnDrop {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
}

impl Drop for UnmountOnDrop {
    fn drop(&mut self) {
        let _ = self.shared.on_context_unmount(&self.ctx);
    }
}

/// Loaded WebAssembly component plugin implementing [`aria_plugin::Plugin`].

pub struct WasmPlugin {
    shared: Arc<WasmPluginShared>,
}

impl std::fmt::Debug for WasmPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPlugin")
            .field("id", &self.shared.manifest.id)
            .field("version", &self.shared.manifest.version)
            .finish()
    }
}

impl WasmPlugin {
    pub(crate) fn new(
        package: &Package,
        limits: &Limits,
        data_root: PathBuf,
        component_path: &Path,
    ) -> Result<Self, LoadError> {
        let engine = SharedEngine::get().map_err(LoadError::Engine)?;
        let bytes = fs::read(component_path).map_err(|err| LoadError::ReadComponent {
            path: component_path.to_path_buf(),
            reason: err.to_string(),
        })?;
        let component = WasmComponent::from_binary(engine.engine(), &bytes).map_err(|err| {
            LoadError::Compile {
                path: component_path.to_path_buf(),
                reason: err.to_string(),
            }
        })?;

        let mut linker = Linker::<HostState>::new(engine.engine());
        bindings::Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state).map_err(
            |err| LoadError::Compile {
                path: component_path.to_path_buf(),
                reason: format!("failed to wire host imports: {err}"),
            },
        )?;
        // Every import resolved against the host and every export the world
        // requires present with its type: the plugin's WIT version check.
        let instance_pre =
            linker
                .instantiate_pre(&component)
                .map_err(|err| LoadError::Compile {
                    path: component_path.to_path_buf(),
                    reason: format!("does not match world {WORLD}: {err:#}"),
                })?;
        // A component built against the current world binds as one. One built
        // against 1.0 lacks the exports added since and binds through the
        // legacy world. A component that binds as neither reports the current
        // world's mismatch.
        let pre = match bindings::PluginPre::new(instance_pre.clone()) {
            Ok(pre) => PluginPreKind::Current(pre),
            Err(err) => match legacy_bindings::LegacyPluginPre::new(instance_pre) {
                Ok(pre) => PluginPreKind::Legacy(pre),
                Err(_) => {
                    return Err(LoadError::Compile {
                        path: component_path.to_path_buf(),
                        reason: format!("does not match world {WORLD}: {err:#}"),
                    });
                }
            },
        };

        let mut manifest = package.plugin_manifest();
        if package.manifest.permissions.project != Access::None
            && !manifest.uses.contains("EditorView")
        {
            let mut uses: Vec<String> = manifest.uses.iter().map(str::to_owned).collect();
            uses.push("EditorView".to_owned());
            manifest.uses = Names::Owned(uses);
        }

        let shared = Arc::new(WasmPluginShared {
            package: package.clone(),
            manifest,
            limits: limits.clone(),
            data_root,
            engine,
            pre,
            state: Mutex::new(InstanceState::Unmounted),
            active_thread: Mutex::new(None),
            next_job: AtomicU64::new(1),
        });

        Ok(Self { shared })
    }
}

impl Plugin for WasmPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.shared.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_declared(ctx, &self.shared.package)?;
        self.shared.mount(ctx)?;

        let guard = UnmountOnDrop {
            shared: Arc::clone(&self.shared),
            ctx: ctx.clone(),
        };
        ctx.on_event::<UnmountSentinel, _>(move |_ev| {
            let _keep_alive = &guard;
        });

        Ok(())
    }

    fn unmount(&self, ctx: &PluginContext) {
        let _ = self.shared.on_context_unmount(ctx);
    }
}

impl WasmPluginShared {
    pub(crate) fn plugin_id(&self) -> &str {
        &self.manifest.id
    }

    pub(crate) fn has_parity_exports(&self) -> bool {
        matches!(self.pre, PluginPreKind::Current(_))
    }

    fn build_store(self: &Arc<Self>) -> Store<HostState> {
        let limiter = StoreLimitsBuilder::new()
            .memory_size(self.limits.memory_bytes)
            .instances(16)
            .tables(16)
            .memories(16)
            .build();
        let host_state = HostState {
            shared: Arc::downgrade(self),
            package: self.package.clone(),
            data_root: self.data_root.clone(),
            limiter,
            phase: CallPhase::Idle,
            active_ctx: None,
            active_read_path: None,
            active_export: None,
            active_encode_path: None,
        };
        let mut store = Store::new(self.engine.engine(), host_state);
        store.limiter(|state| &mut state.limiter);
        let _ = store.set_fuel(self.limits.fuel_per_call);
        store.set_epoch_deadline(deadline_ticks(self.limits.call_deadline));
        store
    }

    pub(super) fn prepare_call(
        &self,
        store: &mut Store<HostState>,
        phase: CallPhase,
        ctx: &PluginContext,
    ) {
        let _ = store.set_fuel(self.limits.fuel_per_call);
        store.set_epoch_deadline(deadline_ticks(self.limits.call_deadline));
        let data = store.data_mut();
        data.phase = phase;
        data.active_ctx = Some(ctx.clone());
    }

    pub(super) fn finish_call(store: &mut Store<HostState>) {
        let data = store.data_mut();
        data.phase = CallPhase::Idle;
        data.active_ctx = None;
        data.active_read_path = None;
        data.active_encode_path = None;
    }

    fn mount(self: &Arc<Self>, ctx: &PluginContext) -> Result<(), PluginError> {
        let mut store = self.build_store();
        let plugin = match &self.pre {
            PluginPreKind::Current(pre) => pre
                .instantiate(&mut store)
                .map(PluginKind::Current)
                .map_err(|err| {
                    PluginError::MountFailed(
                        self.manifest.id.clone(),
                        format!("failed to instantiate wasm component: {err}"),
                    )
                })?,
            PluginPreKind::Legacy(pre) => pre
                .instantiate(&mut store)
                .map(PluginKind::Legacy)
                .map_err(|err| {
                    PluginError::MountFailed(
                        self.manifest.id.clone(),
                        format!("failed to instantiate wasm component: {err}"),
                    )
                })?,
        };

        self.prepare_call(&mut store, CallPhase::Mount, ctx);
        let call_result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            match &plugin {
                PluginKind::Current(p) => p.call_mount(&mut store),
                PluginKind::Legacy(p) => p.call_mount(&mut store),
            }
        };
        Self::finish_call(&mut store);

        match call_result {
            Ok(Ok(())) => {
                *self.state.lock() = InstanceState::Mounted(LiveInstance { store, plugin });
                Ok(())
            }
            Ok(Err(reason)) => {
                *self.state.lock() = InstanceState::Unmounted;
                Err(PluginError::MountFailed(self.manifest.id.clone(), reason))
            }
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *self.state.lock() = InstanceState::Faulted(reason.clone());
                Err(PluginError::MountFailed(self.manifest.id.clone(), reason))
            }
        }
    }

    pub(crate) fn on_context_unmount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let taken = {
            let mut state = self.state.lock();
            match std::mem::replace(&mut *state, InstanceState::Unmounted) {
                InstanceState::Mounted(live) => Some(live),
                InstanceState::Unmounted => None,
                InstanceState::Faulted(reason) => {
                    *state = InstanceState::Faulted(reason);
                    None
                }
            }
        };
        let Some(mut live) = taken else {
            return Ok(());
        };

        self.prepare_call(&mut live.store, CallPhase::Unmount, ctx);
        let call_result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            match &live.plugin {
                PluginKind::Current(p) => p.call_unmount(&mut live.store),
                PluginKind::Legacy(p) => p.call_unmount(&mut live.store),
            }
        };
        Self::finish_call(&mut live.store);

        match call_result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(PluginError::UnmountFailed(self.manifest.id.clone(), reason)),
            Err(trap_err) => Err(PluginError::UnmountFailed(
                self.manifest.id.clone(),
                classify_trap(&trap_err),
            )),
        }
    }

    pub(super) fn run_with_instance<T>(
        &self,
        ctx: &PluginContext,
        phase: CallPhase,
        f: impl FnOnce(&mut LiveInstance, &SharedEngine) -> wasmtime::Result<Result<T, String>>,
    ) -> Result<T, String> {
        let mut guard = self.state.lock();
        let live = match &mut *guard {
            InstanceState::Mounted(live) => live,
            InstanceState::Faulted(reason) => {
                return Err(format!("plugin {} has faulted: {reason}", self.manifest.id));
            }
            InstanceState::Unmounted => {
                return Err(format!("plugin {} is not mounted", self.manifest.id));
            }
        };

        self.prepare_call(&mut live.store, phase, ctx);
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            f(live, self.engine)
        };
        Self::finish_call(&mut live.store);

        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                ctx.fail(&self.manifest.id, reason.clone());
                Err(reason)
            }
        }
    }

    pub(super) fn run_live_call<T>(
        &self,
        ctx: &PluginContext,
        phase: CallPhase,
        f: impl FnOnce(&mut LiveInstance) -> wasmtime::Result<T>,
    ) -> T {
        let mut guard = self.state.lock();
        let live = match &mut *guard {
            InstanceState::Mounted(live) => live,
            InstanceState::Faulted(reason) => {
                let msg = format!("plugin {} has faulted: {reason}", self.manifest.id);
                drop(guard);
                panic!("{msg}");
            }
            InstanceState::Unmounted => {
                let msg = format!("plugin {} is not mounted", self.manifest.id);
                drop(guard);
                panic!("{msg}");
            }
        };
        self.prepare_call(&mut live.store, phase, ctx);
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            f(live)
        };
        Self::finish_call(&mut live.store);
        match result {
            Ok(value) => value,
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                panic!("{reason}");
            }
        }
    }
}

impl HostState {
    pub(super) fn require_mount_phase(&self, op: &str) -> Result<PluginContext, String> {
        if self.phase != CallPhase::Mount {
            return Err(format!(
                "{}: {op} is permitted only during mount",
                self.package.manifest.id
            ));
        }
        self.active_ctx
            .clone()
            .ok_or_else(|| "missing active plugin context".to_owned())
    }

    pub(super) fn require_ctx(&self) -> Result<PluginContext, String> {
        self.active_ctx
            .clone()
            .ok_or_else(|| "missing active plugin context".to_owned())
    }
}

impl bindings::PluginImports for HostState {
    fn log(&mut self, level: bindings::LogLevel, text: String) {
        self.host_log(level, text);
    }

    fn storage_get(&mut self, key: String) -> Result<Option<Vec<u8>>, String> {
        self.host_storage_get(key)
    }

    fn storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        self.host_storage_set(key, value)
    }

    fn storage_delete(&mut self, key: String) -> Result<(), String> {
        self.host_storage_delete(key)
    }

    fn storage_keys(&mut self) -> Result<Vec<String>, String> {
        self.host_storage_keys()
    }

    fn setting_get(&mut self, key: String) -> Result<Option<String>, String> {
        self.host_setting_get(key)
    }

    fn start_job(&mut self, label: String) -> Result<u64, String> {
        self.host_start_job(label)
    }

    fn register_command(&mut self, id: String, title: String) -> Result<(), String> {
        self.host_register_command(id, title)
    }

    fn subscribe(&mut self, event_kind: String) -> Result<(), String> {
        self.host_subscribe(event_kind)
    }

    fn register_importer(&mut self, spec: bindings::ImporterSpec) -> Result<(), String> {
        self.host_register_importer(spec)
    }

    fn register_exporter(&mut self, spec: bindings::ExporterSpec) -> Result<(), String> {
        self.host_register_exporter(spec)
    }

    fn register_encoder(&mut self, spec: bindings::EncoderSpec) -> Result<(), String> {
        self.host_register_encoder(spec)
    }

    fn register_decoder(&mut self, spec: bindings::DecoderSpec) -> Result<(), String> {
        self.host_register_decoder(spec)
    }

    fn register_ai_tool(&mut self, spec: bindings::AiToolSpec) -> Result<(), String> {
        self.host_register_ai_tool(spec)
    }

    fn register_scope(&mut self, spec: bindings::ScopeSpec) -> Result<(), String> {
        self.host_register_scope(spec)
    }

    fn register_audio_effect(&mut self, spec: bindings::AudioEffectSpec) -> Result<(), String> {
        self.host_register_audio_effect(spec)
    }

    fn register_panel(&mut self, spec: bindings::PanelSpec) -> Result<(), String> {
        self.host_register_panel(spec)
    }

    fn refresh_panel(&mut self, id: String) -> Result<(), String> {
        self.host_refresh_panel(id)
    }

    fn register_viewer_overlay(&mut self, spec: bindings::OverlaySpec) -> Result<(), String> {
        self.host_register_viewer_overlay(spec)
    }

    fn refresh_viewer_overlay(&mut self, id: String) -> Result<(), String> {
        self.host_refresh_viewer_overlay(id)
    }

    fn set_effect_category(&mut self, id: String, category: String) -> Result<(), String> {
        self.host_set_effect_category(id, category)
    }

    fn register_effect(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        self.host_register_effect(spec, passes)
    }

    fn register_cpu_effect(&mut self, spec: bindings::EffectSpec) -> Result<(), String> {
        self.host_register_cpu_effect(spec)
    }

    fn register_transition(
        &mut self,
        spec: bindings::TransitionSpec,
        wgsl: String,
    ) -> Result<(), String> {
        self.host_register_transition(spec, wgsl)
    }

    fn register_generator(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        self.host_register_generator(spec, passes)
    }

    fn register_effect_graph(
        &mut self,
        spec: bindings::EffectSpec,
        traits: bindings::EffectTraits,
        passes: Vec<bindings::GraphPass>,
    ) -> Result<(), String> {
        self.host_register_effect_graph(spec, traits, passes)
    }

    fn register_texture(
        &mut self,
        id: String,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<(), String> {
        self.host_register_texture(id, width, height, rgba8)
    }

    fn register_clip_type(&mut self, spec: bindings::ClipTypeSpec) -> Result<(), String> {
        self.host_register_clip_type(spec)
    }

    fn define_clip_type(
        &mut self,
        spec: bindings::ClipTypeSpec,
        schema: bindings::ClipTypeSchema,
        render: bindings::ClipTypeRender,
    ) -> Result<(), String> {
        self.host_define_clip_type(spec, schema, render)
    }

    fn register_driver_language(
        &mut self,
        spec: bindings::DriverLanguageSpec,
    ) -> Result<(), String> {
        self.host_register_driver_language(spec)
    }

    fn register_preset(&mut self, preset_json: String) -> Result<(), String> {
        self.host_register_preset(preset_json)
    }

    fn register_theme(&mut self, id: String, toml: String) -> Result<(), String> {
        self.host_register_theme(id, toml)
    }

    fn register_workspace(&mut self, spec: bindings::WorkspaceSpec) -> Result<(), String> {
        self.host_register_workspace(spec)
    }

    fn register_analyzer(&mut self, spec: bindings::AnalyzerSpec) -> Result<(), String> {
        self.host_register_analyzer(spec)
    }

    fn project_snapshot(&mut self) -> Result<String, String> {
        self.host_project_snapshot()
    }

    fn editor_view(&mut self) -> Result<String, String> {
        self.host_editor_view()
    }

    fn clip(&mut self, id: u64) -> Result<Option<bindings::ClipRef>, String> {
        self.host_clip(id)
    }

    fn clips_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        self.host_clips_in_range(sequence, range)
    }

    fn clips_at(
        &mut self,
        sequence: Option<u64>,
        tick: bindings::RationalTime,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        self.host_clips_at(sequence, tick)
    }

    fn selected_clips(&mut self) -> Result<Vec<bindings::ClipRef>, String> {
        self.host_selected_clips()
    }

    fn markers_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::Marker>, String> {
        self.host_markers_in_range(sequence, range)
    }

    fn in_out(&mut self) -> Result<bindings::InOutMarks, String> {
        self.host_in_out()
    }

    fn project_transact(&mut self, label: String, commands_json: String) -> Result<String, String> {
        self.host_project_transact(label, commands_json)
    }

    fn project_undo(&mut self) -> Result<bool, String> {
        self.host_project_undo()
    }

    fn fs_read(&mut self, path: String) -> Result<Vec<u8>, String> {
        self.host_fs_read(path)
    }

    fn fs_read_at(&mut self, path: String, offset: u64, len: u32) -> Result<Vec<u8>, String> {
        self.host_fs_read_at(path, offset, len)
    }

    fn fs_len(&mut self, path: String) -> Result<u64, String> {
        self.host_fs_len(path)
    }

    fn fs_write(&mut self, path: String, bytes: Vec<u8>) -> Result<(), String> {
        self.host_fs_write(path, bytes)
    }
}
