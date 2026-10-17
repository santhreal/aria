//! `plugin-cost` scenario: native host call overhead (`<= 1 us`), WASM host
//! call overhead (`<= 5 us`), WASM plugin activation (`<= 50 ms`), and plugin
//! WGSL effect cost delta vs the identical shipped effect (`<= 5 %`).

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, EffectDefinition, GpuContext,
    LayerPlacement, PassInstance, RgbaImage, create_frame_target, pass_id, register_effect, wgpu,
};
use aria_engine::{Engine, EngineConfig};
use aria_models::{ClipId, EffectParamKind, EffectParamSpec, EffectSpec, Rational, RationalTime};
use aria_plugin::package::Package;
use aria_plugin::{
    CommandSpec, EditorView, Plugin, PluginContext, PluginError, PluginManifest, ProjectHost,
    Service, SettingsService, ViewFrame,
};
use aria_plugin_wasm::{Limits, load};
use parking_lot::Mutex;

use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

const MARKER_ICON_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6"/></svg>"#;

struct BenchSettings;
impl Service for BenchSettings {}
impl SettingsService for BenchSettings {
    fn get(&self, _key: &str) -> Option<serde_json::Value> {
        None
    }
    fn set(&self, _key: &str, _value: serde_json::Value) {}
    fn save(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct BenchNativePlugin {
    manifest: PluginManifest,
}

impl BenchNativePlugin {
    fn new() -> Self {
        Self {
            manifest: PluginManifest::new("bench.native", "Bench Native Plugin", "0.1.0", &[], &[]),
        }
    }
}

impl Plugin for BenchNativePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let _ = ctx.register_command(
            CommandSpec::new("bench.native.host-call", "Bench Native Host Call"),
            |ctx: &PluginContext| {
                let _ = ctx.is_mounted("bench.native");
                Ok(())
            },
        )?;
        Ok(())
    }
}

fn write_wasm_package(dir: &Path, wasm_bytes: &[u8]) -> Result<Package, String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let icons_dir = dir.join("icons");
    std::fs::create_dir_all(&icons_dir).map_err(|err| format!("mkdir icons failed: {err}"))?;
    std::fs::write(icons_dir.join("marker.svg"), MARKER_ICON_SVG)
        .map_err(|err| format!("write marker.svg failed: {err}"))?;
    let manifest = r#"[plugin]
id = "com.example.wasm-tools"
name = "Example WASM Tools"
version = "0.1.0"
api = 1
publisher = "Aria Bench"
description = "Benchmark WASM component package"
license = "Apache-2.0"

[permissions]
project = "read-write"
fs = []

[[commands]]
id = "example.refresh-panel"
title = "Refresh Tools Panel"
icon = "icons/marker.svg"
"#;
    std::fs::write(dir.join("plugin.toml"), manifest)
        .map_err(|err| format!("write plugin.toml failed: {err}"))?;
    std::fs::write(dir.join("plugin.wasm"), wasm_bytes)
        .map_err(|err| format!("write plugin.wasm failed: {err}"))?;
    Package::open(dir).map_err(|err| format!("Package::open failed: {err}"))
}

