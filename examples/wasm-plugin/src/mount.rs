//! Guest `mount` and `unmount` lifecycle entrypoints.

use serde_json::json;

use crate::audio::{ANALYZERS, AUDIO_INSTANCES};
use crate::encoder::ENCODER_INSTANCES;
use crate::jobs::JOBS;
use crate::panel::{OVERLAY_STATE, OverlayModel, PANEL_STATE, PanelModel};
use crate::tools::{LUMA_BINS, LUMA_HEIGHT, LUMA_SCOPE};
use crate::{
    AiToolSpec, AiToolTier, AnalyzerScope, AnalyzerSpec, AudioEffectSpec, AudioParamSpec,
    ClipTypeRender, ClipTypeSchema, ClipTypeSpec, DriverLanguageSpec, EffectParamDisplay,
    EffectParamKind, EffectParamSpec, EffectSpec, EffectTraits, EncoderSpec, ExporterSpec,
    GraphPass, ImporterSpec, LogLevel, MarkerColor, OutputSize, OverlaySpec, PanelLocation,
    PanelSpec, PassInput, PassStage, ScopeSpec, TransitionSpec, WorkspaceFocus, WorkspacePanes,
    WorkspaceSpec, define_clip_type, log, register_ai_tool, register_analyzer,
    register_audio_effect, register_clip_type, register_command, register_cpu_effect,
    register_driver_language, register_effect, register_effect_graph, register_encoder,
    register_exporter, register_generator, register_importer, register_panel, register_preset,
    register_scope, register_texture, register_theme, register_transition, register_viewer_overlay,
    register_workspace, set_effect_category, setting_get, subscribe, y4m,
};

pub(crate) fn mount() -> Result<(), String> {
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

pub(crate) fn unmount() -> Result<(), String> {
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
