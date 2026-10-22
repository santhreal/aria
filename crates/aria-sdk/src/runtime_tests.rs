//! Unit tests for [`PluginRuntime`] and [`Host`] over the C ABI tables.
//!
//! Defect class closed: ABI dispatch mismatches, leaked or dangling FFI buffers,
//! unguarded guest panics unwinding across `extern "C"`, and missing instance
//! cleanup on unmount or encoder session drop.
//!
//! What this does not catch: dynamic library loading (`dlopen`/`LoadLibrary`) or
//! host-side `PluginContext` registration; those are covered by
//! `aria_plugin::external::tests`.

use parking_lot::Mutex;
use std::ffi::c_void;

use aria_plugin_abi::{
    ARIA_API_MAJOR, AriaBytes, AriaBytesResult, AriaHostV1, AriaInstanceResult, AriaResult,
    AriaStr, free_rust_bytes, free_rust_string,
};

#[derive(Default)]
struct MockHostState {
    logs: Vec<(u32, String)>,
    storage: std::collections::BTreeMap<String, Vec<u8>>,
    commands: Vec<(String, String)>,
    subscriptions: Vec<String>,
    importers: Vec<String>,
    exporters: Vec<String>,
    audio_effects: Vec<String>,
    transacts: Vec<(String, String)>,
    undos: usize,
    panels: Vec<String>,
    refreshes: Vec<String>,
    encoders: Vec<String>,
    overlays: Vec<String>,
    overlay_refreshes: Vec<String>,
    jobs: Vec<String>,
    decoders: Vec<String>,
    ai_tools: Vec<String>,
    scopes: Vec<String>,
    effects: Vec<(String, String)>,
    cpu_effects: Vec<String>,
    transitions: Vec<(String, String)>,
    generators: Vec<(String, String)>,
    clip_types: Vec<String>,
    driver_languages: Vec<String>,
    presets: Vec<String>,
    themes: Vec<(String, String)>,
    workspaces: Vec<String>,
    analyzers: Vec<String>,
    textures: Vec<(String, u32, u32, Vec<u8>)>,
}

