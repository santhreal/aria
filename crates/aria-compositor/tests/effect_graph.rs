//! Effect graphs drawn on a GPU: compute and fragment passes, declared output
//! sizes, plugin textures, layer references, and pipelines built once.
//!
//! Its own test binary: registration adds to process-wide catalogs that the
//! library tests iterate. Every test registers ids no other test uses.
//!
//! Catches: a compute pass that is not dispatched or whose result is not
//! copied into the frame, an output size the runtime ignores, a pass reading
//! the wrong input, a texture binding that reads nothing, a layer input that
//! does not read the referenced clip, a cycle that registers, and a pipeline
//! rebuilt every frame. Graph planning without a device is in the
//! `aria_models::effect_graph` tests.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, EffectDefinition, GpuContext,
    LayerPlacement, PassId, PassInstance, PassRegistration, RegisterError, RgbaImage, pass_id,
    register_effect, register_texture, renders_effect,
};
use aria_models::{
    EffectInstance, EffectParamKind, EffectParamSpec, EffectSpec, GraphPass, OutputSize, PassInput,
};

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping effect graph test: no GPU adapter ({e})");
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

fn try_graph(
    id: &str,
    params: Vec<EffectParamSpec>,
    passes: Vec<GraphPass>,
) -> Result<PassRegistration, RegisterError> {
    register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: format!("{id} label"),
            params,
            ..Default::default()
        },
        passes,
    })
}

fn graph(id: &str, params: Vec<EffectParamSpec>, passes: Vec<GraphPass>) -> PassRegistration {
    try_graph(id, params, passes).unwrap_or_else(|error| panic!("{error}"))
}

fn instance(id: &str) -> PassInstance {
    let id = pass_id(id).unwrap_or_else(|| panic!("`{id}` is renderable"));
    assert!(matches!(id, PassId::Registered(_)));
    PassInstance::new(id, Vec::new())
}

fn config() -> CompositorConfig {
    CompositorConfig::new(W, H).with_background([0, 0, 0, 255])
}

fn render(gpu: &GpuContext, comp: &mut Compositor, plate: [u8; 4], effect: &str) -> RgbaImage {
    let config = config();
    let bmp = solid(plate);
    let effects = [instance(effect)];
    let layer =
        CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    comp.render(gpu, &config, &[layer]).expect("renders")
}

/// Halves the colour of `input_tex` into `output_tex`, skipping texels outside
/// the pass output.
const HALVE_COMPUTE: &str = "@compute @workgroup_size(8, 8, 1)
fn cs(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(aria.size);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let c = textureLoad(input_tex, vec2<i32>(id.xy), 0);
    textureStore(output_tex, vec2<i32>(id.xy), vec4<f32>(c.rgb * 0.5, c.a));
}
";

#[test]
fn a_compute_pass_feeds_a_fragment_pass_that_also_reads_the_source() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.compute_then_fragment",
        Vec::new(),
        vec![
            GraphPass::compute(HALVE_COMPUTE, [8, 8]).named("half"),
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let h = textureSample(pass_half, aria_sampler, in.uv);
    let s = textureSample(source_tex, aria_sampler, in.uv);
    return vec4<f32>(h.r, s.g, h.b, 1.0);
}
",
            )
            .reading([PassInput::pass("half")]),
        ],
    );
    let out = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        "test.graph.compute_then_fragment",
    );
    // Red and blue from the halved pass, green from the source.
    for (x, y) in [(0, 0), (32, 8), (W - 1, H - 1)] {
        assert_px(&out, x, y, [100, 100, 25, 255], 2);
    }
}

#[test]
fn a_compute_pass_last_writes_the_effect_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.compute_last",
        Vec::new(),
        vec![GraphPass::compute(HALVE_COMPUTE, [8, 8])],
    );
    let out = render(
        &gpu,
        &mut comp,
        [200, 100, 50, 255],
        "test.graph.compute_last",
    );
    // Every corner lies in a different 8×8 workgroup, so a dispatch short of
    // the output, or a result not copied into the frame, shows at one of them.
    for (x, y) in [(0, 0), (32, 8), (W - 1, 0), (0, H - 1), (W - 1, H - 1)] {
        assert_px(&out, x, y, [100, 50, 25, 255], 2);
    }
}

