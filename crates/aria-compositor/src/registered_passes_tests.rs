//! Shader checks and the host prelude for registered passes.
//!
//! These tests call `check_pass` and the preludes directly, and call
//! `register_effect`/`register_transition` only with definitions refused before
//! the catalog is touched: the library tests iterate the process-wide catalogs
//! while they run. Registration that succeeds, and drawing, are covered by
//! `tests/registered_passes.rs`.
//!
//! Covers the line a refusal reports, refusals inside the prelude, every
//! interface rule, the accessor component layout, and the limits on passes,
//! floats and parameter names. Does not prove a shader the device rejects.

use super::*;

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

fn effect(id: &str, params: Vec<EffectParamSpec>) -> EffectSpec {
    EffectSpec {
        id: id.into(),
        label: format!("{id} label"),
        params,
        generator: false,
    }
}

const PASSTHROUGH: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv);
}
";

fn check_effect(source: &str, params: &[EffectParamSpec]) -> Result<String, Refusal> {
    let floats = params.iter().map(|p| floats_of(p.kind)).sum();
    check_pass(source, &effect_prelude(params, floats), Kind::Effect)
}

fn refusal(source: &str) -> String {
    check_effect(source, &[])
        .expect_err("the pass is refused")
        .1
}

#[test]
fn a_passthrough_effect_and_transition_validate() {
    let composed = check_effect(PASSTHROUGH, &[]).expect("passthrough validates");
    assert!(composed.starts_with(PASSTHROUGH));
    let transition = "@fragment
fn fs(@location(0) uv: vec2<f32>, @builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, uv);
    let b = textureSample(incoming_tex, aria_sampler, uv);
    return mix(a, b, aria.progress);
}
";
    check_pass(transition, &transition_prelude(), Kind::Transition)
        .expect("a transition reading both inputs validates");
}

#[test]
fn a_parse_error_reports_the_line_of_the_plugin_source() {
    let source = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let broken = ;
    return vec4<f32>(0.0);
}
";
    let (location, message) = check_effect(source, &[]).expect_err("does not parse");
    assert_eq!(location.map(|(line, _)| line), Some(3), "{message}");
    assert!(!message.starts_with("in the host prelude"), "{message}");
}

#[test]
fn a_validation_error_reports_the_line_of_the_plugin_source() {
    let source = "fn helper() -> f32 {
    return 1.0;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let value: f32 = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(value);
}
";
    let (location, message) = check_effect(source, &[]).expect_err("does not type-check");
    assert_eq!(location.map(|(line, _)| line), Some(7), "{message}");
}

#[test]
fn an_error_past_the_plugin_lines_is_labelled_as_the_prelude() {
    let (location, message) = check_pass(PASSTHROUGH, "fn broken( {", Kind::Effect)
        .expect_err("the prelude does not parse");
    assert_eq!(location, None);
    assert!(message.starts_with("in the host prelude: "), "{message}");
}

#[test]
fn a_pass_without_fs_is_refused() {
    let message = refusal(
        "@fragment
fn main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}
",
    );
    assert!(message.contains("no `@fragment fn fs`"), "{message}");
}

#[test]
fn an_fs_that_does_not_return_a_location_zero_vec4_is_refused() {
    for source in [
        "@fragment
fn fs(in: VsOut) -> @location(0) vec3<f32> {
    return vec3<f32>(1.0);
}
",
        "@fragment
fn fs(in: VsOut) -> @location(1) vec4<f32> {
    return vec4<f32>(1.0);
}
",
        "@fragment
fn fs(in: VsOut) -> @location(0) vec4<i32> {
    return vec4<i32>(1);
}
",
    ] {
        let message = refusal(source);
        assert!(
            message.contains("must return `@location(0) vec4<f32>`"),
            "{message}"
        );
    }
}

#[test]
fn an_fs_reading_a_value_the_vertex_stage_does_not_write_is_refused() {
    for source in [
        "@fragment
fn fs(@location(1) other: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(other, 0.0, 1.0);
}
",
        "@fragment
fn fs(@location(0) uv: vec4<f32>) -> @location(0) vec4<f32> {
    return uv;
}
",
        "struct Mine {
    @location(0) uv: vec2<f32>,
    @location(2) extra: f32,
}

@fragment
fn fs(in: Mine) -> @location(0) vec4<f32> {
    return vec4<f32>(in.uv, in.extra, 1.0);
}
",
    ] {
        let message = refusal(source);
        assert!(message.contains("does not write"), "{source}: {message}");
    }
}

#[test]
fn a_binding_outside_the_host_set_is_refused() {
    let message = refusal(
        "@group(0) @binding(4) var extra_tex: texture_2d<f32>;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(extra_tex, aria_sampler, in.uv);
}
",
    );
    assert!(
        message.contains("`extra_tex` is bound at @group(0) @binding(4)"),
        "{message}"
    );
}

#[test]
fn an_immediate_is_refused() {
    let message = refusal(
        "var<immediate> k: f32;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(k);
}
",
    );
    assert!(message.contains("`k` is an immediate"), "{message}");
}

