//! Native C ABI parity contributions (`register_effect`, `register_cpu_effect`,
//! `register_transition`, `register_generator`, `register_clip_type`,
//! `register_driver_language`, `register_preset`, `register_theme`,
//! `register_workspace`).
//!
//! Defect class closed: C-ABI and `aria-sdk` plugins lacking parity with
//! in-process `PluginContext` registrations, panics in `cpu_effect_process` or
//! `driver_evaluate` unwinding across `extern "C"`, and unmount leaving
//! registered effects, transitions, clip types, drivers, presets, themes, or
//! workspaces behind.

#![allow(unsafe_code)]

use std::sync::LazyLock;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, CpuFrameIn as HostCpuFrameIn,
    CpuFrameOut as HostCpuFrameOut, F16_ONE, GpuContext, LayerPlacement, PassInstance, RgbaImage,
    f16_to_f32, f32_to_f16, pass_id, registered_effect, renders_effect, renders_transition,
};
use aria_models::{
    ClipParam, DriverCode, ParamValue, clip_type_spec, effect_spec, transition_spec,
};
use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin::{DriverClipInfo, DriverScope, InputClip, PluginContext};
use aria_plugin_abi::{ARIA_API_MAJOR, ARIA_PLUGIN_V1_SCOPES_SIZE};
use aria_render::compile_driver;
use aria_sdk::{
    Analyzer as SdkAnalyzer, AnalyzerMarker, AnalyzerResult, AnalyzerRun, AnalyzerScope,
    AnalyzerSpec, ClipTypeSpec, CpuEffect, CpuFrameIn, CpuFrameOut, DriverLanguage,
    DriverLanguageSpec, DriverVars, EffectParamSpec, EffectSpec, Host, MarkerColor,
    ParamValue as SdkParamValue, Plugin as SdkPlugin, PluginRuntime, PresetSpec, TransitionSpec,
    WorkspaceFocus, WorkspacePanes, WorkspaceSpec,
};
use parking_lot::Mutex;
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

static SERIAL: Mutex<()> = Mutex::new(());

const W: u32 = 32;
const H: u32 = 16;

const WGSL_INVERT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, aria_sampler, in.uv);
    return mix(c, vec4<f32>(c.a - c.rgb, c.a), param_amount());
}
";

const WGSL_SOLID_GEN: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.25, 0.5, 0.75, 1.0);
}
";

const WGSL_SPLIT_TRANSITION: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    let edge = aria.progress * aria.size.x * aria.texel.x;
    return select(b, a, in.uv.x < edge);
}
";

struct InvertCpuEffect;

impl CpuEffect for InvertCpuEffect {
    fn process(
        &self,
        _host: &Host,
        frame_in: &CpuFrameIn<'_>,
        frame_out: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        let amount = frame_in.param_at(0).unwrap_or(1.0);
        if amount < 0.0 {
            panic!("native cpu boom");
        }
        if amount > 1.0 {
            return Err("native cpu range error".to_owned());
        }
        if amount == 0.0 {
            frame_out.pixels_mut().copy_from_slice(frame_in.pixels());
            return Ok(());
        }
        for y in 0..frame_in.height() {
            for x in 0..frame_in.width() {
                let [r, g, b, a] = frame_in.pixel_f32(x, y);
                frame_out.set_pixel_f32(
                    x,
                    y,
                    [
                        r + (1.0 - 2.0 * r) * amount,
                        g + (1.0 - 2.0 * g) * amount,
                        b + (1.0 - 2.0 * b) * amount,
                        a,
                    ],
                );
            }
        }
        Ok(())
    }
}

struct LinearDriver;

impl DriverLanguage for LinearDriver {
    fn evaluate(
        &self,
        _host: &Host,
        expr: &str,
        vars: &DriverVars,
    ) -> Result<SdkParamValue, String> {
        match expr.trim() {
            "double_time" => Ok(SdkParamValue::Scalar((vars.time * 2.0) as f32)),
            "offset_base" => match vars.value {
                SdkParamValue::Scalar(v) => Ok(SdkParamValue::Scalar(v + 5.0)),
                other => Ok(other),
            },
            "boom" => panic!("native driver boom"),
            other => Err(format!("unknown native expr: {other}")),
        }
    }
}

struct PeakAnalyzer;

struct PeakAnalyzerRun {
    channels: u16,
    frames: i64,
    peaks: Vec<i64>,
}

