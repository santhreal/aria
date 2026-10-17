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
use std::path::Path;

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_BYTES_FOUND, ARIA_BYTES_PANIC, ARIA_PARAM_COLOR, ARIA_PARAM_RECT,
    ARIA_PARAM_SCALAR, ARIA_PARAM_VEC2, ARIA_STATUS_ERR, ARIA_STATUS_OK, ARIA_STATUS_PANIC,
    AriaAudioBlock, AriaAudioParam, AriaBytes, AriaBytesResult, AriaCpuEffectCall,
    AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaHostV1,
    AriaInstanceResult, AriaJobStep, AriaParamValue, AriaResult, AriaScopeCall, AriaStr,
    free_rust_bytes, free_rust_string,
};

use crate::{
    AiToolImage, AiToolOutput, AiToolSpec, AiToolTier, AudioBlock, AudioEffectSpec, AudioParam,
    AudioParamSpec, AudioProcessor, ClipTypeSpec, CpuFrameIn, CpuFrameOut, DecodeFailure, Decoder,
    DecoderProbe, DecoderSpec, DriverLanguageSpec, DriverVars, DropAction, EffectParamSpec,
    EffectSpec, EncoderAudio, EncoderConfig, EncoderFrame, EncoderSession, EncoderSpec,
    EventResponse, ExportReport, ExporterSpec, GraphPass, HandleLook, Host, ImportResult,
    ImporterSpec, JobStep, LogLevel, MarkerColor, OutputSize, OverlayColor, OverlayEvent,
    OverlayScene, OverlayShape, Panel, PanelEvent, PanelLocation, PanelSpec, ParamValue, PassInput,
    Plugin, PluginRuntime, PresetSpec, Rational, RationalTime, ScopeFrame, ScopeImage, ScopeSpec,
    TimeRange, TransitionSpec, UiNode, VideoInfo, VideoStream, WorkspaceFocus, WorkspacePanes,
    WorkspaceSpec,
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

struct ScaleAudio {
    scale: f32,
}

impl AudioProcessor for ScaleAudio {
    fn process(&mut self, params: &[AudioParam<'_>], block: &mut AudioBlock<'_>) {
        let gain = params.first().map_or(self.scale, |p| p.constant());
        if gain < 0.0 {
            panic!("negative gain panic");
        }
        let sc_boost = block
            .sidechain()
            .and_then(|(l, _)| l.first().copied())
            .unwrap_or(0.0);
        let (left, right) = block.channels();
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let p = params.first().map_or(gain, |param| param.at(i));
            *l = (*l + sc_boost) * p;
            *r = (*r + sc_boost) * p;
        }
    }

    fn reset(&mut self) {
        if self.scale.is_nan() {
            panic!("nan reset panic");
        }
        self.scale = 1.0;
    }
}

struct DemoPanel {
    clicks: Mutex<u32>,
}

impl Panel for DemoPanel {
    fn view(&self, _host: &Host) -> UiNode {
        let clicks = *self.clicks.lock();
        if clicks == 99 {
            panic!("panel view panicked");
        }
        UiNode::column([
            UiNode::heading(format!("Clicks {clicks}")),
            UiNode::button("inc", "Increment"),
        ])
    }

    fn event(&self, host: &Host, event: &PanelEvent) -> Result<(), String> {
        match event {
            PanelEvent::Pressed { id } if id.as_ref() == "inc" => {
                *self.clicks.lock() += 1;
                host.refresh_panel("test.panel")
            }
            PanelEvent::Pressed { id } if id.as_ref() == "panic_event" => {
                panic!("panel event panicked");
            }
            PanelEvent::Pressed { id } if id.as_ref() == "arm_view_panic" => {
                *self.clicks.lock() = 99;
                Ok(())
            }
            other => Err(format!("unexpected event: {other:?}")),
        }
    }
}

struct DemoEncoderSession {
    frames: u32,
    samples: usize,
    panic_on_finish: bool,
}

impl EncoderSession for DemoEncoderSession {
    fn push_frame(&mut self, frame: &EncoderFrame<'_>) -> Result<(), String> {
        if frame.width == 0 {
            panic!("zero-width frame panic");
        }
        self.frames += 1;
        Ok(())
    }

    fn push_audio(&mut self, audio: &EncoderAudio<'_>) -> Result<(), String> {
        if audio.channels == 0 {
            panic!("zero-channel audio panic");
        }
        self.samples += audio.samples.len();
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        if self.panic_on_finish {
            panic!("finish encoder panic");
        }
        Ok(())
    }
}

/// Reads nothing: registration through the mock host is what is under test.
struct NoFiles;

impl Decoder for NoFiles {
    fn probe(&self, _host: &Host, path: &Path) -> Result<DecoderProbe, DecodeFailure> {
        Err(DecodeFailure::Unsupported(path.display().to_string()))
    }

    fn open_video(
        &self,
        _host: &Host,
        path: &Path,
    ) -> Result<(VideoInfo, Box<dyn VideoStream>), DecodeFailure> {
        Err(DecodeFailure::Unsupported(path.display().to_string()))
    }
}

struct TestPlugin;

impl Plugin for TestPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.log(LogLevel::Info, "mounting test plugin");
        host.register_command("test.run", "Run Test")?;
        host.subscribe("command-proposed")?;
        host.register_importer(&ImporterSpec {
            id: "test.imp".to_owned(),
            name: "Test Importer".to_owned(),
            extensions: vec!["tst".to_owned()],
        })?;
        host.register_exporter(&ExporterSpec {
            id: "test.exp".to_owned(),
            name: "Test Exporter".to_owned(),
            extension: "tst".to_owned(),
        })?;
        host.register_audio_effect(&AudioEffectSpec {
            id: "test.gain".to_owned(),
            label: "Test Gain".to_owned(),
            category: String::new(),
            params: vec![AudioParamSpec::scalar("gain", "Gain", 0.5, 0.0, 2.0)],
            latency: 0,
            sidechain: true,
        })?;
        host.register_panel(
            PanelSpec::new("test.panel", "Test Panel", PanelLocation::Sidebar),
            DemoPanel {
                clicks: Mutex::new(0),
            },
        )?;
        host.register_encoder(
            EncoderSpec {
                id: "test.enc".to_owned(),
                name: "Test Encoder".to_owned(),
                extension: "mov".to_owned(),
                audio: true,
            },
            |_host: &Host, path: &Path, cfg: &EncoderConfig| {
                if path.as_os_str() == "panic_open" {
                    panic!("open encoder panic");
                }
                let session: Box<dyn EncoderSession> = Box::new(DemoEncoderSession {
                    frames: 0,
                    samples: 0,
                    panic_on_finish: cfg.width == 999,
                });
                Ok(session)
            },
        )?;
        host.register_viewer_overlay("test.overlay", "Test Overlay")?;
        host.register_decoder(
            DecoderSpec {
                id: "test.dec".to_owned(),
                name: "Test Decoder".to_owned(),
                extensions: vec!["tdec".to_owned()],
                priority: 0,
            },
            NoFiles,
        )?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        if id == "panic" {
            panic!("command exploded");
        }
        if id == "fail" {
            return Err("command refused".to_owned());
        }
        host.storage_set("cmd", b"ran")?;
        let val = host.storage_get("cmd")?;
        assert_eq!(val.as_deref(), Some(&b"ran"[..]));
        assert_eq!(host.storage_keys()?, vec!["cmd".to_owned()]);
        host.storage_delete("cmd")?;
        assert_eq!(host.storage_get("cmd")?, None);
        assert_eq!(host.setting_get("notes.limit")?, Some(serde_json::json!(8)));
        assert_eq!(host.setting_get("absent")?, None);
        assert_eq!(host.project_snapshot()?["name"], "Demo");
        assert_eq!(host.editor_view()?["speed"], 1);
        let receipt = host.project_transact("Edit", &[serde_json::json!({"op": 1})])?;
        assert_eq!(receipt["revision"], 2);
        assert!(host.project_undo()?);
        Ok(())
    }

    fn event(&self, _host: &Host, kind: &str, event_json: &str) -> Result<EventResponse, String> {
        if event_json == "panic" {
            panic!("event handler panicked");
        }
        match kind {
            "command-proposed" => Ok(EventResponse::Refuse("too many tracks".to_owned())),
            "resolve-dropped" | "resolve-dropped-path" => {
                Ok(EventResponse::ClaimDrop(DropAction::ImportMedia))
            }
            _ => Ok(EventResponse::Pass),
        }
    }

    fn import(
        &self,
        _host: &Host,
        importer_id: &str,
        _path: &Path,
    ) -> Result<ImportResult, String> {
        if importer_id == "panic" {
            panic!("importer panicked");
        }
        Ok(ImportResult::new("[]", vec!["skipped line".to_owned()]))
    }

    fn export(
        &self,
        _host: &Host,
        exporter_id: &str,
        _project_json: &str,
        _path: &Path,
    ) -> Result<ExportReport, String> {
        if exporter_id == "panic" {
            panic!("exporter panicked");
        }
        Ok(ExportReport {
            skipped: vec!["unexported".to_owned()],
        })
    }

    fn audio_create(
        &self,
        _host: &Host,
        effect_id: &str,
        _sample_rate: u32,
    ) -> Result<Box<dyn AudioProcessor>, String> {
        if effect_id == "panic" {
            panic!("audio_create panicked");
        }
        if effect_id == "nan" {
            return Ok(Box::new(ScaleAudio { scale: f32::NAN }));
        }
        Ok(Box::new(ScaleAudio { scale: 0.5 }))
    }

    fn overlay_scene(&self, _host: &Host, id: &str) -> OverlayScene {
        if id == "panic_scene" {
            panic!("overlay scene panicked");
        }
        OverlayScene::new(vec![OverlayShape::Handle {
            id: "focus".into(),
            at: [100.0, 200.0],
            color: OverlayColor::Accent,
            look: HandleLook::Cross,
        }])
    }

    fn overlay_event(&self, host: &Host, id: &str, event: &OverlayEvent) -> Result<(), String> {
        let OverlayEvent::Drag { handle, .. } = event;
        if &**handle == "panic_event" {
            panic!("overlay event panicked");
        }
        if &**handle == "err_event" {
            return Err("overlay event refused".to_owned());
        }
        host.refresh_viewer_overlay(id)
    }
}

