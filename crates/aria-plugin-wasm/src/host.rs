//! Host implementation of `world aria:plugin/plugin@1.0.0` and `aria_plugin::Plugin`.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use aria_commands::Command;
use aria_models::{
    AudioEffectSpec, ClipId, EffectParamKind, EffectParamSpec, MarkerColor, MarkerKind, Project,
    SequenceId,
};
use aria_plugin::package::{Access, FsScope, Package, register_declared};
use aria_plugin::{
    CallError, ClipRef, DecoderSpec, EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, EditorView,
    EncodeError, Encoder, EncoderConfig, EncoderSpec, Exporter, ExporterSpec, ForeignDecoder,
    FrameData, Importer, ImporterSpec, InterchangeReport, JobStep, LogLevel, Names, Origin,
    PixelFormat, Plugin, PluginCommand, PluginContext, PluginError, PluginManifest, ProjectHost,
    Rational, RationalTime, ScopeSpec, SettingsService, TimeRange, TransactError, Transaction,
    VideoEncoder, VideoFrame,
};
use parking_lot::Mutex;
use serde_json::json;
use wasmtime::component::{Component as WasmComponent, HasSelf, Linker};
use wasmtime::{Store, StoreLimits, StoreLimitsBuilder};

use crate::ai_tools::{self, WasmAiTool};
use crate::audio::WasmAudioFactory;
use crate::bindings;
use crate::contributions;
use crate::decoders::WasmDecoder;
use crate::events::{UnmountSentinel, subscribe_event};
use crate::legacy_bindings;
use crate::overlays::register_wasm_overlay;
use crate::panels::register_wasm_panel;
use crate::runtime::{SharedEngine, classify_trap, deadline_ticks};
use crate::scopes::WasmScope;
use crate::{Limits, LoadError, WORLD};

/// Marker type used with `PluginContext::has` to record a `CapabilityDenial` when a plugin
/// with `permissions.project = "read"` attempts a write operation (`project-transact` or `project-undo`).
struct ProjectWritePermission;

/// Marker type used with `PluginContext::has` to record a `CapabilityDenial` when a plugin
/// attempts to read or write an unpermitted filesystem path via `fs-read` or `fs-write`.
struct FsPermission;

/// Most bytes one `fs-read-at` call returns.
const FS_READ_AT_MAX: u32 = 64 * 1024 * 1024;

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
struct ActiveExport {
    path: String,
    bytes: Vec<u8>,
}

pub(crate) struct HostState {
    shared: Weak<WasmPluginShared>,
    package: Package,
    data_root: PathBuf,
    limiter: StoreLimits,
    phase: CallPhase,
    active_ctx: Option<PluginContext>,
    /// The file an import or decoder call reads, readable for that call.
    active_read_path: Option<PathBuf>,
    active_export: Option<ActiveExport>,
    active_encode_path: Option<PathBuf>,
}

enum PluginPreKind {
    Current(bindings::PluginPre<HostState>),
    Legacy(legacy_bindings::LegacyPluginPre<HostState>),
}

enum PluginKind {
    Current(bindings::Plugin),
    Legacy(legacy_bindings::LegacyPlugin),
}

impl PluginKind {
    fn current(&self) -> wasmtime::Result<&bindings::Plugin> {
        match self {
            Self::Current(plugin) => Ok(plugin),
            Self::Legacy(_) => Err(wasmtime::Error::msg(
                "legacy @1.0.0 component does not export this function",
            )),
        }
    }
}

struct LiveInstance {
    store: Store<HostState>,
    plugin: PluginKind,
}

enum InstanceState {
    Unmounted,
    Mounted(LiveInstance),
    Faulted(String),
}

pub(crate) struct WasmPluginShared {
    package: Package,
    manifest: PluginManifest,
    limits: Limits,
    data_root: PathBuf,
    engine: &'static SharedEngine,
    /// The component, linked against the host and type-checked against the
    /// world at load, so a mismatch fails the load before any guest code runs.
    pre: PluginPreKind,
    state: Mutex<InstanceState>,
    active_thread: Mutex<Option<ThreadId>>,
    /// The id the next `start-job` hands the guest.
    next_job: AtomicU64,
}

struct ActiveThreadGuard<'a> {
    slot: &'a Mutex<Option<ThreadId>>,
}