#[test]
fn passes_write_their_declared_output_sizes() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.sizes",
        Vec::new(),
        vec![
            // A quarter of 64×16 is 16×4; the pass records the size it sees.
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(aria.size.x / 255.0, aria.size.y / 255.0, 0.0, 1.0);
}
",
            )
            .named("quarter")
            .sized(OutputSize::Scale([0.25, 0.25])),
            GraphPass::fragment(
                "@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
",
            )
            .named("fixed")
            .sized(OutputSize::Fixed([3, 5])),
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let dq = textureDimensions(pass_quarter);
    let df = textureDimensions(pass_fixed);
    let seen = textureLoad(pass_quarter, vec2<i32>(0, 0), 0);
    return vec4<f32>(
        f32(dq.x * 10u + dq.y) / 255.0,
        f32(df.x * 10u + df.y) / 255.0,
        seen.r,
        1.0,
    );
}
",
            )
            .reading([PassInput::pass("quarter"), PassInput::pass("fixed")]),
        ],
    );
    let out = render(&gpu, &mut comp, [9, 9, 9, 255], "test.graph.sizes");
    // 16×4 → 164, 3×5 → 35, and the quarter pass saw `aria.size.x` = 16.
    assert_px(&out, 32, 8, [164, 35, 16, 255], 1);
}

#[test]
fn a_texture_input_reads_the_registered_image_premultiplied() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let texture = register_texture(
        "test.graph.tex-red",
        &RgbaImage::new(1, 1, vec![255, 0, 0, 128]),
    )
    .expect("register texture");
    let _fx = graph(
        "test.graph.texture",
        Vec::new(),
        vec![
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(texture_test_graph_tex_red, aria_sampler, in.uv);
    return vec4<f32>(t.rgb, 1.0);
}
",
            )
            .reading([PassInput::Source, PassInput::texture("test.graph.tex-red")]),
        ],
    );
    let out = render(&gpu, &mut comp, [0, 200, 0, 255], "test.graph.texture");
    assert_px(&out, 32, 8, [128, 0, 0, 255], 2);

    // Dropping the texture removes the id for later effects; the effect
    // already registered keeps the image it was given.
    drop(texture);
    let refused = try_graph(
        "test.graph.texture_after_drop",
        Vec::new(),
        vec![
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(texture_test_graph_tex_red, aria_sampler, in.uv);
}
",
            )
            .reading([PassInput::texture("test.graph.tex-red")]),
        ],
    )
    .expect_err("a dropped texture id is not registered");
    assert!(refused.message.contains("not registered"), "{refused}");
    let again = render(&gpu, &mut comp, [0, 200, 0, 255], "test.graph.texture");
    assert_px(&again, 32, 8, [128, 0, 0, 255], 2);
}

#[test]
fn a_layer_input_reads_the_referenced_clip() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.layer",
        vec![EffectParamSpec {
            name: "matte".into(),
            label: "Matte".into(),
            kind: EffectParamKind::Layer,
            ..Default::default()
        }],
        vec![
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let l = textureSample(layer_matte, aria_sampler, in.uv);
    return vec4<f32>(l.rgb, 1.0);
}
",
            )
            .reading([PassInput::Source, PassInput::layer("matte")]),
        ],
    );
    let config = config();
    let referenced = solid([10, 200, 30, 255]);
    let plate = solid([200, 0, 200, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let referenced_layer = CompositeLayer::rgba(&referenced, placement);
    let referenced_item = CompositorLayer::Layer(&referenced_layer);

    // A layer parameter holds one float slot in the flattened parameters, as
    // `aria_models::default_flattened_param_floats` writes it; the clip id it
    // names travels in `layer_params`.
    let reading = |clip: u64| {
        let mut state = EffectInstance::new("test.graph.layer");
        state.layer_params.insert("matte".into(), clip);
        let id = pass_id("test.graph.layer").expect("registered");
        [PassInstance::new(id, vec![0.0]).with_instance(state)]
    };
    let at_seven = reading(7);
    let reader = CompositeLayer::rgba(&plate, placement).with_effects(&at_seven);
    let out = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::Reference {
                    clip: 7,
                    item: &referenced_item,
                },
                CompositorLayer::Layer(&reader),
            ],
        )
        .expect("renders");
    assert_px(&out, 32, 8, [10, 200, 30, 255], 2);

    // A parameter naming a clip no reference draws reads transparent.
    let at_eight = reading(8);
    let reader = CompositeLayer::rgba(&plate, placement).with_effects(&at_eight);
    let out = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::Reference {
                    clip: 7,
                    item: &referenced_item,
                },
                CompositorLayer::Layer(&reader),
            ],
        )
        .expect("renders");
    assert_px(&out, 32, 8, [0, 0, 0, 255], 2);
}