#[test]
fn runtime_executes_every_host_and_plugin_operation_and_catches_panics() {
    let runtime = PluginRuntime::new(TestPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;

    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    let mount_res = runtime.handle_mount(ctx);
    assert_eq!(mount_res.status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(st.commands.len(), 1);
        assert_eq!(st.subscriptions, vec!["command-proposed"]);
        assert_eq!(st.importers.len(), 1);
        assert_eq!(st.exporters.len(), 1);
        assert_eq!(st.audio_effects.len(), 1);
        assert_eq!(st.panels.len(), 1);
        assert_eq!(st.encoders.len(), 1);
        assert_eq!(st.overlays.len(), 1);
        assert_eq!(
            st.decoders,
            vec![r#"{"id":"test.dec","name":"Test Decoder","extensions":["tdec"],"priority":0}"#]
        );
    }

    let cmd_ok =
        runtime.handle_command_run(ctx, AriaStr::from_str("test.run"), AriaStr::from_str("{}"));
    assert_eq!(cmd_ok.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().undos, 1);

    let cmd_err =
        runtime.handle_command_run(ctx, AriaStr::from_str("fail"), AriaStr::from_str("{}"));
    assert_eq!(cmd_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let err_msg = unsafe { cmd_err.value.into_string_with(free_rust_string) };
    assert_eq!(err_msg, "command refused");

    let cmd_panic =
        runtime.handle_command_run(ctx, AriaStr::from_str("panic"), AriaStr::from_str("{}"));
    assert_eq!(cmd_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let panic_msg = unsafe { cmd_panic.value.into_string_with(free_rust_string) };
    assert_eq!(panic_msg, "command exploded");

    let ev_veto = runtime.handle_event(
        ctx,
        AriaStr::from_str("command-proposed"),
        AriaStr::from_str("{}"),
    );
    assert_eq!(ev_veto.status, ARIA_STATUS_OK);
    #[allow(unsafe_code)]
    let veto_json = unsafe { ev_veto.value.into_string_with(free_rust_string) };
    assert_eq!(veto_json, r#"{"veto":"too many tracks"}"#);

    let ev_panic = runtime.handle_event(
        ctx,
        AriaStr::from_str("command-proposed"),
        AriaStr::from_str("panic"),
    );
    assert_eq!(ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ev_panic_msg = unsafe { ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ev_panic_msg, "event handler panicked");

    let imp_panic =
        runtime.handle_import(ctx, AriaStr::from_str("panic"), AriaStr::from_str("a.tst"));
    assert_eq!(imp_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let _ = unsafe { imp_panic.value.into_string_with(free_rust_string) };

    let exp_panic = runtime.handle_export(
        ctx,
        AriaStr::from_str("panic"),
        AriaStr::from_str("{}"),
        AriaStr::from_str("a.tst"),
    );
    assert_eq!(exp_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let _ = unsafe { exp_panic.value.into_string_with(free_rust_string) };

    let created = runtime.handle_audio_create(ctx, AriaStr::from_str("test.gain"), 48_000);
    assert_eq!(created.status, ARIA_STATUS_OK);
    let mut left = [1.0f32, 2.0f32];
    let mut right = [4.0f32, 6.0f32];
    let sc_l = [1.0f32, 1.0f32];
    let sc_r = [0.0f32, 0.0f32];
    let per_frame = [0.5f32, 0.25f32];
    let param = AriaAudioParam {
        constant: 0.5,
        per_frame: per_frame.as_ptr(),
        per_frame_len: per_frame.len(),
    };
    let mut block = AriaAudioBlock {
        left: left.as_mut_ptr(),
        right: right.as_mut_ptr(),
        frames: 2,
        position: 0,
        sample_rate: 48_000,
        has_sidechain: 1,
        sidechain_left: sc_l.as_ptr(),
        sidechain_right: sc_r.as_ptr(),
    };
    #[allow(unsafe_code)]
    let proc_ok = unsafe {
        runtime.handle_audio_process(ctx, created.instance, &raw const param, 1, &raw mut block)
    };
    assert_eq!(proc_ok.status, ARIA_STATUS_OK);
    assert_eq!(left, [1.0, 0.75]);
    assert_eq!(right, [2.5, 1.75]);

    let bad_param = AriaAudioParam {
        constant: -1.0,
        per_frame: std::ptr::null(),
        per_frame_len: 0,
    };
    #[allow(unsafe_code)]
    let proc_panic = unsafe {
        runtime.handle_audio_process(
            ctx,
            created.instance,
            &raw const bad_param,
            1,
            &raw mut block,
        )
    };
    assert_eq!(proc_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let proc_panic_msg = unsafe { proc_panic.value.into_string_with(free_rust_string) };
    assert_eq!(proc_panic_msg, "negative gain panic");

    let dropped = runtime.handle_audio_drop(ctx, created.instance);
    assert_eq!(dropped.status, ARIA_STATUS_OK);

    let view_res = runtime.handle_panel_view(ctx, AriaStr::from_str("test.panel"));
    assert_eq!(view_res.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let view_bytes = unsafe { view_res.bytes.into_vec_with(free_rust_bytes) };
    let tree: UiNode = serde_json::from_slice(&view_bytes).expect("valid UiNode JSON");
    assert_eq!(
        tree,
        UiNode::column([
            UiNode::heading("Clicks 0"),
            UiNode::button("inc", "Increment"),
        ])
    );

    let ev_res = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"inc"}"#),
    );
    assert_eq!(ev_res.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().refreshes, vec!["test.panel".to_owned()]);

    let ev_panic = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"panic_event"}"#),
    );
    assert_eq!(ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ev_panic_msg = unsafe { ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ev_panic_msg, "panel event panicked");

    let arm_res = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"arm_view_panic"}"#),
    );
    assert_eq!(arm_res.status, ARIA_STATUS_OK);
    let view_panic = runtime.handle_panel_view(ctx, AriaStr::from_str("test.panel"));
    assert_eq!(view_panic.status, ARIA_BYTES_PANIC);
    #[allow(unsafe_code)]
    let view_panic_msg = unsafe { view_panic.error.into_string_with(free_rust_string) };
    assert_eq!(view_panic_msg, "panel view panicked");
    let enc_cfg = AriaEncoderConfig {
        width: 4,
        height: 2,
        fps_num: 24,
        fps_den: 1,
        video_bitrate_kbps: 1000,
        audio_bitrate_kbps: 128,
    };
    #[allow(unsafe_code)]
    let enc_opened = unsafe {
        runtime.handle_encoder_open(
            ctx,
            AriaStr::from_str("test.enc"),
            AriaStr::from_str("out.mov"),
            &raw const enc_cfg,
        )
    };
    assert_eq!(enc_opened.status, ARIA_STATUS_OK);
    let rgba = [255u8; 32];
    let raw_frame = AriaEncoderFrame {
        rgba: rgba.as_ptr(),
        rgba_len: rgba.len(),
        width: 4,
        height: 2,
        stride: 16,
        pts_value: 0,
        pts_rate: 24,
    };
    #[allow(unsafe_code)]
    let push_ok = unsafe {
        runtime.handle_encoder_push_frame(ctx, enc_opened.instance, &raw const raw_frame)
    };
    assert_eq!(push_ok.status, ARIA_STATUS_OK);
    let samples = [0.25f32, -0.25f32];
    let raw_audio = AriaEncoderAudio {
        samples: samples.as_ptr(),
        samples_len: samples.len(),
        sample_rate: 48_000,
        channels: 2,
        pts_value: 0,
        pts_rate: 48_000,
    };
    #[allow(unsafe_code)]
    let audio_ok = unsafe {
        runtime.handle_encoder_push_audio(ctx, enc_opened.instance, &raw const raw_audio)
    };
    assert_eq!(audio_ok.status, ARIA_STATUS_OK);
    let fin_ok = runtime.handle_encoder_finish(ctx, enc_opened.instance);
    assert_eq!(fin_ok.status, ARIA_STATUS_OK);

    #[allow(unsafe_code)]
    let enc_panic_open = unsafe {
        runtime.handle_encoder_open(
            ctx,
            AriaStr::from_str("test.enc"),
            AriaStr::from_str("panic_open"),
            &raw const enc_cfg,
        )
    };
    assert_eq!(enc_panic_open.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let open_panic_msg = unsafe { enc_panic_open.error.into_string_with(free_rust_string) };
    assert_eq!(open_panic_msg, "open encoder panic");

    let ovl_res = runtime.handle_overlay_scene(ctx, AriaStr::from_str("test.overlay"));
    assert_eq!(ovl_res.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let ovl_bytes = unsafe { ovl_res.bytes.into_vec_with(free_rust_bytes) };
    let decoded_scene: OverlayScene =
        serde_json::from_slice(&ovl_bytes).expect("valid OverlayScene JSON");
    assert_eq!(
        decoded_scene,
        OverlayScene::new(vec![OverlayShape::Handle {
            id: "focus".into(),
            at: [100.0, 200.0],
            color: OverlayColor::Accent,
            look: HandleLook::Cross,
        }])
    );

    let ovl_ev_ok = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(r#"{"kind":"drag","handle":"focus","phase":"end","at":[120.0,240.0]}"#),
    );
    assert_eq!(ovl_ev_ok.status, ARIA_STATUS_OK);
    assert_eq!(
        state.lock().overlay_refreshes,
        vec!["test.overlay".to_owned()]
    );

    let ovl_ev_err = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(r#"{"kind":"drag","handle":"err_event","phase":"start","at":[0.0,0.0]}"#),
    );
    assert_eq!(ovl_ev_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let ovl_err_msg = unsafe { ovl_ev_err.value.into_string_with(free_rust_string) };
    assert_eq!(ovl_err_msg, "overlay event refused");

    let ovl_ev_panic = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(
            r#"{"kind":"drag","handle":"panic_event","phase":"move","at":[10.0,20.0]}"#,
        ),
    );
    assert_eq!(ovl_ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ovl_panic_msg = unsafe { ovl_ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ovl_panic_msg, "overlay event panicked");

    let ovl_scene_panic = runtime.handle_overlay_scene(ctx, AriaStr::from_str("panic_scene"));
    assert_eq!(ovl_scene_panic.status, ARIA_BYTES_PANIC);
    #[allow(unsafe_code)]
    let scene_panic_msg = unsafe { ovl_scene_panic.error.into_string_with(free_rust_string) };
    assert_eq!(scene_panic_msg, "overlay scene panicked");

    let unmount_res = runtime.handle_unmount(ctx);
    assert_eq!(unmount_res.status, ARIA_STATUS_OK);
}

#[test]
fn legacy_host_table_without_overlays_reports_missing_callback() {
    struct OverlayOnlyPlugin;
    impl Plugin for OverlayOnlyPlugin {
        fn mount(&self, host: &Host) -> Result<(), String> {
            host.register_viewer_overlay("legacy.overlay", "Legacy Overlay")
        }
        fn command_run(&self, host: &Host, _id: &str, _args: &str) -> Result<(), String> {
            host.refresh_viewer_overlay("legacy.overlay")
        }
    }

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_ENCODERS_SIZE;
    assert!(!short_host.has_overlays());

    let runtime = PluginRuntime::new(OverlayOnlyPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;

    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);

    let mount_res = runtime.handle_mount(ctx);
    assert_eq!(mount_res.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let mount_err = unsafe { mount_res.value.into_string_with(free_rust_string) };
    assert!(
        mount_err.contains("register_viewer_overlay"),
        "expected missing callback name in error, got: {mount_err}"
    );

    let refresh_res =
        runtime.handle_command_run(ctx, AriaStr::from_str("c"), AriaStr::from_str("{}"));
    assert_eq!(refresh_res.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refresh_err = unsafe { refresh_res.value.into_string_with(free_rust_string) };
    assert!(
        refresh_err.contains("refresh_viewer_overlay"),
        "expected missing callback name in error, got: {refresh_err}"
    );
}

/// A plugin that counts to three in job steps. Detail, progress, `done`, and `commit`
/// cross the C ABI in `AriaJobStep` and the result value; a panic in a step returns
/// `ARIA_STATUS_PANIC`, and `job_end` receives the cancelled flag.
#[derive(Default)]
struct CountingJobs {
    taken: Mutex<u32>,
}

impl Plugin for CountingJobs {
    fn mount(&self, _host: &Host) -> Result<(), String> {
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args: &str) -> Result<(), String> {
        let job = host.start_job(id)?;
        host.log(LogLevel::Info, &format!("started {job}"));
        Ok(())
    }

    fn job_step(&self, _host: &Host, job: u64) -> Result<JobStep, String> {
        if job == 99 {
            panic!("step exploded");
        }
        if job == 98 {
            return Err("step refused".to_owned());
        }
        let taken = {
            let mut taken = self.taken.lock();
            *taken += 1;
            *taken
        };
        Ok(JobStep {
            progress: (taken < 3).then_some(taken as f32 / 3.0),
            detail: if taken == 3 {
                "counted".to_owned()
            } else {
                String::new()
            },
            done: taken == 3,
            commit: taken == 2,
        })
    }

    fn job_end(&self, host: &Host, job: u64, cancelled: bool) {
        host.log(
            LogLevel::Info,
            &format!("ended {job} cancelled={cancelled}"),
        );
    }
}

#[allow(unsafe_code)]
fn step_once(
    runtime: &PluginRuntime<CountingJobs>,
    ctx: *mut c_void,
    job: u64,
) -> (u32, AriaJobStep, String) {
    let mut step = AriaJobStep {
        progress: 7.0,
        done: 9,
        commit: 9,
    };
    // SAFETY: `step` is a live, writable local for the call.
    let res = unsafe { runtime.handle_job_step(ctx, job, &raw mut step) };
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, step, text)
}

#[test]
fn jobs_start_through_the_host_and_step_across_the_abi() {
    let runtime = PluginRuntime::new(CountingJobs::default());
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    let started =
        runtime.handle_command_run(ctx, AriaStr::from_str("render"), AriaStr::from_str("{}"));
    assert_eq!(started.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().jobs, vec!["render".to_owned()]);
    assert_eq!(
        state.lock().logs,
        vec![(aria_plugin_abi::ARIA_LOG_INFO, "started 41".to_owned())]
    );

    let (status, step, detail) = step_once(&runtime, ctx, 41);
    assert_eq!(status, ARIA_STATUS_OK);
    assert_eq!((step.progress, step.done, step.commit), (1.0 / 3.0, 0, 0));
    assert_eq!(detail, "");
    let (_, step, _) = step_once(&runtime, ctx, 41);
    assert_eq!((step.done, step.commit), (0, 1), "the second step commits");
    let (status, step, detail) = step_once(&runtime, ctx, 41);
    assert_eq!(status, ARIA_STATUS_OK);
    assert!(
        step.progress < 0.0,
        "unknown progress crosses as a negative value"
    );
    assert_eq!((step.done, step.commit, detail.as_str()), (1, 0, "counted"));

    let (status, step, detail) = step_once(&runtime, ctx, 98);
    assert_eq!((status, detail.as_str()), (ARIA_STATUS_ERR, "step refused"));
    assert_eq!(
        (step.progress, step.done, step.commit),
        (7.0, 9, 9),
        "a failed step writes nothing"
    );
    let (status, _, detail) = step_once(&runtime, ctx, 99);
    assert_eq!(
        (status, detail.as_str()),
        (ARIA_STATUS_PANIC, "step exploded")
    );

    #[allow(unsafe_code)]
    // SAFETY: a null step is the case `handle_job_step` rejects.
    let null = unsafe { runtime.handle_job_step(ctx, 41, std::ptr::null_mut()) };
    assert_eq!(null.status, ARIA_STATUS_ERR);

    assert_eq!(runtime.handle_job_end(ctx, 41, 0).status, ARIA_STATUS_OK);
    assert_eq!(runtime.handle_job_end(ctx, 42, 1).status, ARIA_STATUS_OK);
    let logs: Vec<String> = state
        .lock()
        .logs
        .iter()
        .map(|(_, text)| text.clone())
        .collect();
    assert_eq!(
        logs,
        [
            "started 41",
            "ended 41 cancelled=false",
            "ended 42 cancelled=true"
        ]
    );

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_OVERLAYS_SIZE;
    assert!(short_host.has_overlays());
    assert!(!short_host.has_jobs());
    let legacy = PluginRuntime::new(CountingJobs::default());
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused =
        legacy.handle_command_run(ctx, AriaStr::from_str("render"), AriaStr::from_str("{}"));
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("start_job"), "{refused}");
    assert_eq!(
        state.lock().jobs.len(),
        1,
        "a host without start_job is never called"
    );
}

/// Offers `shade`, which returns a label and an image, and fails or panics
/// on request.
struct Shades;

impl Plugin for Shades {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_ai_tool(&AiToolSpec {
            name: "shade".to_owned(),
            description: "Name a shade.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "tone": { "type": "string" } },
            }),
            tier: AiToolTier::Workspace,
        })
    }

    fn ai_tool_call(
        &self,
        _host: &Host,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<AiToolOutput, String> {
        match (name, arguments["tone"].as_str()) {
            ("shade", Some("boom")) => panic!("shade exploded"),
            ("shade", Some(tone)) => Ok(AiToolOutput {
                text: format!("{tone} shade"),
                images: vec![AiToolImage {
                    label: "swatch".to_owned(),
                    png: vec![137, 80, 78, 71],
                }],
            }),
            ("shade", None) => Err("tone is required".to_owned()),
            (other, _) => Err(format!("no AI tool {other}")),
        }
    }
}

fn tool_call(
    runtime: &PluginRuntime<Shades>,
    ctx: *mut c_void,
    name: &str,
    arguments: &str,
) -> (u32, String) {
    let res =
        runtime.handle_ai_tool_call(ctx, AriaStr::from_str(name), AriaStr::from_str(arguments));
    #[allow(unsafe_code)]
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, text)
}

#[test]
fn ai_tools_register_through_the_host_and_answer_across_the_abi() {
    let runtime = PluginRuntime::new(Shades);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    let specs: Vec<serde_json::Value> = state
        .lock()
        .ai_tools
        .iter()
        .map(|spec| serde_json::from_str(spec).expect("spec JSON"))
        .collect();
    assert_eq!(
        specs,
        [serde_json::json!({
            "name": "shade",
            "description": "Name a shade.",
            "parameters": {
                "type": "object",
                "properties": { "tone": { "type": "string" } },
            },
            "tier": "workspace",
        })]
    );

    let (status, json) = tool_call(&runtime, ctx, "shade", r#"{"tone":"teal"}"#);
    assert_eq!(status, ARIA_STATUS_OK, "{json}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).expect("output JSON"),
        serde_json::json!({
            "text": "teal shade",
            "images": [{ "label": "swatch", "png": [137, 80, 78, 71] }],
        })
    );
    assert_eq!(
        tool_call(&runtime, ctx, "shade", "{}"),
        (ARIA_STATUS_ERR, "tone is required".to_owned())
    );
    assert_eq!(
        tool_call(&runtime, ctx, "tint", r#"{"tone":"teal"}"#),
        (ARIA_STATUS_ERR, "no AI tool tint".to_owned())
    );
    let (status, text) = tool_call(&runtime, ctx, "shade", "{tone");
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(
        text.starts_with("the arguments of shade are not JSON"),
        "{text}"
    );
    assert_eq!(
        tool_call(&runtime, ctx, "shade", r#"{"tone":"boom"}"#),
        (ARIA_STATUS_PANIC, "shade exploded".to_owned())
    );

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_DECODERS_SIZE;
    assert!(short_host.has_decoders());
    assert!(!short_host.has_ai_tools());
    let legacy = PluginRuntime::new(Shades);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("AI tools"), "{refused}");
    assert_eq!(
        state.lock().ai_tools.len(),
        1,
        "a host without register_ai_tool is never called"
    );
}

/// Offers the scope `meter`, which copies the frame's first pixel's red and
/// green into a 2x1 image and reads back the frame size, and the scope
/// `boom`, which panics.
struct Meter;

impl Plugin for Meter {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_scope(&ScopeSpec {
            id: "meter".to_owned(),
            name: "Meter".to_owned(),
            width: 2,
            height: 1,
        })
    }

    fn scope_measure(
        &self,
        _host: &Host,
        id: &str,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage<'_>,
    ) -> Result<String, String> {
        match id {
            "meter" => {
                let [r, g, _, _] = frame.pixel(0, 0).ok_or("an empty frame")?;
                image.put(0, 0, [r, 0, 0, 255]);
                image.put(1, 0, [0, g, 0, 255]);
                Ok(format!("{}x{}", frame.width, frame.height))
            }
            "boom" => panic!("scope exploded"),
            other => Err(format!("no scope {other}")),
        }
    }
}

/// Measure a 1x1 frame of `pixel` with scope `id` into an image `image_len`
/// bytes long that states it is 2x1.
fn scope_call(
    runtime: &PluginRuntime<Meter>,
    ctx: *mut c_void,
    id: &str,
    pixel: [u8; 4],
    image_len: usize,
) -> (u32, String, Vec<u8>) {
    let mut image = vec![0_u8; image_len];
    let mut call = AriaScopeCall {
        frame: pixel.as_ptr(),
        frame_len: pixel.len(),
        frame_width: 1,
        frame_height: 1,
        image: image.as_mut_ptr(),
        image_len: image.len(),
        image_width: 2,
        image_height: 1,
    };
    #[allow(unsafe_code)]
    // SAFETY: `call` points at `pixel` and `image`, which outlive the call.
    let res = unsafe { runtime.handle_scope_measure(ctx, AriaStr::from_str(id), &raw mut call) };
    #[allow(unsafe_code)]
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, text, image)
}

#[test]
fn scopes_register_through_the_host_and_measure_across_the_abi() {
    let runtime = PluginRuntime::new(Meter);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    let specs: Vec<serde_json::Value> = state
        .lock()
        .scopes
        .iter()
        .map(|spec| serde_json::from_str(spec).expect("spec JSON"))
        .collect();
    assert_eq!(
        specs,
        [serde_json::json!({ "id": "meter", "name": "Meter", "width": 2, "height": 1 })]
    );

    assert_eq!(
        scope_call(&runtime, ctx, "meter", [10, 20, 30, 255], 8),
        (
            ARIA_STATUS_OK,
            "1x1".to_owned(),
            vec![10, 0, 0, 255, 0, 20, 0, 255]
        )
    );
    assert_eq!(
        scope_call(&runtime, ctx, "tint", [10, 20, 30, 255], 8),
        (ARIA_STATUS_ERR, "no scope tint".to_owned(), vec![0; 8])
    );
    let (status, text, image) = scope_call(&runtime, ctx, "meter", [10, 20, 30, 255], 4);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(
        text.contains("a 2x1 image in 4 bytes"),
        "a short image buffer is refused before the plugin runs: {text}"
    );
    assert_eq!(image, [0; 4]);
    assert_eq!(
        scope_call(&runtime, ctx, "boom", [10, 20, 30, 255], 8),
        (ARIA_STATUS_PANIC, "scope exploded".to_owned(), vec![0; 8])
    );
    #[allow(unsafe_code)]
    // SAFETY: a null call is refused before it is read.
    let null = unsafe {
        runtime.handle_scope_measure(ctx, AriaStr::from_str("meter"), std::ptr::null_mut())
    };
    assert_eq!(null.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    // SAFETY: `null.value` was allocated by the runtime with the Rust allocator.
    drop(unsafe { null.value.into_string_with(free_rust_string) });

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_AI_TOOLS_SIZE;
    assert!(short_host.has_ai_tools());
    assert!(!short_host.has_scopes());
    let legacy = PluginRuntime::new(Meter);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("scopes"), "{refused}");
    assert_eq!(
        state.lock().scopes.len(),
        1,
        "a host without register_scope is never called"
    );
}

struct ContribPlugin;

impl Plugin for ContribPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_effect(
            &EffectSpec::new("example.tint", "Tint")
                .with_category("Color")
                .with_params(vec![EffectParamSpec::scalar(
                    "amount", "Amount", 0.5, 0.0, 1.0,
                )]),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
        )?;
        host.register_cpu_effect(
            EffectSpec::new("example.invert", "Invert").with_category("Color"),
            |_host: &Host, input: &CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>| {
                let delta = input.param_at(0).unwrap_or(0.0);
                for y in 0..input.height() {
                    for x in 0..input.width() {
                        let [r, g, b, a] = input.pixel_f32(x, y);
                        output.set_pixel_f32(x, y, [1.0 - r, 1.0 - g, (1.0 - b) + delta, a]);
                    }
                }
                Ok(())
            },
        )?;
        host.register_transition(
            &TransitionSpec::new("example.wipe", "Wipe"),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.5); }",
        )?;
        host.register_generator(
            &EffectSpec::new("example.bars", "Bars Generator").with_category("Generate"),
            "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.25); }",
        )?;
        host.register_cpu_generator(
            EffectSpec::new("example.solid", "Solid Generator").with_category("Generate"),
            |_host: &Host, _input: &CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>| {
                output.fill_f32([0.25, 0.5, 0.75, 1.0]);
                Ok(())
            },
        )?;
        host.register_clip_type(
            &ClipTypeSpec::new(
                "example.card",
                "Title Card",
                "example.solid",
                MarkerColor::Teal,
            )
            .with_handles(vec!["origin".to_owned()]),
        )?;
        host.register_driver_language(
            DriverLanguageSpec::new("example.expr", "Example Expr"),
            |_host: &Host, expr: &str, vars: &DriverVars| match expr {
                "double" => match vars.value {
                    ParamValue::Scalar(v) => Ok(ParamValue::Scalar(v * 2.0)),
                    other => Ok(other),
                },
                "vec2" => Ok(ParamValue::Vec2([vars.time as f32, vars.clip_time as f32])),
                "color" => Ok(ParamValue::Color([1, 2, 3, 255])),
                "rect" => Ok(ParamValue::Rect([0.1, 0.2, 0.3, 0.4])),
                "err" => Err("bad expr".to_owned()),
                "panic" => panic!("driver exploded"),
                _ => Ok(vars.value),
            },
        )?;
        host.register_preset(
            &PresetSpec::new("Soft Fade")
                .with_effect("vignette")
                .with_scalar_property("opacity", 0.75),
        )?;
        host.register_theme("midnight", "[dark]\nbg = \"#101216\"\n")?;
        host.register_workspace(&WorkspaceSpec::new("color", "Color Grading").with_panes(
            WorkspacePanes {
                scopes: Some(true),
                focus: Some(WorkspaceFocus::Preview),
                ..WorkspacePanes::default()
            },
        ))?;
        Ok(())
    }

    fn cpu_effect_process(
        &self,
        _host: &Host,
        id: &str,
        _input: &CpuFrameIn<'_>,
        _output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        if id == "example.panic" {
            panic!("cpu effect exploded");
        }
        Err(format!("no cpu effect {id}"))
    }
}

struct GraphPlugin;

impl Plugin for GraphPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_texture("example.lut", 2, 1, &[255, 0, 0, 255, 0, 0, 255, 128])?;
        host.register_effect_graph(
            &EffectSpec::new("example.graph", "Graph").with_category("Stylize"),
            &[
                GraphPass::compute("compute-src", [8, 4])
                    .named("half")
                    .sized(OutputSize::Scale([0.5, 0.5])),
                GraphPass::fragment("fragment-src").reading([
                    PassInput::Pass("half".to_owned()),
                    PassInput::Source,
                    PassInput::Layer("matte".to_owned()),
                    PassInput::Texture("example.lut".to_owned()),
                ]),
            ],
        )
    }
}

