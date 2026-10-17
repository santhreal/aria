//! Reference WebAssembly component guest plugin for Aria (`world aria:plugin/plugin@1.0.0`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/aria-plugin-wasm/wit",
});

mod y4m;

const DEFAULT_MARKER_NAME: &str = "wasm-marker-v1";

/// The id of the luma histogram scope.
const LUMA_SCOPE: &str = "example.luma-histogram";
/// Columns of the luma histogram: four code values each.
const LUMA_BINS: u32 = 64;
/// Rows of the luma histogram.
const LUMA_HEIGHT: u32 = 32;
/// The colour of a luma histogram bar.
const LUMA_BAR: [u8; 4] = [64, 200, 190, 255];

/// A 1x1 RGBA PNG, returned by the `example_swatch` AI tool.
const SWATCH_PNG: [u8; 70] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 208, 170, 191, 242, 31, 0,
    4, 209, 2, 125, 61, 100, 161, 106, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct GainInstance {
    sample_rate: u32,
    frames_processed: u64,
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
static AUDIO_INSTANCES: Mutex<Option<HashMap<u64, GainInstance>>> = Mutex::new(None);

struct EncoderState {
    path: String,
    config: EncoderConfig,
    frames: u64,
    rgba_bytes: usize,
    audio_samples: usize,
}

static ENCODER_INSTANCES: Mutex<Option<HashMap<u64, EncoderState>>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    Normal,
    MalformedList,
    TrapView,
}

struct PanelModel {
    presses: u32,
    level: f64,
    note: String,
    mode: PanelMode,
}

impl Default for PanelModel {
    fn default() -> Self {
        Self {
            presses: 0,
            level: 0.5,
            note: "ready".to_owned(),
            mode: PanelMode::Normal,
        }
    }
}

static PANEL_STATE: Mutex<Option<PanelModel>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlayMode {
    Normal,
    TrapScene,
}

struct OverlayModel {
    focus_x: f32,
    focus_y: f32,
    drags: u32,
    refreshes: u32,
    mode: OverlayMode,
}

impl Default for OverlayModel {
    fn default() -> Self {
        Self {
            focus_x: 960.0,
            focus_y: 540.0,
            drags: 0,
            refreshes: 0,
            mode: OverlayMode::Normal,
        }
    }
}

static OVERLAY_STATE: Mutex<Option<OverlayModel>> = Mutex::new(None);

/// Steps a counting job takes before it is done.
const COUNT_STEPS: u32 = 4;

struct JobModel {
    taken: u32,
    endless: bool,
}

static JOBS: Mutex<Option<HashMap<u64, JobModel>>> = Mutex::new(None);

struct AnalyzerInstance {
    channels: u16,
    frames: i64,
    peaks: Vec<i64>,
}

static ANALYZERS: Mutex<Option<HashMap<u64, AnalyzerInstance>>> = Mutex::new(None);

/// Start a job that counts to [`COUNT_STEPS`], or one that steps until
/// cancelled, and record its id in storage under `last-job`.
fn start_counting_job(endless: bool) -> Result<(), String> {
    let job = start_job(if endless { "endless" } else { "count" })?;
    JOBS.lock()
        .map_err(|_| "job state lock poisoned".to_owned())?
        .get_or_insert_with(HashMap::new)
        .insert(job, JobModel { taken: 0, endless });
    storage_set("last-job", job.to_string().as_bytes())
}
struct ExampleWasmPlugin;

