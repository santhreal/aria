//! Effects and transitions registered while the process runs, drawn on a GPU.
//!
//! Its own test binary: registration adds to process-wide catalogs that the
//! library tests iterate. Every test registers ids no other test uses.
//!
//! Covers one and several passes, what `input_tex` and `source_tex` hold at
//! each pass, parameter offsets, the header words (size, texel, time, pass
//! index and count), a transition at a known progress, removal falling back to
//! passthrough and crossfade, re-registration drawing the new shader, a refused
//! duplicate leaving the first registration in place, and a shader the device
//! refuses. Shader checks without a device are in the library tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorError, CompositorLayer,
    CpuEffectDefinition, CpuFrameIn, CpuFrameOut, EffectDefinition, EffectFault, EffectFaultKind,
    EffectParams, GpuContext, LayerPlacement, PassId, PassInstance, PassRegistration, RgbaImage,
    TransitionDefinition, effect_is_noop, pass_id, register_cpu_effect, register_effect,
    register_effect_with_fault_sink, register_transition, renders_effect, renders_transition,
};
use aria_models::{EffectParamKind, EffectParamSpec, EffectSpec, TransitionSpec};

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping registered pass test: no GPU adapter ({e})");
            None
        }
    }
}

macro_rules! gpu_or_skip {
    () => {
        match try_gpu() {
            Some(g) => g,
            None => return,
        }
    };
}

const W: u32 = 64;
const H: u32 = 16;

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

fn solid(rgba: [u8; 4]) -> RgbaImage {
    RgbaImage::new(W, H, rgba.repeat((W * H) as usize))
}

fn param(name: &str, kind: EffectParamKind) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind,
        default: 0.0,
        min: 0.0,
        max: 1.0,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
        ..Default::default()
    }
}

fn effect(id: &str, params: Vec<EffectParamSpec>, passes: &[&str]) -> PassRegistration {
    register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            category: String::new(),
            label: format!("{id} label"),
            params,
            generator: false,
            stateful: false,
            ..Default::default()
        },
        passes: passes.iter().map(|pass| (*pass).into()).collect(),
    })
    .unwrap_or_else(|error| panic!("{error}"))
}

fn transition(id: &str, shader: &str) -> PassRegistration {
    register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: id.into(),
            label: format!("{id} label"),
            ..Default::default()
        },
        shader: shader.into(),
    })
    .unwrap_or_else(|error| panic!("{error}"))
}

fn instance(id: &str, params: Vec<f32>) -> PassInstance {
    let id = pass_id(id).unwrap_or_else(|| panic!("`{id}` is renderable"));
    assert!(matches!(id, PassId::Registered(_)));
    PassInstance::new(id, params)
}

fn render(
    gpu: &GpuContext,
    comp: &mut Compositor,
    plate: [u8; 4],
    effects: &[PassInstance],
) -> RgbaImage {
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let bmp = solid(plate);
    let layer =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(effects);
    comp.render(gpu, &config, &[layer]).expect("renders")
}

const INVERT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, aria_sampler, in.uv);
    return mix(c, vec4<f32>(c.a - c.rgb, c.a), param_amount());
}
";

#[test]
fn a_single_pass_effect_draws_its_shader_with_its_parameter() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.invert",
        vec![param("amount", EffectParamKind::Scalar)],
        &[INVERT],
    );
    assert!(renders_effect("test.gpu.invert"));

    let full = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.gpu.invert", vec![1.0])],
    );
    assert_px(&full, 32, 8, [55, 155, 205, 255], 2);
    let none = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.gpu.invert", vec![0.0])],
    );
    assert_px(&none, 32, 8, [200, 100, 50, 255], 2);
}

#[test]
fn parameters_reach_their_accessors_at_their_offsets() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.offsets",
        vec![
            param("a", EffectParamKind::Scalar),
            param("b", EffectParamKind::Vec2),
            param("c", EffectParamKind::Color),
            param("d", EffectParamKind::Scalar),
        ],
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(param_a(), param_b().y, param_c().w * param_d(), 1.0);
}
"],
    );
    // a, b.x b.y, c.r c.g c.b c.a, d
    let params = vec![0.2, 0.9, 0.4, 0.1, 0.2, 0.3, 0.6, 0.5];
    let img = render(
        &gpu,
        &mut comp,
        [0, 0, 0, 255],
        &[instance("test.gpu.offsets", params)],
    );
    assert_px(&img, 32, 8, [51, 102, 77, 255], 2);
}