impl SdkAnalyzer for PeakAnalyzer {
    fn open(
        &self,
        _host: &Host,
        _sample_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AnalyzerRun>, String> {
        Ok(Box::new(PeakAnalyzerRun {
            channels,
            frames: 0,
            peaks: Vec::new(),
        }))
    }
}

impl AnalyzerRun for PeakAnalyzerRun {
    fn push(&mut self, samples: &[f32]) -> Result<(), String> {
        let ch = usize::from(self.channels.max(1));
        for frame in samples.chunks_exact(ch) {
            if frame[0].is_nan() {
                panic!("native analyzer push boom");
            }
            if frame[0].abs() >= 0.5 {
                self.peaks.push(self.frames);
            }
            self.frames += 1;
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        Ok(AnalyzerResult {
            beats: self.peaks,
            markers: vec![AnalyzerMarker {
                frame: self.frames,
                duration_frames: 0,
                name: "End".to_owned(),
                color: MarkerColor::Teal,
            }],
            ..AnalyzerResult::default()
        })
    }
}

struct ParityGuest;

impl SdkPlugin for ParityGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        let amount_param = EffectParamSpec::scalar("amount", "Amount", 1.0, 0.0, 1.0);
        host.register_effect(
            &EffectSpec::new("native.parity.wgsl_invert", "Native WGSL Invert")
                .with_category("Stylize")
                .with_params(vec![amount_param.clone()]),
            WGSL_INVERT,
        )?;
        host.register_cpu_effect(
            EffectSpec::new("native.parity.cpu_invert", "Native CPU Invert")
                .with_category("Stylize")
                .with_params(vec![amount_param]),
            InvertCpuEffect,
        )?;
        host.register_generator(
            &EffectSpec::new("native.parity.solid_gen", "Native Solid Generator")
                .with_category("Generators"),
            WGSL_SOLID_GEN,
        )?;
        host.register_transition(
            &TransitionSpec::new("native.parity.split", "Native Split"),
            WGSL_SPLIT_TRANSITION,
        )?;
        host.register_clip_type(&ClipTypeSpec::new(
            "native.parity.solid_clip",
            "Native Solid Clip",
            "native.parity.solid_gen",
            MarkerColor::Blue,
        ))?;
        host.register_driver_language(
            DriverLanguageSpec::new("native-linear", "Native Linear"),
            LinearDriver,
        )?;
        host.register_preset(&PresetSpec::new("Native Parity Preset").with_effect("vignette"))?;
        host.register_theme("native-dark", "[dark]\naccent = \"#f97316\"\n")?;
        host.register_workspace(&WorkspaceSpec {
            id: "native.parity.workspace".to_owned(),
            name: "Native Workspace".to_owned(),
            panes: WorkspacePanes {
                sidebar_width: Some(300.0),
                sidebar_collapsed: Some(false),
                inspector_width: Some(340.0),
                inspector_collapsed: None,
                timeline_height: Some(260.0),
                scopes: Some(true),
                focus: Some(WorkspaceFocus::Preview),
                tree: None,
            },
        })?;
        host.register_analyzer(
            AnalyzerSpec::new(
                "native.parity.peak_analyzer",
                "Native Peak Analyzer",
                AnalyzerScope::Clip,
            ),
            PeakAnalyzer,
        )?;
        Ok(())
    }
}

struct LegacyGuest;

impl SdkPlugin for LegacyGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_command("native.legacy.cmd", "Legacy Command")
    }
}

