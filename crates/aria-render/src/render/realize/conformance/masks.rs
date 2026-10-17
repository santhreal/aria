//! Masks cut every painting topology alone and as either side of a pair, and
//! limit a canvas pass to their coverage.
//!
//! The sweep takes its topologies from the capability table, as the paint
//! sweep does, so a new painting topology is cut here before it ships. Both
//! mask families run: a shape mask, which the fx pipeline draws on textured
//! pixels and a stencil draws on the rest, and a path mask, which is always a
//! stencil. Each keeps the left [`KEEP`] of the layer's quad, off its center,
//! so a mask placed on a box other than the quad cuts in the wrong column.
//!
//! What this does not catch: geometry inside [`SLACK`] of the cut, feather,
//! and inversion, which the compositor's stencil suite covers.

use aria_compositor::{ColorGrade, MaskOp};
use aria_core::RgbaImage;
use aria_models::{AnimationSlot, MaskKind, MatteSource};
use aria_shapes::PathPoint;

use crate::Renderer;
use crate::scene::{LayerSource, Nested, Scene, SceneLayer, SceneMask, TextAnimation};

use super::samples::{Fixtures, full_canvas};
use super::{H, INK_ALPHA, W, harness, render, topologies};

/// The fraction of the quad's width, from its left edge, a mask keeps.
const KEEP: f32 = 0.35;

/// Pixels either side of a cut the mask's anti-aliasing may soften.
const SLACK: f32 = 2.0;

/// Readback difference allowed per channel between two draws of one layer.
const TOLERANCE: i32 = 3;