/// The graph reaches the host as the JSON array `register_effect` reads in
/// place of WGSL, field for field, and a texture reaches `register_texture`
/// with its size and bytes. A host table that ends before `register_texture`
/// refuses the texture without calling past its end.
#[test]
fn an_effect_graph_and_a_texture_cross_the_abi_as_declared() {
    let runtime = PluginRuntime::new(GraphPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);
    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(
            st.textures,
            [(
                "example.lut".to_owned(),
                2,
                1,
                vec![255, 0, 0, 255, 0, 0, 255, 128]
            )]
        );
        assert_eq!(st.effects.len(), 1);
        let (spec, graph) = &st.effects[0];
        let spec: serde_json::Value = serde_json::from_str(spec).expect("spec JSON");
        assert_eq!(spec["id"], "example.graph");
        let graph: serde_json::Value = serde_json::from_str(graph).expect("graph JSON");
        assert_eq!(
            graph,
            serde_json::json!([
                {
                    "name": "half",
                    "stage": { "compute": { "workgroup": [8, 4] } },
                    "inputs": [],
                    "output": { "scale": [0.5, 0.5] },
                    "source": "compute-src",
                },
                {
                    "name": "",
                    "stage": "fragment",
                    "inputs": [
                        { "pass": "half" },
                        "source",
                        { "layer": "matte" },
                        { "texture": "example.lut" },
                    ],
                    "output": "same",
                    "source": "fragment-src",
                },
            ])
        );
    }

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_QUERIES_SIZE;
    assert!(short_host.has_project_queries());
    assert!(!short_host.has_textures());
    let legacy = PluginRuntime::new(GraphPlugin);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("texture registration"), "{refused}");
    assert_eq!(
        state.lock().textures.len(),
        1,
        "a host without register_texture is never called"
    );
}