#[test]
fn the_header_holds_size_texel_and_time() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.header",
        Vec::new(),
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(aria.time / 10.0, aria.size.x / 255.0, aria.texel.y * aria.size.y * 0.5, 1.0);
}
"],
    );
    let at_two = instance("test.gpu.header", Vec::new()).with_time(2.0);
    let img = render(&gpu, &mut comp, [0, 0, 0, 255], &[at_two]);
    assert_px(&img, 32, 8, [51, 64, 128, 255], 2);
}

const ADD_QUARTER_RED: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let previous = textureSample(input_tex, aria_sampler, in.uv);
    let source = textureSample(source_tex, aria_sampler, in.uv);
    return vec4<f32>(previous.r + 0.25, source.g, f32(aria.pass_index) * 0.25 + f32(aria.pass_count) * 0.0625, 1.0);
}
";

#[test]
fn each_pass_reads_the_previous_output_and_the_effect_input() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.threepass",
        Vec::new(),
        &[ADD_QUARTER_RED, ADD_QUARTER_RED, ADD_QUARTER_RED],
    );
    // Red climbs a quarter per pass from 0; green stays the source's even
    // though two passes write between; blue is the last pass's index and the
    // pass count: 2 * 0.25 + 3 * 0.0625.
    let img = render(
        &gpu,
        &mut comp,
        [0, 100, 0, 255],
        &[instance("test.gpu.threepass", Vec::new())],
    );
    assert_px(&img, 32, 8, [191, 100, 175, 255], 2);
}

#[test]
fn source_tex_is_the_input_of_the_effect_not_of_the_layer() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _first = effect(
        "test.gpu.greenify",
        Vec::new(),
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(c.r, 0.8, c.b, c.a);
}
"],
    );
    let _second = effect(
        "test.gpu.twopass",
        Vec::new(),
        &[ADD_QUARTER_RED, ADD_QUARTER_RED],
    );
    let chain = [
        instance("test.gpu.greenify", Vec::new()),
        instance("test.gpu.twopass", Vec::new()),
    ];
    let img = render(&gpu, &mut comp, [0, 100, 0, 255], &chain);
    // Green 0.8 comes from the first effect's output; blue is 1 * 0.25 + 2 * 0.0625.
    assert_px(&img, 32, 8, [128, 204, 96, 255], 2);
}

#[test]
fn a_registered_effect_after_a_shipped_one_reads_its_output() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.afterpixelate",
        Vec::new(),
        &[ADD_QUARTER_RED, ADD_QUARTER_RED],
    );
    let chain = [
        PassInstance::new("pixelate", vec![4.0]),
        instance("test.gpu.afterpixelate", Vec::new()),
    ];
    let img = render(&gpu, &mut comp, [0, 100, 0, 255], &chain);
    assert_px(&img, 32, 8, [128, 100, 96, 255], 2);
}

#[test]
fn a_parameter_count_mismatch_is_an_error() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = effect(
        "test.gpu.mismatch",
        vec![param("amount", EffectParamKind::Scalar)],
        &[INVERT],
    );
    let config = CompositorConfig::new(W, H);
    let bmp = solid([0, 0, 0, 255]);
    let effects = [instance("test.gpu.mismatch", vec![1.0, 2.0])];
    let layer =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    match comp.render(&gpu, &config, &[layer]) {
        Err(CompositorError::InvalidEffectParams {
            effect,
            expected,
            actual,
        }) => {
            assert_eq!(effect, "test.gpu.mismatch");
            assert_eq!((expected, actual), (1, 2));
        }
        other => panic!("expected InvalidEffectParams, got {other:?}"),
    }
}