impl<'a> ActiveThreadGuard<'a> {
    fn enter(slot: &'a Mutex<Option<ThreadId>>) -> Self {
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

    fn prepare_call(&self, store: &mut Store<HostState>, phase: CallPhase, ctx: &PluginContext) {
        let _ = store.set_fuel(self.limits.fuel_per_call);
        store.set_epoch_deadline(deadline_ticks(self.limits.call_deadline));
        let data = store.data_mut();
        data.phase = phase;
        data.active_ctx = Some(ctx.clone());
    }

    fn finish_call(store: &mut Store<HostState>) {
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

    fn run_with_instance<T>(
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

    pub(crate) fn call_command_run(
        &self,
        ctx: &PluginContext,
        id: &str,
        args_json: &str,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Command, |live, _engine| {
            match &live.plugin {
                PluginKind::Current(p) => p.call_command_run(&mut live.store, id, args_json),
                PluginKind::Legacy(p) => p.call_command_run(&mut live.store, id, args_json),
            }
        })
    }

    pub(crate) fn call_event(
        &self,
        ctx: &PluginContext,
        kind: &str,
        event_json: &str,
    ) -> Result<Option<String>, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Ok(None);
        }
        self.run_with_instance(ctx, CallPhase::Event, |live, _engine| match &live.plugin {
            PluginKind::Current(p) => p.call_event(&mut live.store, kind, event_json),
            PluginKind::Legacy(p) => p.call_event(&mut live.store, kind, event_json),
        })
    }

    pub(crate) fn call_import(
        &self,
        ctx: &PluginContext,
        importer_id: &str,
        file: &Path,
    ) -> Result<bindings::ImportResult, String> {
        let path_str = file.to_string_lossy().into_owned();
        self.run_with_instance(ctx, CallPhase::Import, |live, _engine| {
            live.store.data_mut().active_read_path = Some(file.to_path_buf());
            live.plugin
                .current()?
                .call_import(&mut live.store, importer_id, &path_str)
        })
    }

    pub(crate) fn call_export(
        &self,
        ctx: &PluginContext,
        exporter_id: &str,
        project_json: &str,
    ) -> Result<(bindings::Report, Vec<u8>), String> {
        let stage_path = self
            .package
            .dir
            .join(".wasm-export-stage")
            .to_string_lossy()
            .into_owned();
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

        self.prepare_call(&mut live.store, CallPhase::Export, ctx);
        live.store.data_mut().active_export = Some(ActiveExport {
            path: stage_path.clone(),
            bytes: Vec::new(),
        });

        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            live.plugin.current().and_then(|p| {
                p.call_export(&mut live.store, exporter_id, project_json, &stage_path)
            })
        };
        let captured = live
            .store
            .data_mut()
            .active_export
            .take()
            .map(|exp| exp.bytes)
            .unwrap_or_default();
        Self::finish_call(&mut live.store);