#[test]
fn contributions_register_and_dispatch_cpu_effect_and_driver_across_abi() {
    let runtime = PluginRuntime::new(ContribPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(st.effects.len(), 1);
        assert_eq!(st.cpu_effects.len(), 1);
        assert_eq!(st.transitions.len(), 1);
        assert_eq!(st.generators.len(), 2);
        assert_eq!(st.clip_types.len(), 1);
        assert_eq!(st.driver_languages.len(), 1);
        assert_eq!(st.presets.len(), 1);
        assert_eq!(st.themes.len(), 1);
        assert_eq!(st.workspaces.len(), 1);
    }

    // 1. CPU effect processes a 1x1 RGBA16F frame with scalar param (preserving >1.0 HDR values).
    let input_px = [
        crate::f16::from_f32(0.75),
        crate::f16::from_f32(0.25),
        crate::f16::from_f32(-0.5),
        crate::f16::ONE,
    ];
    let mut output_px = [0u16; 4];
    let params = [2.5f32];
    let mut call = AriaCpuEffectCall {
        input: input_px.as_ptr(),
        output: output_px.as_mut_ptr(),
        pixels_len: 4,
        width: 1,
        height: 1,
        params: params.as_ptr(),
        params_len: 1,
        params_json: AriaStr::from_str(""),
        time: 0.5,
    };
    #[allow(unsafe_code)]
    let res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.invert"), &raw mut call)
    };
    assert_eq!(res.status, ARIA_STATUS_OK);
    assert_eq!(
        output_px,
        [
            crate::f16::from_f32(0.25),
            crate::f16::from_f32(0.75),
            crate::f16::from_f32(4.0),
            crate::f16::ONE,
        ]
    );

    // 2. Generator fills the output frame.
    let mut gen_px = [0u16; 4];
    call.output = gen_px.as_mut_ptr();
    call.params = std::ptr::null();
    call.params_len = 0;
    #[allow(unsafe_code)]
    let gen_res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.solid"), &raw mut call)
    };
    assert_eq!(gen_res.status, ARIA_STATUS_OK);
    assert_eq!(
        gen_px,
        [
            crate::f16::from_f32(0.25),
            crate::f16::from_f32(0.5),
            crate::f16::from_f32(0.75),
            crate::f16::ONE,
        ]
    );

    // 3. Panicking CPU effect catches unwind and returns ARIA_STATUS_PANIC.
    #[allow(unsafe_code)]
    let panic_res = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("example.panic"), &raw mut call)
    };
    assert_eq!(panic_res.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let panic_msg = unsafe { panic_res.value.into_string_with(free_rust_string) };
    assert_eq!(panic_msg, "cpu effect exploded");

    // 4. Driver evaluate handles Scalar, Vec2, Color, Rect, Err, and Panic.
    let mut drv_call = AriaDriverEvalCall {
        time: 1.5,
        clip_time: 0.5,
        frame_rate: 24.0,
        duration: 10.0,
        canvas_width: 1920,
        canvas_height: 1080,
        value: AriaParamValue {
            kind: ARIA_PARAM_SCALAR,
            x: 0.4,
            ..AriaParamValue::default()
        },
        seed: 42,
        output: AriaParamValue::default(),
    };
    #[allow(unsafe_code)]
    let drv_ok = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("double"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_ok.status, ARIA_STATUS_OK);
    assert_eq!(drv_call.output.kind, ARIA_PARAM_SCALAR);
    assert!((drv_call.output.x - 0.8).abs() < 1e-6);

    for (expr, expected_kind) in [
        ("vec2", ARIA_PARAM_VEC2),
        ("color", ARIA_PARAM_COLOR),
        ("rect", ARIA_PARAM_RECT),
    ] {
        #[allow(unsafe_code)]
        let res = unsafe {
            runtime.handle_driver_evaluate(
                ctx,
                AriaStr::from_str("example.expr"),
                AriaStr::from_str(expr),
                &raw mut drv_call,
            )
        };
        assert_eq!(res.status, ARIA_STATUS_OK);
        assert_eq!(drv_call.output.kind, expected_kind);
    }

    #[allow(unsafe_code)]
    let drv_err = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("err"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let drv_err_msg = unsafe { drv_err.value.into_string_with(free_rust_string) };
    assert_eq!(drv_err_msg, "bad expr");

    #[allow(unsafe_code)]
    let drv_panic = unsafe {
        runtime.handle_driver_evaluate(
            ctx,
            AriaStr::from_str("example.expr"),
            AriaStr::from_str("panic"),
            &raw mut drv_call,
        )
    };
    assert_eq!(drv_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let drv_panic_msg = unsafe { drv_panic.value.into_string_with(free_rust_string) };
    assert_eq!(drv_panic_msg, "driver exploded");

    // 5. Old host table (SCOPES_SIZE) refuses contribution registrations cleanly.
    let mut scopes_only_host = MOCK_HOST;
    scopes_only_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_SCOPES_SIZE;
    assert!(scopes_only_host.has_scopes());
    assert!(!scopes_only_host.has_contributions());
    let legacy = PluginRuntime::new(ContribPlugin);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&scopes_only_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused_msg = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused_msg.contains("effect registration"), "{refused_msg}");
}

#[test]
fn host_project_queries_round_trip_and_refuse_on_shorter_table() {
    let mut state = Mutex::new(MockHostState::default());
    let ctx = (&raw mut state).cast::<c_void>();
    #[allow(unsafe_code)]
    let host = unsafe { Host::from_raw(&MOCK_HOST, ctx) };

    let t0 = RationalTime::new(0, Rational::FPS_24);
    let t24 = RationalTime::new(24, Rational::FPS_24);
    let range = TimeRange::new(t0, t24);

    let c1 = host.clip(1).expect("clip 1").expect("present");
    assert_eq!(c1.id, 1);
    assert_eq!(c1.track, 10);
    assert_eq!(c1.end, t24);
    assert_eq!(host.clip(99).expect("clip 99"), None);

    let in_range = host.clips_in_range(Some(7), range).expect("clips_in_range");
    assert_eq!(in_range.len(), 1);
    assert_eq!(in_range[0].id, 7);

    assert!(host.clips_at(None, t0).expect("clips_at").is_empty());
    assert_eq!(host.selected_clips().expect("selected_clips").len(), 1);

    let markers = host
        .markers_in_range(None, range)
        .expect("markers_in_range");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "Beat");
    assert_eq!(markers[0].color, MarkerColor::Teal);

    let io = host.in_out().expect("in_out");
    assert_eq!(io.mark_in, Some(RationalTime::new(4, Rational::FPS_24)));
    assert_eq!(io.mark_out, Some(RationalTime::new(20, Rational::FPS_24)));

    static SHORT_HOST: AriaHostV1 = {
        let mut h = MOCK_HOST;
        h.struct_size = aria_plugin_abi::ARIA_HOST_V1_CONTRIBUTIONS_SIZE;
        h
    };
    assert!(SHORT_HOST.has_contributions());
    assert!(!SHORT_HOST.has_project_queries());
    #[allow(unsafe_code)]
    let short = unsafe { Host::from_raw(&SHORT_HOST, ctx) };
    assert!(short.clip(1).is_err());
    assert!(short.clips_in_range(None, range).is_err());
    assert!(short.clips_at(None, t0).is_err());
    assert!(short.selected_clips().is_err());
    assert!(short.markers_in_range(None, range).is_err());
    assert!(short.in_out().is_err());
}