impl Guest for ExampleWasmPlugin {
    fn mount() -> Result<(), String> {
        log(LogLevel::Info, "mounting com.example.wasm-tools");
        if let Ok(mut guard) = PANEL_STATE.lock() {
            *guard = Some(PanelModel::default());
        }
        if let Ok(mut guard) = OVERLAY_STATE.lock() {
            *guard = Some(OverlayModel::default());
        }

        register_command("example.add-marker", "Add Example Marker")?;
        register_command("example.inspect-and-undo", "Inspect Project and Undo")?;
        register_command("example.query-project", "Query Project and View")?;
        register_command("example.storage-roundtrip", "Exercise Plugin Storage")?;
        register_command("example.infinite-loop", "Infinite Loop Fault")?;
        register_command("example.eat-memory", "Exhaust Linear Memory")?;
        register_command("example.trap", "Trigger Unreachable Trap")?;
        register_command(
            "example.late-register",
            "Attempt Registration Outside Mount",
        )?;
        register_command(
            "example.read-unpermitted-path",
            "Attempt Unpermitted FS Read",
        )?;
        register_command("example.refresh-panel", "Refresh Tools Panel")?;
        register_command("example.refresh-overlay", "Refresh Safe Frame Overlay")?;
        register_command(
            "example.late-register-overlay",
            "Attempt Overlay Registration Outside Mount",
        )?;
        register_command("example.trap-overlay-scene", "Trap Viewer Overlay Scene")?;
        register_command("example.count-job", "Count in a Background Job")?;
        register_command("example.endless-job", "Step Until Cancelled")?;

        register_importer(&ImporterSpec {
            id: "example.markers".to_owned(),
            name: "Marker Lines".to_owned(),
            extensions: vec!["mrk".to_owned()],
        })?;

        register_exporter(&ExporterSpec {
            id: "example.markers".to_owned(),
            name: "Marker Lines".to_owned(),
            extension: "mrk".to_owned(),
        })?;

        register_encoder(&EncoderSpec {
            id: "example.raw-rgba".to_owned(),
            name: "Raw RGBA Stream".to_owned(),
            extension: "rgba".to_owned(),
            audio: true,
        })?;

        register_audio_effect(&AudioEffectSpec {
            id: "example.gain".to_owned(),
            label: "WASM Gain".to_owned(),
            params: vec![AudioParamSpec {
                name: "gain".to_owned(),
                label: "Gain".to_owned(),
                default_value: 1.0,
                min: -4.0,
                max: 4.0,
            }],
            latency: 0,
            sidechain: true,
        })?;
        set_effect_category("example.gain", "Dynamics")?;

        register_panel(&PanelSpec {
            id: "example.tools-panel".to_owned(),
            title: "WASM Tools".to_owned(),
            location: PanelLocation::Sidebar,
        })?;

        register_viewer_overlay(&OverlaySpec {
            id: "example.safe-frame".to_owned(),
            title: "Safe Frame & Focus".to_owned(),
        })?;
        y4m::register()?;
        register_ai_tool(&AiToolSpec {
            name: "example_add_marker".to_owned(),
            description: "Add a marker at a frame of the open sequence.".to_owned(),
            parameters_json: json!({
                "type": "object",
                "properties": {
                    "frame": { "type": "integer", "minimum": 0 },
                    "name": { "type": "string" },
                },
                "required": ["frame"],
            })
            .to_string(),
            tier: AiToolTier::Workspace,
        })?;
        register_ai_tool(&AiToolSpec {
            name: "example_swatch".to_owned(),
            description: "Show the example plugin's 1x1 swatch.".to_owned(),
            parameters_json: json!({ "type": "object", "properties": {} }).to_string(),
            tier: AiToolTier::ReadOnly,
        })?;
        register_scope(&ScopeSpec {
            id: LUMA_SCOPE.to_owned(),
            name: "Luma Histogram".to_owned(),
            width: LUMA_BINS,
            height: LUMA_HEIGHT,
        })?;
        register_driver_language(&DriverLanguageSpec {
            id: "wasm-expr".to_owned(),
            name: "WASM Expr".to_owned(),
        })?;
        register_preset(
            &json!({
                "version": 1,
                "name": "WASM Preset",
                "effects": [{ "effect_id": "vignette" }],
            })
            .to_string(),
        )?;
        register_theme("wasm-dark", "[dark]\naccent = \"#38bdf8\"\n")?;
        register_workspace(&WorkspaceSpec {
            id: "example.wasm-workspace".to_owned(),
            name: "WASM Workspace".to_owned(),
            panes: WorkspacePanes {
                sidebar_width: Some(280.0),
                sidebar_collapsed: Some(false),
                inspector_width: Some(320.0),
                inspector_collapsed: None,
                timeline_height: Some(240.0),
                scopes: Some(true),
                focus: Some(WorkspaceFocus::Timeline),
            },
        })?;
        register_analyzer(&AnalyzerSpec {
            id: "example.wasm-peaks".to_owned(),
            label: "WASM Peak Analyzer".to_owned(),
            category: None,
            scope: AnalyzerScope::Clip,
        })?;
        if setting_get("example.register_parity")?.as_deref() == Some("true") {
            let scalar_param =
                |name: &str, label: &str, default_value: f32, min: f32, max: f32| EffectParamSpec {
                    name: name.to_owned(),
                    label: label.to_owned(),
                    kind: EffectParamKind::Scalar,
                    default_value,
                    min,
                    max,
                    default_color: (0, 0, 0, 0),
                    default_vec2: (0.0, 0.0),
                    display: EffectParamDisplay::Slider,
                    soft_min: None,
                    soft_max: None,
                    unit: None,
                    tooltip: None,
                    group: None,
                    visible_when: None,
                    choices: Vec::new(),
                    default_int: 0,
                    default_bool: false,
                    default_choice: String::new(),
                    default_vec3: (0.0, 0.0, 0.0),
                    default_curve: Vec::new(),
                    default_gradient: Vec::new(),
                    default_text: String::new(),
                };
            register_effect(
                &EffectSpec {
                    id: "example.wgsl-invert".to_owned(),
                    category: "Stylize".to_owned(),
                    label: "WASM WGSL Invert".to_owned(),
                    params: vec![scalar_param("amount", "Amount", 1.0, 0.0, 1.0)],
                    groups: Vec::new(),
                    generator: false,
                    stateful: false,
                },
                &[
                    "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    let c = textureSample(input_tex, aria_sampler, in.uv);\n    return mix(c, vec4<f32>(c.a - c.rgb, c.a), param_amount());\n}\n"
                        .to_owned(),
                ],
            )?;
            register_cpu_effect(&EffectSpec {
                id: "example.cpu-invert".to_owned(),
                category: "Stylize".to_owned(),
                label: "WASM CPU Invert".to_owned(),
                params: vec![scalar_param("amount", "Amount", 1.0, 0.0, 1.0)],
                groups: Vec::new(),
                generator: false,
                stateful: false,
            })?;
            register_generator(
                &EffectSpec {
                    id: "example.wgsl-solid".to_owned(),
                    category: "Generators".to_owned(),
                    label: "WASM Solid Generator".to_owned(),
                    params: Vec::new(),
                    groups: Vec::new(),
                    generator: true,
                    stateful: false,
                },
                &[
                    "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return vec4<f32>(0.25, 0.5, 0.75, 1.0);\n}\n"
                        .to_owned(),
                ],
            )?;
            register_texture("example.lut", 1, 1, &[0, 0, 255, 255])?;
            register_effect_graph(
                &EffectSpec {
                    id: "example.wgsl-graph".to_owned(),
                    category: "Stylize".to_owned(),
                    label: "WASM WGSL Graph".to_owned(),
                    params: Vec::new(),
                    groups: Vec::new(),
                    generator: false,
                    stateful: false,
                },
                EffectTraits {
                    bounds: (1.0, 2.0, 3.0, 4.0),
                    reads_outside: true,
                    time_dependent: true,
                },
                &[
                    GraphPass {
                        name: "half".to_owned(),
                        stage: PassStage::Compute((8, 8)),
                        inputs: Vec::new(),
                        output: OutputSize::Same,
                        wgsl: "@compute @workgroup_size(8, 8, 1)\nfn cs(@builtin(global_invocation_id) id: vec3<u32>) {\n    let size = vec2<u32>(aria.size);\n    if (id.x >= size.x || id.y >= size.y) {\n        return;\n    }\n    let c = textureLoad(input_tex, vec2<i32>(id.xy), 0);\n    textureStore(output_tex, vec2<i32>(id.xy), vec4<f32>(c.rgb * 0.5, c.a));\n}\n"
                            .to_owned(),
                    },
                    GraphPass {
                        name: String::new(),
                        stage: PassStage::Fragment,
                        inputs: vec![
                            PassInput::Pass("half".to_owned()),
                            PassInput::Texture("example.lut".to_owned()),
                        ],
                        output: OutputSize::Same,
                        wgsl: "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    let h = textureSample(pass_half, aria_sampler, in.uv);\n    let t = textureSample(texture_example_lut, aria_sampler, in.uv);\n    return vec4<f32>(h.r, h.g, t.b, 1.0);\n}\n"
                            .to_owned(),
                    },
                ],
            )?;
            register_transition(
                &TransitionSpec {
                    id: "example.split-wipe".to_owned(),
                    label: "WASM Split Wipe".to_owned(),
                    params: vec![scalar_param("feather", "Feather", 0.05, 0.0, 0.5)],
                },
                "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    let a = textureSample(outgoing_tex, aria_sampler, in.uv);\n    let b = textureSample(incoming_tex, aria_sampler, in.uv);\n    let edge = aria.progress * aria.size.x * aria.texel.x + param_feather() * 0.0;\n    return select(b, a, in.uv.x < edge);\n}\n",
            )?;
            register_clip_type(&ClipTypeSpec {
                id: "example.solid-clip".to_owned(),
                label: "WASM Solid Clip".to_owned(),
                generator: "example.wgsl-solid".to_owned(),
                colour: MarkerColor::Teal,
                icon_svg: None,
                handles: Vec::new(),
            })?;
            define_clip_type(
                &ClipTypeSpec {
                    id: "example.meter".to_owned(),
                    label: "WASM Meter".to_owned(),
                    generator: String::new(),
                    colour: MarkerColor::Green,
                    icon_svg: None,
                    handles: Vec::new(),
                },
                &ClipTypeSchema {
                    fields: vec![scalar_param("level", "Level", 0.25, 0.0, 1.0)],
                    groups: Vec::new(),
                    time_dependent: false,
                },
                &ClipTypeRender::Cpu,
            )?;
        }
        subscribe("command-applied")?;
        subscribe("playhead-moved")?;
        subscribe("command-proposed")?;
        subscribe("resolve-dropped")?;

        Ok(())
    }

