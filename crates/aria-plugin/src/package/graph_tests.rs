//! Tests for the render-graph fields of `plugin.toml`.
//!
//! Closes the class "a pass table, a texture, or an effect trait reads
//! differently from what the manifest states": both spellings of `passes` (an
//! inline array mixing file paths and tables, and `[[effects.passes]]`) read
//! to the exact declared passes, texture ids and texture inputs land under the
//! package id, and the effect traits are kept. Refusals are rows of
//! `every_manifest_rule_refuses_with_its_reason`; registration is covered by
//! `tests/package_graph.rs`.

use aria_models::{EffectBounds, OutputSize, PassInput, PassStage};

use super::*;

const MANIFEST: &str = r#"
[plugin]
id = "com.example.graph"
name = "Graph"
version = "1.0.0"
api = 1

[[textures]]
id = "lut"
file = "textures/lut.png"

[[textures]]
id = "com.example.graph.grain"
file = "./textures/grain.png"

[[effects]]
id = "glow"
name = "Glow"
time_dependent = true
reads_outside = true
bounds = { left = 4, top = 4.5, right = 6, bottom = 0 }
passes = [
    "shaders/prep.wgsl",
    { name = "half", file = "shaders/half.wgsl", stage = { compute = { workgroup = [8, 4] } }, output = { scale = [0.5, 0.25] } },
    { file = "shaders/mix.wgsl", inputs = ["source", { pass = "half" }, { layer = "matte" }, { texture = "lut" }] },
]

[[generators]]
id = "noise"
name = "Noise"

[[generators.passes]]
name = "tile"
file = "shaders/tile.wgsl"
output = { fixed = [64, 32] }

[[generators.passes]]
file = "shaders/grain.wgsl"
stage = "fragment"
inputs = [{ pass = "tile" }, { texture = "com.example.graph.grain" }]
"#;

#[test]
fn pass_tables_textures_and_traits_read_as_written() {
    let manifest = PackageManifest::parse(MANIFEST).expect("parses");
    assert_eq!(
        manifest.textures,
        [
            DeclaredTexture {
                id: "com.example.graph.lut".into(),
                file: "textures/lut.png".into(),
            },
            DeclaredTexture {
                id: "com.example.graph.grain".into(),
                file: "textures/grain.png".into(),
            },
        ]
    );
    assert_eq!(
        manifest.effects,
        [DeclaredEffect {
            id: "com.example.graph.glow".into(),
            name: "Glow".into(),
            category: None,
            passes: vec![
                DeclaredPass::file("shaders/prep.wgsl"),
                DeclaredPass {
                    file: "shaders/half.wgsl".into(),
                    name: "half".into(),
                    stage: PassStage::Compute { workgroup: [8, 4] },
                    inputs: Vec::new(),
                    output: OutputSize::Scale([0.5, 0.25]),
                },
                DeclaredPass {
                    file: "shaders/mix.wgsl".into(),
                    name: String::new(),
                    stage: PassStage::Fragment,
                    inputs: vec![
                        PassInput::Source,
                        PassInput::pass("half"),
                        PassInput::layer("matte"),
                        PassInput::texture("com.example.graph.lut"),
                    ],
                    output: OutputSize::Same,
                },
            ],
            params: Vec::new(),
            groups: Vec::new(),
            stateful: false,
            bounds: EffectBounds::new(4.0, 4.5, 6.0, 0.0),
            reads_outside: true,
            time_dependent: true,
        }]
    );
    assert_eq!(
        manifest.generators,
        [DeclaredEffect {
            id: "com.example.graph.noise".into(),
            name: "Noise".into(),
            category: None,
            passes: vec![
                DeclaredPass {
                    file: "shaders/tile.wgsl".into(),
                    name: "tile".into(),
                    stage: PassStage::Fragment,
                    inputs: Vec::new(),
                    output: OutputSize::Fixed([64, 32]),
                },
                DeclaredPass {
                    file: "shaders/grain.wgsl".into(),
                    name: String::new(),
                    stage: PassStage::Fragment,
                    inputs: vec![
                        PassInput::pass("tile"),
                        PassInput::texture("com.example.graph.grain"),
                    ],
                    output: OutputSize::Same,
                },
            ],
            params: Vec::new(),
            groups: Vec::new(),
            stateful: false,
            bounds: EffectBounds::ZERO,
            reads_outside: false,
            time_dependent: false,
        }]
    );
}

/// A declared pass becomes the [`aria_models::GraphPass`] the compositor
/// plans, with the file's WGSL as its source and every graph field kept.
#[test]
fn a_declared_pass_keeps_its_graph_fields_with_its_source() {
    let manifest = PackageManifest::parse(MANIFEST).expect("parses");
    let mix = &manifest.effects[0].passes[2];
    let pass = mix.graph_pass("WGSL".into());
    assert_eq!(
        pass,
        aria_models::GraphPass::fragment("WGSL").reading([
            PassInput::Source,
            PassInput::pass("half"),
            PassInput::layer("matte"),
            PassInput::texture("com.example.graph.lut"),
        ])
    );
    let half = manifest.effects[0].passes[1].graph_pass(String::new());
    assert_eq!(
        half,
        aria_models::GraphPass::compute("", [8, 4])
            .named("half")
            .sized(OutputSize::Scale([0.5, 0.25]))
    );
}