/// A shape mask and a path mask, each keeping the left [`KEEP`] of the quad
/// and overhanging its other three edges.
fn masks() -> [(&'static str, SceneMask); 2] {
    let shape = SceneMask {
        kind: MaskKind::Rectangle,
        feather: 0.0,
        op: MaskOp::Add,
        opacity: 1.0,
        expansion: 0.0,
        center: [(KEEP - 1.1) * 0.5, 0.0],
        size: [KEEP + 0.1, 1.2],
        rotation_rad: 0.0,
        roundness: 0.0,
        invert: false,
        path: Vec::new(),
        path_closed: false,
    };
    let path = SceneMask {
        kind: MaskKind::Path,
        center: [0.0, 0.0],
        size: [1.0, 1.0],
        path: [[-0.1, -0.1], [KEEP, -0.1], [KEEP, 1.1], [-0.1, 1.1]]
            .map(PathPoint::corner)
            .to_vec(),
        path_closed: true,
        ..shape
    };
    [("shape mask", shape), ("path mask", path)]
}

fn lone(layer: SceneLayer) -> Scene {
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene.layers.push(layer);
    scene
}

/// `layer` as the outgoing side of a crossfade at progress 0, and as the
/// content of a matte whose stencil covers the canvas: both draw it alone.
pub(super) fn pairs_of(layer: &SceneLayer) -> [(&'static str, SceneLayer); 2] {
    let transition = full_canvas(LayerSource::Transition {
        outgoing: Box::new(layer.clone()),
        incoming: Box::new(full_canvas(LayerSource::Solid([0, 0, 0, 0]))),
        transition_id: "crossfade".into(),
        progress: 0.0,
    });
    let matte = full_canvas(LayerSource::Matte {
        layers: Box::new([
            layer.clone(),
            full_canvas(LayerSource::Solid([255, 255, 255, 255])),
        ]),
        read: MatteSource::Alpha,
        invert: false,
        softness: 0.0,
    });
    [("a transition side", transition), ("matte content", matte)]
}

#[track_caller]
pub(super) fn assert_same(what: &str, got: &RgbaImage, want: &RgbaImage) {
    for y in 0..H {
        for x in 0..W {
            let (have, expect) = (got.pixel(x, y), want.pixel(x, y));
            let off =
                (0..4).any(|ch| (i32::from(have[ch]) - i32::from(expect[ch])).abs() > TOLERANCE);
            assert!(
                !off,
                "{what}: pixel({x}, {y}) = {have:?}, alone it is {expect:?}"
            );
        }
    }
}

/// Whether any pixel of `image` whose center lies in `from..to` is inked.
fn inked_in(image: &RgbaImage, from: f32, to: f32) -> bool {
    (0..H).any(|y| {
        (0..W).any(|x| {
            let center = x as f32 + 0.5;
            center > from && center < to && image.pixel(x, y)[3] > INK_ALPHA
        })
    })
}

/// `cut` matches `alone` left of `edge` and is empty right of it.
#[track_caller]
fn assert_cut(what: &str, cut: &RgbaImage, alone: &RgbaImage, edge: f32) {
    for y in 0..H {
        for x in 0..W {
            let center = x as f32 + 0.5;
            let (have, expect) = (cut.pixel(x, y), alone.pixel(x, y));
            if center < edge - SLACK {
                let off = (0..4)
                    .any(|ch| (i32::from(have[ch]) - i32::from(expect[ch])).abs() > TOLERANCE);
                assert!(
                    !off,
                    "{what}: pixel({x}, {y}) inside the mask = {have:?}, unmasked {expect:?}"
                );
            } else if center > edge + SLACK {
                assert!(
                    have[3] <= INK_ALPHA,
                    "{what}: pixel({x}, {y}) outside the mask = {have:?}"
                );
            }
        }
    }
}

/// Every topology that draws its own picture, and text drawn as glyphs,
/// named.
fn painting_layers(fx: &Fixtures, renderer: &mut Renderer) -> Vec<(String, SceneLayer)> {
    let mut out = Vec::new();
    for topology in topologies(fx, renderer) {
        let spec = topology.layer.source.spec();
        if !spec.paints || spec.wraps_layers || !topology.under.is_empty() {
            continue;
        }
        if let LayerSource::Text { content, style, .. } = &topology.layer.source {
            let glyphs = SceneLayer {
                source: LayerSource::Text {
                    content: content.clone(),
                    style: style.clone(),
                    animation: Some(TextAnimation {
                        id: "char_fade_in".into(),
                        slot: AnimationSlot::In,
                        t: 1.0,
                        intensity: 1.0,
                        stagger: 1.0,
                    }),
                },
                ..topology.layer.clone()
            };
            out.push(("text drawn as glyphs".to_owned(), glyphs));
        }
        out.push((topology.kind().to_owned(), topology.layer));
    }
    out
}

/// A mask cuts every painting topology at its quad, alone and as the side of
/// a transition or a matte.
///
/// Masks drew on media clips alone: a shape mask rode the fx pipeline, which
/// solids, shapes and text have none of, a path mask was cut on a canvas-sized
/// box for text and paths, and a side dropped its mask entirely.
#[test]
fn a_mask_cuts_every_painting_topology_alone_and_as_a_side() {
    let (fx, mut renderer) = harness_or_skip!();
    let canvas = [W as f32, H as f32];
    let layers = painting_layers(&fx, &mut renderer);
    assert!(layers.len() > 1, "no painting topology to cut");
    for (kind, layer) in layers {
        let (center, size) = renderer
            .layer_quad(&layer, canvas)
            .unwrap_or_else(|| panic!("{kind}: no quad"));
        let edge = center[0] - size[0] * 0.5 + size[0] * KEEP;
        let alone = render(&mut renderer, &fx.project, &lone(layer.clone()));
        assert!(
            inked_in(&alone, f32::MIN, edge - SLACK) && inked_in(&alone, edge + SLACK, f32::MAX),
            "{kind}: draws nothing on one side of x {edge}, so the cut proves nothing"
        );
        for (mask_kind, mask) in masks() {
            let what = format!("{kind} under a {mask_kind}");
            let masked = SceneLayer {
                masks: vec![mask],
                ..layer.clone()
            };
            let cut = render(&mut renderer, &fx.project, &lone(masked.clone()));
            assert_cut(&what, &cut, &alone, edge);
            if matches!(masked.source.spec().nested, Nested::Refuses) {
                continue;
            }
            for (pair, wrapper) in pairs_of(&masked) {
                let paired = render(&mut renderer, &fx.project, &lone(wrapper));
                assert_same(&format!("{what} as {pair}"), &paired, &cut);
            }
        }
    }
}

/// A masked canvas pass changes the canvas inside its mask and keeps it
/// outside, for both mask families.
#[test]
fn a_masked_canvas_pass_changes_only_the_canvas_inside_its_mask() {
    let (fx, mut renderer) = harness_or_skip!();
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let red = [255, 0, 0, 255];
    let canvas = |pass: Option<SceneLayer>| {
        let mut scene = lone(full_canvas(LayerSource::Solid(red)));
        scene.layers.extend(pass);
        scene
    };
    let graded = |masks| SceneLayer {
        color_grade: Some(grade),
        masks,
        ..full_canvas(LayerSource::CanvasPass)
    };
    let everywhere = render(
        &mut renderer,
        &fx.project,
        &canvas(Some(graded(Vec::new()))),
    );
    let edge = W as f32 * KEEP;
    for (mask_kind, mask) in masks() {
        let image = render(
            &mut renderer,
            &fx.project,
            &canvas(Some(graded(vec![mask]))),
        );
        for y in 0..H {
            for x in 0..W {
                let center = x as f32 + 0.5;
                let want = if center < edge - SLACK {
                    everywhere.pixel(x, y)
                } else if center > edge + SLACK {
                    red
                } else {
                    continue;
                };
                let have = image.pixel(x, y);
                let off =
                    (0..4).any(|ch| (i32::from(have[ch]) - i32::from(want[ch])).abs() > TOLERANCE);
                assert!(
                    !off,
                    "{mask_kind}: pixel({x}, {y}) = {have:?}, expected {want:?}"
                );
            }
        }
    }
}