#[test]
fn a_dropped_effect_passes_the_layer_through_and_stops_resolving() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let fx = effect(
        "test.gpu.dropped",
        vec![param("amount", EffectParamKind::Scalar)],
        &[INVERT],
    );
    let held = instance("test.gpu.dropped", vec![1.0]);
    assert!(!held.is_noop());
    assert!(!effect_is_noop("test.gpu.dropped", &[0.0]));
    drop(fx);

    assert!(held.is_noop());
    assert!(effect_is_noop("test.gpu.dropped", &[1.0]));
    assert!(pass_id("test.gpu.dropped").is_none());
    assert!(!renders_effect("test.gpu.dropped"));
    assert!(aria_models::effect_spec("test.gpu.dropped").is_none());
    let img = render(&gpu, &mut comp, [200, 100, 50, 255], &[held]);
    assert_px(&img, 32, 8, [200, 100, 50, 255], 1);
}

#[test]
fn re_registering_an_id_draws_the_new_shader() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let solid_shader = |rgb: &str| {
        format!(
            "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {{
    return vec4<f32>({rgb}, 1.0);
}}
"
        )
    };
    let red = solid_shader("1.0, 0.0, 0.0");
    let green = solid_shader("0.0, 1.0, 0.0");

    let first = effect("test.gpu.redo", Vec::new(), &[&red]);
    let stale = instance("test.gpu.redo", Vec::new());
    let img = render(
        &gpu,
        &mut comp,
        [0, 0, 0, 255],
        std::slice::from_ref(&stale),
    );
    assert_px(&img, 32, 8, [255, 0, 0, 255], 1);
    drop(first);

    let _second = effect("test.gpu.redo", Vec::new(), &[&green]);
    let fresh = instance("test.gpu.redo", Vec::new());
    assert_ne!(fresh.id, stale.id, "a new registration is a different pass");
    let img = render(&gpu, &mut comp, [0, 0, 0, 255], &[fresh]);
    assert_px(&img, 32, 8, [0, 255, 0, 255], 1);
    // An instance resolved against the dropped registration draws nothing.
    let img = render(&gpu, &mut comp, [9, 9, 9, 255], &[stale]);
    assert_px(&img, 32, 8, [9, 9, 9, 255], 1);
}

#[test]
fn a_refused_duplicate_leaves_the_first_registration_drawing() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _first = effect(
        "test.gpu.duplicate",
        vec![param("amount", EffectParamKind::Scalar)],
        &[INVERT],
    );
    let error = register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "test.gpu.duplicate".into(),
            category: String::new(),
            label: "Again".into(),
            params: Vec::new(),
            generator: false,
            stateful: false,
            ..Default::default()
        },
        passes: vec![
            "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.0, 1.0);
}
"
            .into(),
        ],
    })
    .expect_err("the id is taken");
    assert_eq!(error.id, "test.gpu.duplicate");
    assert_eq!(error.pass, None);

    let img = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.gpu.duplicate", vec![1.0])],
    );
    assert_px(&img, 32, 8, [55, 155, 205, 255], 2);
}

#[test]
fn a_pass_the_device_refuses_records_the_failure_and_draws_nothing() {
    let gpu = gpu_or_skip!();
    if gpu.device.features().contains(wgpu::Features::SHADER_F64) {
        eprintln!("skipping: the device accepts f64, so the shader is not refused");
        return;
    }
    let mut comp = Compositor::new(&gpu);
    let fx = effect(
        "test.gpu.refused",
        Vec::new(),
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let wide: f64 = f64(0.5);
    return vec4<f32>(f32(wide), 0.0, 0.0, 1.0);
}
"],
    );
    assert_eq!(fx.pass().failure(), None);
    let img = render(
        &gpu,
        &mut comp,
        [9, 9, 9, 255],
        &[instance("test.gpu.refused", Vec::new())],
    );
    assert_px(&img, 32, 8, [9, 9, 9, 255], 1);
    let failure = fx.pass().failure().expect("the refusal is recorded");
    assert!(failure.starts_with("pass 0:"), "{failure}");
    // The refusal is cached: a second frame neither panics nor rebuilds.
    let img = render(
        &gpu,
        &mut comp,
        [9, 9, 9, 255],
        &[instance("test.gpu.refused", Vec::new())],
    );
    assert_px(&img, 32, 8, [9, 9, 9, 255], 1);
}