#[allow(unsafe_code)]
unsafe fn state_mut<'a>(ctx: *mut c_void) -> &'a Mutex<MockHostState> {
    // SAFETY: Tests pass `&Mutex<MockHostState>` cast to `*mut c_void`.
    unsafe { &*(ctx as *const Mutex<MockHostState>) }
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_log(ctx: *mut c_void, level: u32, text: AriaStr) {
    // SAFETY: Validated by test harness.
    let s = unsafe { text.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().logs.push((level, s));
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_storage_get(ctx: *mut c_void, key: AriaStr) -> AriaBytesResult {
    // SAFETY: Validated by test harness.
    let k = unsafe { key.as_str() }.unwrap();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    let map = &unsafe { state_mut(ctx) }.lock().storage;
    match map.get(k) {
        Some(bytes) => AriaBytesResult::found(bytes.clone()),
        None => AriaBytesResult::not_found(),
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_storage_set(
    ctx: *mut c_void,
    key: AriaStr,
    value: AriaBytes,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let k = unsafe { key.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let v = unsafe { value.as_slice() }.to_vec();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().storage.insert(k, v);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_storage_delete(ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let k = unsafe { key.as_str() }.unwrap();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().storage.remove(k);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_storage_keys(ctx: *mut c_void) -> AriaResult {
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    let keys: Vec<String> = unsafe { state_mut(ctx) }
        .lock()
        .storage
        .keys()
        .cloned()
        .collect();
    AriaResult::ok(serde_json::to_string(&keys).unwrap())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_setting_get(_ctx: *mut c_void, key: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let k = unsafe { key.as_str() }.unwrap();
    if k == "notes.limit" {
        AriaResult::ok("8".to_owned())
    } else {
        AriaResult::ok_empty()
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_command(
    ctx: *mut c_void,
    id: AriaStr,
    title: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let id = unsafe { id.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let title = unsafe { title.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().commands.push((id, title));
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_subscribe(ctx: *mut c_void, kind: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let kind = unsafe { kind.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().subscriptions.push(kind);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_importer(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().importers.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_exporter(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().exporters.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_audio_effect(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().audio_effects.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_project_snapshot(_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok(r#"{"name":"Demo"}"#.to_owned())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_editor_view(_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok(r#"{"playing":false,"speed":1}"#.to_owned())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_project_transact(
    ctx: *mut c_void,
    label: AriaStr,
    commands_json: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let label = unsafe { label.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let cmds = unsafe { commands_json.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }
        .lock()
        .transacts
        .push((label, cmds));
    AriaResult::ok(r#"{"revision":2,"outcomes":[]}"#.to_owned())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_project_undo(ctx: *mut c_void) -> AriaResult {
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().undos += 1;
    AriaResult::ok("true".to_owned())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_panel(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().panels.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_refresh_panel(ctx: *mut c_void, id: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let id = unsafe { id.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().refreshes.push(id);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_encoder(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().encoders.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_viewer_overlay(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().overlays.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_refresh_viewer_overlay(ctx: *mut c_void, id: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let id = unsafe { id.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().overlay_refreshes.push(id);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_start_job(ctx: *mut c_void, label: AriaStr) -> AriaInstanceResult {
    // SAFETY: Validated by test harness.
    let label = unsafe { label.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    let mut state = unsafe { state_mut(ctx) }.lock();
    state.jobs.push(label);
    AriaInstanceResult::ok(state.jobs.len() as u64 + 40)
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_decoder(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().decoders.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_ai_tool(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().ai_tools.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_scope(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().scopes.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_effect(
    ctx: *mut c_void,
    spec: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let wgsl = unsafe { wgsl.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().effects.push((spec, wgsl));
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_cpu_effect(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().cpu_effects.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_transition(
    ctx: *mut c_void,
    spec: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let wgsl = unsafe { wgsl.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }
        .lock()
        .transitions
        .push((spec, wgsl));
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_generator(
    ctx: *mut c_void,
    spec: AriaStr,
    wgsl: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let wgsl = unsafe { wgsl.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }
        .lock()
        .generators
        .push((spec, wgsl));
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_clip_type(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().clip_types.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_driver_language(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().driver_languages.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_preset(ctx: *mut c_void, preset: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let preset = unsafe { preset.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().presets.push(preset);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_theme(
    ctx: *mut c_void,
    id: AriaStr,
    toml: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let id = unsafe { id.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let toml = unsafe { toml.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().themes.push((id, toml));
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_workspace(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().workspaces.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_analyzer(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
    // SAFETY: Validated by test harness.
    let spec = unsafe { spec.as_str() }.unwrap().to_owned();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }.lock().analyzers.push(spec);
    AriaResult::ok_empty()
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_clip(_ctx: *mut c_void, id: u64) -> AriaResult {
    if id == 1 {
        AriaResult::ok(
            serde_json::json!({
                "id": 1,
                "track": 10,
                "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
                "end": { "value": 24, "rate": { "num": 24, "den": 1 } },
                "kind": "media",
                "name": "Clip 1",
            })
            .to_string(),
        )
    } else {
        AriaResult::ok_empty()
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_clips_in_range(
    _ctx: *mut c_void,
    sequence_id: u64,
    range_json: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let raw = unsafe { range_json.as_str() }.unwrap();
    assert!(raw.contains("\"start\""));
    AriaResult::ok(
        serde_json::json!([{
            "id": sequence_id.max(1),
            "track": 10,
            "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
            "end": { "value": 24, "rate": { "num": 24, "den": 1 } },
            "kind": "media",
            "name": "Clip 1",
        }])
        .to_string(),
    )
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_clips_at(
    _ctx: *mut c_void,
    _sequence_id: u64,
    tick_json: AriaStr,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let raw = unsafe { tick_json.as_str() }.unwrap();
    assert!(raw.contains("\"value\""));
    AriaResult::ok("[]".to_owned())
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_selected_clips(_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok(
        serde_json::json!([{
            "id": 1,
            "track": 10,
            "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
            "end": { "value": 24, "rate": { "num": 24, "den": 1 } },
            "kind": "media",
            "name": "Clip 1",
        }])
        .to_string(),
    )
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_markers_in_range(
    _ctx: *mut c_void,
    _sequence_id: u64,
    _range_json: AriaStr,
) -> AriaResult {
    AriaResult::ok(
        serde_json::json!([{
            "id": 5,
            "tick": { "value": 12, "rate": { "num": 24, "den": 1 } },
            "name": "Beat",
            "color": "teal",
        }])
        .to_string(),
    )
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_in_out(_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok(
        serde_json::json!({
            "mark_in": { "value": 4, "rate": { "num": 24, "den": 1 } },
            "mark_out": { "value": 20, "rate": { "num": 24, "den": 1 } },
        })
        .to_string(),
    )
}

#[allow(unsafe_code)]
unsafe extern "C" fn mock_register_texture(
    ctx: *mut c_void,
    id: AriaStr,
    width: u32,
    height: u32,
    pixels: AriaBytes,
) -> AriaResult {
    // SAFETY: Validated by test harness.
    let id = unsafe { id.as_str() }.unwrap().to_owned();
    // SAFETY: Validated by test harness.
    let pixels = unsafe { pixels.as_slice() }.to_vec();
    // SAFETY: `ctx` points to `Mutex<MockHostState>`.
    unsafe { state_mut(ctx) }
        .lock()
        .textures
        .push((id, width, height, pixels));
    AriaResult::ok_empty()
}

static MOCK_HOST: AriaHostV1 = AriaHostV1 {
    struct_size: std::mem::size_of::<AriaHostV1>(),
    api_major: ARIA_API_MAJOR,
    reserved: 0,
    free_string: free_rust_string,
    free_bytes: free_rust_bytes,
    log: mock_log,
    storage_get: mock_storage_get,
    storage_set: mock_storage_set,
    storage_delete: mock_storage_delete,
    storage_keys: mock_storage_keys,
    setting_get: mock_setting_get,
    register_command: mock_register_command,
    subscribe: mock_subscribe,
    register_importer: mock_register_importer,
    register_exporter: mock_register_exporter,
    register_audio_effect: mock_register_audio_effect,
    project_snapshot: mock_project_snapshot,
    editor_view: mock_editor_view,
    project_transact: mock_project_transact,
    project_undo: mock_project_undo,
    register_panel: mock_register_panel,
    refresh_panel: mock_refresh_panel,
    register_encoder: mock_register_encoder,
    register_viewer_overlay: mock_register_viewer_overlay,
    refresh_viewer_overlay: mock_refresh_viewer_overlay,
    start_job: mock_start_job,
    register_decoder: mock_register_decoder,
    register_ai_tool: mock_register_ai_tool,
    register_scope: mock_register_scope,
    register_effect: mock_register_effect,
    register_cpu_effect: mock_register_cpu_effect,
    register_transition: mock_register_transition,
    register_generator: mock_register_generator,
    register_clip_type: mock_register_clip_type,
    register_driver_language: mock_register_driver_language,
    register_preset: mock_register_preset,
    register_theme: mock_register_theme,
    register_workspace: mock_register_workspace,
    register_analyzer: mock_register_analyzer,
    clip: mock_clip,
    clips_in_range: mock_clips_in_range,
    clips_at: mock_clips_at,
    selected_clips: mock_selected_clips,
    markers_in_range: mock_markers_in_range,
    in_out: mock_in_out,
    register_texture: mock_register_texture,
};

#[path = "runtime_tests_contributions.rs"]
mod contributions;
#[path = "runtime_tests_dispatch.rs"]
mod dispatch;
#[path = "runtime_tests_services.rs"]
mod services;