fn bound_parity() -> PluginRuntime<ParityGuest> {
    let runtime = PluginRuntime::new(ParityGuest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

fn bound_legacy() -> PluginRuntime<LegacyGuest> {
    let runtime = PluginRuntime::new(LegacyGuest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

fn loaded(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Box<dyn aria_plugin::Plugin>) {
    let dir = temp.path().join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native Parity"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI parity test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    let pkg = Package::open(&dir).expect("open package");
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    (PluginContext::new(), plugin)
}

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

struct TestDriverScope {
    time: f64,
    value: ParamValue,
}

impl DriverScope for TestDriverScope {
    fn time(&self) -> f64 {
        self.time
    }
    fn property(&self) -> ClipParam {
        ClipParam::Opacity
    }
    fn clip_time(&self) -> f64 {
        self.time
    }
    fn frame_rate(&self) -> f64 {
        24.0
    }
    fn duration(&self) -> f64 {
        10.0
    }
    fn canvas(&self) -> [u32; 2] {
        [1920, 1080]
    }
    fn value(&self) -> ParamValue {
        self.value
    }
    fn value_at(&self, _seconds: f64) -> ParamValue {
        self.value
    }
    fn key_count(&self) -> usize {
        0
    }
    fn key_time(&self, _index: usize) -> Option<f64> {
        None
    }
    fn input(&mut self, _index: usize) -> Result<ParamValue, String> {
        Err("unused".to_owned())
    }
    fn input_at(&mut self, _index: usize, _seconds: f64) -> Result<ParamValue, String> {
        Err("unused".to_owned())
    }
    fn clip_info(&mut self, _clip: &InputClip) -> Result<DriverClipInfo, String> {
        Err("unused".to_owned())
    }
    fn seed(&self) -> u64 {
        42
    }
    fn expired(&self) -> bool {
        false
    }
}

#[test]
fn native_plugin_registers_all_contributions_renders_on_lavapipe_and_cleans_up_on_unmount() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<ParityGuest>> = LazyLock::new(bound_parity);
    let temp = TempDir::new().expect("temp dir");
    let plugin_id = "test.native.parity.all";
    let (ctx, plugin) = loaded(&temp, plugin_id, &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount parity plugin");

    // 1. Catalog presence for all registrations.
    let wgsl_fx = effect_spec("native.parity.wgsl_invert").expect("wgsl effect in catalog");
    assert_eq!(wgsl_fx.category, "Stylize");
    assert!(!wgsl_fx.generator);
    assert!(renders_effect("native.parity.wgsl_invert"));

    let cpu_fx = effect_spec("native.parity.cpu_invert").expect("cpu effect in catalog");
    assert_eq!(cpu_fx.category, "Stylize");
    assert!(renders_effect("native.parity.cpu_invert"));

    let gen_fx = effect_spec("native.parity.solid_gen").expect("generator in catalog");
    assert!(gen_fx.generator);
    assert!(renders_effect("native.parity.solid_gen"));

    let tr = transition_spec("native.parity.split").expect("transition in catalog");
    assert_eq!(tr.label, "Native Split");
    assert!(renders_transition("native.parity.split"));

    let ct = clip_type_spec("native.parity.solid_clip").expect("clip type in catalog");
    assert_eq!(ct.generator, "native.parity.solid_gen");
    assert_eq!(ct.colour, aria_models::MarkerColor::Blue);

    assert!(
        ctx.driver_languages()
            .iter()
            .any(|d| d.spec.id == "native-linear")
    );
    assert!(
        ctx.presets()
            .iter()
            .any(|p| p.preset.name == "Native Parity Preset")
    );
    assert!(ctx.themes().iter().any(|t| &*t.id == "native-dark"));
    assert!(
        ctx.workspaces()
            .iter()
            .any(|w| w.spec.id == "native.parity.workspace")
    );

    // 2. Driver evaluation and frame cost recording.
    let prog =
        compile_driver(&DriverCode::new("native-linear", "double_time")).expect("compile driver");
    let mut scope = TestDriverScope {
        time: 3.5,
        value: ParamValue::Scalar(1.0),
    };
    assert_eq!(
        prog.evaluate(&mut scope).expect("evaluate double_time"),
        ParamValue::Scalar(7.0)
    );
    let prog_offset = compile_driver(&DriverCode::new("native-linear", "offset_base"))
        .expect("compile offset_base");
    assert_eq!(
        prog_offset
            .evaluate(&mut scope)
            .expect("evaluate offset_base"),
        ParamValue::Scalar(6.0)
    );

    // 3. Compositor rendering on lavapipe: WGSL effect, CPU effect, generator, transition, and CPU guest error bypass.
    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(W, H, [200, 100, 50, 255].repeat((W * H) as usize));

    // WGSL effect
    let wgsl_pass = PassInstance::new(
        pass_id("native.parity.wgsl_invert").expect("wgsl pass_id"),
        vec![1.0],
    );
    let wgsl_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&wgsl_pass));
    let wgsl_img = comp
        .render(&gpu, &config, &[wgsl_layer])
        .expect("render wgsl effect");
    assert_px(&wgsl_img, 16, 8, [55, 155, 205, 255], 2);

    // CPU effect (exact RGB inversion)
    let cpu_pass = PassInstance::new(
        pass_id("native.parity.cpu_invert").expect("cpu pass_id"),
        vec![1.0],
    );
    let cpu_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&cpu_pass));
    let cpu_img = comp
        .render(&gpu, &config, &[cpu_layer])
        .expect("render cpu effect");
    assert_px(&cpu_img, 16, 8, [55, 155, 205, 255], 1);

    // 3b. Foreign RGBA16F identity pass (amount = 0.0) preserves HDR (> 1.0),
    // negative values, padded row strides, and a 4096-channel f16 gradient bit-exact.
    let native_cpu =
        registered_effect("native.parity.cpu_invert").expect("registered native cpu effect");
    let hdr_stride = 12; // 2 pixels wide (8 active u16s) + 4 u16 padding per row
    let mut hdr_in = vec![0xdead_u16; hdr_stride * 2];
    let hdr_row0 = [
        f32_to_f16(4.0),
        f32_to_f16(-1.25),
        f32_to_f16(12.5),
        F16_ONE,
        f32_to_f16(-0.00390625),
        f32_to_f16(0.0),
        f32_to_f16(65504.0),
        F16_ONE,
    ];
    let hdr_row1 = [
        0x0001_u16, // smallest positive subnormal f16
        0x8001_u16, // smallest negative subnormal f16
        f32_to_f16(2.75),
        f32_to_f16(0.5),
        f32_to_f16(-8.0),
        f32_to_f16(0.125),
        f32_to_f16(1.5),
        F16_ONE,
    ];
    hdr_in[..8].copy_from_slice(&hdr_row0);
    hdr_in[hdr_stride..hdr_stride + 8].copy_from_slice(&hdr_row1);
    let mut hdr_out = vec![0u16; hdr_stride * 2];
    assert!(native_cpu.run_cpu(
        HostCpuFrameIn::new(&hdr_in, 2, 2, hdr_stride),
        &mut HostCpuFrameOut::new(&mut hdr_out, 2, 2, hdr_stride),
        &[0.0],
        0.25,
    ));
    assert_eq!(&hdr_out[..8], &hdr_row0);
    assert_eq!(&hdr_out[hdr_stride..hdr_stride + 8], &hdr_row1);

    let grad_in: Vec<u16> = (0u16..4096)
        .map(|i| {
            if i < 2048 {
                i * 15 // spans subnormals, [0.0, 1.0], and HDR up to ~30705
            } else {
                0x8000 | ((i - 2048) * 15) // negative subnormals and negative finite f16
            }
        })
        .collect();
    assert_eq!(grad_in.len(), 4096);
    let mut grad_out = vec![0u16; 4096];
    assert!(native_cpu.run_cpu(
        HostCpuFrameIn::new(&grad_in, 32, 32, 32 * 4),
        &mut HostCpuFrameOut::new(&mut grad_out, 32, 32, 32 * 4),
        &[0.0],
        0.5,
    ));
    assert_eq!(grad_out, grad_in);

    // 3c. Verify `aria_sdk::f16::{to_f32, from_f32}` agree with `aria_compositor`
    // on all 65,536 `u16` bit patterns.
    for bits in 0u16..=u16::MAX {
        let sdk_f = aria_sdk::f16::to_f32(bits);
        let host_f = f16_to_f32(bits);
        assert_eq!(
            sdk_f.to_bits(),
            host_f.to_bits(),
            "to_f32 mismatch at {bits:#06x}"
        );
        assert_eq!(
            aria_sdk::f16::from_f32(sdk_f),
            f32_to_f16(host_f),
            "from_f32 mismatch at {bits:#06x}"
        );
    }
    // CPU effect guest error (> 1.0): bypasses for the frame, records a fault, stays mounted.
    let err_pass = PassInstance::new(
        pass_id("native.parity.cpu_invert").expect("cpu pass_id"),
        vec![2.0],
    );
    let err_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&err_pass));
    let err_img = comp
        .render(&gpu, &config, &[err_layer])
        .expect("render bypassed cpu effect");
    assert_px(&err_img, 16, 8, [200, 100, 50, 255], 1);
    let faults = ctx.faults();
    assert!(
        faults.iter().any(|f| f.plugin == plugin_id
            && f.message.contains("native.parity.cpu_invert")
            && f.message.contains("native cpu range error")),
        "expected CPU effect fault in {faults:?}"
    );
    assert!(ctx.is_mounted(plugin_id));

    // WGSL transition
    let out_bmp = RgbaImage::new(W, H, [255, 0, 0, 255].repeat((W * H) as usize));
    let in_bmp = RgbaImage::new(W, H, [0, 0, 255, 255].repeat((W * H) as usize));
    let out_layer = CompositeLayer::rgba(&out_bmp, LayerPlacement::full_canvas(&config));
    let in_layer = CompositeLayer::rgba(&in_bmp, LayerPlacement::full_canvas(&config));
    let tr_img = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::Transition {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "native.parity.split",
                progress: 0.5,
            }],
        )
        .expect("render transition");
    assert_px(&tr_img, 8, 8, [255, 0, 0, 255], 1);
    assert_px(&tr_img, 24, 8, [0, 0, 255, 255], 1);
    let registered_analyzers = ctx.analyzers();
    assert_eq!(registered_analyzers.len(), 1);
    assert_eq!(
        registered_analyzers[0].spec.id,
        "native.parity.peak_analyzer"
    );
    // 4. Unmount removes every contribution.
    ctx.unmount(plugin_id).expect("unmount parity plugin");
    assert!(!ctx.is_mounted(plugin_id));
    assert!(effect_spec("native.parity.wgsl_invert").is_none());
    assert!(!renders_effect("native.parity.wgsl_invert"));
    assert!(effect_spec("native.parity.cpu_invert").is_none());
    assert!(!renders_effect("native.parity.cpu_invert"));
    assert!(effect_spec("native.parity.solid_gen").is_none());
    assert!(transition_spec("native.parity.split").is_none());
    assert!(!renders_transition("native.parity.split"));
    assert!(clip_type_spec("native.parity.solid_clip").is_none());
    assert!(ctx.driver_languages().is_empty());
    assert!(ctx.presets().is_empty());
    assert!(ctx.themes().is_empty());
    assert!(ctx.workspaces().is_empty());
    assert!(ctx.analyzers().is_empty());
}