fn blend(gpu: &GpuContext, comp: &mut Compositor, id: &str, progress: f32) -> RgbaImage {
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let outgoing = solid([255, 0, 0, 255]);
    let incoming = solid([0, 0, 255, 255]);
    let out_layer = CompositeLayer::rgba(&outgoing, placement);
    let in_layer = CompositeLayer::rgba(&incoming, placement);
    let items = [CompositorLayer::Transition {
        outgoing: &out_layer,
        incoming: &in_layer,
        transition_id: id,
        progress,
    }];
    comp.render_compositor_layers(gpu, &config, &items)
        .expect(id)
}

const SPLIT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    let edge = aria.progress * aria.size.x * aria.texel.x;
    return select(b, a, in.uv.x < edge);
}
";

#[test]
fn a_registered_transition_draws_at_its_progress() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _tr = transition("test.gpu.split", SPLIT);
    assert!(renders_transition("test.gpu.split"));
    assert!(aria_models::transition_spec("test.gpu.split").is_some());

    let quarter = blend(&gpu, &mut comp, "test.gpu.split", 0.25);
    assert_px(&quarter, 8, 8, [255, 0, 0, 255], 1);
    assert_px(&quarter, 24, 8, [0, 0, 255, 255], 1);
    let most = blend(&gpu, &mut comp, "test.gpu.split", 0.75);
    assert_px(&most, 40, 8, [255, 0, 0, 255], 1);
    assert_px(&most, 56, 8, [0, 0, 255, 255], 1);
}

#[test]
fn a_dropped_transition_crossfades() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let tr = transition("test.gpu.gone", SPLIT);
    let drawn = blend(&gpu, &mut comp, "test.gpu.gone", 0.5);
    assert_px(&drawn, 8, 8, [255, 0, 0, 255], 1);
    drop(tr);

    assert!(!renders_transition("test.gpu.gone"));
    assert!(aria_models::transition_spec("test.gpu.gone").is_none());
    let faded = blend(&gpu, &mut comp, "test.gpu.gone", 0.5);
    assert_px(&faded, 8, 8, [128, 0, 128, 255], 3);
}

#[test]
fn a_registered_transition_may_not_shadow_a_shipped_id() {
    let error = register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: "crossfade".into(),
            label: "Mine".into(),
            ..Default::default()
        },
        shader: SPLIT.into(),
    })
    .expect_err("a shipped id is not namespaced");
    assert_eq!(error.id, "crossfade");
    assert_eq!(error.pass, None);
}

#[test]
fn cpu_invert_effect_produces_exact_inverted_pixels() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = register_effect(EffectDefinition::cpu(
        EffectSpec {
            id: "test.cpu.invert".into(),
            category: String::new(),
            label: "CPU Invert".into(),
            params: vec![param("amount", EffectParamKind::Scalar)],
            generator: false,
            stateful: false,
            ..Default::default()
        },
        |input: CpuFrameIn<'_>,
         output: &mut CpuFrameOut<'_>,
         params: &EffectParams<'_>,
         _time: f32| {
            let amount = params.scalar("amount").unwrap_or(1.0);
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let [r, g, b, a] = input.pixel_f32(x, y);
                    output.set_pixel_f32(
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
        },
    ))
    .expect("registers cpu invert");
    assert!(renders_effect("test.cpu.invert"));

    let inverted = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.cpu.invert", vec![1.0])],
    );
    assert_px(&inverted, 0, 0, [55, 155, 205, 255], 0);
    assert_px(&inverted, 32, 8, [55, 155, 205, 255], 0);
    assert_px(&inverted, 63, 15, [55, 155, 205, 255], 0);

    let untouched = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.cpu.invert", vec![0.0])],
    );
    assert_px(&untouched, 0, 0, [200, 100, 50, 255], 0);
    assert_px(&untouched, 32, 8, [200, 100, 50, 255], 0);
}

