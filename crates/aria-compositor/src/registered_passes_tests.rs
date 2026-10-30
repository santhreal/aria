//! Shader checks and the host prelude for registered passes.
//!
//! These tests call `check_pass` and the preludes directly, and call
//! `register_effect`/`register_transition` only with definitions refused before
//! the catalog is touched: the library tests iterate the process-wide catalogs
//! while they run. Registration that succeeds, and drawing, are covered by
//! `tests/registered_passes.rs`.
//!
//! Covers the line a refusal reports, refusals inside the prelude, every
//! interface rule, the accessor component layout, the limits on passes,
//! floats and parameter names, and which passes can `discard`. Does not prove
//! a shader the device rejects.

use super::*;
use crate::registered_graph::graph_prelude;

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

fn effect(id: &str, params: Vec<EffectParamSpec>) -> EffectSpec {
    EffectSpec {
        id: id.into(),
        category: String::new(),
        label: format!("{id} label"),
        params,
        generator: false,
        stateful: false,
        ..Default::default()
    }
}

const PASSTHROUGH: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv);
}
";

fn check_effect(source: &str, params: &[EffectParamSpec]) -> Result<Composed, Refusal> {
    check_pass(
        source,
        &graph_prelude(params, &[], Entry::Fragment),
        Kind::Effect,
    )
}

fn refusal(source: &str) -> String {
    check_effect(source, &[])
        .expect_err("the pass is refused")
        .1
}

#[test]
fn a_passthrough_effect_and_transition_validate() {
    let composed = check_effect(PASSTHROUGH, &[]).expect("passthrough validates");
    assert!(composed.source.starts_with(PASSTHROUGH));
    let transition = "@fragment
fn fs(@location(0) uv: vec2<f32>, @builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, uv);
    let b = textureSample(incoming_tex, aria_sampler, uv);
    return mix(a, b, aria.progress);
}
";
    check_pass(transition, &transition_prelude(&[]), Kind::Transition)
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
        &transition_prelude(&[]),
        Kind::Transition,
    )
    .expect_err("a transition has no input_tex");
    assert!(message.contains("input_tex"), "{message}");
}

#[test]
fn accessors_read_consecutive_elements_from_storage_buffer() {
    let params = [
        param("a", EffectParamKind::Scalar),
        param("b", EffectParamKind::Vec2),
        param("c", EffectParamKind::Color),
        param("d", EffectParamKind::Scalar),
        param("e", EffectParamKind::Vec2),
        param("f", EffectParamKind::Int),
        param("g", EffectParamKind::Bool),
        param("h", EffectParamKind::Choice),
        param("p3", EffectParamKind::Point3),
        param("sz", EffectParamKind::Size),
        param("ang", EffectParamKind::Angle),
        param("crv", EffectParamKind::Curve),
        param("grd", EffectParamKind::Gradient),
        param("txt", EffectParamKind::Text),
    ];
    let prelude = graph_prelude(&params, &[], Entry::Fragment);
    for line in [
        "params: array<f32>,",
        "@group(0) @binding(3) var<storage, read> aria: AriaPass;",
        "fn param_a() -> f32 { return aria.params[0]; }",
        "fn param_b() -> vec2<f32> { return vec2<f32>(aria.params[1], aria.params[2]); }",
        "fn param_c() -> vec4<f32> { return vec4<f32>(aria.params[3], aria.params[4], \
         aria.params[5], aria.params[6]); }",
        "fn param_d() -> f32 { return aria.params[7]; }",
        "fn param_e() -> vec2<f32> { return vec2<f32>(aria.params[8], aria.params[9]); }",
        "fn param_f() -> i32 { return i32(round(aria.params[10])); }",
        "fn param_g() -> bool { return aria.params[11] >= 0.5; }",
        "fn param_h() -> u32 { return u32(max(round(aria.params[12]), 0.0)); }",
        "fn param_p3() -> vec3<f32> { return vec3<f32>(aria.params[13], aria.params[14], aria.params[15]); }",
        "fn param_sz() -> vec2<f32> { return vec2<f32>(aria.params[16], aria.params[17]); }",
        "fn param_ang() -> f32 { return aria.params[18]; }",
    ] {
        assert!(prelude.contains(line), "missing `{line}` in\n{prelude}");
    }
    let source = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let s: f32 = param_a() + param_d() + param_ang() + f32(param_f()) + f32(param_h()) + param_crv(0.5);
    let v: vec2<f32> = param_b() + param_e() + param_sz();
    let p: vec3<f32> = param_p3();
    let c: vec4<f32> = param_c() + param_grd(0.25);
    if param_g() {
        return c * s + vec4<f32>(v, p.z, 0.0);
    }
    return c;
}
";
    check_effect(source, &params).expect("every accessor has the declared type");
}

