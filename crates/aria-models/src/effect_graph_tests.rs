//! Graph declarations: the defaults that make a list a chain, the execution
//! order, and every refusal naming the pass at fault.
//!
//! Closes the class "a graph the compositor cannot run reaches the GPU": each
//! structural rule of [`plan_graph`] has a case that is refused with the pass
//! index and a case beside it that passes, so loosening a rule turns a case
//! red. WGSL itself is checked by the compositor, not here.

use super::*;
use crate::effects::EffectParamSpec;

fn spec_with(params: Vec<EffectParamSpec>) -> EffectSpec {
    EffectSpec {
        id: "test.graph".into(),
        label: "Graph".into(),
        params,
        ..EffectSpec::default()
    }
}

fn layer_param(name: &str) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Layer,
        ..EffectParamSpec::default()
    }
}

fn scalar_param(name: &str) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Scalar,
        max: 1.0,
        ..EffectParamSpec::default()
    }
}

#[track_caller]
fn refused(passes: &[GraphPass], params: Vec<EffectParamSpec>) -> GraphError {
    match plan_graph(&spec_with(params), passes) {
        Ok(plan) => panic!("expected a refusal, planned {plan:?}"),
        Err(error) => error,
    }
}

#[test]
fn an_unnamed_list_is_a_chain_from_the_source() {
    let plan = plan_graph(
        &spec_with(Vec::new()),
        &[
            GraphPass::fragment("a"),
            GraphPass::fragment("b"),
            GraphPass::fragment("c"),
        ],
    )
    .expect("a chain plans");
    assert_eq!(plan.names, ["pass0", "pass1", "pass2"]);
    assert_eq!(
        plan.inputs,
        vec![
            vec![PassInput::Source],
            vec![PassInput::pass("pass0")],
            vec![PassInput::pass("pass1")],
        ]
    );
    assert_eq!(plan.order, [0, 1, 2]);
}

#[test]
fn passes_run_after_what_they_read_whatever_the_declaration_order() {
    // `final` reads `blur` and `down`; `blur` reads `down`, declared after it.
    let plan = plan_graph(
        &spec_with(Vec::new()),
        &[
            GraphPass::compute("b", [8, 8])
                .named("blur")
                .reading([PassInput::pass("down")])
                .sized(OutputSize::Scale([0.5, 0.5])),
            GraphPass::fragment("d")
                .named("down")
                .reading([PassInput::Source])
                .sized(OutputSize::Scale([0.5, 0.5])),
            GraphPass::fragment("f")
                .named("final")
                .reading([PassInput::pass("blur"), PassInput::pass("down")]),
        ],
    )
    .expect("a diamond plans");
    assert_eq!(plan.order, [1, 0, 2]);
    assert_eq!(plan.index_of("down"), Some(1));
}

#[test]
fn a_cycle_is_refused_at_a_pass_on_it_and_spelled_out() {
    let error = refused(
        &[
            GraphPass::fragment("a")
                .named("a")
                .reading([PassInput::pass("b")]),
            GraphPass::fragment("b")
                .named("b")
                .reading([PassInput::pass("a")]),
            GraphPass::fragment("c")
                .named("c")
                .reading([PassInput::pass("b")]),
        ],
        Vec::new(),
    );
    assert_eq!(error.pass, Some(0), "{error}");
    assert_eq!(error.message, "pass `a` is on a cycle: a reads b reads a");
}

#[test]
fn a_pass_reading_itself_is_a_cycle() {
    let error = refused(
        &[GraphPass::fragment("a")
            .named("loop")
            .reading([PassInput::pass("loop")])],
        Vec::new(),
    );
    assert_eq!(error.pass, Some(0));
    assert!(error.message.contains("reads itself"), "{error}");
}

#[test]
fn every_input_names_something_that_exists() {
    let unknown_pass = refused(
        &[GraphPass::fragment("a").reading([PassInput::pass("ghost")])],
        Vec::new(),
    );
    assert_eq!(unknown_pass.pass, Some(0));
    assert!(
        unknown_pass
            .message
            .contains("`ghost`, which is not declared")
    );

    let not_a_layer = refused(
        &[GraphPass::fragment("a").reading([PassInput::layer("amount")])],
        vec![scalar_param("amount")],
    );
    assert!(
        not_a_layer.message.contains("not a layer parameter"),
        "{not_a_layer}"
    );
    let missing_layer = refused(
        &[GraphPass::fragment("a").reading([PassInput::layer("matte")])],
        Vec::new(),
    );
    assert!(missing_layer.message.contains("not a layer parameter"));
    plan_graph(
        &spec_with(vec![layer_param("matte")]),
        &[GraphPass::fragment("a").reading([PassInput::Source, PassInput::layer("matte")])],
    )
    .expect("a declared layer parameter is an input");

    let bad_texture = refused(
        &[GraphPass::fragment("a").reading([PassInput::texture("no spaces")])],
        Vec::new(),
    );
    assert!(bad_texture.message.contains("texture id"), "{bad_texture}");
    plan_graph(
        &spec_with(Vec::new()),
        &[GraphPass::fragment("a").reading([PassInput::texture("acme.noise-1")])],
    )
    .expect("a dotted texture id is an input");
}