#[test]
fn cpu_effect_between_two_wgsl_passes_composes_in_order() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _first = effect(
        "test.cpu.wgsl_first",
        Vec::new(),
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(c.r + (50.0 / 255.0), 100.0 / 255.0, c.b, c.a);
}
"],
    );
    let _mid = register_cpu_effect(
        EffectSpec {
            id: "test.cpu.mid_swap".into(),
            category: String::new(),
            label: "Mid Swap".into(),
            params: Vec::new(),
            generator: false,
            stateful: false,
            ..Default::default()
        },
        |input: CpuFrameIn<'_>,
         output: &mut CpuFrameOut<'_>,
         _params: &EffectParams<'_>,
         _time: f32| {
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let [r, g, b, a] = input.pixel_f32(x, y);
                    output.set_pixel_f32(x, y, [b, 1.0 - g, r, a]);
                }
            }
            Ok(())
        },
        None,
    )
    .expect("registers cpu mid");
    let _last = effect(
        "test.cpu.wgsl_last",
        Vec::new(),
        &["@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(c.r + (30.0 / 255.0), c.g, c.b, c.a);
}
"],
    );

    // Start with [20, 0, 80, 255]:
    // 1. wgsl_first -> [70, 100, 80, 255]
    // 2. cpu mid_swap -> [80, 155, 70, 255]
    // 3. wgsl_last -> [110, 155, 70, 255]
    let chain = [
        instance("test.cpu.wgsl_first", Vec::new()),
        instance("test.cpu.mid_swap", Vec::new()),
        instance("test.cpu.wgsl_last", Vec::new()),
    ];
    let img = render(&gpu, &mut comp, [20, 0, 80, 255], &chain);
    assert_px(&img, 16, 4, [110, 155, 70, 255], 1);
    assert_px(&img, 48, 12, [110, 155, 70, 255], 1);
}

#[test]
fn cpu_generator_draws_its_pattern_on_a_clip_with_no_media() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _gen = register_effect(CpuEffectDefinition::new(
        EffectSpec {
            id: "test.cpu.split_gen".into(),
            category: String::new(),
            label: "Split Generator".into(),
            params: vec![
                param("left", EffectParamKind::Color),
                param("right", EffectParamKind::Color),
            ],
            generator: true,
            stateful: false,
            ..Default::default()
        },
        |input: CpuFrameIn<'_>,
         output: &mut CpuFrameOut<'_>,
         params: &EffectParams<'_>,
         _time: f32| {
            let left = params.color("left").unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let right = params.color("right").unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let half = input.width() / 2;
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let color = if x < half { left } else { right };
                    output.set_pixel_f32(x, y, color);
                }
            }
            Ok(())
        },
    ))
    .expect("registers cpu generator");
    assert_eq!(
        aria_models::effect_spec("test.cpu.split_gen").map(|s| s.generator),
        Some(true)
    );

    let config = CompositorConfig::new(W, H).with_background([9, 9, 9, 255]);
    let effects = [instance(
        "test.cpu.split_gen",
        vec![
            10.0 / 255.0,
            200.0 / 255.0,
            90.0 / 255.0,
            1.0,
            240.0 / 255.0,
            40.0 / 255.0,
            120.0 / 255.0,
            1.0,
        ],
    )];
    // A generator clip is placed as a transparent solid clip with no media.
    let layer = CompositeLayer::solid([0, 0, 0, 0], LayerPlacement::full_canvas(&config))
        .with_effects(&effects);
    let img = comp
        .render(&gpu, &config, &[layer])
        .expect("renders generator clip");
    assert_px(&img, 8, 8, [10, 200, 90, 255], 0);
    assert_px(&img, 48, 8, [240, 40, 120, 255], 0);
}