#[test]
fn an_effect_without_parameters_declares_a_storage_array() {
    let prelude = graph_prelude(&[], &[], Entry::Fragment);
    assert!(prelude.contains("params: array<f32>,"));
    assert!(prelude.contains("@group(0) @binding(3) var<storage, read> aria: AriaPass;"));
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
fn parameters_may_flatten_past_the_old_uniform_float_limit() {
    let colours = MAX_EFFECT_FLOATS / 4 + 4;
    let params: Vec<EffectParamSpec> = (0..colours)
        .map(|i| param(&format!("c{i}"), EffectParamKind::Color))
        .collect();
    let source = format!(
        "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {{\n    return param_c0() + param_c{}();\n}}\n",
        colours - 1
    );
    check_effect(&source, &params).expect("storage buffer handles >256 floats");
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
            ..Default::default()
        },
        shader: "@fragment\nfn fs() -> @location(0) vec3<f32> { return vec3<f32>(0.0); }\n".into(),
    })
    .expect_err("wrong return type");
    assert_eq!(error.pass, Some(0));
    assert!(registered_transition("test.lib.badtransition").is_none());
}

/// An effect pass whose `fs` runs `statement` and returns through a helper
/// that runs `in_helper`.
fn pass_running(in_helper: &str, statement: &str) -> String {
    format!(
        "fn helper(c: vec4<f32>) -> vec4<f32> {{
    {in_helper}
    return c;
}}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {{
    var c = textureSample(input_tex, aria_sampler, in.uv);
    {statement}
    return helper(c);
}}
"
    )
}

/// A pass marked as unable to `discard` is drawn over a target that is never
/// cleared, so a `discard` the scan misses shows an earlier frame. Each
/// statement that holds a block is a place the scan must look.
#[test]
fn a_discard_in_any_block_or_helper_marks_the_pass() {
    let discards = |source: &str| {
        check_effect(source, &[])
            .unwrap_or_else(|(_, message)| panic!("{message}\n{source}"))
            .discards
    };
    for (place, in_helper, statement) in [
        ("fs", "", "discard;"),
        ("if", "", "if c.a < 0.1 { discard; }"),
        (
            "else",
            "",
            "if c.a < 0.1 { c = c * 0.5; } else { discard; }",
        ),
        (
            "switch",
            "",
            "switch u32(c.a * 4.0) { case 1u: { discard; } default: {} }",
        ),
        (
            "loop",
            "",
            "for (var i = 0; i < 2; i++) { if c.a < 0.1 { discard; } }",
        ),
        (
            "continuing",
            "",
            "loop { if c.a > 2.0 { break; } continuing { if c.a < 0.1 { discard; } break if true; } }",
        ),
        ("block", "", "{ { discard; } }"),
        ("helper", "if c.a < 0.1 { discard; }", ""),
    ] {
        assert!(
            discards(&pass_running(in_helper, statement)),
            "a discard in {place} is not found"
        );
    }
    let flow = "if c.a < 0.1 { c = c * 0.5; } else { c = c * 2.0; }
    switch u32(c.a * 4.0) { case 1u: { c = c.bgra; } default: {} }
    for (var i = 0; i < 2; i++) { c = c * 0.9; }
    loop { if c.a > 2.0 { break; } continuing { c = c * 0.5; break if true; } }
    { c = c.abgr; }";
    assert!(
        !discards(&pass_running("if c.a < 0.1 { return c * 0.5; }", flow)),
        "control flow without a discard marks the pass"
    );

    let transition = |statement: &str| {
        let source = format!(
            "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {{
    {statement}
    return textureSample(incoming_tex, aria_sampler, in.uv);
}}
"
        );
        check_pass(&source, &transition_prelude(&[]), Kind::Transition)
            .unwrap_or_else(|(_, message)| panic!("{message}\n{source}"))
            .discards
    };
    assert!(transition("if in.uv.x < aria.progress { discard; }"));
    assert!(!transition(""));
}
