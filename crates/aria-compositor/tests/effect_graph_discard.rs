//! Registered fragment passes and transitions that `discard`.
//!
//! A registered fragment pass draws without blending. A pass whose shader can
//! `discard` clears its target before the draw; any other pass writes every
//! pixel and neither loads nor clears its target.
//!
//! Closes: a pass that can `discard` drawn over a target it does not clear,
//! which shows an earlier frame where the shader discards. Each test draws a
//! frame that discards nothing, then a frame that discards part of the target,
//! through each target a registered pass writes: the target an effect's
//! intermediate pass keeps across frames, the pool slot an effect's last pass
//! writes, and the slot a transition writes. Which shaders can `discard` is
//! covered by the library tests.
//!
//! Not caught: a backend that zeroes a target it does not load, where a missed
//! `discard` reads as transparent anyway.

use aria_compositor::{TransitionDefinition, register_transition};
use aria_models::TransitionSpec;

use super::*;

const BLACK: [u8; 4] = [0, 0, 0, 255];

/// Discards left of `edge`; the effect's first pass, which writes a target
/// kept across frames.
const CUT_LEFT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    if in.uv.x < param_edge() {
        discard;
    }
    return textureSample(input_tex, aria_sampler, in.uv);
}
";

/// Discards right of `1 - edge`; the effect's last pass, which writes a pool
/// slot.
const CUT_RIGHT: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    if in.uv.x > 1.0 - param_edge() {
        discard;
    }
    return textureSample(input_tex, aria_sampler, in.uv);
}
";

#[test]
fn an_effect_pass_that_discards_leaves_transparent_not_an_earlier_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let id = "test.graph.discard";
    let edge = EffectParamSpec {
        name: "edge".into(),
        label: "Edge".into(),
        kind: EffectParamKind::Scalar,
        min: 0.0,
        max: 1.0,
        ..Default::default()
    };
    let _fx = graph(
        id,
        vec![edge],
        vec![
            GraphPass::fragment(CUT_LEFT).named("cut"),
            GraphPass::fragment(CUT_RIGHT).reading([PassInput::pass("cut")]),
        ],
    );
    let pass = instance(id).id;
    let mut draw = |plate: [u8; 4], edge: f32| {
        let config = config();
        let image = solid(plate);
        let effects = [PassInstance::new(pass.clone(), vec![edge])];
        let layer = CompositeLayer::rgba(&image, LayerPlacement::full_canvas(&config))
            .with_effects(&effects);
        comp.render(&gpu, &config, &[layer]).expect("renders")
    };

    let whole = draw([200, 0, 0, 255], 0.0);
    for x in [4, 32, 60] {
        assert_px(&whole, x, 8, [200, 0, 0, 255], 1);
    }
    let cut = draw([0, 200, 0, 255], 0.25);
    assert_px(&cut, 4, 8, BLACK, 1);
    assert_px(&cut, 32, 8, [0, 200, 0, 255], 1);
    assert_px(&cut, 60, 8, BLACK, 1);
}

/// Discards left of the progress; elsewhere draws the incoming side.
const WIPE_DISCARDING: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    if in.uv.x < aria.progress {
        discard;
    }
    return textureSample(incoming_tex, aria_sampler, in.uv);
}
";

#[test]
fn a_transition_that_discards_leaves_transparent_not_an_earlier_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let id = "test.graph.discard_transition";
    let _tr = register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: id.into(),
            label: format!("{id} label"),
            ..Default::default()
        },
        shader: WIPE_DISCARDING.into(),
    })
    .unwrap_or_else(|error| panic!("{error}"));
    let mut draw = |progress: f32| {
        let config = config();
        let placement = LayerPlacement::full_canvas(&config);
        let outgoing = solid([200, 0, 0, 255]);
        let incoming = solid([0, 0, 200, 255]);
        let out_layer = CompositeLayer::rgba(&outgoing, placement);
        let in_layer = CompositeLayer::rgba(&incoming, placement);
        let items = [CompositorLayer::Transition {
            outgoing: &out_layer,
            incoming: &in_layer,
            transition_id: id,
            progress,
        }];
        comp.render_compositor_layers(&gpu, &config, &items)
            .expect(id)
    };

    let early = draw(0.01);
    for x in [16, 48] {
        assert_px(&early, x, 8, [0, 0, 200, 255], 1);
    }
    let half = draw(0.5);
    assert_px(&half, 16, 8, BLACK, 1);
    assert_px(&half, 48, 8, [0, 0, 200, 255], 1);
}
