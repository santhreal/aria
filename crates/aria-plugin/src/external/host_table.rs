//! `AriaHostV1` callback table and host-side adapter implementations for native plugins.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_LOG_DEBUG, ARIA_LOG_INFO, ARIA_LOG_TRACE, ARIA_LOG_WARN, AriaAudioBlock,
    AriaAudioParam, AriaBytes, AriaBytesResult, AriaEncoderAudio, AriaEncoderConfig,
    AriaEncoderFrame, AriaHostV1, AriaInstanceResult, AriaResult, AriaStr, free_rust_bytes,
    free_rust_string,
};
use serde::Deserialize;
use serde_json::json;

use super::events::subscribe_event;
use super::{HostCallContext, NativePluginState};
use crate::package::Access;
use crate::{
    AudioBlock, AudioEffectSpec, AudioProcessor, AudioProcessorFactory, Command,
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, EditorView, EffectParamKind, EffectParamSpec,
    EncodeError, Encoder, EncoderConfig, EncoderSpec, Exporter, ExporterSpec, Importer,
    ImporterSpec, InterchangeReport, LogLevel, Origin, OverlayEvent, OverlayScene, OverlaySpec,
    PanelEvent, PanelLocation, PanelSpec, PanelView, ParamSignal, PixelFormat, PluginCommand,
    PluginContext, Project, ProjectHost, RationalTime, Service, SettingsService, TransactError,
    Transaction, UiNode, VideoEncoder, VideoFrame, ViewerOverlay,
};

/// Capability marker recorded in `PluginContext::denials()` when a plugin with
/// `project = "read"` attempts a write operation (`project_transact` or `project_undo`).
pub(crate) struct ProjectWrite;

impl Service for ProjectWrite {}

/// Static `AriaHostV1` function table handed to `aria_plugin_entry`.
pub(crate) static HOST_V1: AriaHostV1 = AriaHostV1 {
    struct_size: std::mem::size_of::<AriaHostV1>(),
    api_major: ARIA_API_MAJOR,
    reserved: 0,
    free_string: free_rust_string,
    free_bytes: free_rust_bytes,
    log: host_log,
    storage_get: host_storage_get,
    storage_set: host_storage_set,
    storage_delete: host_storage_delete,
    storage_keys: host_storage_keys,
    setting_get: host_setting_get,
    register_command: host_register_command,
    subscribe: host_subscribe,
    register_importer: host_register_importer,
    register_exporter: host_register_exporter,
    register_audio_effect: host_register_audio_effect,
    project_snapshot: host_project_snapshot,
    editor_view: host_editor_view,
    project_transact: host_project_transact,
    project_undo: host_project_undo,
    register_panel: host_register_panel,
    refresh_panel: host_refresh_panel,
    register_encoder: host_register_encoder,
    register_viewer_overlay: host_register_viewer_overlay,
    refresh_viewer_overlay: host_refresh_viewer_overlay,
    start_job: host_start_job,
    register_decoder: host_register_decoder,
    register_ai_tool: host_register_ai_tool,
    register_scope: host_register_scope,
    register_effect: host_register_effect,
    register_cpu_effect: host_register_cpu_effect,
    register_transition: host_register_transition,
    register_generator: host_register_generator,
    register_clip_type: host_register_clip_type,
    register_driver_language: host_register_driver_language,
    register_preset: host_register_preset,
    register_theme: host_register_theme,
    register_workspace: host_register_workspace,
    register_analyzer: host_register_analyzer,
    clip: host_clip,
    clips_in_range: host_clips_in_range,
    clips_at: host_clips_at,
    selected_clips: host_selected_clips,
    markers_in_range: host_markers_in_range,
    in_out: host_in_out,
    register_texture: host_register_texture,
};

#[derive(Deserialize)]
struct RawImporterSpec {
    id: String,
    name: String,
    extensions: Vec<String>,
}

#[derive(Deserialize)]
struct RawExporterSpec {
    id: String,
    name: String,
    extension: String,
}

#[derive(Deserialize)]
struct RawEncoderSpec {
    id: String,
    name: String,
    extension: String,
    #[serde(default)]
    audio: bool,
}