    fn unmount() -> Result<(), String> {
        log(LogLevel::Info, "unmounting com.example.wasm-tools");
        if let Ok(mut guard) = JOBS.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = ANALYZERS.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = AUDIO_INSTANCES.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = PANEL_STATE.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = OVERLAY_STATE.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = ENCODER_INSTANCES.lock() {
            *guard = None;
        }
        Ok(())
    }

    fn command_run(id: String, args_json: String) -> Result<(), String> {
        match id.as_str() {
            "example.add-marker" => run_add_marker(&args_json),
            "example.inspect-and-undo" => run_inspect_and_undo(),
            "example.query-project" => run_query_project(),
            "example.storage-roundtrip" => run_storage_roundtrip(),
            "example.infinite-loop" => spin_forever(),
            "example.eat-memory" => exhaust_memory(),
            "example.trap" => panic!("explicit guest trap requested"),
            "example.late-register" => {
                register_command("example.illegal", "Illegal Late Registration")
            }
            "example.read-unpermitted-path" => {
                let target = if args_json.trim().is_empty() || args_json.trim() == "{}" {
                    "/etc/hosts".to_owned()
                } else {
                    serde_json::from_str::<String>(&args_json)
                        .unwrap_or_else(|_| args_json.trim().to_owned())
                };
                let _ = fs_read(&target)?;
                Ok(())
            }
            "example.refresh-panel" => {
                let mut guard = PANEL_STATE
                    .lock()
                    .map_err(|_| "panel state lock poisoned".to_owned())?;
                let state = guard.get_or_insert_with(PanelModel::default);
                state.presses += 10;
                drop(guard);
                refresh_panel("example.tools-panel")
            }
            "example.refresh-overlay" => {
                let mut guard = OVERLAY_STATE
                    .lock()
                    .map_err(|_| "overlay state lock poisoned".to_owned())?;
                let state = guard.get_or_insert_with(OverlayModel::default);
                state.refreshes += 1;
                drop(guard);
                refresh_viewer_overlay("example.safe-frame")
            }
            "example.late-register-overlay" => register_viewer_overlay(&OverlaySpec {
                id: "example.illegal-overlay".to_owned(),
                title: "Illegal Late Overlay".to_owned(),
            }),
            "example.trap-overlay-scene" => {
                let mut guard = OVERLAY_STATE
                    .lock()
                    .map_err(|_| "overlay state lock poisoned".to_owned())?;
                let state = guard.get_or_insert_with(OverlayModel::default);
                state.mode = OverlayMode::TrapScene;
                drop(guard);
                refresh_viewer_overlay("example.safe-frame")
            }
            "example.count-job" => start_counting_job(false),
            "example.endless-job" => start_counting_job(true),
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn event(kind: String, event_json: String) -> Result<Option<String>, String> {
        let current = match storage_get("event-count")? {
            Some(bytes) => String::from_utf8_lossy(&bytes)
                .trim()
                .parse::<u64>()
                .unwrap_or(0),
            None => 0,
        };
        let next = current + 1;
        storage_set("event-count", next.to_string().as_bytes())?;
        storage_set(&format!("event-{kind}.json"), event_json.as_bytes())?;

        if kind == "command-proposed" && event_json.contains("veto-by-wasm") {
            return Ok(Some(json!({ "veto": "vetoed by wasm plugin" }).to_string()));
        }
        if (kind == "resolve-dropped" || kind == "resolve-dropped-path")
            && event_json.contains(".mrk")
        {
            return Ok(Some(json!("ImportMedia").to_string()));
        }
        if (kind == "resolve-dropped" || kind == "resolve-dropped-path")
            && event_json.contains("https://")
        {
            return Ok(Some(json!("Handled").to_string()));
        }
        Ok(None)
    }

    fn import(importer_id: String, path: String) -> Result<ImportResult, String> {
        if importer_id != "example.markers" {
            return Err(format!("unknown importer: {importer_id}"));
        }
        let bytes = fs_read(&path)?;
        let text = String::from_utf8(bytes).map_err(|err| format!("invalid UTF-8: {err}"))?;

        let mut commands = Vec::new();
        let mut skipped = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "INFINITE_LOOP" {
                spin_forever()?;
            }
            let mut parts = line.splitn(3, char::is_whitespace);
            let frame_str = parts.next().unwrap_or("");
            let color_str = parts.next().unwrap_or("");
            let name_str = parts.next().unwrap_or("").trim();

            let Ok(frame) = frame_str.parse::<i64>() else {
                skipped.push(line.to_owned());
                continue;
            };
            if !is_valid_marker_color(color_str) || name_str.is_empty() {
                skipped.push(line.to_owned());
                continue;
            }
            commands.push(json!({
                "type": "AddMarker",
                "at": {
                    "value": frame,
                    "rate": { "num": 24, "den": 1 }
                },
                "name": name_str,
                "color": color_str,
            }));
        }

        let commands_json = serde_json::to_string(&commands).map_err(|err| err.to_string())?;
        Ok(ImportResult {
            commands_json,
            skipped,
        })
    }

    fn export(exporter_id: String, project_json: String, path: String) -> Result<Report, String> {
        if exporter_id != "example.markers" {
            return Err(format!("unknown exporter: {exporter_id}"));
        }
        let project: Value = serde_json::from_str(&project_json).map_err(|err| err.to_string())?;
        if project.get("name").and_then(Value::as_str) == Some("INFINITE_LOOP") {
            spin_forever()?;
        }

        let mut out = String::new();
        if let Some(markers) = project
            .get("timeline")
            .and_then(|tl| tl.get("markers"))
            .and_then(Value::as_array)
        {
            for marker in markers {
                let frame = marker
                    .get("tick")
                    .and_then(|t| t.get("value"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let color = marker
                    .get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("teal");
                let name = marker.get("name").and_then(Value::as_str).unwrap_or("");
                out.push_str(&format!("{frame} {color} {name}\n"));
            }
        }

        let mut skipped = Vec::new();
        if let Some(tracks) = project
            .get("timeline")
            .and_then(|tl| tl.get("tracks"))
            .and_then(Value::as_array)
        {
            if !tracks.is_empty() {
                skipped.push(format!("{} tracks omitted by marker export", tracks.len()));
            }
        }

        fs_write(&path, out.as_bytes())?;
        Ok(Report { skipped })
    }

    fn audio_create(effect_id: String, sample_rate: u32) -> Result<Instance, String> {
        if effect_id != "example.gain" {
            return Err(format!("unknown audio effect: {effect_id}"));
        }
        let id = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let mut guard = AUDIO_INSTANCES
            .lock()
            .map_err(|_| "audio instance lock poisoned".to_owned())?;
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            id,
            GainInstance {
                sample_rate,
                frames_processed: 0,
            },
        );
        Ok(id)
    }

    fn audio_process(
        instance: Instance,
        params: Vec<AudioParam>,
        block: AudioBlock,
    ) -> Result<StereoBuffers, String> {
        let mut guard = AUDIO_INSTANCES
            .lock()
            .map_err(|_| "audio instance lock poisoned".to_owned())?;
        let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
            return Err(format!("unknown audio instance: {instance}"));
        };
        let _ = state.sample_rate;

        let frames = block.left.len().min(block.right.len());
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);

        let base_gain = params.first().map_or(1.0, |p| p.constant);
        if base_gain <= -2.0 {
            panic!("the processor faulted");
        }
        if base_gain < 0.0 {
            let mut acc = 1u64;
            for _ in 0..25_000_000u64 {
                acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1);
                std::hint::black_box(acc);
            }
        }

