//! `AriaHostV1` callback table and host-side adapter implementations for native plugins.

#![allow(unsafe_code)]

use std::ffi::c_void;

use aria_plugin_abi::{ARIA_API_MAJOR, AriaHostV1, AriaStr, free_rust_bytes, free_rust_string};

use super::HostCallContext;
use crate::Service;

mod media;
mod panels;
mod project;
mod registration;
mod specs;
mod storage;

use self::media::{
    host_register_audio_effect, host_register_decoder, host_register_encoder,
    host_register_exporter, host_register_importer,
};
use self::panels::{
    host_refresh_panel, host_refresh_viewer_overlay, host_register_panel,
    host_register_viewer_overlay, host_start_job,
};
use self::project::{
    host_clip, host_clips_at, host_clips_in_range, host_editor_view, host_in_out,
    host_markers_in_range, host_project_snapshot, host_project_transact, host_project_undo,
    host_selected_clips,
};
pub(crate) use self::registration::register_native_command;
use self::registration::{
    host_register_ai_tool, host_register_analyzer, host_register_clip_type, host_register_command,
    host_register_cpu_effect, host_register_driver_language, host_register_effect,
    host_register_generator, host_register_preset, host_register_scope, host_register_texture,
    host_register_theme, host_register_transition, host_register_workspace, host_subscribe,
};
use self::storage::{
    host_log, host_setting_get, host_storage_delete, host_storage_get, host_storage_keys,
    host_storage_set,
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

/// Convert `host_ctx` back into a shared `&HostCallContext`.
///
/// # Safety
///
/// `host_ctx` must be a valid pointer to a `HostCallContext` placed on the stack
/// by `NativePluginState` for the duration of the active plugin callback.
pub(super) unsafe fn call_ctx<'a>(
    host_ctx: *mut c_void,
) -> Result<&'a HostCallContext<'a>, String> {
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
pub(super) unsafe fn read_str<'a>(s: AriaStr, label: &str) -> Result<&'a str, String> {
    // SAFETY: Guaranteed by caller contract on `read_str`.
    unsafe { s.as_str() }.map_err(|err| format!("invalid UTF-8 in {label}: {err}"))
}