        match result {
            Ok(Ok(report)) => Ok((report, captured)),
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

    pub(crate) fn call_audio_create(
        &self,
        ctx: &PluginContext,
        effect_id: &str,
        sample_rate: u32,
    ) -> Result<u64, String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_create(&mut live.store, effect_id, sample_rate)
        })
    }

    pub(crate) fn call_audio_process(
        &self,
        ctx: &PluginContext,
        instance: u64,
        params: &[bindings::AudioParam],
        block: &bindings::AudioBlock,
    ) -> Result<bindings::StereoBuffers, String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_process(&mut live.store, instance, params, block)
        })
    }

    pub(crate) fn call_audio_reset(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_reset(&mut live.store, instance)
        })
    }

    pub(crate) fn call_audio_drop(&self, ctx: &PluginContext, instance: u64) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_drop(&mut live.store, instance)
        })
    }

    fn run_live_call<T>(
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

    pub(crate) fn call_panel_view(&self, ctx: &PluginContext, id: &str) -> Vec<bindings::UiNode> {
        self.run_live_call(ctx, CallPhase::Panel, |live| {
            live.plugin.current()?.call_panel_view(&mut live.store, id)
        })
    }

    pub(crate) fn call_panel_event(
        &self,
        ctx: &PluginContext,
        id: &str,
        event: &bindings::PanelEvent,
    ) -> Result<(), String> {
        self.run_live_call(ctx, CallPhase::Panel, |live| {
            live.plugin
                .current()?
                .call_panel_event(&mut live.store, id, event)
        })
    }

    pub(crate) fn call_overlay_scene(
        &self,
        ctx: &PluginContext,
        id: &str,
    ) -> Vec<bindings::OverlayShape> {
        self.run_live_call(ctx, CallPhase::Overlay, |live| {
            live.plugin
                .current()?
                .call_overlay_scene(&mut live.store, id)
        })
    }

    pub(crate) fn call_overlay_event(
        &self,
        ctx: &PluginContext,
        id: &str,
        event: &bindings::OverlayEvent,
    ) -> Result<(), String> {
        self.run_live_call(ctx, CallPhase::Overlay, |live| {
            live.plugin
                .current()?
                .call_overlay_event(&mut live.store, id, event)
        })
    }

    fn run_encoder_call<T>(
        &self,
        ctx: &PluginContext,
        encode_path: &Path,
        f: impl FnOnce(&mut LiveInstance) -> wasmtime::Result<Result<T, String>>,
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
        self.prepare_call(&mut live.store, CallPhase::Encode, ctx);
        live.store.data_mut().active_encode_path = Some(encode_path.to_path_buf());
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            f(live)
        };
        Self::finish_call(&mut live.store);
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                std::panic::resume_unwind(Box::new(reason));
            }
        }
    }

    pub(crate) fn call_encoder_open(
        &self,
        ctx: &PluginContext,
        encoder_id: &str,
        path: &Path,
        config: &bindings::EncoderConfig,
    ) -> Result<u64, String> {
        let path_str = path.to_string_lossy().into_owned();
        self.run_encoder_call(ctx, path, |live| {
            live.plugin.current()?.call_encoder_open(
                &mut live.store,
                encoder_id,
                &path_str,
                *config,
            )
        })
    }

    pub(crate) fn call_encoder_push_frame(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        frame: &bindings::EncoderFrame,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_push_frame(&mut live.store, instance, frame)
        })
    }

    pub(crate) fn call_encoder_push_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        audio: &bindings::EncoderAudio,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_push_audio(&mut live.store, instance, audio)
        })
    }

    pub(crate) fn call_encoder_finish(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_finish(&mut live.store, instance)
        })
    }

    pub(crate) fn call_encoder_drop(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Encode, |live, _engine| {
            live.plugin
                .current()?
                .call_encoder_drop(&mut live.store, instance)
        })
    }

    fn call_job_step(&self, ctx: &PluginContext, job: u64) -> Result<JobStep, String> {
        let step = self.run_with_instance(ctx, CallPhase::Job, |live, _engine| {
            live.plugin.current()?.call_job_step(&mut live.store, job)
        })?;
        Ok(JobStep {
            progress: step.progress,
            detail: step.detail,
            done: step.done,
            commit: step.commit,
        })
    }

    fn call_job_end(&self, ctx: &PluginContext, job: u64, cancelled: bool) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Job, |live, _engine| {
            live.plugin
                .current()?
                .call_job_end(&mut live.store, job, cancelled)
                .map(Ok)
        })
    }

    /// Run the AI tool registered as `name`. A trap fails the plugin.
    pub(crate) fn call_ai_tool(
        &self,
        ctx: &PluginContext,
        name: &str,
        arguments_json: &str,
    ) -> Result<bindings::AiToolOutput, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot run its AI tool {name} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::AiTool, |live, _engine| {
            live.plugin
                .current()?
                .call_ai_tool_call(&mut live.store, name, arguments_json)
        })
    }

    /// Measure `frame` with the scope registered as `id`. A trap fails the
    /// plugin.
    pub(crate) fn call_scope_measure(
        &self,
        ctx: &PluginContext,
        id: &str,
        frame: &bindings::ScopeFrame,
    ) -> Result<bindings::ScopeDrawing, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot measure its scope {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::Scope, |live, _engine| {
            live.plugin
                .current()?
                .call_scope_measure(&mut live.store, id, frame)
        })
    }

    /// Run decoder call `f` with `path` readable. A trap fails the plugin.
    fn run_decoder_call<T>(
        &self,
        ctx: &PluginContext,
        path: &Path,
        f: impl FnOnce(&mut LiveInstance) -> wasmtime::Result<T>,
    ) -> Result<T, CallError> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(CallError::Failed(format!(
                "plugin {} cannot decode from inside one of its own calls",
                self.manifest.id
            )));
        }
        self.run_with_instance(ctx, CallPhase::Decode, |live, _engine| {
            live.store.data_mut().active_read_path = Some(path.to_path_buf());
            f(live).map(Ok)
        })
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_probe(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
    ) -> Result<bindings::MediaProbe, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_probe(&mut live.store, decoder_id, &path_str)
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_open_video(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
    ) -> Result<bindings::OpenedVideo, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_open_video(&mut live.store, decoder_id, &path_str)
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_seek(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        pts: i64,
    ) -> Result<(), CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_seek(&mut live.store, instance, pts)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_next_frame(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
    ) -> Result<Option<bindings::DecodedFrame>, CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_next_frame(&mut live.store, instance)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_open_audio(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
        sample_rate: u32,
        channels: u16,
    ) -> Result<u64, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin.current()?.call_decoder_open_audio(
                &mut live.store,
                decoder_id,
                &path_str,
                sample_rate,
                channels,
            )
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_read_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        max_samples: u32,
    ) -> Result<Vec<f32>, CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_read_audio(&mut live.store, instance, max_samples)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_seek_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        frame: i64,
    ) -> Result<(), CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_seek_audio(&mut live.store, instance, frame)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_close(&self, ctx: &PluginContext, path: &Path, instance: u64) {
        // A plugin that faulted or unmounted holds no streams to close.
        let _ = self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_close(&mut live.store, instance)
        });
    }

    pub(crate) fn call_cpu_effect_process(
        &self,
        ctx: &PluginContext,
        id: &str,
        params: &[f32],
        frame: &bindings::VideoFrame,
    ) -> Result<bindings::VideoFrame, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot run CPU effect {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
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
        self.prepare_call(&mut live.store, CallPhase::CpuEffect, ctx);
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            live.plugin
                .current()
                .and_then(|p| p.call_cpu_effect_process(&mut live.store, id, params, frame))
        };
        Self::finish_call(&mut live.store);
        match result {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                std::panic::resume_unwind(Box::new(reason));
            }
        }
    }

    pub(crate) fn call_driver_evaluate(
        &self,
        ctx: &PluginContext,
        id: &str,
        expr: &str,
        vars: bindings::DriverVars,
    ) -> Result<bindings::ParamValue, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot evaluate driver {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::Driver, |live, _engine| {
            live.plugin
                .current()?
                .call_driver_evaluate(&mut live.store, id, expr, vars)
        })
    }

    pub(crate) fn call_analyzer_open(
        &self,
        ctx: &PluginContext,
        id: &str,
        sample_rate: u32,
        channels: u16,
    ) -> Result<u64, String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_open(&mut live.store, id, sample_rate, channels)
        })
    }

    pub(crate) fn call_analyzer_push(
        &self,
        ctx: &PluginContext,
        instance: u64,
        samples: &[f32],
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_push(&mut live.store, instance, samples)
        })
    }

    pub(crate) fn call_analyzer_finish(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<String, String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_finish(&mut live.store, instance)
        })
    }

    pub(crate) fn call_analyzer_drop(&self, ctx: &PluginContext, instance: u64) {
        let _ = self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_drop(&mut live.store, instance)
                .map(Ok)
        });
    }
}