#[test]
fn cpu_effect_returning_err_passes_frame_through_and_reports_fault() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let should_fail = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&should_fail);
    let faults: Arc<std::sync::Mutex<Vec<EffectFault>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&faults);

    let _fx = register_effect_with_fault_sink(
        EffectDefinition::cpu(
            EffectSpec {
                id: "test.cpu.fallible".into(),
                category: String::new(),
                label: "Fallible".into(),
                params: Vec::new(),
                generator: false,
                stateful: false,
                ..Default::default()
            },
            move |input: CpuFrameIn<'_>,
                  output: &mut CpuFrameOut<'_>,
                  _params: &EffectParams<'_>,
                  _time: f32| {
                if flag.load(Ordering::Acquire) {
                    return Err("transient decode hiccup".into());
                }
                for y in 0..input.height() {
                    for x in 0..input.width() {
                        output.set_pixel_f32(x, y, [0.0, 1.0, 0.0, 1.0]);
                    }
                }
                Ok(())
            },
        ),
        Some(Arc::new(move |fault: &EffectFault| {
            recorded.lock().expect("faults").push(fault.clone());
        })),
    )
    .expect("registers fallible cpu effect");

    let passthrough = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.cpu.fallible", Vec::new())],
    );
    assert_px(&passthrough, 32, 8, [200, 100, 50, 255], 0);
    assert_eq!(
        *faults.lock().expect("faults"),
        vec![EffectFault {
            effect_id: "test.cpu.fallible".into(),
            kind: EffectFaultKind::Failed("transient decode hiccup".into()),
        }]
    );

    // Subsequent frames still execute the effect once it succeeds.
    should_fail.store(false, Ordering::Release);
    let recovered = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        &[instance("test.cpu.fallible", Vec::new())],
    );
    assert_px(&recovered, 32, 8, [0, 255, 0, 255], 0);
}

#[test]
fn cpu_effect_panic_is_caught_records_failure_and_keeps_chain_rendering() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let faults: Arc<std::sync::Mutex<Vec<EffectFault>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&faults);

    let fx = register_effect_with_fault_sink(
        EffectDefinition::cpu(
            EffectSpec {
                id: "test.cpu.panics".into(),
                category: String::new(),
                label: "Panics".into(),
                params: Vec::new(),
                generator: false,
                stateful: false,
                ..Default::default()
            },
            |_input: CpuFrameIn<'_>,
             _output: &mut CpuFrameOut<'_>,
             _params: &EffectParams<'_>,
             _time: f32|
             -> Result<(), String> {
                panic!("out of bounds in cpu effect");
            },
        ),
        Some(Arc::new(move |fault: &EffectFault| {
            recorded.lock().expect("faults").push(fault.clone());
        })),
    )
    .expect("registers panicking cpu effect");
    let _after = effect("test.cpu.after_panic", Vec::new(), &[ADD_QUARTER_RED]);

    let chain = [
        instance("test.cpu.panics", Vec::new()),
        instance("test.cpu.after_panic", Vec::new()),
    ];
    let img = render(&gpu, &mut comp, [0, 100, 50, 255], &chain);
    // The panicking CPU effect passes [0, 100, 50, 255] through to `after_panic`,
    // which adds 0.25 (64) to red and sets blue to 1 * 0.0625 (16).
    assert_px(&img, 32, 8, [64, 100, 16, 255], 2);
    assert_eq!(
        fx.pass().failure().as_deref(),
        Some("panic: out of bounds in cpu effect")
    );
    assert_eq!(
        *faults.lock().expect("faults"),
        vec![EffectFault {
            effect_id: "test.cpu.panics".into(),
            kind: EffectFaultKind::Panicked("out of bounds in cpu effect".into()),
        }]
    );
}
#[test]
fn registered_pass_debug_is_deterministic_across_serials_and_tracks_shader_and_stateful() {
    let first = effect("test.debug.det", Vec::new(), &[ADD_QUARTER_RED]);
    let first_dbg = format!("{:?}", first.pass());
    let first_digest = first.pass().digest();
    assert!(!first.pass().is_stateful());
    drop(first);

    let second = effect("test.debug.det", Vec::new(), &[ADD_QUARTER_RED]);
    assert_eq!(second.pass().digest(), first_digest);
    assert_eq!(format!("{:?}", second.pass()), first_dbg);
    drop(second);

    let altered_shader = effect(
        "test.debug.det",
        Vec::new(),
        &[
            "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return textureSample(input_tex, aria_sampler, in.uv);\n}\n",
        ],
    );
    assert_ne!(format!("{:?}", altered_shader.pass()), first_dbg);
    drop(altered_shader);

    let stateful = register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "test.debug.det".into(),
            category: String::new(),
            label: "Stateful".into(),
            params: Vec::new(),
            generator: false,
            stateful: true,
            ..Default::default()
        },
        passes: vec![ADD_QUARTER_RED.into()],
    })
    .expect("registers stateful effect");
    assert!(stateful.pass().is_stateful());
    assert_ne!(format!("{:?}", stateful.pass()), first_dbg);
}