#[test]
fn panicking_native_cpu_effect_is_caught_across_abi_bypasses_frame_and_fails_plugin() {
    let _serial = SERIAL.lock();
    static RT: LazyLock<PluginRuntime<ParityGuest>> = LazyLock::new(bound_parity);
    let temp = TempDir::new().expect("temp dir");
    let plugin_id = "test.native.parity.panic";
    let (ctx, plugin) = loaded(&temp, plugin_id, &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount parity plugin");

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(W, H, [200, 100, 50, 255].repeat((W * H) as usize));

    let panic_pass = PassInstance::new(
        pass_id("native.parity.cpu_invert").expect("cpu pass_id"),
        vec![-1.0],
    );
    let layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&panic_pass));
    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("render catches panic and bypasses");
    assert_px(&img, 16, 8, [200, 100, 50, 255], 1);

    assert!(
        !ctx.is_mounted(plugin_id),
        "panicking CPU effect must fail and unmount the plugin"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0]
            .reason
            .contains("native.parity.cpu_invert panicked")
            && failures[0].reason.contains("native cpu boom"),
        "{}",
        failures[0].reason
    );
    assert!(effect_spec("native.parity.cpu_invert").is_none());
    assert!(effect_spec("native.parity.wgsl_invert").is_none());
    assert!(transition_spec("native.parity.split").is_none());
    assert!(clip_type_spec("native.parity.solid_clip").is_none());
    assert!(ctx.driver_languages().is_empty());
    assert!(ctx.presets().is_empty());
    assert!(ctx.themes().is_empty());
    assert!(ctx.workspaces().is_empty());
    assert!(ctx.analyzers().is_empty());
}