fn call_error(error: bindings::DecodeError) -> CallError {
    match error {
        bindings::DecodeError::Unsupported(reason) => CallError::Unsupported(reason),
        bindings::DecodeError::Failed(reason) => CallError::Failed(reason),
    }
}

struct WasmCommandHandler {
    shared: Arc<WasmPluginShared>,
    id: String,
}

impl PluginCommand for WasmCommandHandler {
    fn run(&self, ctx: &PluginContext) -> Result<(), String> {
        self.shared.call_command_run(ctx, &self.id, "{}")
    }
}

struct WasmImporter {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl Importer for WasmImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let result = self
            .shared
            .call_import(&self.ctx, &self.id, file)
            .map_err(TransactError::Aborted)?;
        let commands: Vec<Command> =
            serde_json::from_str(&result.commands_json).map_err(|err| {
                TransactError::Aborted(format!("invalid import commands JSON: {err}"))
            })?;
        for command in commands {
            tx.apply(command)?;
        }
        Ok(InterchangeReport {
            skipped: result.skipped,
        })
    }
}

struct WasmExporter {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl Exporter for WasmExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport> {
        let project_json = serde_json::to_string(project)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let (report, bytes) = self
            .shared
            .call_export(&self.ctx, &self.id, &project_json)
            .map_err(std::io::Error::other)?;
        out.write_all(&bytes)?;
        Ok(InterchangeReport {
            skipped: report.skipped,
        })
    }
}

struct WasmEncoder {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl Encoder for WasmEncoder {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let (width, height) = config.size;
        let (audio_sample_rate, audio_channels) = config.audio.map_or(
            (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS)),
            |audio| (audio.sample_rate, u32::from(audio.channels)),
        );
        let wit_cfg = bindings::EncoderConfig {
            width,
            height,
            fps_num: i64::from(config.frame_rate.num),
            fps_den: config.frame_rate.den,
            video_bitrate_kbps: config.bitrate_kbps.unwrap_or(0),
            audio_bitrate_kbps: 0,
        };
        let instance = self
            .shared
            .call_encoder_open(&self.ctx, &self.id, path, &wit_cfg)
            .map_err(EncodeError::Start)?;
        Ok(Box::new(WasmEncoderSession {
            shared: Arc::clone(&self.shared),
            ctx: self.ctx.clone(),
            path: path.to_path_buf(),
            instance,
            audio_sample_rate,
            audio_channels,
            finished: false,
        }))
    }
}

struct WasmEncoderSession {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    path: PathBuf,
    instance: u64,
    audio_sample_rate: u32,
    audio_channels: u32,
    finished: bool,
}

impl VideoEncoder for WasmEncoderSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(EncodeError::Unsupported(format!(
                "plugin encoder expects Rgba8 frame, got {:?}",
                frame.format
            )));
        }
        let FrameData::Cpu(cpu) = &frame.data else {
            return Err(EncodeError::Unsupported(
                "plugin encoder requires CPU RGBA frame".to_owned(),
            ));
        };
        let Some(plane) = cpu.planes.first() else {
            return Err(EncodeError::Encode(
                "RGBA frame has no pixel plane".to_owned(),
            ));
        };
        let (width, height) = frame.coded_size;
        let wit_frame = bindings::EncoderFrame {
            rgba: plane.data.clone(),
            width,
            height,
            stride: plane.stride as u32,
            pts_value: frame.pts.value,
            pts_rate: frame.pts.rate.num,
        };
        self.shared
            .call_encoder_push_frame(&self.ctx, &self.path, self.instance, &wit_frame)
            .map_err(EncodeError::Encode)
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let wit_audio = bindings::EncoderAudio {
            samples: samples.to_vec(),
            sample_rate: self.audio_sample_rate,
            channels: self.audio_channels,
            pts_value: pts.value,
            pts_rate: pts.rate.num,
        };
        self.shared
            .call_encoder_push_audio(&self.ctx, &self.path, self.instance, &wit_audio)
            .map_err(EncodeError::Encode)
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        self.finished = true;
        self.shared
            .call_encoder_finish(&self.ctx, &self.path, self.instance)
            .map_err(EncodeError::Encode)
    }
}