/// Closes the defect class where `aria_sdk::f16::{to_f32, from_f32}` corrupts
/// subnormal, negative, HDR (`> 1.0`), infinity, or NaN IEEE-754 binary16
/// bit patterns.
#[test]
fn f16_helpers_round_trip_every_u16_bit_pattern() {
    for bits in 0u16..=u16::MAX {
        let f = crate::f16::to_f32(bits);
        let back = crate::f16::from_f32(f);
        if (bits & 0x7c00) == 0x7c00 && (bits & 0x03ff) != 0 {
            assert!(f.is_nan(), "NaN f16 {bits:#06x} must decode to NaN");
            assert_eq!(back & 0x8000, bits & 0x8000, "NaN sign preserved");
            assert_eq!(back & 0x7c00, 0x7c00, "NaN exponent preserved");
            assert_ne!(back & 0x03ff, 0, "NaN mantissa non-zero");
        } else {
            assert_eq!(
                back, bits,
                "f16 {bits:#06x} -> {f} -> {back:#06x} failed round-trip"
            );
        }
    }
    assert_eq!(crate::f16::from_f32(1.0), crate::f16::ONE);
    assert_eq!(crate::f16::to_f32(crate::f16::ONE), 1.0);
    assert_eq!(crate::f16::to_f32(crate::f16::from_f32(4.0)), 4.0);
    assert_eq!(crate::f16::to_f32(crate::f16::from_f32(-1.25)), -1.25);
}