#[test]
fn a_transition_may_not_bind_the_effect_names() {
    let (_, message) = check_pass(
        "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv);
}
",
        &transition_prelude(),
        Kind::Transition,
    )
    .expect_err("a transition has no input_tex");
    assert!(message.contains("input_tex"), "{message}");
}

#[test]
fn accessors_read_consecutive_components_across_vec4_slots() {
    let params = [
        param("a", EffectParamKind::Scalar),
        param("b", EffectParamKind::Vec2),
        param("c", EffectParamKind::Color),
        param("d", EffectParamKind::Scalar),
        param("e", EffectParamKind::Vec2),
    ];
    let prelude = effect_prelude(&params, 10);
    for line in [
        "params: array<vec4<f32>, 3>,",
        "fn param_a() -> f32 { return aria.params[0].x; }",
        "fn param_b() -> vec2<f32> { return vec2<f32>(aria.params[0].y, aria.params[0].z); }",
        "fn param_c() -> vec4<f32> { return vec4<f32>(aria.params[0].w, aria.params[1].x, \
         aria.params[1].y, aria.params[1].z); }",
        "fn param_d() -> f32 { return aria.params[1].w; }",
        "fn param_e() -> vec2<f32> { return vec2<f32>(aria.params[2].x, aria.params[2].y); }",
    ] {
        assert!(prelude.contains(line), "missing `{line}` in\n{prelude}");
    }
    let source = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let s: f32 = param_a() + param_d();
    let v: vec2<f32> = param_b() + param_e();
    let c: vec4<f32> = param_c();
    return c * s + vec4<f32>(v, 0.0, 0.0);
}
";
    check_effect(source, &params).expect("every accessor has the declared type");
}

#[test]
fn an_effect_without_parameters_declares_one_vec4() {
    assert!(effect_prelude(&[], 0).contains("params: array<vec4<f32>, 1>,"));
    assert_eq!(param_vec4s(0), 1);
    assert_eq!(param_vec4s(4), 1);
    assert_eq!(param_vec4s(5), 2);
    assert_eq!(param_vec4s(MAX_EFFECT_FLOATS), MAX_EFFECT_FLOATS / 4);
}

#[test]
fn parameter_names_must_be_wgsl_identifiers() {
    for name in ["", "two words", "dash-ed", "ünicode", "semi;colon"] {
        let error = register_effect(EffectDefinition {
            spec: effect(
                "test.lib.badname",
                vec![param(name, EffectParamKind::Scalar)],
            ),
            passes: vec![PASSTHROUGH.into()],
        })
        .expect_err("a non-identifier name is refused");
        assert!(
            error.message.contains("not a WGSL identifier"),
            "{name:?}: {error}"
        );
        assert_eq!(error.pass, None);
    }
    for name in ["amount", "_x", "r2", "9lives"] {
        assert!(is_identifier(name), "{name}");
    }
}

#[test]
fn an_effect_needs_one_to_max_passes() {
    let zero = register_effect(EffectDefinition {
        spec: effect("test.lib.nopasses", Vec::new()),
        passes: Vec::new(),
    })
    .expect_err("no passes");
    assert!(zero.message.contains("at least one pass"), "{zero}");

    let over = register_effect(EffectDefinition {
        spec: effect("test.lib.manypasses", Vec::new()),
        passes: vec![PASSTHROUGH.into(); MAX_EFFECT_PASSES + 1],
    })
    .expect_err("too many passes");
    assert!(
        over.message
            .contains(&format!("at most {MAX_EFFECT_PASSES}")),
        "{over}"
    );
}

#[test]
fn parameters_may_not_flatten_past_the_float_limit() {
    let colours = MAX_EFFECT_FLOATS / 4 + 1;
    let params = (0..colours)
        .map(|i| param(&format!("c{i}"), EffectParamKind::Color))
        .collect();
    let error = register_effect(EffectDefinition {
        spec: effect("test.lib.wide", params),
        passes: vec![PASSTHROUGH.into()],
    })
    .expect_err("too many floats");
    assert!(
        error.message.contains(&format!("{} floats", colours * 4)),
        "{error}"
    );
}

#[test]
fn a_refused_pass_names_its_index_and_line() {
    let error = register_effect(EffectDefinition {
        spec: effect("test.lib.secondbad", Vec::new()),
        passes: vec![PASSTHROUGH.into(), "@fragment\nfn fs( {\n".into()],
    })
    .expect_err("the second pass does not parse");
    assert_eq!(error.pass, Some(1));
    assert_eq!(error.location.map(|(line, _)| line), Some(2), "{error}");
    let shown = error.to_string();
    assert!(
        shown.starts_with("test.lib.secondbad pass 1 at 2:"),
        "{shown}"
    );
    assert!(registered_effect("test.lib.secondbad").is_none());
}

#[test]
fn a_refused_transition_is_not_registered() {
    let error = register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: "test.lib.badtransition".into(),
            label: "Bad".into(),
        },
        shader: "@fragment\nfn fs() -> @location(0) vec3<f32> { return vec3<f32>(0.0); }\n".into(),
    })
    .expect_err("wrong return type");
    assert_eq!(error.pass, Some(0));
    assert!(registered_transition("test.lib.badtransition").is_none());
}
