//! WASM visual effects, generators, effect graphs, transitions, clip types, drivers, presets, themes, workspaces, and analyzers.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, CpuFrameIn, CpuFrameOut,
    F16_ONE, GpuContext, LayerPlacement, PassInstance, RgbaImage, f32_to_f16, pass_id,
    plugin_texture, registered_effect, renders_effect, renders_transition,
};
use std::sync::atomic::{AtomicBool, Ordering};

use aria_commands::{Command, EditCommand};
use aria_core::{ColorSpace, EncoderConfig, Rational, RationalTime, TimeRange};
use aria_models::{
    ClipParam, DriverCode, EffectBounds, Generator, MarkerColor, MediaSource, ParamValue, Project,
    TrackKind, audio_effect_spec, clip_type_spec, effect_spec, transition_spec,
};
use aria_plugin::{
    AnalyzerError, AudioSource, CommandProposed, DriverClipInfo, DriverScope, InputClip, Origin,
    RenderHost, SettingsService,
};
use aria_plugin_wasm::{Limits, load};

use super::{FIXTURE_WASM, SERIAL, setup_host_context, write_package};

struct ParityDriverScope {
    time: f64,
    value: ParamValue,
}

impl DriverScope for ParityDriverScope {
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
        7
    }
    fn expired(&self) -> bool {
        false
    }
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