#[test]
fn old_abi_native_plugin_with_smaller_struct_size_still_loads_and_refuses_missing_callbacks() {
    let _serial = SERIAL.lock();
    static LEGACY_RT: LazyLock<PluginRuntime<LegacyGuest>> = LazyLock::new(bound_legacy);
    let mut legacy_table = plugin_table!(LEGACY_RT);
    legacy_table.struct_size = ARIA_PLUGIN_V1_SCOPES_SIZE;
    assert!(legacy_table.has_scopes() && !legacy_table.has_contributions());

    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.native.parity.legacy-ok", &legacy_table);
    ctx.mount(plugin.as_ref())
        .expect("old-ABI plugin mounts cleanly");
    assert!(ctx.is_mounted("test.native.parity.legacy-ok"));
    ctx.unmount("test.native.parity.legacy-ok")
        .expect("unmount legacy plugin");

    // An old-ABI table that tries to register a CPU effect without `cpu_effect_process` is refused.
    static PARITY_RT: LazyLock<PluginRuntime<ParityGuest>> = LazyLock::new(bound_parity);
    let mut short_table = plugin_table!(PARITY_RT);
    short_table.struct_size = ARIA_PLUGIN_V1_SCOPES_SIZE;
    let (ctx2, plugin2) = loaded(&temp, "test.native.parity.short-refused", &short_table);
    let err = ctx2
        .mount(plugin2.as_ref())
        .expect_err("short table cannot register CPU effect")
        .to_string();
    assert!(err.contains("cpu_effect_process"), "{err}");
    assert!(effect_spec("native.parity.wgsl_invert").is_none());
}