        for i in 0..frames {
            let g = match params.first() {
                Some(param) => param.per_frame.get(i).copied().unwrap_or(param.constant),
                None => 1.0,
            };
            left.push(block.left[i] * g);
            right.push(block.right[i] * g);
        }
        state.frames_processed += frames as u64;

        Ok(StereoBuffers { left, right })
    }

    fn audio_reset(instance: Instance) -> Result<(), String> {
        let mut guard = AUDIO_INSTANCES
            .lock()
            .map_err(|_| "audio instance lock poisoned".to_owned())?;
        let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
            return Err(format!("unknown audio instance: {instance}"));
        };
        state.frames_processed = 0;
        Ok(())
    }

    fn audio_drop(instance: Instance) -> Result<(), String> {
        let mut guard = AUDIO_INSTANCES
            .lock()
            .map_err(|_| "audio instance lock poisoned".to_owned())?;
        if let Some(map) = guard.as_mut() {
            map.remove(&instance);
        }
        Ok(())
    }

    fn panel_view(id: String) -> Vec<UiNode> {
        if id != "example.tools-panel" {
            return Vec::new();
        }
        let mut guard = PANEL_STATE.lock().expect("panel state lock");
        let state = guard.get_or_insert_with(PanelModel::default);
        match state.mode {
            PanelMode::TrapView => panic!("explicit panel-view trap"),
            PanelMode::MalformedList => {
                state.mode = PanelMode::Normal;
                return vec![UiNode::Column(vec![99])];
            }
            PanelMode::Normal => {}
        }

        vec![
            UiNode::Column(vec![1, 2, 3, 4]),
            UiNode::Heading(format!("Tools ({})", state.presses)),
            UiNode::Button(UiButton {
                id: "bump".to_owned(),
                label: format!("Count: {}", state.presses),
                disabled: false,
            }),
            UiNode::Slider(UiSlider {
                id: "level".to_owned(),
                label: "Level".to_owned(),
                value: state.level,
                min: 0.0,
                max: 1.0,
                step: 0.25,
            }),
            UiNode::Input(UiInput {
                id: "note".to_owned(),
                value: state.note.clone(),
                placeholder: "Enter note".to_owned(),
            }),
        ]
    }

    fn panel_event(id: String, event: PanelEvent) -> Result<(), String> {
        if id != "example.tools-panel" {
            return Err(format!("unknown panel: {id}"));
        }
        let mut guard = PANEL_STATE
            .lock()
            .map_err(|_| "panel state lock poisoned".to_owned())?;
        let state = guard.get_or_insert_with(PanelModel::default);
        match event {
            PanelEvent::Pressed(control_id) if control_id == "bump" => {
                state.presses += 1;
                Ok(())
            }
            PanelEvent::Slid(slid) if slid.id == "level" => {
                state.level = slid.value;
                Ok(())
            }
            PanelEvent::Submitted(sub) if sub.id == "note" => match sub.text.as_str() {
                "ERROR" => Err("note rejected by guest".to_owned()),
                "MALFORMED" => {
                    state.mode = PanelMode::MalformedList;
                    Ok(())
                }
                "TRAP_VIEW" => {
                    state.mode = PanelMode::TrapView;
                    Ok(())
                }
                "TRAP_EVENT" => panic!("explicit panel-event trap"),
                _ => {
                    state.note = sub.text;
                    Ok(())
                }
            },
            other => Err(format!("unhandled panel event: {other:?}")),
        }
    }

    fn overlay_scene(id: String) -> Vec<OverlayShape> {
        if id != "example.safe-frame" {
            return Vec::new();
        }
        let guard = OVERLAY_STATE.lock().expect("overlay state lock");
        let state = guard.as_ref().cloned().unwrap_or_default();
        drop(guard);
        if state.mode == OverlayMode::TrapScene {
            panic!("explicit overlay-scene trap");
        }
        let (frame_w, frame_h) = sequence_frame_size();
        let fx = state.focus_x;
        let fy = state.focus_y;
        vec![
            OverlayShape::Rect(OverlayRect {
                origin: Point {
                    x: frame_w * 0.05,
                    y: frame_h * 0.05,
                },
                size: Point {
                    x: frame_w * 0.90,
                    y: frame_h * 0.90,
                },
                color: OverlayColor::Muted,
                width: 1.0,
                fill: None,
            }),
            OverlayShape::Rect(OverlayRect {
                origin: Point {
                    x: frame_w * 0.10,
                    y: frame_h * 0.10,
                },
                size: Point {
                    x: frame_w * 0.80,
                    y: frame_h * 0.80,
                },
                color: OverlayColor::Text,
                width: 1.0,
                fill: None,
            }),
            OverlayShape::Line(OverlayLine {
                from: Point {
                    x: fx - 24.0,
                    y: fy,
                },
                to: Point {
                    x: fx + 24.0,
                    y: fy,
                },
                color: OverlayColor::Accent,
                width: 1.5,
            }),
            OverlayShape::Line(OverlayLine {
                from: Point {
                    x: fx,
                    y: fy - 24.0,
                },
                to: Point {
                    x: fx,
                    y: fy + 24.0,
                },
                color: OverlayColor::Accent,
                width: 1.5,
            }),
            OverlayShape::Text(OverlayText {
                at: Point {
                    x: fx + 10.0,
                    y: fy - 10.0,
                },
                text: format!(
                    "focus ({fx:.0}, {fy:.0}) drags={} refreshes={}",
                    state.drags, state.refreshes
                ),
                color: OverlayColor::Text,
                size: 12.0,
            }),
            OverlayShape::Handle(OverlayHandle {
                id: "focus".to_owned(),
                at: Point { x: fx, y: fy },
                color: OverlayColor::Accent,
                look: HandleLook::Cross,
            }),
        ]
    }

    fn overlay_event(id: String, event: OverlayEvent) -> Result<(), String> {
        if id != "example.safe-frame" {
            return Err(format!("unknown overlay: {id}"));
        }
        let mut guard = OVERLAY_STATE
            .lock()
            .map_err(|_| "overlay state lock poisoned".to_owned())?;
        let state = guard.get_or_insert_with(OverlayModel::default);
        match event {
            OverlayEvent::Drag(drag) if drag.handle == "focus" => {
                state.focus_x = drag.at.x;
                state.focus_y = drag.at.y;
                if drag.phase == DragPhase::End {
                    state.drags += 1;
                }
                Ok(())
            }
            OverlayEvent::Drag(drag) => Err(format!("unknown overlay handle: {}", drag.handle)),
        }
    }

    fn encoder_open(
        encoder_id: String,
        path: String,
        config: EncoderConfig,
    ) -> Result<Instance, String> {
        if encoder_id != "example.raw-rgba" {
            return Err(format!("unknown encoder: {encoder_id}"));
        }
        if path.ends_with("trap_open.rgba") {
            panic!("explicit encoder-open trap");
        }
        if path.ends_with("err_open.rgba") {
            return Err("encoder refused to open".to_owned());
        }
        let id = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let mut guard = ENCODER_INSTANCES
            .lock()
            .map_err(|_| "encoder instance lock poisoned".to_owned())?;
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            id,
            EncoderState {
                path,
                config,
                frames: 0,
                rgba_bytes: 0,
                audio_samples: 0,
            },
        );
        Ok(id)
    }

    fn encoder_push_frame(instance: Instance, frame: EncoderFrame) -> Result<(), String> {
        let mut guard = ENCODER_INSTANCES
            .lock()
            .map_err(|_| "encoder instance lock poisoned".to_owned())?;
        let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
            return Err(format!("unknown encoder instance: {instance}"));
        };
        if state.path.ends_with("trap_push.rgba") {
            panic!("explicit encoder-push trap");
        }
        if state.path.ends_with("unpermitted_fs.rgba") {
            fs_write("/etc/hosts", b"forbidden")?;
        }
        state.frames += 1;
        state.rgba_bytes += frame.rgba.len();
        Ok(())
    }

    fn encoder_push_audio(instance: Instance, audio: EncoderAudio) -> Result<(), String> {
        let mut guard = ENCODER_INSTANCES
            .lock()
            .map_err(|_| "encoder instance lock poisoned".to_owned())?;
        let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
            return Err(format!("unknown encoder instance: {instance}"));
        };
        if state.path.ends_with("trap_audio.rgba") {
            panic!("explicit encoder-audio trap");
        }
        state.audio_samples += audio.samples.len();
        Ok(())
    }

    fn encoder_finish(instance: Instance) -> Result<(), String> {
        let mut guard = ENCODER_INSTANCES
            .lock()
            .map_err(|_| "encoder instance lock poisoned".to_owned())?;
        let Some(state) = guard.as_mut().and_then(|map| map.remove(&instance)) else {
            return Err(format!("unknown encoder instance: {instance}"));
        };
        drop(guard);
        if state.path.ends_with("trap_finish.rgba") {
            panic!("explicit encoder-finish trap");
        }
        let summary = format!(
            "RGBA1 {}x{}@{}/{}:frames={}:bytes={}:samples={}\n",
            state.config.width,
            state.config.height,
            state.config.fps_num,
            state.config.fps_den,
            state.frames,
            state.rgba_bytes,
            state.audio_samples
        );
        fs_write(&state.path, summary.as_bytes())
    }

    fn encoder_drop(instance: Instance) -> Result<(), String> {
        let mut guard = ENCODER_INSTANCES
            .lock()
            .map_err(|_| "encoder instance lock poisoned".to_owned())?;
        if let Some(map) = guard.as_mut() {
            map.remove(&instance);
        }
        Ok(())
    }

    fn job_step(job: u64) -> Result<JobStep, String> {
        let mut guard = JOBS
            .lock()
            .map_err(|_| "job state lock poisoned".to_owned())?;
        let model = guard
            .get_or_insert_with(HashMap::new)
            .get_mut(&job)
            .ok_or_else(|| format!("unknown job {job}"))?;
        model.taken += 1;
        let taken = model.taken;
        if model.endless {
            return Ok(JobStep {
                progress: None,
                detail: format!("step {taken}"),
                done: false,
                commit: false,
            });
        }
        let done = taken >= COUNT_STEPS;
        Ok(JobStep {
            progress: Some(taken as f32 / COUNT_STEPS as f32),
            detail: if done {
                format!("counted to {COUNT_STEPS}")
            } else {
                format!("step {taken}")
            },
            done,
            commit: false,
        })
    }

    fn job_end(job: u64, cancelled: bool) {
        if let Ok(mut guard) = JOBS.lock()
            && let Some(jobs) = guard.as_mut()
        {
            jobs.remove(&job);
        }
        let _ = storage_set("job-end", format!("{job}:{cancelled}").as_bytes());
    }

    fn decoder_probe(decoder_id: String, path: String) -> Result<MediaProbe, DecodeError> {
        match decoder_id.as_str() {
            y4m::DECODER_ID => y4m::probe(&path),
            other => Err(DecodeError::Unsupported(format!(
                "unknown decoder: {other}"
            ))),
        }
    }

    fn decoder_open_video(decoder_id: String, path: String) -> Result<OpenedVideo, DecodeError> {
        match decoder_id.as_str() {
            y4m::DECODER_ID => y4m::open(&path),
            other => Err(DecodeError::Unsupported(format!(
                "unknown decoder: {other}"
            ))),
        }
    }

    fn decoder_seek(instance: Instance, pts: i64) -> Result<(), String> {
        y4m::seek(instance, pts)
    }

    fn decoder_next_frame(instance: Instance) -> Result<Option<DecodedFrame>, String> {
        y4m::next_frame(instance)
    }

    fn decoder_open_audio(
        _decoder_id: String,
        path: String,
        _sample_rate: u32,
        _channels: u16,
    ) -> Result<Instance, DecodeError> {
        Err(DecodeError::Unsupported(format!("{path} has no audio")))
    }

    fn decoder_read_audio(instance: Instance, _max_samples: u32) -> Result<Vec<f32>, String> {
        Err(format!("unknown audio stream: {instance}"))
    }

    fn decoder_seek_audio(instance: Instance, _frame: i64) -> Result<(), String> {
        Err(format!("unknown audio stream: {instance}"))
    }

    fn decoder_close(instance: Instance) {
        y4m::close(instance);
    }

    fn ai_tool_call(name: String, arguments_json: String) -> Result<AiToolOutput, String> {
        match name.as_str() {
            "example_add_marker" => {
                let arguments: Value =
                    serde_json::from_str(&arguments_json).map_err(|err| err.to_string())?;
                let frame = arguments
                    .get("frame")
                    .and_then(Value::as_i64)
                    .ok_or("frame must be a whole number")?;
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_MARKER_NAME);
                run_add_marker(&json!({ "frame": frame, "name": name }).to_string())?;
                Ok(AiToolOutput {
                    text: format!("added marker {name} at frame {frame}"),
                    images: Vec::new(),
                })
            }
            "example_swatch" => Ok(AiToolOutput {
                text: "the example swatch".to_owned(),
                images: vec![AiToolImage {
                    label: "swatch".to_owned(),
                    png: SWATCH_PNG.to_vec(),
                }],
            }),
            other => Err(format!("no AI tool {other}")),
        }
    }

    fn scope_measure(id: String, frame: ScopeFrame) -> Result<ScopeDrawing, String> {
        if id != LUMA_SCOPE {
            return Err(format!("no scope {id}"));
        }
        Ok(luma_histogram(&frame))
    }

    fn cpu_effect_process(
        id: String,
        params: Vec<EffectParam>,
        mut frame: VideoFrame,
    ) -> Result<VideoFrame, String> {
        if id == "example.meter" {
            let level = f32_to_f16(params.first().copied().unwrap_or(0.0));
            for px in frame.pixels.chunks_exact_mut(4) {
                px.copy_from_slice(&[level, level, level, f32_to_f16(1.0)]);
            }
            return Ok(frame);
        }
        if id != "example.cpu-invert" {
            return Err(format!("unknown CPU effect {id}"));
        }
        let amount = params.first().copied().unwrap_or(1.0);
        if amount < 0.0 {
            panic!("explicit CPU effect trap");
        }
        if amount > 1.0 {
            return Err("cpu effect guest error".to_owned());
        }
        if amount == 0.0 {
            return Ok(frame);
        }
        for px in frame.pixels.chunks_exact_mut(4) {
            for ch in &mut px[..3] {
                let v = f16_to_f32(*ch);
                *ch = f32_to_f16(v + (1.0 - 2.0 * v) * amount);
            }
        }
        Ok(frame)
    }

    fn driver_evaluate(id: String, expr: String, vars: DriverVars) -> Result<ParamValue, String> {
        if id != "wasm-expr" {
            return Err(format!("unknown driver language {id}"));
        }
        match expr.trim() {
            "double_time" => Ok(ParamValue::Scalar((vars.time * 2.0) as f32)),
            "plus_ten" => match vars.value {
                ParamValue::Scalar(v) => Ok(ParamValue::Scalar(v + 10.0)),
                other => Ok(other),
            },
            "canvas_vec" => Ok(ParamValue::Vec2((
                vars.canvas_width as f32,
                vars.canvas_height as f32,
            ))),
            "trap" => panic!("explicit driver trap"),
            other => Err(format!("unknown wasm-expr: {other}")),
        }
    }

    fn analyzer_open(id: String, _sample_rate: u32, channels: u16) -> Result<Instance, String> {
        if id != "example.wasm-peaks" {
            return Err(format!("unknown analyzer {id}"));
        }
        let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        ANALYZERS
            .lock()
            .map_err(|_| "analyzer lock poisoned".to_owned())?
            .get_or_insert_with(HashMap::new)
            .insert(
                instance,
                AnalyzerInstance {
                    channels,
                    frames: 0,
                    peaks: Vec::new(),
                },
            );
        Ok(instance)
    }

    fn analyzer_push(instance: Instance, samples: Vec<f32>) -> Result<(), String> {
        let mut guard = ANALYZERS
            .lock()
            .map_err(|_| "analyzer lock poisoned".to_owned())?;
        let state = guard
            .as_mut()
            .and_then(|m| m.get_mut(&instance))
            .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
        let ch = usize::from(state.channels.max(1));
        for frame in samples.chunks_exact(ch) {
            if frame[0].is_nan() {
                panic!("explicit analyzer trap");
            }
            if frame[0].abs() >= 0.5 {
                state.peaks.push(state.frames);
            }
            state.frames += 1;
        }
        Ok(())
    }

    fn analyzer_finish(instance: Instance) -> Result<String, String> {
        let mut guard = ANALYZERS
            .lock()
            .map_err(|_| "analyzer lock poisoned".to_owned())?;
        let state = guard
            .as_mut()
            .and_then(|m| m.remove(&instance))
            .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
        Ok(json!({
            "beats": state.peaks,
            "markers": [{
                "frame": state.frames,
                "duration_frames": 0,
                "name": "WasmEnd",
                "color": "teal"
            }]
        })
        .to_string())
    }

    fn analyzer_drop(instance: Instance) {
        if let Ok(mut guard) = ANALYZERS.lock()
            && let Some(map) = guard.as_mut()
        {
            map.remove(&instance);
        }
    }
}

const fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x03ff) as u32;
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let shift = mant.leading_zeros() - 21;
        let mant = (mant << shift) & 0x03ff;
        let exp = (127 - 15 + 1) - shift;
        f32::from_bits(sign | (exp << 23) | (mant << 13))
    } else if exp == 0x1f {
        f32::from_bits(sign | 0x7f80_0000 | (mant << 13))
    } else {
        f32::from_bits(sign | ((exp + (127 - 15)) << 23) | (mant << 13))
    }
}

const fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return if mant == 0 {
            sign | 0x7c00
        } else {
            sign | 0x7e00 | ((mant >> 13) as u16)
        };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 0x1f {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let full_mant = mant | 0x0080_0000;
        let shift = (14 - half_exp) as u32;
        let round_bit = 1u32 << (shift - 1);
        let mask = round_bit - 1;
        let mut half_mant = (full_mant >> shift) as u16;
        if (full_mant & round_bit) != 0 && ((full_mant & mask) != 0 || (half_mant & 1) != 0) {
            half_mant += 1;
        }
        return sign | half_mant;
    }
    let mut half_mant = (mant >> 13) as u16;
    if (mant & 0x1000) != 0 && ((mant & 0x0fff) != 0 || (half_mant & 1) != 0) {
        half_mant += 1;
    }
    sign | (((half_exp as u16) << 10) + half_mant)
}

