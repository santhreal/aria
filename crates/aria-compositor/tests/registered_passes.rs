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

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorError, CompositorLayer,
    EffectDefinition, GpuContext, LayerPlacement, PassId, PassInstance, PassRegistration,
    RgbaImage, TransitionDefinition, effect_is_noop, pass_id, register_effect, register_transition,
    renders_effect, renders_transition,
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
    }
}

fn effect(id: &str, params: Vec<EffectParamSpec>, passes: &[&str]) -> PassRegistration {
    register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: format!("{id} label"),
            params,
            generator: false,
        },
        passes: passes.iter().map(|pass| (*pass).to_owned()).collect(),
    })
    .unwrap_or_else(|error| panic!("{error}"))
}

fn transition(id: &str, shader: &str) -> PassRegistration {
    register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: id.into(),
            label: format!("{id} label"),
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
            label: "Again".into(),
            params: Vec::new(),
            generator: false,
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
        },
        shader: SPLIT.into(),
    })
    .expect_err("a shipped id is not namespaced");
    assert_eq!(error.id, "crossfade");
    assert_eq!(error.pass, None);
}