#[derive(Deserialize)]
struct RawAudioParamSpec {
    name: String,
    label: String,
    #[serde(alias = "default")]
    default_value: f32,
    min: f32,
    max: f32,
}

#[derive(Deserialize)]
struct RawAudioEffectSpec {
    id: String,
    label: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    params: Vec<RawAudioParamSpec>,
    #[serde(default)]
    latency: u32,
    #[serde(default)]
    sidechain: bool,
}

#[derive(Deserialize)]
struct RawImportResult {
    #[serde(default)]
    commands_json: String,
    #[serde(default)]
    skipped: Vec<String>,
}

#[derive(Deserialize)]
struct RawExportReport {
    #[serde(default)]
    skipped: Vec<String>,
}

#[derive(Deserialize)]
struct RawPanelSpec {
    id: String,
    title: String,
    location: String,
}

#[derive(Deserialize)]
struct RawOverlaySpec {
    id: String,
    title: String,
}

fn parse_panel_location(raw: &str) -> Result<PanelLocation, String> {
    match raw {
        "sidebar" | "Sidebar" => Ok(PanelLocation::Sidebar),
        "inspector" | "Inspector" => Ok(PanelLocation::Inspector),
        "timeline" | "Timeline" => Ok(PanelLocation::Timeline),
        "overlay" | "Overlay" => Ok(PanelLocation::Overlay),
        "viewer" | "Viewer" => Ok(PanelLocation::Viewer),
        "home" | "Home" => Ok(PanelLocation::Home),
        other => Err(format!("unknown panel location `{other}`")),
    }
}