/// Bin every pixel's Rec. 709 luma into [`LUMA_BINS`] columns and draw each
/// column's count as a bar, scaled so the fullest bin reaches the top.
fn luma_histogram(frame: &ScopeFrame) -> ScopeDrawing {
    let bins = LUMA_BINS as usize;
    let mut counts = vec![0_u32; bins];
    let mut total = 0_u64;
    for px in frame.pixels.chunks_exact(4) {
        let luma =
            (2126 * u32::from(px[0]) + 7152 * u32::from(px[1]) + 722 * u32::from(px[2])) / 10_000;
        total += u64::from(luma);
        counts[luma as usize * bins / 256] += 1;
    }
    let height = LUMA_HEIGHT as usize;
    let mut pixels = vec![0_u8; bins * height * 4];
    let fullest = counts.iter().copied().max().unwrap_or(0).max(1);
    for (column, count) in counts.iter().enumerate() {
        let rows = (*count as usize * height).div_ceil(fullest as usize);
        for row in height - rows..height {
            let at = (row * bins + column) * 4;
            pixels[at..at + 4].copy_from_slice(&LUMA_BAR);
        }
    }
    let samples = (frame.pixels.len() / 4).max(1) as u64;
    ScopeDrawing {
        pixels,
        readout: format!("mean luma {}", total / samples),
    }
}