impl Drop for WasmEncoderSession {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.shared.call_encoder_drop(&self.ctx, self.instance);
        }
    }
}

impl HostState {
    fn require_mount_phase(&self, op: &str) -> Result<PluginContext, String> {
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

    fn require_ctx(&self) -> Result<PluginContext, String> {
        self.active_ctx
            .clone()
            .ok_or_else(|| "missing active plugin context".to_owned())
    }

    fn require_project_read(&self) -> Result<(PluginContext, Arc<ProjectHost>), String> {
        let ctx = self.require_ctx()?;
        if self.package.manifest.permissions.project == Access::None {
            let _ = ctx.try_get::<ProjectHost>();
            return Err(format!(
                "permission denied: {}: project access requires permissions.project = \"read\" or \"read-write\"",
                self.package.manifest.id
            ));
        }
        let host = ctx
            .try_get::<ProjectHost>()
            .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
        Ok((ctx, host))
    }

    fn require_project_write(&self) -> Result<(PluginContext, Arc<ProjectHost>), String> {
        let ctx = self.require_ctx()?;
        match self.package.manifest.permissions.project {
            Access::None => {
                let _ = ctx.try_get::<ProjectHost>();
                return Err(format!(
                    "permission denied: {}: project write requires permissions.project = \"read-write\"",
                    self.package.manifest.id
                ));
            }
            Access::Read => {
                let _ = ctx.has::<ProjectWritePermission>();
                return Err(format!(
                    "permission denied: {}: project write requires permissions.project = \"read-write\" (have \"read\")",
                    self.package.manifest.id
                ));
            }
            Access::ReadWrite => {}
        }
        let host = ctx
            .try_get::<ProjectHost>()
            .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
        Ok((ctx, host))
    }

    fn storage_root(&self) -> PathBuf {
        self.package.data_dir(&self.data_root)
    }

    fn resolve_storage_key(&self, key: &str) -> Result<PathBuf, String> {
        let rel = sanitize_relative_path(key).ok_or_else(|| {
            format!("invalid storage key {key:?}: must be a relative path without `..`")
        })?;
        Ok(self.storage_root().join(rel))
    }

    fn check_fs_path(&self, raw_path: &str, write: bool) -> Result<PathBuf, String> {
        let path = Path::new(raw_path);
        if !write {
            if let Some(active) = &self.active_read_path {
                if path == active {
                    return Ok(active.clone());
                }
            }
        }
        if write {
            if let Some(active) = &self.active_encode_path {
                if path == active {
                    return Ok(active.clone());
                }
            }
        }

        for scope in &self.package.manifest.permissions.fs {
            if let FsScope::Path(prefix) = scope {
                if path.is_absolute() && !contains_parent_dir(path) && path.starts_with(prefix) {
                    return Ok(path.to_path_buf());
                }
            }
        }

        if let Ok(ctx) = self.require_ctx() {
            let _ = ctx.has::<FsPermission>();
        }
        let mode = if write { "write" } else { "read" };
        Err(format!(
            "permission denied: {}: fs {mode} for {raw_path:?} is not granted by permissions.fs",
            self.package.manifest.id
        ))
    }
}

impl bindings::PluginImports for HostState {
    fn log(&mut self, level: bindings::LogLevel, text: String) {
        let plugin = self.package.manifest.id.as_str();
        if let Some(ctx) = &self.active_ctx {
            let level = match level {
                bindings::LogLevel::Trace => LogLevel::Trace,
                bindings::LogLevel::Debug => LogLevel::Debug,
                bindings::LogLevel::Info => LogLevel::Info,
                bindings::LogLevel::Warn => LogLevel::Warn,
                bindings::LogLevel::Error => LogLevel::Error,
            };
            ctx.log_for(plugin, level, &text);
        } else {
            match level {
                bindings::LogLevel::Trace => tracing::trace!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Debug => tracing::debug!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Info => tracing::info!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Warn => tracing::warn!(plugin = %plugin, "{text}"),
                bindings::LogLevel::Error => tracing::error!(plugin = %plugin, "{text}"),
            }
        }
    }

    fn storage_get(&mut self, key: String) -> Result<Option<Vec<u8>>, String> {
        let path = self.resolve_storage_key(&key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("storage_get {key:?}: {err}")),
        }
    }