#[test]
fn two_inputs_may_not_share_a_binding() {
    let error = refused(
        &[GraphPass::fragment("a").reading([
            PassInput::texture("acme.noise"),
            PassInput::texture("acme_noise"),
        ])],
        Vec::new(),
    );
    assert_eq!(error.pass, Some(0));
    assert!(
        error.message.contains("`texture_acme_noise` twice"),
        "{error}"
    );
    // `source` twice binds nothing new and is allowed.
    plan_graph(
        &spec_with(Vec::new()),
        &[GraphPass::fragment("a").reading([PassInput::Source, PassInput::Source])],
    )
    .expect("the source frame may be read at several slots");
}

#[test]
fn names_are_unique_identifiers() {
    let repeated = refused(
        &[
            GraphPass::fragment("a").named("x"),
            GraphPass::fragment("b").named("x"),
        ],
        Vec::new(),
    );
    assert_eq!(repeated.pass, Some(1));
    assert!(repeated.message.contains("repeats the name of pass 0"));
    let spaced = refused(&[GraphPass::fragment("a").named("two words")], Vec::new());
    assert!(spaced.message.contains("not an identifier"));
}

#[test]
fn every_pass_feeds_the_last_one() {
    let error = refused(
        &[
            GraphPass::fragment("a").named("unused"),
            GraphPass::fragment("b")
                .named("out")
                .reading([PassInput::Source]),
        ],
        Vec::new(),
    );
    assert_eq!(error.pass, Some(0));
    assert_eq!(
        error.message,
        "pass `unused` feeds nothing: the last pass `out` does not read it"
    );
}

#[test]
fn the_last_pass_writes_the_frame_at_the_input_size() {
    let error = refused(
        &[GraphPass::fragment("a").sized(OutputSize::Scale([0.5, 0.5]))],
        Vec::new(),
    );
    assert_eq!(error.pass, Some(0));
    assert!(error.message.contains("output size `same`"), "{error}");
}

#[test]
fn sizes_and_workgroups_have_bounds() {
    for output in [
        OutputSize::Scale([0.0, 1.0]),
        OutputSize::Scale([f32::NAN, 1.0]),
        OutputSize::Scale([MAX_PASS_SCALE * 2.0, 1.0]),
        OutputSize::Fixed([0, 4]),
        OutputSize::Fixed([MAX_PASS_SIDE + 1, 4]),
    ] {
        let error = refused(
            &[
                GraphPass::fragment("a").named("a").sized(output),
                GraphPass::fragment("b"),
            ],
            Vec::new(),
        );
        assert_eq!(error.pass, Some(0), "{output:?}: {error}");
    }
    for workgroup in [[0, 8], [8, 0], [32, 16]] {
        let error = refused(&[GraphPass::compute("c", workgroup)], Vec::new());
        assert_eq!(error.pass, Some(0), "{workgroup:?}: {error}");
    }
    plan_graph(&spec_with(Vec::new()), &[GraphPass::compute("c", [16, 16])])
        .expect("a 256-invocation workgroup is allowed");
}

#[test]
fn an_empty_or_oversized_graph_is_refused_as_a_whole() {
    assert_eq!(refused(&[], Vec::new()).pass, None);
    let many: Vec<GraphPass> = (0..=MAX_GRAPH_PASSES)
        .map(|_| GraphPass::fragment("p"))
        .collect();
    assert_eq!(refused(&many, Vec::new()).pass, None);
}

#[test]
fn output_sizes_resolve_against_the_input() {
    assert_eq!(OutputSize::Same.resolve(64, 16), (64, 16));
    assert_eq!(OutputSize::Scale([0.5, 0.25]).resolve(64, 16), (32, 4));
    assert_eq!(OutputSize::Scale([0.01, 0.01]).resolve(64, 16), (1, 1));
    assert_eq!(OutputSize::Fixed([256, 1]).resolve(64, 16), (256, 1));
}

#[test]
fn a_graph_round_trips_through_json_and_toml() {
    let passes = vec![
        GraphPass::compute("c", [8, 4])
            .named("down")
            .reading([PassInput::Source, PassInput::layer("matte")])
            .sized(OutputSize::Scale([0.5, 0.5])),
        GraphPass::fragment("f")
            .named("out")
            .reading([PassInput::pass("down"), PassInput::texture("acme.lut")]),
        GraphPass::fragment("g").sized(OutputSize::Fixed([3, 2])),
    ];
    let json = serde_json::to_string(&passes).expect("serializes");
    let back: Vec<GraphPass> = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, passes);

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Doc {
        passes: Vec<GraphPass>,
    }
    let toml_text = toml::to_string(&Doc {
        passes: passes.clone(),
    })
    .expect("serializes");
    let back: Doc = toml::from_str(&toml_text).expect("deserializes");
    assert_eq!(back.passes, passes);

    // The written forms other tiers produce by hand.
    let hand: GraphPass = serde_json::from_str(
        r#"{"name":"blur","stage":{"compute":{"workgroup":[8,8]}},
            "inputs":["source",{"pass":"down"},{"layer":"matte"},{"texture":"acme.noise"}],
            "output":{"scale":[0.5,0.5]},"source":"x"}"#,
    )
    .expect("the documented JSON form parses");
    assert_eq!(hand.stage, PassStage::Compute { workgroup: [8, 8] });
    assert_eq!(hand.inputs.len(), 4);
    let minimal: GraphPass = serde_json::from_str(r#"{"source":"x"}"#).expect("parses");
    assert_eq!(minimal, GraphPass::fragment("x"));
}