fn run_add_marker(args_json: &str) -> Result<(), String> {
    let configured_color = match setting_get("example.marker_color")? {
        Some(raw) => serde_json::from_str::<String>(&raw).unwrap_or(raw),
        None => "teal".to_owned(),
    };
    let color = if is_valid_marker_color(&configured_color) {
        configured_color
    } else {
        "teal".to_owned()
    };

    let parsed_args: Value = if args_json.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(args_json).unwrap_or(Value::Null)
    };

    let frame = parsed_args
        .get("frame")
        .and_then(Value::as_i64)
        .unwrap_or(24);
    let name = parsed_args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MARKER_NAME);

    let commands = json!([{
        "type": "AddMarker",
        "at": {
            "value": frame,
            "rate": { "num": 24, "den": 1 }
        },
        "name": name,
        "color": color,
    }]);
    let commands_json = serde_json::to_string(&commands).map_err(|err| err.to_string())?;
    let receipt = project_transact("Add WASM Marker", &commands_json)?;
    storage_set("last-receipt.json", receipt.as_bytes())?;
    Ok(())
}

fn run_inspect_and_undo() -> Result<(), String> {
    let snapshot_json = project_snapshot()?;
    let snapshot: Value = serde_json::from_str(&snapshot_json).map_err(|err| err.to_string())?;
    let view_json = editor_view()?;
    let view: Value = serde_json::from_str(&view_json).map_err(|err| err.to_string())?;
    let undid = project_undo()?;

    let summary = json!({
        "project_name": snapshot.get("name").and_then(Value::as_str).unwrap_or(""),
        "playhead": view.get("playhead"),
        "undid": undid,
    });
    storage_set("last-inspection.json", summary.to_string().as_bytes())?;
    Ok(())
}