    fn storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        let path = self.resolve_storage_key(&key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("storage_set {key:?}: {err}"))?;
        }
        fs::write(&path, &value).map_err(|err| format!("storage_set {key:?}: {err}"))
    }

    fn storage_delete(&mut self, key: String) -> Result<(), String> {
        let path = self.resolve_storage_key(&key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("storage_delete {key:?}: {err}")),
        }
    }

    fn storage_keys(&mut self) -> Result<Vec<String>, String> {
        let root = self.storage_root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut keys = Vec::new();
        collect_storage_keys(&root, &root, &mut keys)?;
        keys.sort();
        Ok(keys)
    }

    fn setting_get(&mut self, key: String) -> Result<Option<String>, String> {
        let ctx = self.require_ctx()?;
        let Some(settings) = ctx.try_get::<dyn SettingsService>() else {
            return Ok(None);
        };
        Ok(settings.get(&key).map(|v| v.to_string()))
    }

    fn start_job(&mut self, label: String) -> Result<u64, String> {
        let ctx = self.require_ctx()?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let job = shared.next_job.fetch_add(1, Ordering::Relaxed);
        let (stepping, step_ctx, end_ctx) = (Arc::clone(&shared), ctx.clone(), ctx.clone());
        ctx.start_stepped_job(
            &label,
            move || stepping.call_job_step(&step_ctx, job),
            move |cancelled| {
                // A faulted or unmounted instance has no job to end.
                let _ = shared.call_job_end(&end_ctx, job, cancelled);
            },
        )
        .map(|_| job)
        .map_err(|err| err.to_string())
    }

    fn register_command(&mut self, id: String, title: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-command")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let spec = self.package.command_spec(id.clone(), title);
        ctx.register_command(spec, WasmCommandHandler { shared, id })
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    fn subscribe(&mut self, event_kind: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("subscribe")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        subscribe_event(&ctx, &shared, &event_kind)
    }

    fn register_importer(&mut self, spec: bindings::ImporterSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-importer")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = ImporterSpec {
            id: spec.id,
            name: spec.name,
            extensions: spec.extensions,
        };
        ctx.register_importer(
            host_spec,
            WasmImporter {
                shared,
                ctx: ctx.clone(),
                id,
            },
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    fn register_exporter(&mut self, spec: bindings::ExporterSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-exporter")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = ExporterSpec {
            id: spec.id,
            name: spec.name,
            extension: spec.extension,
        };
        ctx.register_exporter(
            host_spec,
            WasmExporter {
                shared,
                ctx: ctx.clone(),
                id,
            },
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    fn register_encoder(&mut self, spec: bindings::EncoderSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-encoder")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = EncoderSpec {
            id: spec.id,
            name: spec.name,
            extension: spec.extension,
            audio: spec.audio,
        };
        ctx.register_encoder(
            host_spec,
            WasmEncoder {
                shared,
                ctx: ctx.clone(),
                id,
            },
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    fn register_decoder(&mut self, spec: bindings::DecoderSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-decoder")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let decoder = WasmDecoder::new(shared, ctx.clone(), spec.id.clone());
        ctx.register_decoder(
            DecoderSpec {
                id: spec.id,
                name: spec.name,
                extensions: spec.extensions,
                priority: spec.priority,
            },
            ForeignDecoder::new(decoder),
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    fn register_ai_tool(&mut self, spec: bindings::AiToolSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-ai-tool")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let tool = WasmAiTool::new(shared, spec.name.clone());
        ctx.register_ai_tool(ai_tools::host_spec(spec)?, tool)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    fn register_scope(&mut self, spec: bindings::ScopeSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-scope")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let scope = WasmScope::new(shared, spec.id.clone());
        ctx.register_scope(
            ScopeSpec {
                id: spec.id,
                name: spec.name,
                width: spec.width,
                height: spec.height,
            },
            scope,
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    fn register_audio_effect(&mut self, spec: bindings::AudioEffectSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-audio-effect")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let param_count = spec.params.len();
        let effect_id = spec.id.clone();
        let model_spec = AudioEffectSpec {
            id: spec.id,
            category: String::new(),
            label: spec.label,
            params: spec
                .params
                .into_iter()
                .map(|p| EffectParamSpec {
                    name: p.name,
                    label: p.label,
                    kind: EffectParamKind::Scalar,
                    default: p.default_value,
                    min: p.min,
                    max: p.max,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                    ..Default::default()
                })
                .collect(),
            latency: spec.latency,
            sidechain: spec.sidechain,
        };
        let factory = WasmAudioFactory::new(shared, ctx.clone(), effect_id, param_count);
        ctx.register_audio_effect(model_spec, factory)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    fn register_panel(&mut self, spec: bindings::PanelSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-panel")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        register_wasm_panel(&ctx, shared, spec)
    }

    fn refresh_panel(&mut self, id: String) -> Result<(), String> {
        let ctx = self.require_ctx()?;
        ctx.refresh_panel(&id).map_err(|err| err.to_string())
    }

    fn register_viewer_overlay(&mut self, spec: bindings::OverlaySpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-viewer-overlay")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        register_wasm_overlay(&ctx, shared, spec)
    }

    fn refresh_viewer_overlay(&mut self, id: String) -> Result<(), String> {
        let ctx = self.require_ctx()?;
        ctx.refresh_viewer_overlay(&id)
            .map_err(|err| err.to_string())
    }

    fn set_effect_category(&mut self, id: String, category: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("set-effect-category")?;
        ctx.set_effect_category(&id, &category)
    }

    fn register_effect(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-effect")?;
        contributions::register_effect(&ctx, spec, passes)
    }

    fn register_cpu_effect(&mut self, spec: bindings::EffectSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-cpu-effect")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_cpu_effect(&ctx, shared, spec)
    }

    fn register_transition(
        &mut self,
        spec: bindings::TransitionSpec,
        wgsl: String,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-transition")?;
        contributions::register_transition(&ctx, spec, wgsl)
    }

    fn register_generator(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-generator")?;
        contributions::register_generator(&ctx, spec, passes)
    }

    fn register_effect_graph(
        &mut self,
        spec: bindings::EffectSpec,
        traits: bindings::EffectTraits,
        passes: Vec<bindings::GraphPass>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-effect-graph")?;
        contributions::register_effect_graph(&ctx, spec, traits, passes)
    }

    fn register_texture(
        &mut self,
        id: String,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-texture")?;
        contributions::register_texture(&ctx, id, width, height, pixels)
    }

    fn register_clip_type(&mut self, spec: bindings::ClipTypeSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-clip-type")?;
        contributions::register_clip_type(&ctx, spec)
    }

    fn define_clip_type(
        &mut self,
        spec: bindings::ClipTypeSpec,
        schema: bindings::ClipTypeSchema,
        render: bindings::ClipTypeRender,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("define-clip-type")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::define_clip_type(&ctx, shared, spec, schema, render)
    }

    fn register_driver_language(
        &mut self,
        spec: bindings::DriverLanguageSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-driver-language")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_driver_language(&ctx, shared, spec)
    }

    fn register_preset(&mut self, preset_json: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-preset")?;
        contributions::register_preset(&ctx, &preset_json)
    }

    fn register_theme(&mut self, id: String, toml: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-theme")?;
        contributions::register_theme(&ctx, &id, &toml)
    }

    fn register_workspace(&mut self, spec: bindings::WorkspaceSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-workspace")?;
        contributions::register_workspace(&ctx, spec)
    }

    fn register_analyzer(&mut self, spec: bindings::AnalyzerSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-analyzer")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_analyzer(&ctx, shared, spec)
    }

    fn project_snapshot(&mut self) -> Result<String, String> {
        let (_ctx, host) = self.require_project_read()?;
        let snapshot = host.snapshot();
        serde_json::to_string(snapshot.as_ref()).map_err(|err| err.to_string())
    }

    fn editor_view(&mut self) -> Result<String, String> {
        let (ctx, _host) = self.require_project_read()?;
        let state = ctx.try_get::<EditorView>().and_then(|view| view.current());
        match state {
            Some(view) => Ok(json!({
                "selection": view.selection,
                "playhead": view.playhead,
                "mark_in": view.mark_in,
                "mark_out": view.mark_out,
                "playing": view.playing,
                "speed": view.speed,
            })
            .to_string()),
            None => Ok(json!({
                "selection": [],
                "playhead": null,
                "mark_in": null,
                "mark_out": null,
                "playing": false,
                "speed": 0,
            })
            .to_string()),
        }
    }

    fn clip(&mut self, id: u64) -> Result<Option<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        Ok(host.clip(ClipId::from_raw(id)).map(clip_ref_to_wit))
    }

    fn clips_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let range = wit_to_time_range(range)?;
        Ok(host
            .clips_in_range(sequence.map(SequenceId::from_raw), range)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    fn clips_at(
        &mut self,
        sequence: Option<u64>,
        tick: bindings::RationalTime,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let tick = wit_to_rational_time(tick)?;
        Ok(host
            .clips_at(sequence.map(SequenceId::from_raw), tick)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    fn selected_clips(&mut self) -> Result<Vec<bindings::ClipRef>, String> {
        let (ctx, host) = self.require_project_read()?;
        let selection = ctx
            .try_get::<EditorView>()
            .and_then(|view| view.current())
            .map(|state| state.selection)
            .unwrap_or_default();
        Ok(host
            .selected_clips(&selection)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    fn markers_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::Marker>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let range = wit_to_time_range(range)?;
        Ok(host
            .markers_in_range(sequence.map(SequenceId::from_raw), range)
            .into_iter()
            .map(marker_to_wit)
            .collect())
    }

    fn in_out(&mut self) -> Result<bindings::InOutMarks, String> {
        let (ctx, _host) = self.require_project_read()?;
        let state = ctx.try_get::<EditorView>().and_then(|view| view.current());
        Ok(match state {
            Some(view) => bindings::InOutMarks {
                mark_in: view.mark_in.map(rational_time_to_wit),
                mark_out: view.mark_out.map(rational_time_to_wit),
            },
            None => bindings::InOutMarks {
                mark_in: None,
                mark_out: None,
            },
        })
    }

    fn project_transact(&mut self, label: String, commands_json: String) -> Result<String, String> {
        let (_ctx, host) = self.require_project_write()?;
        let commands: Vec<Command> = serde_json::from_str(&commands_json)
            .map_err(|err| format!("invalid commands JSON: {err}"))?;
        let origin = Origin::plugin(&self.package.manifest.id);
        let receipt = host
            .transact(origin, label, commands)
            .map_err(|err| err.to_string())?;
        Ok(json!({ "revision": receipt.revision, "outcomes": receipt.outcomes }).to_string())
    }

    fn project_undo(&mut self) -> Result<bool, String> {
        let (_ctx, host) = self.require_project_write()?;
        host.undo().map_err(|err| err.to_string())
    }

    fn fs_read(&mut self, path: String) -> Result<Vec<u8>, String> {
        let allowed = self.check_fs_path(&path, false)?;
        fs::read(&allowed).map_err(|err| format!("fs_read {path:?}: {err}"))
    }

    fn fs_read_at(&mut self, path: String, offset: u64, len: u32) -> Result<Vec<u8>, String> {
        if len > FS_READ_AT_MAX {
            return Err(format!(
                "fs_read_at {path:?}: {len} bytes is over the {FS_READ_AT_MAX}-byte limit per call"
            ));
        }
        let allowed = self.check_fs_path(&path, false)?;
        let mut file =
            fs::File::open(&allowed).map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        let mut bytes = Vec::with_capacity(len as usize);
        file.take(u64::from(len))
            .read_to_end(&mut bytes)
            .map_err(|err| format!("fs_read_at {path:?}: {err}"))?;
        Ok(bytes)
    }

    fn fs_len(&mut self, path: String) -> Result<u64, String> {
        let allowed = self.check_fs_path(&path, false)?;
        fs::metadata(&allowed)
            .map(|meta| meta.len())
            .map_err(|err| format!("fs_len {path:?}: {err}"))
    }

    fn fs_write(&mut self, path: String, bytes: Vec<u8>) -> Result<(), String> {
        if let Some(active) = &mut self.active_export {
            if path == active.path {
                active.bytes = bytes;
                return Ok(());
            }
        }
        let allowed = self.check_fs_path(&path, true)?;
        if let Some(parent) = allowed.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("fs_write {path:?}: {err}"))?;
        }
        fs::write(&allowed, &bytes).map_err(|err| format!("fs_write {path:?}: {err}"))
    }
}

pub(crate) fn sanitize_relative_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn contains_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|comp| matches!(comp, Component::ParentDir))
}

fn collect_storage_keys(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            collect_storage_keys(root, &path, out)?;
        } else if path.is_file() {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let key = rel.to_string_lossy().replace('\\', "/");
            if !key.is_empty() {
                out.push(key);
            }
        }
    }
    Ok(())
}

fn rational_time_to_wit(time: RationalTime) -> bindings::RationalTime {
    bindings::RationalTime {
        value: time.value,
        rate_num: time.rate.num,
        rate_den: time.rate.den,
    }
}

fn wit_to_rational_time(time: bindings::RationalTime) -> Result<RationalTime, String> {
    let rate = Rational::new(time.rate_num, time.rate_den);
    if !rate.is_valid() {
        return Err(format!(
            "invalid rational rate {}/{}: num and den must be positive",
            time.rate_num, time.rate_den
        ));
    }
    Ok(RationalTime::new(time.value, rate))
}

fn wit_to_time_range(range: bindings::TimeRange) -> Result<TimeRange, String> {
    let start = wit_to_rational_time(range.start)?;
    let duration = wit_to_rational_time(range.duration)?;
    TimeRange::new(start, duration).map_err(|err| format!("invalid time range: {err}"))
}

fn clip_ref_to_wit(clip: ClipRef) -> bindings::ClipRef {
    bindings::ClipRef {
        id: clip.id.raw(),
        track: clip.track.raw(),
        start: rational_time_to_wit(clip.start),
        end: rational_time_to_wit(clip.end),
        kind: clip.kind,
        name: clip.name,
    }
}

fn marker_to_wit(marker: aria_models::Marker) -> bindings::Marker {
    let color = match marker.color {
        MarkerColor::Teal => bindings::MarkerColor::Teal,
        MarkerColor::Blue => bindings::MarkerColor::Blue,
        MarkerColor::Purple => bindings::MarkerColor::Purple,
        MarkerColor::Pink => bindings::MarkerColor::Pink,
        MarkerColor::Red => bindings::MarkerColor::Red,
        MarkerColor::Orange => bindings::MarkerColor::Orange,
        MarkerColor::Yellow => bindings::MarkerColor::Yellow,
        MarkerColor::Green => bindings::MarkerColor::Green,
    };
    let kind = match marker.kind {
        MarkerKind::Comment => bindings::MarkerKind::Comment,
        MarkerKind::Chapter => bindings::MarkerKind::Chapter,
        MarkerKind::Segmentation => bindings::MarkerKind::Segmentation,
    };
    bindings::Marker {
        id: marker.id.raw(),
        tick: rational_time_to_wit(marker.tick),
        duration: rational_time_to_wit(marker.duration),
        name: marker.name,
        color,
        comment: marker.comment,
        kind,
    }
}