#[test]
fn wasm_parity_contributions_register_render_on_lavapipe_fault_and_clean_up_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, settings) = setup_host_context();
    settings.set("example.register_parity", serde_json::json!(true));
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    // 1. Audio effect category set via `set-effect-category`.
    let gain_spec = audio_effect_spec("example.gain").expect("example.gain registered");
    assert_eq!(gain_spec.category, "Dynamics");

    // 2. Catalog presence for WGSL effect, CPU effect, generator, transition, clip type, driver, preset, theme, workspace.
    let wgsl_fx = effect_spec("example.wgsl-invert").expect("wgsl effect in catalog");
    assert_eq!(wgsl_fx.category, "Stylize");
    assert!(!wgsl_fx.generator);
    assert!(renders_effect("example.wgsl-invert"));

    let cpu_fx = effect_spec("example.cpu-invert").expect("cpu effect in catalog");
    assert_eq!(cpu_fx.category, "Stylize");
    assert!(renders_effect("example.cpu-invert"));

    let gen_fx = effect_spec("example.wgsl-solid").expect("generator in catalog");
    assert!(gen_fx.generator);
    assert!(renders_effect("example.wgsl-solid"));

    let graph_fx = effect_spec("example.wgsl-graph").expect("effect graph in catalog");
    assert_eq!(graph_fx.category, "Stylize");
    assert_eq!(graph_fx.bounds, EffectBounds::new(1.0, 2.0, 3.0, 4.0));
    assert!(graph_fx.reads_outside, "reads-outside was dropped");
    assert!(graph_fx.time_dependent, "time-dependent was dropped");
    assert!(renders_effect("example.wgsl-graph"));
    let lut = plugin_texture("example.lut").expect("texture registered");
    assert_eq!((lut.width(), lut.height()), (1, 1));
    drop(lut);

    let tr = transition_spec("example.split-wipe").expect("transition in catalog");
    assert_eq!(tr.label, "WASM Split Wipe");
    assert!(renders_transition("example.split-wipe"));

    let ct = clip_type_spec("example.solid-clip").expect("clip type in catalog");
    assert_eq!(ct.generator, "example.wgsl-solid");
    assert_eq!(ct.colour, MarkerColor::Teal);
    let meter = clip_type_spec("example.meter").expect("defined clip type in catalog");
    assert_eq!(meter.generator, "example.meter");
    let meter_fx = effect_spec("example.meter").expect("the meter's generator in catalog");
    assert!(meter_fx.generator);
    assert_eq!(meter_fx.label, "WASM Meter");
    let fields: Vec<&str> = meter_fx.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(fields, ["level"]);

    assert!(
        ctx.driver_languages()
            .iter()
            .any(|d| d.spec.id == "wasm-expr")
    );
    assert!(ctx.presets().iter().any(|p| p.preset.name == "WASM Preset"));
    assert!(ctx.themes().iter().any(|t| &*t.id == "wasm-dark"));
    assert!(
        ctx.workspaces()
            .iter()
            .any(|w| w.spec.id == "example.wasm-workspace")
    );
    assert!(
        ctx.analyzers()
            .iter()
            .any(|a| a.spec.id == "example.wasm-peaks")
    );
    let empty_project = Project::new("wasm analyzer", Rational::FPS_24);
    let analysis = ctx
        .run_analyzer(
            "example.wasm-peaks",
            &empty_project,
            AudioSource::Open,
            TimeRange::at_rate(0, 1, Rational::FPS_24),
            &AtomicBool::new(false),
        )
        .expect("run example.wasm-peaks");
    assert_eq!(analysis.markers.len(), 1);
    assert_eq!(analysis.markers[0].frame, 2000);
    assert_eq!(analysis.markers[0].name, "WasmEnd");
    assert_eq!(analysis.markers[0].color, MarkerColor::Teal);

    let cancel = AtomicBool::new(false);
    let cancelled = ctx.run_analyzer_with_progress(
        "example.wasm-peaks",
        &empty_project,
        AudioSource::Open,
        TimeRange::at_rate(0, 12, Rational::FPS_24),
        &cancel,
        |done, _total| {
            if done > 0 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    );
    assert_eq!(cancelled, Err(AnalyzerError::Cancelled));

    // 3. Driver evaluation.
    let prog = aria_render::compile_driver(&DriverCode::new("wasm-expr", "double_time"))
        .expect("compile double_time");
    let mut scope = ParityDriverScope {
        time: 4.25,
        value: ParamValue::Scalar(2.0),
    };
    assert_eq!(
        prog.evaluate(&mut scope).expect("evaluate double_time"),
        ParamValue::Scalar(8.5)
    );
    let prog_plus = aria_render::compile_driver(&DriverCode::new("wasm-expr", "plus_ten"))
        .expect("compile plus_ten");
    assert_eq!(
        prog_plus.evaluate(&mut scope).expect("evaluate plus_ten"),
        ParamValue::Scalar(12.0)
    );
    let prog_canvas = aria_render::compile_driver(&DriverCode::new("wasm-expr", "canvas_vec"))
        .expect("compile canvas_vec");
    assert_eq!(
        prog_canvas
            .evaluate(&mut scope)
            .expect("evaluate canvas_vec"),
        ParamValue::Vec2([1920.0, 1080.0])
    );

    // 4. Compositor rendering on lavapipe: WGSL effect, CPU effect, transition, and CPU guest error fault.
    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let w = 32;
    let h = 16;
    let config = CompositorConfig::new(w, h).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(w, h, [200, 100, 50, 255].repeat((w * h) as usize));

    // WGSL effect
    let wgsl_pass = PassInstance::new(
        pass_id("example.wgsl-invert").expect("wgsl pass_id"),
        vec![1.0],
    );
    let wgsl_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&wgsl_pass));
    let wgsl_img = comp
        .render(&gpu, &config, &[wgsl_layer])
        .expect("render wgsl effect");
    assert_px(&wgsl_img, 16, 8, [55, 155, 205, 255], 2);

    // WGSL effect graph: red and green halved by the compute pass, blue from
    // the registered texture.
    let graph_pass = PassInstance::new(
        pass_id("example.wgsl-graph").expect("graph pass_id"),
        Vec::new(),
    );
    let graph_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&graph_pass));
    let graph_img = comp
        .render(&gpu, &config, &[graph_layer])
        .expect("render effect graph");
    assert_px(&graph_img, 16, 8, [100, 50, 255, 255], 2);

    // CPU effect (exact RGB inversion)
    let cpu_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
        vec![1.0],
    );
    let cpu_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&cpu_pass));
    let cpu_img = comp
        .render(&gpu, &config, &[cpu_layer])
        .expect("render cpu effect");
    assert_px(&cpu_img, 16, 8, [55, 155, 205, 255], 1);

    // 4b. Foreign WASM RGBA16F identity pass (amount = 0.0) preserves HDR (> 1.0),
    // negative values, padded row strides, and a 4096-channel f16 gradient bit-exact.
    let wasm_cpu = registered_effect("example.cpu-invert").expect("registered wasm cpu effect");
    let hdr_stride = 12; // 2 pixels wide (8 active u16s) + 4 u16 padding per row
    let mut hdr_in = vec![0xbeef_u16; hdr_stride * 2];
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
        0x0001_u16,
        0x8001_u16,
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
    assert!(wasm_cpu.run_cpu(
        CpuFrameIn::new(&hdr_in, 2, 2, hdr_stride),
        &mut CpuFrameOut::new(&mut hdr_out, 2, 2, hdr_stride),
        &[0.0],
        0.25,
    ));
    assert_eq!(&hdr_out[..8], &hdr_row0);
    assert_eq!(&hdr_out[hdr_stride..hdr_stride + 8], &hdr_row1);

    let grad_in: Vec<u16> = (0u16..4096)
        .map(|i| {
            if i < 2048 {
                i * 15
            } else {
                0x8000 | ((i - 2048) * 15)
            }
        })
        .collect();
    assert_eq!(grad_in.len(), 4096);
    let mut grad_out = vec![0u16; 4096];
    assert!(wasm_cpu.run_cpu(
        CpuFrameIn::new(&grad_in, 32, 32, 32 * 4),
        &mut CpuFrameOut::new(&mut grad_out, 32, 32, 32 * 4),
        &[0.0],
        0.5,
    ));
    assert_eq!(grad_out, grad_in);
    // The defined clip type draws its `level` field through cpu-effect-process.
    let meter_pass = registered_effect("example.meter").expect("registered meter generator");
    let blank = [0u16; 16];
    let mut drawn = [0u16; 16];
    assert!(meter_pass.run_cpu(
        CpuFrameIn::new(&blank, 2, 2, 8),
        &mut CpuFrameOut::new(&mut drawn, 2, 2, 8),
        &[0.5],
        0.0,
    ));
    let half = f32_to_f16(0.5);
    assert_eq!(&drawn[..4], &[half, half, half, F16_ONE]);
    assert_eq!(&drawn[12..], &[half, half, half, F16_ONE]);
    drop(meter_pass);
    // CPU effect guest error (amount = 2.0): bypasses frame, records fault in ctx.faults(), plugin stays mounted.
    let err_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
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
        faults.iter().any(|f| f.plugin == "com.example.wasm-tools"
            && f.message.contains("example.cpu-invert")
            && f.message.contains("cpu effect guest error")),
        "expected CPU effect fault in {faults:?}"
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    // WGSL transition
    let out_bmp = RgbaImage::new(w, h, [255, 0, 0, 255].repeat((w * h) as usize));
    let in_bmp = RgbaImage::new(w, h, [0, 0, 255, 255].repeat((w * h) as usize));
    let out_layer = CompositeLayer::rgba(&out_bmp, LayerPlacement::full_canvas(&config));
    let in_layer = CompositeLayer::rgba(&in_bmp, LayerPlacement::full_canvas(&config));
    let tr_img = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::Transition {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "example.split-wipe",
                progress: 0.5,
            }],
        )
        .expect("render transition");
    assert_px(&tr_img, 8, 8, [255, 0, 0, 255], 1);
    assert_px(&tr_img, 24, 8, [0, 0, 255, 255], 1);

    // 5. Unmount removes every contribution.
    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(effect_spec("example.wgsl-invert").is_none());
    assert!(!renders_effect("example.wgsl-invert"));
    assert!(effect_spec("example.cpu-invert").is_none());
    assert!(!renders_effect("example.cpu-invert"));
    assert!(effect_spec("example.wgsl-solid").is_none());
    assert!(effect_spec("example.wgsl-graph").is_none());
    assert!(!renders_effect("example.wgsl-graph"));
    assert!(plugin_texture("example.lut").is_none());
    assert!(transition_spec("example.split-wipe").is_none());
    assert!(!renders_transition("example.split-wipe"));
    assert!(clip_type_spec("example.solid-clip").is_none());
    assert!(clip_type_spec("example.meter").is_none());
    assert!(effect_spec("example.meter").is_none());
    assert!(ctx.driver_languages().is_empty());
    assert!(ctx.presets().is_empty());
    assert!(ctx.themes().is_empty());
    assert!(ctx.workspaces().is_empty());
    assert!(ctx.analyzers().is_empty());
}

