//! A layer drawn as a side of a transition or a matte draws with every part of
//! its look, as it draws alone.
//!
//! A side was realized by a second match that dropped the LUT, the mask and
//! the motion-blur samples of its layer. [`looks`] destructures
//! [`SceneLayer`] with no rest pattern, so a new field does not compile until
//! it has a case here or a recorded reason it has none.
//!
//! What this does not catch: a crop, which reads back as the uncropped quad on
//! the uniform samples, and a look that changes only one sample.

use std::collections::BTreeSet;

use aria_compositor::{ColorGrade, CubeLut, MaskOp};
use aria_models::{BlendMode, MaskKind};
use aria_shapes::PathPoint;

use crate::scene::{
    LayerSource, ResolvedPass, Scene, SceneBlurPass, SceneChromaKey, SceneLayer, SceneLut,
    SceneMask, SceneOutline, SceneStyles,
};

use super::masks::{assert_same, pairs_of};
use super::samples::full_canvas;
use super::{H, W, harness, render, topologies};

/// One case per part of the look `base` can carry, each applied to `base`.
fn looks(base: &SceneLayer, lut: &str) -> Vec<(&'static str, SceneLayer)> {
    let SceneLayer {
        // Which clip a layer draws, and what: the pair under test is the
        // source.
        clip: _,
        source: _,
        // The quad, which every case reads back.
        center: _,
        anchor_point: _,
        size: _,
        rotation: _,
        opacity: _,
        // The samples are uniform: a crop of one reads back as the whole.
        uv: _,
        effects: _,
        masks: _,
        chroma_key: _,
        color_grade: _,
        lut: _,
        blend_mode: _,
        styles: _,
        blur_passes: _,
    } = base;
    let with = |change: &dyn Fn(&mut SceneLayer)| {
        let mut layer = base.clone();
        change(&mut layer);
        layer
    };
    let mask = |kind, path: Vec<PathPoint>| SceneMask {
        kind,
        feather: 0.0,
        op: MaskOp::Add,
        opacity: 1.0,
        expansion: 0.0,
        center: [-0.25, 0.0],
        size: [0.5, 1.2],
        rotation_rad: 0.0,
        roundness: 0.0,
        invert: false,
        path,
        path_closed: true,
    };
    let half = [[0.0, -0.1], [0.4, -0.1], [0.4, 1.1], [0.0, 1.1]].map(PathPoint::corner);
    vec![
        ("rotation", with(&|l| l.rotation = 0.4)),
        ("opacity", with(&|l| l.opacity = 0.5)),
        (
            "effect chain",
            with(&|l| {
                l.effects = vec![ResolvedPass::new(
                    "color_overlay",
                    vec![0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.5],
                )];
            }),
        ),
        (
            "shape mask",
            with(&|l| l.masks = vec![mask(MaskKind::Circle, Vec::new())]),
        ),
        (
            "path mask",
            with(&|l| l.masks = vec![mask(MaskKind::Path, half.to_vec())]),
        ),
        (
            "mask stack",
            with(&|l| {
                l.masks = vec![
                    mask(MaskKind::Circle, Vec::new()),
                    SceneMask {
                        op: MaskOp::Subtract,
                        opacity: 0.6,
                        ..mask(MaskKind::Path, half.to_vec())
                    },
                ];
            }),
        ),
        (
            "chroma key",
            with(&|l| {
                l.chroma_key = Some(SceneChromaKey {
                    rgb: [0, 0, 255],
                    strength: 0.5,
                    shadow: 0.0,
                });
            }),
        ),
        (
            "color grade",
            with(&|l| {
                l.color_grade = Some(ColorGrade {
                    saturation: -1.0,
                    ..ColorGrade::IDENTITY
                });
            }),
        ),
        (
            "lut",
            with(&|l| {
                l.lut = Some(SceneLut {
                    path: lut.to_owned(),
                    intensity: 1.0,
                });
            }),
        ),
        ("blend mode", with(&|l| l.blend_mode = BlendMode::Multiply)),
        (
            "styles",
            with(&|l| {
                l.styles = Some(SceneStyles {
                    shadow: None,
                    glow: None,
                    outline: Some(SceneOutline {
                        rgba: [255, 255, 0, 255],
                        width: 3.0,
                    }),
                    background: None,
                });
            }),
        ),
        (
            "motion blur",
            with(&|l| {
                let [x, y] = l.center;
                l.blur_passes = [-6.0, 0.0, 6.0]
                    .map(|dx| SceneBlurPass {
                        center: [x + dx, y],
                        size: [32.0, 32.0],
                        rotation: 0.0,
                        opacity: 1.0,
                    })
                    .to_vec();
            }),
        ),
    ]
}

/// Every part of a layer's look draws on the layer as a transition side and
/// as matte content as it draws alone, for textured and untextured pixels,
/// and changes at least one of the two.
#[test]
fn a_side_draws_every_part_of_its_layers_look() {
    let (fx, mut renderer) = harness_or_skip!();
    let dir = tempfile::tempdir().expect("tempdir");
    let cube = dir.path().join("swap-rg.cube");
    std::fs::write(
        &cube,
        CubeLut::from_fn(2, |[r, g, b]| [g, r, b]).to_cube_string("swap-rg"),
    )
    .expect("write the LUT fixture");
    let cube = cube.to_string_lossy().into_owned();
    let under = full_canvas(LayerSource::Solid([200, 120, 40, 255]));
    let scene = |layer: SceneLayer| {
        let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
        scene.layers.extend([under.clone(), layer]);
        scene
    };
    let all = topologies(&fx, &mut renderer);
    let mut unchanged: BTreeSet<&str> = BTreeSet::new();
    let mut cases = 0usize;
    for kind in ["solid", "still"] {
        let base = &all
            .iter()
            .find(|topology| topology.kind() == kind)
            .unwrap_or_else(|| panic!("no '{kind}' sample"))
            .layer;
        let plain = render(&mut renderer, &fx.project, &scene(base.clone()));
        for (look, layer) in looks(base, &cube) {
            let alone = render(&mut renderer, &fx.project, &scene(layer.clone()));
            if kind == "solid" {
                unchanged.insert(look);
            }
            if alone.pixels != plain.pixels {
                unchanged.remove(look);
            }
            cases += 1;
            for (pair, wrapper) in pairs_of(&layer) {
                let paired = render(&mut renderer, &fx.project, &scene(wrapper));
                assert_same(
                    &format!("a {kind} with its {look} as {pair}"),
                    &paired,
                    &alone,
                );
            }
        }
    }
    assert!(cases > 0, "no look was drawn");
    assert!(
        unchanged.is_empty(),
        "these looks change neither sample, so their side proofs prove nothing: {unchanged:?}"
    );
}