fn run_query_project() -> Result<(), String> {
    let t0 = RationalTime {
        value: 0,
        rate_num: 24,
        rate_den: 1,
    };
    let t48 = RationalTime {
        value: 48,
        rate_num: 24,
        rate_den: 1,
    };
    let range = TimeRange {
        start: t0,
        duration: t48,
    };
    let in_range = clips_in_range(None, range)?;
    let first_by_id = if let Some(first) = in_range.first() {
        clip(first.id)?
    } else {
        None
    };
    let missing = clip(999_999)?;
    let at_12 = clips_at(
        None,
        RationalTime {
            value: 12,
            rate_num: 24,
            rate_den: 1,
        },
    )?;
    let at_48 = clips_at(None, t48)?;
    let selected = selected_clips()?;
    let markers = markers_in_range(None, range)?;
    let io = in_out()?;
    let summary = json!({
        "in_range_ids": in_range.iter().map(|c| c.id).collect::<Vec<_>>(),
        "first_by_id": first_by_id.as_ref().map(|c| json!({
            "id": c.id,
            "track": c.track,
            "start": c.start.value,
            "end": c.end.value,
            "kind": c.kind,
            "name": c.name,
        })),
        "missing_is_none": missing.is_none(),
        "at_12_ids": at_12.iter().map(|c| c.id).collect::<Vec<_>>(),
        "at_48_ids": at_48.iter().map(|c| c.id).collect::<Vec<_>>(),
        "selected_ids": selected.iter().map(|c| c.id).collect::<Vec<_>>(),
        "marker_names": markers.iter().map(|m| &m.name).collect::<Vec<_>>(),
        "mark_in": io.mark_in.map(|t| t.value),
        "mark_out": io.mark_out.map(|t| t.value),
    });
    storage_set("last-queries.json", summary.to_string().as_bytes())?;
    Ok(())
}

fn run_storage_roundtrip() -> Result<(), String> {
    storage_set("alpha", b"one")?;
    storage_set("beta", b"two")?;
    let alpha = storage_get("alpha")?.ok_or_else(|| "missing key alpha".to_owned())?;
    if alpha != b"one" {
        return Err(format!("unexpected alpha value: {alpha:?}"));
    }
    let keys = storage_keys()?;
    storage_delete("alpha")?;
    let deleted = storage_get("alpha")?;
    if deleted.is_some() {
        return Err("alpha still present after storage_delete".to_owned());
    }
    storage_set(
        "storage-check.json",
        json!({ "keys_before_delete": keys }).to_string().as_bytes(),
    )?;
    Ok(())
}

fn spin_forever() -> Result<(), String> {
    let mut acc = 1u64;
    loop {
        acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1);
        std::hint::black_box(acc);
    }
}

fn exhaust_memory() -> Result<(), String> {
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for i in 0..64u8 {
        let block = vec![i ^ 0xA5; 1024 * 1024];
        std::hint::black_box(&block);
        chunks.push(block);
    }
    std::hint::black_box(&chunks);
    Ok(())
}

fn is_valid_marker_color(color: &str) -> bool {
    matches!(
        color,
        "teal" | "blue" | "purple" | "pink" | "red" | "orange" | "yellow" | "green"
    )
}

fn sequence_frame_size() -> (f32, f32) {
    let Ok(snapshot_json) = project_snapshot() else {
        return (1920.0, 1080.0);
    };
    let Ok(doc) = serde_json::from_str::<Value>(&snapshot_json) else {
        return (1920.0, 1080.0);
    };
    if let Some(aspect) = doc
        .get("timeline")
        .and_then(|tl| tl.get("canvas"))
        .and_then(|c| c.get("aspect"))
        .and_then(Value::as_str)
    {
        match aspect {
            "16:9" => return (1920.0, 1080.0),
            "9:16" => return (1080.0, 1920.0),
            "1:1" => return (1080.0, 1080.0),
            "4:5" => return (1080.0, 1350.0),
            "21:9" => return (2520.0, 1080.0),
            _ => {}
        }
    }
    if let Some(media_entries) = doc.get("media").and_then(Value::as_array) {
        let mut best: Option<(u32, u32)> = None;
        for entry in media_entries {
            let item = entry.get(1).unwrap_or(entry);
            let width = item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            let height = item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            if width > 0 && height > 0 {
                let area = u64::from(width) * u64::from(height);
                if best.is_none_or(|(bw, bh)| area > u64::from(bw) * u64::from(bh)) {
                    best = Some((width, height));
                }
            }
        }
        if let Some((w, h)) = best {
            return (w as f32, h as f32);
        }
    }
    (1920.0, 1080.0)
}

impl Clone for OverlayModel {
    fn clone(&self) -> Self {
        Self {
            focus_x: self.focus_x,
            focus_y: self.focus_y,
            drags: self.drags,
            refreshes: self.refreshes,
            mode: self.mode,
        }
    }
}

export!(ExampleWasmPlugin);