#[test]
fn trapping_wasm_cpu_effect_bypasses_frame_records_fault_and_fails_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, settings) = setup_host_context();
    settings.set("example.register_parity", serde_json::json!(true));
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let w = 32;
    let h = 16;
    let config = CompositorConfig::new(w, h).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(w, h, [200, 100, 50, 255].repeat((w * h) as usize));

    let trap_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
        vec![-1.0],
    );
    let trap_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&trap_pass));
    let img = comp
        .render(&gpu, &config, &[trap_layer])
        .expect("compositor catches trap and bypasses frame");
    assert_px(&img, 16, 8, [200, 100, 50, 255], 1);

    assert!(
        !ctx.is_mounted("com.example.wasm-tools"),
        "trapping CPU effect must fail and unmount the plugin"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("example.cpu-invert panicked")
            && failures[0].reason.contains("wasm trap:"),
        "{}",
        failures[0].reason
    );
    assert!(effect_spec("example.cpu-invert").is_none());
    assert!(effect_spec("example.wgsl-invert").is_none());
    assert!(transition_spec("example.split-wipe").is_none());
    assert!(clip_type_spec("example.solid-clip").is_none());

    // A trapping WASM analyzer unwinds into `run_analyzer`, records a single
    // failure, unmounts the plugin, and returns `AnalyzerError::Panicked`.
    let (ctx2, _host2, _view2, _settings2) = setup_host_context();
    let plugin2 = load(&pkg, &Limits::default()).expect("load wasm plugin 2");
    ctx2.mount(plugin2.as_ref()).expect("mount wasm plugin 2");
    let nan_wav = temp.path().join("nan.wav");
    let frames = 64usize;
    let channels = 2u16;
    let rate = 48_000u32;
    let data_bytes = (frames * usize::from(channels) * 4) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    wav.extend_from_slice(&(channels * 4).to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..(frames * usize::from(channels)) {
        wav.extend_from_slice(&f32::NAN.to_bits().to_le_bytes());
    }
    std::fs::write(&nan_wav, wav).expect("write nan.wav");
    let mut nan_proj = Project::new("nan analyzer", Rational::FPS_24);
    let media = nan_proj.add_media(MediaSource::new(
        &nan_wav,
        0,
        0,
        Rational::new(1, 1000),
        100,
        true,
    ));
    let lane = nan_proj.add_track(TrackKind::Audio, "A1");
    nan_proj
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 100, Rational::new(1, 1000)),
            RationalTime::zero(Rational::FPS_24),
        )
        .expect("add nan audio clip");
    let trap_err = ctx2
        .run_analyzer(
            "example.wasm-peaks",
            &nan_proj,
            AudioSource::Open,
            TimeRange::at_rate(0, 1, Rational::FPS_24),
            &AtomicBool::new(false),
        )
        .expect_err("trapping wasm analyzer must fail");
    assert!(
        matches!(&trap_err, AnalyzerError::Panicked { id, reason } if id == "example.wasm-peaks" && reason.contains("wasm trap:")),
        "expected AnalyzerError::Panicked, got {trap_err:?}"
    );
    assert!(!ctx2.is_mounted("com.example.wasm-tools"));
    assert_eq!(ctx2.failures().len(), 1);

    // Re-entrant calls on the same thread into `run_with_instance`, `call_export`,
    // `run_encoder_call`, and `call_cpu_effect_process` return a refusal error instead
    // of deadlocking `WasmPluginShared::state`.
    let (ctx3, host3, _view3, settings3) = setup_host_context();
    settings3.set("example.register_parity", serde_json::json!(true));
    let _render_reg = ctx3.register(RenderHost::new(gpu));
    let plugin3 = load(&pkg, &Limits::default()).expect("load wasm plugin 3");
    ctx3.mount(plugin3.as_ref()).expect("mount wasm plugin 3");
    let reentrant_errors = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let errs_sink = std::sync::Arc::clone(&reentrant_errors);
    let ctx3_inner = (*ctx3).clone();
    let export_out = temp.path().join("reentrant.mrk");
    let enc_out = temp.path().join("reentrant.rgba");
    let _sub = ctx3.on_waterfall(move |_ev: &mut CommandProposed| {
        let mut errs = errs_sink.lock();
        if errs.is_empty() {
            errs.push(
                ctx3_inner
                    .run_command("example.add-marker")
                    .expect_err("re-entrant run_command must fail")
                    .to_string(),
            );
            errs.push(
                ctx3_inner
                    .export_file(
                        "example.markers",
                        &Project::new("p", Rational::FPS_24),
                        &export_out,
                    )
                    .expect_err("re-entrant export_file must fail")
                    .to_string(),
            );
            errs.push(
                ctx3_inner
                    .open_encoder(
                        "example.raw-rgba",
                        &enc_out,
                        &EncoderConfig::new((8, 8), Rational::FPS_24, ColorSpace::BT709),
                    )
                    .err()
                    .expect("re-entrant open_encoder must fail")
                    .to_string(),
            );
        }
    });
    ctx3.run_command("example.add-marker")
        .expect("outer add-marker succeeds while inner re-entrant calls are refused");
    let captured_errs = reentrant_errors.lock().clone();
    assert_eq!(captured_errs.len(), 3, "{captured_errs:?}");
    for err in &captured_errs {
        assert!(
            err.contains("cannot re-enter from inside one of its own calls"),
            "unexpected re-entrancy error: {err}"
        );
    }
    drop(_sub);

    host3
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Sticker,
                name: "S1".to_owned(),
                index: None,
                pinned: false,
            }),
        )
        .expect("add sticker track");
    let s_track = host3
        .snapshot()
        .timeline()
        .tracks_ordered()
        .find(|t| t.kind == TrackKind::Sticker)
        .expect("sticker track")
        .id;
    host3
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddGenerated {
                track: s_track,
                generator: Generator::SolidColor {
                    rgba: [200, 100, 50, 255],
                },
                timeline: TimeRange::at_rate(0, 24, Rational::FPS_24),
            }),
        )
        .expect("add sticker clip");
    let s_clip = host3
        .snapshot()
        .timeline()
        .track(s_track)
        .expect("s_track")
        .clips_ordered()
        .next()
        .expect("s_clip")
        .id;
    host3
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddEffect {
                clip: s_clip,
                effect_id: "example.cpu-invert".to_owned(),
            }),
        )
        .expect("attach example.cpu-invert");
    settings3.set(
        "example.render_frame_args",
        serde_json::json!({ "size": "exact", "width": 16, "height": 16 }),
    );
    ctx3.run_command("example.render-frame")
        .expect("re-entrant render-frame completes without deadlocking");
    assert!(
        ctx3.faults().iter().any(|f| {
            f.plugin == "com.example.wasm-tools"
                && f.message.contains(
                    "cannot run CPU effect example.cpu-invert from inside one of its own calls",
                )
        }),
        "expected re-entrancy fault, got: {:?}",
        ctx3.faults()
    );
    assert!(ctx3.is_mounted("com.example.wasm-tools"));
}