/// Convert `host_ctx` back into a shared `&HostCallContext`.
///
/// # Safety
///
/// `host_ctx` must be a valid pointer to a `HostCallContext` placed on the stack
/// by `NativePluginState` for the duration of the active plugin callback.
unsafe fn call_ctx<'a>(host_ctx: *mut c_void) -> Result<&'a HostCallContext<'a>, String> {
    if host_ctx.is_null() {
        return Err("host_ctx is null".to_owned());
    }
    // SAFETY: Guaranteed by caller contract on `call_ctx`.
    Ok(unsafe { &*(host_ctx as *const HostCallContext<'a>) })
}

/// Read an `AriaStr` as `&str`.
///
/// # Safety
///
/// `s` must point to `s.len` valid bytes for the duration of the call.
unsafe fn read_str<'a>(s: AriaStr, label: &str) -> Result<&'a str, String> {
    // SAFETY: Guaranteed by caller contract on `read_str`.
    unsafe { s.as_str() }.map_err(|err| format!("invalid UTF-8 in {label}: {err}"))
}

fn validate_storage_key(key: &str) -> Result<(), String> {
    if key.is_empty()
        || key == "."
        || key == ".."
        || key.contains('/')
        || key.contains('\\')
        || key.contains('\0')
    {
        return Err(format!(
            "invalid storage key `{key}`: key must be a single file name"
        ));
    }
    Ok(())
}

extern "C" fn host_log(host_ctx: *mut c_void, level: u32, text: AriaStr) {
    // SAFETY: `host_ctx` and `text` are provided by the guest during an active callback.
    let Ok(ctx) = (unsafe { call_ctx(host_ctx) }) else {
        return;
    };
    // SAFETY: `text` borrows guest memory for the duration of `host_log`.
    let Ok(message) = (unsafe { read_str(text, "log text") }) else {
        return;
    };
    let level = match level {
        ARIA_LOG_TRACE => LogLevel::Trace,
        ARIA_LOG_DEBUG => LogLevel::Debug,
        ARIA_LOG_INFO => LogLevel::Info,
        ARIA_LOG_WARN => LogLevel::Warn,
        _ => LogLevel::Error,
    };
    ctx.scoped_ctx.log_for(&ctx.state.id, level, message);
}

extern "C" fn host_storage_get(host_ctx: *mut c_void, key: AriaStr) -> AriaBytesResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_storage_get`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            validate_storage_key(key)?;
            let path = ctx.storage_dir().join(key);
            match std::fs::read(&path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(format!("storage read failed for `{key}`: {err}")),
            }
        })
    };
    match result {
        Ok(Some(bytes)) => AriaBytesResult::found(bytes),
        Ok(None) => AriaBytesResult::not_found(),
        Err(err) => AriaBytesResult::err(err),
    }
}

extern "C" fn host_storage_set(
    host_ctx: *mut c_void,
    key: AriaStr,
    value: AriaBytes,
) -> AriaResult {
    // SAFETY: `host_ctx`, `key`, and `value` are valid for the duration of `host_storage_set`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            validate_storage_key(key)?;
            let bytes = value.as_slice();
            let dir = ctx.storage_dir();
            std::fs::create_dir_all(&dir)
                .map_err(|err| format!("storage directory creation failed: {err}"))?;
            std::fs::write(dir.join(key), bytes)
                .map_err(|err| format!("storage write failed for `{key}`: {err}"))
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_storage_delete(host_ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_storage_delete`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "storage key")?;
            validate_storage_key(key)?;
            let path = ctx.storage_dir().join(key);
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(format!("storage delete failed for `{key}`: {err}")),
            }
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_storage_keys(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_storage_keys`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let dir = ctx.storage_dir();
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return Ok("[]".to_owned());
                }
                Err(err) => return Err(format!("storage list failed: {err}")),
            };
            let mut keys: Vec<String> = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            keys.sort();
            serde_json::to_string(&keys).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_setting_get(host_ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `key` are valid for the duration of `host_setting_get`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let key = read_str(key, "setting key")?;
            let Some(settings) = ctx.scoped_ctx.try_get::<dyn SettingsService>() else {
                return Ok(None);
            };
            match settings.get(key) {
                Some(val) => serde_json::to_string(&val)
                    .map(Some)
                    .map_err(|err| err.to_string()),
                None => Ok(None),
            }
        })
    };
    match result {
        Ok(Some(json)) => AriaResult::ok(json),
        Ok(None) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_command(
    host_ctx: *mut c_void,
    id: AriaStr,
    title: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `title` are valid for the duration of `host_register_command`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let id = read_str(id, "command id")?;
            let title = read_str(title, "command title")?;
            register_native_command(ctx.scoped_ctx, ctx.state, id.to_owned(), title.to_owned())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(crate) fn register_native_command(
    scoped_ctx: &PluginContext,
    state: &Arc<NativePluginState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let command = NativePluginCommand {
        state: Arc::clone(state),
        id: id.clone(),
    };
    scoped_ctx
        .register_command(state.package.command_spec(id, title), command)
        .map(drop)
        .map_err(|err| err.to_string())
}

extern "C" fn host_subscribe(host_ctx: *mut c_void, event_kind: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `event_kind` are valid for the duration of `host_subscribe`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let kind = read_str(event_kind, "event kind")?;
            subscribe_event(ctx.scoped_ctx, ctx.state, kind)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_importer(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_importer`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "importer spec")?;
            let raw: RawImporterSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid importer spec JSON: {err}"))?;
            let importer = NativeImporter {
                importer_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_importer(
                    ImporterSpec {
                        id: raw.id,
                        name: raw.name,
                        extensions: raw.extensions,
                    },
                    importer,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_exporter(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_exporter`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "exporter spec")?;
            let raw: RawExporterSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid exporter spec JSON: {err}"))?;
            let exporter = NativeExporter {
                exporter_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_exporter(
                    ExporterSpec {
                        id: raw.id,
                        name: raw.name,
                        extension: raw.extension,
                    },
                    exporter,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_audio_effect(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_audio_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "audio effect spec")?;
            let raw: RawAudioEffectSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid audio effect spec JSON: {err}"))?;
            let param_count = raw.params.len();
            let params = raw
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
                .collect();
            let spec = AudioEffectSpec {
                id: raw.id.clone(),
                category: raw.category,
                label: raw.label,
                params,
                latency: raw.latency,
                sidechain: raw.sidechain,
            };
            let factory = NativeAudioFactory {
                effect_id: raw.id,
                param_count,
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_audio_effect(spec, factory)
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_project_snapshot(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_project_snapshot`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for project_snapshot"
                        .to_owned(),
                );
            }
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            let project = host.snapshot();
            serde_json::to_string(project.as_ref()).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_editor_view(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_editor_view`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<EditorView>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for editor_view"
                        .to_owned(),
                );
            }
            let view_state = ctx
                .unscoped_ctx
                .try_get::<EditorView>()
                .and_then(|view| view.current());
            let payload = match view_state {
                Some(state) => json!({
                    "selection": state.selection,
                    "playhead": state.playhead,
                    "mark_in": state.mark_in,
                    "mark_out": state.mark_out,
                    "playing": state.playing,
                    "speed": state.speed,
                }),
                None => json!({
                    "selection": Vec::<u64>::new(),
                    "playhead": RationalTime::new(0, aria_core::Rational::FPS_24),
                    "mark_in": Option::<RationalTime>::None,
                    "mark_out": Option::<RationalTime>::None,
                    "playing": false,
                    "speed": 1,
                }),
            };
            Ok(payload.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

fn require_project_read_host<'a>(
    ctx: &'a HostCallContext<'a>,
    fn_name: &str,
) -> Result<Arc<ProjectHost>, String> {
    if ctx.state.package.manifest.permissions.project == Access::None {
        let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
        return Err(format!(
            "permission denied: project = \"read\" or \"read-write\" is required for {fn_name}"
        ));
    }
    ctx.scoped_ctx
        .try_get::<ProjectHost>()
        .ok_or_else(|| "ProjectHost service is not registered".to_owned())
}

fn optional_sequence_id(sequence_id: u64) -> Option<crate::SequenceId> {
    (sequence_id != 0).then_some(crate::SequenceId::from_raw(sequence_id))
}

extern "C" fn host_clip(host_ctx: *mut c_void, id: u64) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_clip`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(&ctx, "clip")?;
            match host.clip(crate::ClipId::from_raw(id)) {
                Some(found) => serde_json::to_string(&found)
                    .map(Some)
                    .map_err(|err| err.to_string()),
                None => Ok(None),
            }
        })
    };
    match result {
        Ok(Some(json)) => AriaResult::ok(json),
        Ok(None) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_clips_in_range(
    host_ctx: *mut c_void,
    sequence_id: u64,
    range_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `range_json` are valid for the duration of `host_clips_in_range`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(&ctx, "clips_in_range")?;
            let raw = read_str(range_json, "time range JSON")?;
            let range: crate::TimeRange = serde_json::from_str(raw)
                .map_err(|err| format!("invalid time range JSON: {err}"))?;
            let clips = host.clips_in_range(optional_sequence_id(sequence_id), range);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_clips_at(
    host_ctx: *mut c_void,
    sequence_id: u64,
    tick_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `tick_json` are valid for the duration of `host_clips_at`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(&ctx, "clips_at")?;
            let raw = read_str(tick_json, "tick JSON")?;
            let tick: RationalTime =
                serde_json::from_str(raw).map_err(|err| format!("invalid tick JSON: {err}"))?;
            let clips = host.clips_at(optional_sequence_id(sequence_id), tick);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_selected_clips(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_selected_clips`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(&ctx, "selected_clips")?;
            let selection = ctx
                .unscoped_ctx
                .try_get::<EditorView>()
                .and_then(|view| view.current())
                .map(|state| state.selection)
                .unwrap_or_default();
            let clips = host.selected_clips(&selection);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_markers_in_range(
    host_ctx: *mut c_void,
    sequence_id: u64,
    range_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `range_json` are valid for the duration of `host_markers_in_range`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(&ctx, "markers_in_range")?;
            let raw = read_str(range_json, "time range JSON")?;
            let range: crate::TimeRange = serde_json::from_str(raw)
                .map_err(|err| format!("invalid time range JSON: {err}"))?;
            let markers = host.markers_in_range(optional_sequence_id(sequence_id), range);
            serde_json::to_string(&markers).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_in_out(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_in_out`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<EditorView>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for in_out"
                        .to_owned(),
                );
            }
            let view_state = ctx
                .unscoped_ctx
                .try_get::<EditorView>()
                .and_then(|view| view.current());
            let in_out = view_state.as_ref().map(crate::in_out).unwrap_or_default();
            serde_json::to_string(&in_out).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_project_transact(
    host_ctx: *mut c_void,
    label: AriaStr,
    commands_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `label`, and `commands_json` are valid for the duration of `host_project_transact`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            match ctx.state.package.manifest.permissions.project {
                Access::None => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_transact"
                            .to_owned(),
                    );
                }
                Access::Read => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectWrite>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_transact"
                            .to_owned(),
                    );
                }
                Access::ReadWrite => {}
            }
            let label = read_str(label, "transaction label")?;
            let commands_raw = read_str(commands_json, "commands JSON")?;
            let commands: Vec<Command> = serde_json::from_str(commands_raw)
                .map_err(|err| format!("invalid commands JSON: {err}"))?;
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            let receipt = host
                .transact(Origin::plugin(&ctx.state.id), label.to_owned(), commands)
                .map_err(|err| err.to_string())?;
            Ok(json!({
                "revision": receipt.revision,
                "outcomes": receipt.outcomes,
            })
            .to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_project_undo(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_project_undo`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            match ctx.state.package.manifest.permissions.project {
                Access::None => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_undo"
                            .to_owned(),
                    );
                }
                Access::Read => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectWrite>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_undo"
                            .to_owned(),
                    );
                }
                Access::ReadWrite => {}
            }
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            let stepped = catch_unwind(AssertUnwindSafe(|| host.undo()))
                .map_err(|_| TransactError::Reentrant.to_string())?
                .map_err(|err| err.to_string())?;
            Ok(if stepped { "true" } else { "false" }.to_owned())
        })
    };
    match result {
        Ok(val) => AriaResult::ok(val),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_panel(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_panel`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_panels() {
                return Err(
                    "plugin table does not include panel callbacks (panel_view, panel_event)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "panel spec")?;
            let raw: RawPanelSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid panel spec JSON: {err}"))?;
            let location = parse_panel_location(&raw.location)?;
            let view = NativePanelView {
                panel_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_panel(
                    PanelSpec {
                        id: raw.id,
                        title: raw.title.into(),
                        location,
                    },
                    view,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_refresh_panel(host_ctx: *mut c_void, id: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `id` are valid for the duration of `host_refresh_panel`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let id = read_str(id, "panel id")?;
            ctx.scoped_ctx
                .refresh_panel(id)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_encoder(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_encoder`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_encoders() {
                return Err(
                    "plugin table does not include encoder callbacks (encoder_open..encoder_drop)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "encoder spec")?;
            let raw: RawEncoderSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid encoder spec JSON: {err}"))?;
            let encoder = NativeEncoder {
                encoder_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_encoder(
                    EncoderSpec {
                        id: raw.id,
                        name: raw.name,
                        extension: raw.extension,
                        audio: raw.audio,
                    },
                    encoder,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_decoder(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_decoder`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_decoders() {
                return Err(
                    "plugin table does not include decoder callbacks (decoder_probe..decoder_close)"
                        .to_owned(),
                );
            }
            super::decoders::register(ctx, read_str(spec_json, "decoder spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_ai_tool(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_ai_tool`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_ai_tools() {
                return Err("plugin table does not include the ai_tool_call callback".to_owned());
            }
            super::ai_tools::register(ctx, read_str(spec_json, "AI tool spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_scope(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_scope`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_scopes() {
                return Err("plugin table does not include the scope_measure callback".to_owned());
            }
            super::scopes::register(ctx, read_str(spec_json, "scope spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_effect(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_effect(
                ctx,
                read_str(spec_json, "effect spec")?,
                read_str(wgsl, "effect WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_cpu_effect(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_cpu_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_cpu_effect(ctx, read_str(spec_json, "CPU effect spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_transition(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_transition`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_transition(
                ctx,
                read_str(spec_json, "transition spec")?,
                read_str(wgsl, "transition WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_generator(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `spec_json`, and `wgsl` are valid for the duration of `host_register_generator`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_generator(
                ctx,
                read_str(spec_json, "generator spec")?,
                read_str(wgsl, "generator WGSL")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_clip_type(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_clip_type`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_clip_type(ctx, read_str(spec_json, "clip type spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_driver_language(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_driver_language`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_driver_language(
                ctx,
                read_str(spec_json, "driver language spec")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_preset(host_ctx: *mut c_void, preset_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `preset_json` are valid for the duration of `host_register_preset`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_preset(ctx, read_str(preset_json, "preset JSON")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_theme(host_ctx: *mut c_void, id: AriaStr, toml: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `toml` are valid for the duration of `host_register_theme`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_theme(
                ctx,
                read_str(id, "theme id")?,
                read_str(toml, "theme TOML")?,
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_texture(
    host_ctx: *mut c_void,
    id: AriaStr,
    width: u32,
    height: u32,
    pixels: AriaBytes,
) -> AriaResult {
    // SAFETY: `host_ctx`, `id`, and `pixels` are valid for the duration of `host_register_texture`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_texture(
                ctx,
                read_str(id, "texture id")?,
                width,
                height,
                pixels.as_slice(),
            )
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_workspace(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_workspace`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_workspace(ctx, read_str(spec_json, "workspace spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_analyzer(host_ctx: *mut c_void, spec_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_analyzer`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            super::contributions::register_analyzer(ctx, read_str(spec_json, "analyzer spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_register_viewer_overlay(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_viewer_overlay`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_overlays() {
                return Err(
                    "plugin table does not include overlay callbacks (overlay_scene, overlay_event)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "viewer overlay spec")?;
            let raw: RawOverlaySpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid viewer overlay spec JSON: {err}"))?;
            let overlay = NativeOverlay {
                overlay_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_viewer_overlay(
                    OverlaySpec {
                        id: raw.id,
                        title: raw.title.into(),
                    },
                    overlay,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_refresh_viewer_overlay(host_ctx: *mut c_void, id: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `id` are valid for the duration of `host_refresh_viewer_overlay`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let id = read_str(id, "viewer overlay id")?;
            ctx.scoped_ctx
                .refresh_viewer_overlay(id)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

extern "C" fn host_start_job(host_ctx: *mut c_void, label: AriaStr) -> AriaInstanceResult {
    // SAFETY: `host_ctx` and `label` are valid for the duration of `host_start_job`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let label = read_str(label, "job label")?;
            if !ctx.state.table.has_jobs() {
                return Err("plugin table does not export job_step and job_end".to_owned());
            }
            let job = ctx.state.next_job.fetch_add(1, Ordering::Relaxed);
            let (stepping, ending) = (Arc::clone(ctx.state), Arc::clone(ctx.state));
            let (step_ctx, end_ctx) = (ctx.scoped_ctx.clone(), ctx.scoped_ctx.clone());
            ctx.scoped_ctx
                .start_stepped_job(
                    label,
                    move || stepping.call_job_step(&step_ctx, job),
                    move |cancelled| ending.call_job_end(&end_ctx, job, cancelled),
                )
                .map(|_| job)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(job) => AriaInstanceResult::ok(job),
        Err(err) => AriaInstanceResult::err(err),
    }
}

struct NativeOverlay {
    overlay_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl ViewerOverlay for NativeOverlay {
    fn scene(&self, _ctx: &PluginContext) -> OverlayScene {
        self.state
            .call_overlay_scene(&self.scoped_ctx, &self.overlay_id)
    }

    fn event(&self, _ctx: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        self.state
            .call_overlay_event(&self.scoped_ctx, &self.overlay_id, event)
    }
}

struct NativePanelView {
    panel_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl PanelView for NativePanelView {
    fn view(&self, _ctx: &PluginContext) -> UiNode {
        self.state.call_panel_view(&self.scoped_ctx, &self.panel_id)
    }

    fn event(&self, _ctx: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        self.state
            .call_panel_event(&self.scoped_ctx, &self.panel_id, event)
    }
}

struct NativePluginCommand {
    state: Arc<NativePluginState>,
    id: String,
}

impl PluginCommand for NativePluginCommand {
    fn run(&self, ctx: &PluginContext) -> Result<(), String> {
        self.state.call_command_run(ctx, &self.id, "{}")
    }
}

struct NativeImporter {
    importer_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Importer for NativeImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let path_str = file.to_string_lossy();
        let raw_json = self
            .state
            .call_import(&self.scoped_ctx, &self.importer_id, &path_str)?;
        let parsed: RawImportResult = serde_json::from_str(&raw_json)
            .map_err(|err| TransactError::Aborted(format!("invalid import result JSON: {err}")))?;
        let commands: Vec<Command> = if parsed.commands_json.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&parsed.commands_json).map_err(|err| {
                TransactError::Aborted(format!("invalid import commands JSON: {err}"))
            })?
        };
        for command in commands {
            tx.apply(command)?;
        }
        Ok(InterchangeReport {
            skipped: parsed.skipped,
        })
    }
}

struct NativeExporter {
    exporter_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

static EXPORT_STAGE_SEQ: AtomicU64 = AtomicU64::new(1);

impl Exporter for NativeExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport> {
        let project_json = serde_json::to_string(project).map_err(std::io::Error::other)?;
        let dir = self.state.resolve_storage_dir(&self.scoped_ctx);
        std::fs::create_dir_all(&dir)?;
        let seq = EXPORT_STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
        let stage_path = dir.join(format!(".export-{}-{seq}.stage", self.exporter_id));
        let stage_str = stage_path.to_string_lossy().into_owned();
        let outcome = self.state.call_export(
            &self.scoped_ctx,
            &self.exporter_id,
            &project_json,
            &stage_str,
        );
        match outcome {
            Ok(report_json) => {
                let bytes = match std::fs::read(&stage_path) {
                    Ok(bytes) => bytes,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(err) => {
                        let _ = std::fs::remove_file(&stage_path);
                        return Err(err);
                    }
                };
                let _ = std::fs::remove_file(&stage_path);
                out.write_all(&bytes)?;
                let parsed: RawExportReport = if report_json.trim().is_empty() {
                    RawExportReport {
                        skipped: Vec::new(),
                    }
                } else {
                    serde_json::from_str(&report_json).map_err(std::io::Error::other)?
                };
                Ok(InterchangeReport {
                    skipped: parsed.skipped,
                })
            }
            Err(super::ExportCallError::Failed(msg)) => {
                let _ = std::fs::remove_file(&stage_path);
                Err(std::io::Error::other(msg))
            }
            Err(super::ExportCallError::Panicked(msg)) => {
                let _ = std::fs::remove_file(&stage_path);
                resume_unwind(Box::new(msg));
            }
        }
    }
}

struct NativeAudioFactory {
    effect_id: String,
    param_count: usize,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl AudioProcessorFactory for NativeAudioFactory {
    fn create(&self, sample_rate: u32) -> Box<dyn AudioProcessor> {
        match self
            .state
            .call_audio_create(&self.scoped_ctx, &self.effect_id, sample_rate)
        {
            Ok(instance) => Box::new(NativeAudioProcessor {
                effect_id: self.effect_id.clone(),
                instance,
                param_count: self.param_count,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            }),
            Err(msg) => resume_unwind(Box::new(msg)),
        }
    }
}

struct NativeAudioProcessor {
    effect_id: String,
    instance: u64,
    param_count: usize,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl AudioProcessor for NativeAudioProcessor {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let mut scratch_params = Vec::with_capacity(self.param_count);
        for slot in 0..self.param_count {
            let param = match block.param(slot) {
                Some(ParamSignal::Constant(val)) => AriaAudioParam {
                    constant: val,
                    per_frame: std::ptr::null(),
                    per_frame_len: 0,
                },
                Some(ParamSignal::PerFrame(values)) => AriaAudioParam {
                    constant: 0.0,
                    per_frame: values.as_ptr(),
                    per_frame_len: values.len(),
                },
                None => AriaAudioParam {
                    constant: 0.0,
                    per_frame: std::ptr::null(),
                    per_frame_len: 0,
                },
            };
            scratch_params.push(param);
        }
        let frames = block.frames();
        let position = block.position();
        let sample_rate = block.sample_rate();
        let (has_sidechain, sidechain_left, sidechain_right) = match block.sidechain() {
            Some((left, right)) => (1, left.as_ptr(), right.as_ptr()),
            None => (0, std::ptr::null(), std::ptr::null()),
        };
        let (left, right) = block.channels();
        let mut raw_block = AriaAudioBlock {
            left: left.as_mut_ptr(),
            right: right.as_mut_ptr(),
            frames,
            position,
            sample_rate,
            has_sidechain,
            sidechain_left,
            sidechain_right,
        };
        if let Err(msg) = self.state.call_audio_process(
            &self.scoped_ctx,
            &self.effect_id,
            self.instance,
            &scratch_params,
            &mut raw_block,
        ) {
            resume_unwind(Box::new(msg));
        }
    }

    fn reset(&mut self) {
        if let Err(msg) =
            self.state
                .call_audio_reset(&self.scoped_ctx, &self.effect_id, self.instance)
        {
            resume_unwind(Box::new(msg));
        }
    }
}

impl Drop for NativeAudioProcessor {
    fn drop(&mut self) {
        self.state.call_audio_drop(&self.scoped_ctx, self.instance);
    }
}

struct NativeEncoder {
    encoder_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Encoder for NativeEncoder {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let path_str = path.to_string_lossy();
        let (width, height) = config.size;
        let (audio_sample_rate, audio_channels) = config.audio.map_or(
            (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS)),
            |audio| (audio.sample_rate, u32::from(audio.channels)),
        );
        let raw_cfg = AriaEncoderConfig {
            width,
            height,
            fps_num: i64::from(config.frame_rate.num),
            fps_den: config.frame_rate.den,
            video_bitrate_kbps: config.bitrate_kbps.unwrap_or(0),
            audio_bitrate_kbps: 0,
        };
        let instance = self
            .state
            .call_encoder_open(&self.scoped_ctx, &self.encoder_id, &path_str, &raw_cfg)
            .map_err(EncodeError::Start)?;
        Ok(Box::new(NativeEncoderSession {
            encoder_id: self.encoder_id.clone(),
            instance,
            finished: false,
            audio_sample_rate,
            audio_channels,
            state: Arc::clone(&self.state),
            scoped_ctx: self.scoped_ctx.clone(),
        }))
    }
}

struct NativeEncoderSession {
    encoder_id: String,
    instance: u64,
    finished: bool,
    audio_sample_rate: u32,
    audio_channels: u32,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl VideoEncoder for NativeEncoderSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(EncodeError::Unsupported(format!(
                "plugin encoder expects Rgba8 frame, got {:?}",
                frame.format
            )));
        }
        let aria_core::FrameData::Cpu(cpu) = &frame.data else {
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
        let raw = AriaEncoderFrame {
            rgba: plane.data.as_ptr(),
            rgba_len: plane.data.len(),
            width,
            height,
            stride: plane.stride as u32,
            pts_value: frame.pts.value,
            pts_rate: frame.pts.rate.num,
        };
        self.state
            .call_encoder_push_frame(&self.scoped_ctx, self.instance, &raw)
            .map_err(EncodeError::Encode)
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let raw = AriaEncoderAudio {
            samples: samples.as_ptr(),
            samples_len: samples.len(),
            sample_rate: self.audio_sample_rate,
            channels: self.audio_channels,
            pts_value: pts.value,
            pts_rate: pts.rate.num,
        };
        self.state
            .call_encoder_push_audio(&self.scoped_ctx, self.instance, &raw)
            .map_err(EncodeError::Encode)
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        self.finished = true;
        self.state
            .call_encoder_finish(&self.scoped_ctx, self.instance)
            .map_err(EncodeError::Encode)
    }

    fn choice(&self) -> aria_core::EncoderChoice {
        aria_core::EncoderChoice::only(self.encoder_id.clone(), None)
    }
}

impl Drop for NativeEncoderSession {
    fn drop(&mut self) {
        if !self.finished {
            self.state
                .call_encoder_drop(&self.scoped_ctx, self.instance);
        }
    }
}