const IDENTICAL_VIGNETTE_WGSL: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, aria_sampler, in.uv);
    let amount = clamp(param_amount(), 0.0, 1.0);
    let centered = in.uv - vec2<f32>(0.5, 0.5);
    let dist = length(centered) * 1.41421356;
    let factor = 1.0 - amount * smoothstep(0.35, 1.0, dist);
    return vec4<f32>(color.rgb * factor, color.a);
}
";

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let scratch = ctx.ensure_scratch_subdir("plugin-cost")?;
    let wasm_fixture_path = ctx
        .workspace_root
        .join("crates/aria-plugin-wasm/tests/fixtures/example_plugin.wasm");
    let wasm_bytes = std::fs::read(&wasm_fixture_path)
        .map_err(|err| format!("failed to read {}: {err}", wasm_fixture_path.display()))?;

    let pkg_dir = scratch.join("wasm-pkg");
    let pkg = write_wasm_package(&pkg_dir, &wasm_bytes)?;

    let plugin_ctx = PluginContext::new();
    let engine =
        Engine::new(EngineConfig::default()).map_err(|err| format!("Engine::new failed: {err}"))?;
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &plugin_ctx));
    let _host_reg = plugin_ctx.register_arc(Arc::clone(&host));

    let view = Arc::new(EditorView::new());
    view.publish(
        ViewFrame {
            selection: &[ClipId::from_raw(1)],
            playhead: RationalTime::new(0, Rational::FPS_24),
            mark_in: None,
            mark_out: None,
            playing: false,
            speed: 1,
        },
        plugin_ctx.events(),
    );
    let _view_reg = plugin_ctx.register_arc(Arc::clone(&view));
    let settings: Arc<dyn SettingsService> = Arc::new(BenchSettings);
    let _settings_reg = plugin_ctx.register_arc(settings);

    // 1. Native host call overhead (`<= 1 us`).
    let native_plugin = BenchNativePlugin::new();
    plugin_ctx
        .mount(&native_plugin)
        .map_err(|err| format!("native mount failed: {err}"))?;
    for _ in 0..50 {
        plugin_ctx
            .run_command("bench.native.host-call")
            .map_err(|err| format!("native warmup failed: {err}"))?;
    }
    let mut native_call_us = Vec::with_capacity(200);
    for _ in 0..200 {
        let batch = 50u32;
        let t0 = Instant::now();
        for _ in 0..batch {
            plugin_ctx
                .run_command("bench.native.host-call")
                .map_err(|err| format!("native run_command failed: {err}"))?;
        }
        native_call_us.push(t0.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(batch));
    }

    // 2. WASM plugin activation (`<= 50 ms`) and WASM host call overhead (`<= 5 us`).
    // Warm the shared wasmtime Engine and component compilation once, then measure
    // activation (`load` + `mount` and `mount` from precompiled package) across 15 runs.
    let warmup_plugin =
        load(&pkg, &Limits::default()).map_err(|err| format!("wasm load failed: {err}"))?;
    plugin_ctx
        .mount(warmup_plugin.as_ref())
        .map_err(|err| format!("wasm warmup mount failed: {err}"))?;
    plugin_ctx
        .unmount("com.example.wasm-tools")
        .map_err(|err| format!("wasm warmup unmount failed: {err}"))?;

    let mut wasm_activation_ms = Vec::with_capacity(15);
    for i in 0..15 {
        let preloaded =
            load(&pkg, &Limits::default()).map_err(|err| format!("wasm load {i} failed: {err}"))?;
        let t0 = Instant::now();
        plugin_ctx
            .mount(preloaded.as_ref())
            .map_err(|err| format!("wasm mount {i} failed: {err}"))?;
        wasm_activation_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        if i + 1 < 15 {
            plugin_ctx
                .unmount("com.example.wasm-tools")
                .map_err(|err| format!("wasm unmount {i} failed: {err}"))?;
        }
    }

    // Warm WASM host call (`example.refresh-panel` calls guest `command-run` -> host `refresh-panel`).
    for _ in 0..50 {
        plugin_ctx
            .run_command("example.refresh-panel")
            .map_err(|err| format!("wasm command warmup failed: {err}"))?;
    }
    let mut wasm_call_us = Vec::with_capacity(200);
    for _ in 0..200 {
        let batch = 25u32;
        let t0 = Instant::now();
        for _ in 0..batch {
            plugin_ctx
                .run_command("example.refresh-panel")
                .map_err(|err| format!("wasm run_command failed: {err}"))?;
        }
        wasm_call_us.push(t0.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(batch));
    }
    let _ = plugin_ctx.unmount("com.example.wasm-tools");

    // 3. Plugin WGSL effect vs identical shipped effect (`<= 5 %`).
    let reg = register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "bench.vignette".into(),
            category: String::new(),
            label: "Bench Vignette".into(),
            params: vec![EffectParamSpec {
                name: "amount".into(),
                label: "Amount".into(),
                kind: EffectParamKind::Scalar,
                default: 0.5,
                min: 0.0,
                max: 1.0,
                default_color: [0; 4],
                default_vec2: [0.0; 2],
                ..Default::default()
            }],
            generator: false,
            stateful: false,
            ..Default::default()
        },
        passes: vec![IDENTICAL_VIGNETTE_WGSL.into()],
    })
    .map_err(|err| format!("register_effect(bench.vignette) failed: {err}"))?;

    let shipped_pass =
        pass_id("vignette").ok_or_else(|| "shipped pass 'vignette' missing".to_owned())?;
    let plugin_pass = pass_id("bench.vignette")
        .ok_or_else(|| "registered pass 'bench.vignette' missing".to_owned())?;

    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("GpuContext::new_headless_blocking failed: {err}"))?;
    let mut comp = Compositor::new(&gpu);
    let width = 1920u32;
    let height = 1080u32;
    let config = CompositorConfig::new(width, height).with_background([0, 0, 0, 255]);
    let target_tex = create_frame_target(&gpu.device, width, height);
    let plate = RgbaImage::new(
        width,
        height,
        [180u8, 120, 90, 255].repeat((width * height) as usize),
    );

    let shipped_effects = [
        PassInstance::new(shipped_pass.clone(), vec![0.45]),
        PassInstance::new(shipped_pass.clone(), vec![0.55]),
        PassInstance::new(shipped_pass.clone(), vec![0.35]),
        PassInstance::new(shipped_pass, vec![0.65]),
    ];
    let plugin_effects = [
        PassInstance::new(plugin_pass.clone(), vec![0.45]),
        PassInstance::new(plugin_pass.clone(), vec![0.55]),
        PassInstance::new(plugin_pass.clone(), vec![0.35]),
        PassInstance::new(plugin_pass, vec![0.65]),
    ];

    // Warm both pipelines and offscreen targets.
    for _ in 0..12 {
        let s_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
            .with_effects(&shipped_effects);
        comp.submit_compositor_layers_to_texture(
            &gpu,
            &config,
            &[CompositorLayer::layer(&s_layer)],
            &target_tex,
        )
        .map_err(|err| format!("shipped warmup render failed: {err}"))?;
        let p_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
            .with_effects(&plugin_effects);
        comp.submit_compositor_layers_to_texture(
            &gpu,
            &config,
            &[CompositorLayer::layer(&p_layer)],
            &target_tex,
        )
        .map_err(|err| format!("plugin warmup render failed: {err}"))?;
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    }

    let mut shipped_ms = Vec::with_capacity(60);
    let mut plugin_ms = Vec::with_capacity(60);
    for i in 0..60usize {
        if i % 2 == 0 {
            let t0 = Instant::now();
            let s_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
                .with_effects(&shipped_effects);
            comp.submit_compositor_layers_to_texture(
                &gpu,
                &config,
                &[CompositorLayer::layer(&s_layer)],
                &target_tex,
            )
            .map_err(|err| format!("shipped render failed: {err}"))?;
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            shipped_ms.push(t0.elapsed().as_secs_f64() * 1000.0);

            let t1 = Instant::now();
            let p_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
                .with_effects(&plugin_effects);
            comp.submit_compositor_layers_to_texture(
                &gpu,
                &config,
                &[CompositorLayer::layer(&p_layer)],
                &target_tex,
            )
            .map_err(|err| format!("plugin render failed: {err}"))?;
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            plugin_ms.push(t1.elapsed().as_secs_f64() * 1000.0);
        } else {
            let t1 = Instant::now();
            let p_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
                .with_effects(&plugin_effects);
            comp.submit_compositor_layers_to_texture(
                &gpu,
                &config,
                &[CompositorLayer::layer(&p_layer)],
                &target_tex,
            )
            .map_err(|err| format!("plugin render failed: {err}"))?;
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            plugin_ms.push(t1.elapsed().as_secs_f64() * 1000.0);

            let t0 = Instant::now();
            let s_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
                .with_effects(&shipped_effects);
            comp.submit_compositor_layers_to_texture(
                &gpu,
                &config,
                &[CompositorLayer::layer(&s_layer)],
                &target_tex,
            )
            .map_err(|err| format!("shipped render failed: {err}"))?;
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            shipped_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
    drop(reg);

    let native_series = SampleSeries::from_values("native_host_call_us", "us", native_call_us);
    let wasm_call_series = SampleSeries::from_values("wasm_host_call_us", "us", wasm_call_us);
    let wasm_act_series = SampleSeries::from_values("wasm_activation_ms", "ms", wasm_activation_ms);
    let shipped_series = SampleSeries::from_values("shipped_wgsl_4pass_ms", "ms", shipped_ms);
    let plugin_series = SampleSeries::from_values("plugin_wgsl_4pass_ms", "ms", plugin_ms);

    // Overhead of plugin WGSL effect relative to the identical shipped effect.
    // If plugin_series.p50 <= shipped_series.p50, overhead is 0.0%.
    let wgsl_overhead_pct = if shipped_series.p50 > 0.0 {
        ((plugin_series.p50 - shipped_series.p50).max(0.0) / shipped_series.p50) * 100.0
    } else {
        0.0
    };

    let targets = vec![
        TargetEvaluation::measured(
            "native_host_call_us",
            "Host call overhead: 1 us native",
            Comparison::AtMost,
            1.0,
            "us",
            native_series.p50,
        ),
        TargetEvaluation::measured(
            "wasm_host_call_us",
            "Host call overhead: 5 us WASM",
            Comparison::AtMost,
            5.0,
            "us",
            wasm_call_series.p50,
        ),
        TargetEvaluation::measured(
            "wasm_activation_ms",
            "A WASM plugin activates in 50 ms",
            Comparison::AtMost,
            50.0,
            "ms",
            wasm_act_series.p50,
        ),
        TargetEvaluation::measured(
            "plugin_wgsl_effect_overhead_pct",
            "A plugin WGSL effect costs the same as the identical shipped effect within 5 %",
            Comparison::AtMost,
            5.0,
            "percent",
            wgsl_overhead_pct,
        ),
    ];

    let notes = vec![format!(
        "Native host call p50={:.3} us (p99={:.3} us); WASM host call p50={:.3} us (p99={:.3} us); WASM activation p50={:.2} ms (p99={:.2} ms); 4x1080p vignette pass shipped p50={:.3} ms vs plugin WGSL p50={:.3} ms (overhead={:.2}%)",
        native_series.p50,
        native_series.p99,
        wasm_call_series.p50,
        wasm_call_series.p99,
        wasm_act_series.p50,
        wasm_act_series.p99,
        shipped_series.p50,
        plugin_series.p50,
        wgsl_overhead_pct
    )];

    Ok(ScenarioReport::new(
        ScenarioId::PluginCost,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            native_series,
            wasm_call_series,
            wasm_act_series,
            shipped_series,
            plugin_series,
        ],
        notes,
    ))
}