/// Two nests drawing the same clip as a reference each read the frame they
/// drew: a reference inside a nest clears its target before it draws, so the
/// second nest does not read what the first one left there.
#[test]
fn a_nest_reads_the_reference_it_drew_not_the_one_before() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.nest_layer",
        vec![EffectParamSpec {
            name: "matte".into(),
            label: "Matte".into(),
            kind: EffectParamKind::Layer,
            ..Default::default()
        }],
        vec![
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let l = textureSample(layer_matte, aria_sampler, in.uv);
    return vec4<f32>(l.rgb, 1.0);
}
",
            )
            .reading([PassInput::Source, PassInput::layer("matte")]),
        ],
    );
    let config = config();
    let full = LayerPlacement::full_canvas(&config);
    let green = solid([10, 200, 30, 255]);
    let blue = solid([20, 40, 220, 255]);
    let plate = solid([200, 0, 200, 255]);
    // The first nest's reference covers its canvas; the second one's covers
    // the left half of it.
    let first_reference = CompositeLayer::rgba(&green, full);
    let second_reference = CompositeLayer::rgba(
        &blue,
        LayerPlacement {
            center: [W as f32 / 4.0, H as f32 / 2.0],
            size: [W as f32 / 2.0, H as f32],
            rotation: 0.0,
            opacity: 1.0,
        },
    );
    let mut state = EffectInstance::new("test.graph.nest_layer");
    state.layer_params.insert("matte".into(), 7);
    let id = pass_id("test.graph.nest_layer").expect("registered");
    let reading = [PassInstance::new(id, vec![0.0]).with_instance(state)];
    let reader = CompositeLayer::rgba(&plate, full).with_effects(&reading);
    let first = [
        CompositorLayer::Reference {
            clip: 7,
            item: &CompositorLayer::Layer(&first_reference),
        },
        CompositorLayer::Layer(&reader),
    ];
    let second = [
        CompositorLayer::Reference {
            clip: 7,
            item: &CompositorLayer::Layer(&second_reference),
        },
        CompositorLayer::Layer(&reader),
    ];
    let half = |x: f32| LayerPlacement {
        center: [x + W as f32 / 4.0, H as f32 / 2.0],
        size: [W as f32 / 2.0, H as f32],
        rotation: 0.0,
        opacity: 1.0,
    };
    let first_shown = CompositeLayer::nest(1, half(0.0));
    let second_shown = CompositeLayer::nest(2, half(W as f32 / 2.0));
    let out = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[
                CompositorLayer::Nest {
                    nest: 1,
                    items: &first,
                },
                CompositorLayer::Nest {
                    nest: 2,
                    items: &second,
                },
                CompositorLayer::Layer(&first_shown),
                CompositorLayer::Layer(&second_shown),
            ],
        )
        .expect("renders");
    assert_px(&out, 16, 8, [10, 200, 30, 255], 2);
    // The second nest's picture, squeezed into the right half: its left half
    // reads the blue reference, its right half reads the cleared target.
    assert_px(&out, 40, 8, [20, 40, 220, 255], 2);
    assert_px(&out, 56, 8, [0, 0, 0, 255], 2);
}

#[test]
fn a_cycle_between_passes_is_refused_naming_a_pass_on_it() {
    let error = try_graph(
        "test.graph.cycle",
        Vec::new(),
        vec![
            GraphPass::fragment(
                "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
            )
            .named("a")
            .reading([PassInput::pass("b")]),
            GraphPass::fragment(
                "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
            )
            .named("b")
            .reading([PassInput::pass("a")]),
            GraphPass::fragment(
                "@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
            )
            .named("out")
            .reading([PassInput::pass("a")]),
        ],
    )
    .expect_err("a cycle registered");
    assert_eq!(error.pass, Some(0), "{error}");
    assert!(error.message.contains("cycle"), "{error}");
    assert!(!renders_effect("test.graph.cycle"));
}

#[test]
fn graph_pipelines_build_once_across_frames() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let _fx = graph(
        "test.graph.builds",
        Vec::new(),
        vec![
            GraphPass::compute(HALVE_COMPUTE, [8, 8]).named("half"),
            GraphPass::fragment(
                "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv);
}
",
            )
            .reading([PassInput::pass("half")]),
        ],
    );
    assert_eq!(comp.registered_pipeline_builds(), 0);
    for frame in 0..3 {
        let out = render(&gpu, &mut comp, [200, 100, 50, 255], "test.graph.builds");
        assert_px(&out, 32, 8, [100, 50, 25, 255], 2);
        assert_eq!(
            comp.registered_pipeline_builds(),
            2,
            "frame {frame} rebuilt a pipeline"
        );
    }
}