#[test]
fn effect_with_more_than_256_floats_renders_storage_buffer_parameters() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // 70 Color parameters = 280 floats, past the old 256-float uniform cap.
    let count = 70usize;
    let params: Vec<EffectParamSpec> = (0..count)
        .map(|i| param(&format!("c{i}"), EffectParamKind::Color))
        .collect();
    let shader = format!(
        "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {{\n    _ = in.uv;\n    return param_c0() + param_c{}();\n}}\n",
        count - 1
    );
    let _fx = effect("test.gpu.wide_storage", params, &[&shader]);

    let mut values = vec![0.0f32; count * 4];
    // c0 = [100/255, 40/255, 0, 0.5]
    values[0] = 100.0 / 255.0;
    values[1] = 40.0 / 255.0;
    values[2] = 0.0;
    values[3] = 0.5;
    // c69 (offset 276..280) = [50/255, 160/255, 220/255, 0.5]
    let tail = (count - 1) * 4;
    values[tail] = 50.0 / 255.0;
    values[tail + 1] = 160.0 / 255.0;
    values[tail + 2] = 220.0 / 255.0;
    values[tail + 3] = 0.5;

    let img = render(
        &gpu,
        &mut comp,
        [0, 0, 0, 255],
        &[instance("test.gpu.wide_storage", values)],
    );
    assert_px(&img, 32, 8, [150, 200, 220, 255], 1);
}

#[test]
fn transition_parameters_change_rendered_blend_on_registered_and_shipped_passes() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _tr = register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: "test.gpu.tinted_blend".into(),
            label: "Tinted Blend".into(),
            params: vec![
                param("weight", EffectParamKind::Scalar),
                param("tint", EffectParamKind::Color),
            ],
        },
        shader: "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    let base = mix(a, b, aria.progress);
    return mix(base, param_tint(), param_weight());
}
"
        .into(),
    })
    .expect("registers parameterized transition");

    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let outgoing = solid([255, 0, 0, 255]);
    let incoming = solid([0, 0, 255, 255]);
    let out_layer = CompositeLayer::rgba(&outgoing, placement);
    let in_layer = CompositeLayer::rgba(&incoming, placement);

    let untinted = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::TransitionWithParams {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "test.gpu.tinted_blend",
                progress: 0.5,
                params: &[0.0, 0.0, 1.0, 0.0, 1.0],
            }],
        )
        .expect("untinted transition");
    assert_px(&untinted, 32, 8, [128, 0, 128, 255], 2);

    let green_tinted = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::TransitionWithParams {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "test.gpu.tinted_blend",
                progress: 0.5,
                params: &[1.0, 0.0, 1.0, 0.0, 1.0],
            }],
        )
        .expect("green tinted transition");
    assert_px(&green_tinted, 32, 8, [0, 255, 0, 255], 1);

    // Shipped wipe_left with feather = 0.0 vs feather = 0.5 at progress = 0.5:
    // at x = 20 (uv.x = 20.5/64 ≈ 0.32), hard wipe is pure incoming ([0, 0, 255, 255])
    // while feather = 0.5 blends outgoing and incoming across the edge.
    let hard_wipe = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::TransitionWithParams {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "wipe_left",
                progress: 0.5,
                params: &[0.0],
            }],
        )
        .expect("hard wipe");
    let soft_wipe = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::TransitionWithParams {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "wipe_left",
                progress: 0.5,
                params: &[0.5],
            }],
        )
        .expect("soft wipe");
    assert_px(&hard_wipe, 20, 8, [0, 0, 255, 255], 1);
    let soft_px = soft_wipe.pixel(20, 8);
    assert!(
        soft_px[0] > 20 && soft_px[2] < 235,
        "feathered wipe_left at x=20 must blend outgoing red and incoming blue, got {soft_px:?}"
    );
}
