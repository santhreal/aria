//! Pixel conformance for the realize dispatch: every [`LayerSource`] topology
//! either paints or refuses, and the accounting is derived from the enum.
//!
//! The class this closes: the realize match in `layers.rs`, which the top
//! level and every transition or matte side go through, is exhaustive, so a
//! new topology cannot compile without an arm. An arm can still push a realized
//! layer that draws nothing, or drop the placement, the image, or the fill it
//! was handed, and that compiles and passes. Here each topology is the only
//! content in a scene rendered onto an empty canvas and read back, so an arm
//! that stops drawing turns a named test red.
//!
//! Fail-by-default: [`declared_kinds`] reads the `LayerSource` capability table
//! at run time and compares it against the [`LayerSource::kind`] of every
//! sample, and each row's `nested` column records the sub-scene decision. A
//! new topology fails the sweep until it has a sample, an expectation, and
//! a recorded sub-scene decision — and it cannot reach the table at all without
//! an arm in that file's exhaustive match.
//!
//! What this does not catch: geometry inside the region a probe reads (a layer
//! half a pixel off still passes), and the grade / LUT / effect plumbing each
//! arm forwards, which the compositor's own readback suites own.

mod samples;

use std::collections::BTreeSet;

use aria_compositor::ColorGrade;
use aria_core::RgbaImage;
use aria_models::{AnimationSlot, Project};
use aria_text::{TextAlign, TextBackground, TextStyle, TextVerticalAlign};

use crate::Renderer;
use crate::scene::{LayerSource, Nested, Scene, SceneLayer, SizeSpec, TextAnimation};

use samples::{
    CENTER, Fixtures, TEXT_CONTENT, full_canvas, placed, square_path, text_style, topologies,
};

/// Conformance canvas. Small enough that a render is a few milliseconds and
/// large enough that a 32 px quad leaves an untouched border to probe.
const W: u32 = 64;
const H: u32 = 64;

/// Alpha above which a pixel counts as inked.
const INK_ALPHA: u8 = 8;

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// Every `LayerSource` variant, named the way [`LayerSource::kind`] names it,
/// read from the capability table.
///
/// The table is the registry. Deriving the list from it rather than typing it
/// means a topology added to `scene_layers.rs` and nowhere else fails this
/// suite instead of going quietly unproven, and a `kind()` that drifts from its
/// row fails too.
///
/// This used to parse `pub enum LayerSource` out of `scene.rs` as source text —
/// a workaround for the axis not being data. It is data now.
fn declared_kinds() -> BTreeSet<String> {
    let kinds: BTreeSet<String> = crate::scene::layer_source_specs()
        .iter()
        .map(|spec| spec.id.to_owned())
        .collect();
    assert!(!kinds.is_empty(), "the layer topology table is empty");
    kinds
}

// ---------------------------------------------------------------------------
// What a topology owes
// ---------------------------------------------------------------------------

/// One readback assertion over a rendered canvas.
///
/// Rects are `[x0, y0, x1, y1]`, inclusive. Readbacks are premultiplied, which
/// equals straight alpha for the opaque content used here.
enum Expect {
    /// The pixel at `at` is within `tol` per channel of `rgba`.
    Pixel {
        at: [u32; 2],
        rgba: [u8; 4],
        tol: i32,
    },
    /// At least `min` of the pixels in `rect` are inked. For bundled art and
    /// shaped text, whose silhouette is not a constant of this test; `min` is
    /// derived from the asset itself.
    Coverage { rect: [u32; 4], min: f32 },
    /// Every opaque pixel in `rect` is within `tol` of `rgb`. Pins the ink
    /// colour of content whose shape is not fixed.
    InkColor {
        rect: [u32; 4],
        rgb: [u8; 3],
        tol: i32,
    },
    /// Every pixel in `rect` is still the empty canvas.
    Empty { rect: [u32; 4] },
}

/// The sub-scene contract for one topology.
///
/// The decision is the topology's own `nested` column, so a new topology
/// cannot reach this suite without someone recording whether a transition
/// side draws it or turns it down.
fn nested_contract(source: &LayerSource) -> Nested {
    source.spec().nested
}

/// One topology's sample scene and the proof it owes.
struct Topology {
    /// The layer under test — the top of the stack.
    layer: SceneLayer,
    /// Layers drawn beneath it, for topologies that modify what is already on
    /// the canvas. Empty for everything that draws its own pixels.
    under: Vec<SceneLayer>,
    expect: Vec<Expect>,
}

impl Topology {
    fn kind(&self) -> &'static str {
        self.layer.source.kind()
    }
}

// ---------------------------------------------------------------------------
// Running a proof
// ---------------------------------------------------------------------------

/// Render `scene` and read it back, or `None` when the host has no GPU.
fn render(renderer: &mut Renderer, project: &Project, scene: &Scene) -> RgbaImage {
    renderer
        .render_scene(project, scene)
        .expect("render the conformance scene")
}

fn scene_of(topology: &Topology) -> Scene {
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene.layers.extend(topology.under.iter().cloned());
    scene.layers.push(topology.layer.clone());
    scene
}

#[track_caller]
fn check(image: &RgbaImage, expect: &[Expect], what: &str) {
    for e in expect {
        match e {
            Expect::Pixel { at, rgba, tol } => {
                let got = image.pixel(at[0], at[1]);
                for ch in 0..4 {
                    let d = i32::from(got[ch]) - i32::from(rgba[ch]);
                    assert!(
                        d.abs() <= *tol,
                        "{what}: pixel({}, {}) = {got:?}, expected ~{rgba:?} \
                         (channel {ch} off by {d}, tol {tol})",
                        at[0],
                        at[1]
                    );
                }
            }
            Expect::Coverage { rect, min } => {
                let (inked, total) = count_ink(image, *rect);
                let got = inked as f32 / total as f32;
                assert!(
                    got >= *min,
                    "{what}: {got:.3} of {rect:?} is inked, expected at least {min:.3} \
                     — the arm drew nothing, or drew it somewhere else"
                );
            }
            Expect::InkColor { rect, rgb, tol } => {
                let mut opaque = 0usize;
                for_each(image, *rect, |x, y, px| {
                    if px[3] != 255 {
                        return;
                    }
                    opaque += 1;
                    for ch in 0..3 {
                        let d = i32::from(px[ch]) - i32::from(rgb[ch]);
                        assert!(
                            d.abs() <= *tol,
                            "{what}: opaque pixel({x}, {y}) = {px:?}, expected ink ~{rgb:?} \
                             (channel {ch} off by {d}, tol {tol})"
                        );
                    }
                });
                assert!(
                    opaque > 0,
                    "{what}: no opaque pixel in {rect:?} to check the ink colour of"
                );
            }
            Expect::Empty { rect } => {
                for_each(image, *rect, |x, y, px| {
                    assert_eq!(
                        px,
                        [0, 0, 0, 0],
                        "{what}: pixel({x}, {y}) = {px:?}, expected the empty canvas"
                    );
                });
            }
        }
    }
}

fn for_each(image: &RgbaImage, [x0, y0, x1, y1]: [u32; 4], mut f: impl FnMut(u32, u32, [u8; 4])) {
    for y in y0..=y1.min(image.height - 1) {
        for x in x0..=x1.min(image.width - 1) {
            f(x, y, image.pixel(x, y));
        }
    }
}

fn count_ink(image: &RgbaImage, rect: [u32; 4]) -> (usize, usize) {
    let mut inked = 0usize;
    let mut total = 0usize;
    for_each(image, rect, |_, _, px| {
        total += 1;
        inked += usize::from(px[3] > INK_ALPHA);
    });
    assert!(total > 0, "empty probe rect {rect:?}");
    (inked, total)
}

/// The whole harness for one topology: fixtures, renderer, samples.
///
/// `None` when the host has no GPU adapter, which is the only skip this suite
/// allows.
fn harness() -> Option<(Fixtures, Renderer)> {
    match Renderer::new_headless() {
        Ok(renderer) => Some((Fixtures::new(), renderer)),
        Err(e) => {
            eprintln!("skipping realize conformance: no headless GPU ({e})");
            None
        }
    }
}

macro_rules! harness_or_skip {
    () => {
        match harness() {
            Some(h) => h,
            None => return,
        }
    };
}

/// Render the named topology alone and check every pixel it owes.
#[track_caller]
fn prove(kind: &str) {
    let (fx, mut renderer) = harness_or_skip!();
    let all = topologies(&fx, &mut renderer);
    let topology = all
        .iter()
        .find(|t| t.kind() == kind)
        .unwrap_or_else(|| panic!("no '{kind}' sample; the sweep would have caught this too"));
    let image = render(&mut renderer, &fx.project, &scene_of(topology));
    check(&image, &topology.expect, kind);
}

mod looks;
mod masks;
mod per_topology;
mod repeat;
mod text_parity;

// ---------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------

/// Every declared topology has a sample, and every sample paints or refuses as
/// its recorded contract says.
///
/// This is the fail-by-default gate. The topology list comes from the
/// `LayerSource` capability table, not from this file, so a new topology is
/// red here until it has a sample and a pixel expectation.
#[test]
fn every_layer_source_variant_has_a_pixel_proof() {
    let declared = declared_kinds();
    let (fx, mut renderer) = harness_or_skip!();
    let all = topologies(&fx, &mut renderer);

    let covered: BTreeSet<String> = all.iter().map(|t| t.kind().to_owned()).collect();
    assert_eq!(
        covered.len(),
        all.len(),
        "two samples claim the same topology: {covered:?}"
    );
    assert_eq!(
        declared, covered,
        "every LayerSource variant needs a sample here, and every sample's \
         kind() must be the variant's own name"
    );

    for topology in &all {
        let image = render(&mut renderer, &fx.project, &scene_of(topology));
        check(&image, &topology.expect, topology.kind());
    }
}

/// A topology nested as a transition side is realized by the sub-scene match
/// and draws the same pixels, or is refused with a message naming it.
#[test]
fn nested_topologies_paint_or_refuse_by_name() {
    let (fx, mut renderer) = harness_or_skip!();
    let all = topologies(&fx, &mut renderer);
    let mut painted = 0usize;
    let mut refused = 0usize;

    for topology in &all {
        // Progress 0 of a crossfade is the outgoing side alone, so a nested
        // side owes exactly the pixels it owes at the top level.
        let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
        scene.layers.extend(topology.under.iter().cloned());
        scene.layers.push(full_canvas(LayerSource::Transition {
            outgoing: Box::new(topology.layer.clone()),
            incoming: Box::new(full_canvas(LayerSource::Solid([0, 0, 0, 0]))),
            transition_id: "crossfade".into(),
            progress: 0.0,
        }));

        let kind = topology.kind();
        match nested_contract(&topology.layer.source) {
            Nested::Paints => {
                let image = render(&mut renderer, &fx.project, &scene);
                check(&image, &topology.expect, &format!("nested {kind}"));
                painted += 1;
            }
            Nested::Refuses => {
                let err = renderer
                    .render_scene(&fx.project, &scene)
                    .expect_err(&format!("nested {kind} must be refused, not drawn"))
                    .to_string();
                // The topology's own name, spaced the way prose spells it.
                let named = kind.replace('-', " ");
                assert!(
                    err.contains(&named),
                    "the refusal of a nested {kind} must name it: got {err:?}, \
                     expected it to contain {named:?}"
                );
                refused += 1;
            }
        }
    }

    assert_eq!(
        painted + refused,
        all.len(),
        "every sample owes a nested decision"
    );
    assert!(
        painted > 0 && refused > 0,
        "the sweep exercised only one arm"
    );
}

// ---------------------------------------------------------------------------
// Paths a topology reaches only through a look
// ---------------------------------------------------------------------------

/// Per-character text realizes as instanced glyphs plus a background card,
/// which is a second draw path behind the same `Text` arm.
#[test]
fn animated_text_paints_glyphs_and_its_background_card() {
    let (fx, mut renderer) = harness_or_skip!();
    let style = TextStyle {
        background: Some(TextBackground {
            rgba: [0, 0, 255, 255],
            radius: 0.0,
        }),
        ..text_style()
    };
    let layer = placed(
        LayerSource::Text {
            content: TEXT_CONTENT.into(),
            style,
            animation: Some(TextAnimation {
                id: "char_fade_in".into(),
                slot: AnimationSlot::In,
                // Fully in: no residual fade, so the glyphs are at full opacity.
                t: 1.0,
                intensity: 1.0,
                stagger: 1.0,
            }),
        },
        SizeSpec::BitmapScaled([1.0, 1.0]),
    );
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene.layers.push(layer);
    let image = render(&mut renderer, &fx.project, &scene);

    // The card is opaque blue behind yellow glyphs, so both colours must be on
    // the canvas: a missing card leaves holes, missing glyphs leave a plain
    // card.
    let mut card = 0usize;
    let mut glyph = 0usize;
    for px in image.pixels.as_chunks::<4>().0 {
        if px[3] < 200 {
            continue;
        }
        if px[2] > 200 && px[0] < 60 {
            card += 1;
        }
        if px[0] > 200 && px[1] > 200 && px[2] < 60 {
            glyph += 1;
        }
    }
    assert!(card > 20, "no background card on the canvas ({card} px)");
    assert!(glyph > 20, "no glyph ink on the canvas ({glyph} px)");
}

/// A canvas pass carrying only a grade still processes the canvas: the grade is
/// a second reason the pass runs, independent of its effect chain.
#[test]
fn canvas_pass_applies_a_grade_with_no_effect_chain() {
    let (fx, mut renderer) = harness_or_skip!();
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene
        .layers
        .push(full_canvas(LayerSource::Solid([255, 0, 0, 255])));
    scene.layers.push(SceneLayer {
        color_grade: Some(ColorGrade {
            saturation: -1.0,
            ..ColorGrade::IDENTITY
        }),
        ..full_canvas(LayerSource::CanvasPass)
    });
    let image = render(&mut renderer, &fx.project, &scene);

    let expect = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    }
    .apply([1.0, 0.0, 0.0]);
    let quant = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    check(
        &image,
        &[Expect::Pixel {
            at: CENTER,
            rgba: [quant(expect[0]), quant(expect[1]), quant(expect[2]), 255],
            tol: 3,
        }],
        "graded canvas pass",
    );
}

// ---------------------------------------------------------------------------
// The measured quad
// ---------------------------------------------------------------------------

/// How far an edge of a CPU raster's measured quad may sit from its ink: the
/// path rasterizer's anti-alias margin at the sweep's largest scale, plus a
/// pixel of edge softening. A quad that ignored a layer scale, an anchor or a
/// title's padding is further off than this.
const RASTER_SLACK: f32 = 5.0;

/// The ink bounds of a readback, `[x0, y0, x1, y1]` in pixel edges.
fn ink_bounds(image: &RgbaImage) -> Option<[f32; 4]> {
    let mut bounds: Option<[u32; 4]> = None;
    for y in 0..image.height {
        for x in 0..image.width {
            if image.pixel(x, y)[3] > INK_ALPHA {
                let b = bounds.get_or_insert([x, y, x, y]);
                *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
    }
    bounds.map(|[x0, y0, x1, y1]| [x0 as f32, y0 as f32, (x1 + 1) as f32, (y1 + 1) as f32])
}

/// A sample moved off its anchor and stretched unevenly both ways, the inputs a
/// measured quad has to fold in the way realize does. A title is also set at
/// two alignment corners, and animated with padding, which the static raster
/// includes and the glyph placement does not.
fn displaced(layer: &SceneLayer) -> Vec<SceneLayer> {
    let stretch = |[x, y]: [f32; 2], [sx, sy]: [f32; 2]| [x * sx, y * sy];
    let mut out = Vec::new();
    for (anchor, factor) in [([0.35, 0.65], [1.5, 0.6]), ([0.65, 0.35], [0.6, 1.5])] {
        let size = match layer.size {
            SizeSpec::Fixed(size) => SizeSpec::Fixed(stretch(size, factor)),
            SizeSpec::BitmapScaled(scale) => SizeSpec::BitmapScaled(stretch(scale, factor)),
        };
        out.push(SceneLayer {
            anchor_point: anchor,
            size,
            ..layer.clone()
        });
    }
    if let LayerSource::Text { content, style, .. } = &layer.source {
        let title = |style: TextStyle, animation: Option<TextAnimation>| SceneLayer {
            source: LayerSource::Text {
                content: content.clone(),
                style,
                animation,
            },
            ..layer.clone()
        };
        for (align, vertical_align) in [
            (TextAlign::Left, TextVerticalAlign::Top),
            (TextAlign::Right, TextVerticalAlign::Bottom),
        ] {
            out.push(title(
                TextStyle {
                    align,
                    vertical_align,
                    ..style.clone()
                },
                None,
            ));
        }
        out.push(title(
            style.clone().with_padding(8),
            Some(TextAnimation {
                id: "char_fade_in".into(),
                slot: AnimationSlot::In,
                t: 1.0,
                intensity: 1.0,
                stagger: 1.0,
            }),
        ));
    }
    out
}

/// Every topology that draws its own picture inks only inside the quad
/// [`Renderer::layer_quad`] measures for it, and a CPU raster's ink reaches
/// every edge of its quad.
///
/// The selection handles and the canvas pick are drawn from that quad, so a
/// quad off the ink is a box beside the picture and a click that misses it.
/// The raster topologies are required to be in the sweep: their quad is the
/// one only the measure knows.
///
/// What this does not catch: an error inside [`RASTER_SLACK`], a quad too
/// large around art with transparent margins (a sticker, a Lottie), and the
/// quad of a topology that draws no picture of its own.
#[test]
fn every_painting_topology_inks_inside_its_measured_quad() {
    let (fx, mut renderer) = harness_or_skip!();
    let canvas = [W as f32, H as f32];
    let mut measured = BTreeSet::new();
    for topology in topologies(&fx, &mut renderer) {
        let spec = topology.layer.source.spec();
        if !spec.paints || spec.wraps_layers || !topology.under.is_empty() {
            continue;
        }
        for layer in displaced(&topology.layer) {
            let what = format!(
                "{} anchored at {:?}, sized {:?}",
                topology.kind(),
                layer.anchor_point,
                layer.size
            );
            let (center, size) = renderer
                .layer_quad(&layer, canvas)
                .unwrap_or_else(|| panic!("{what}: no quad"));
            let quad = [
                center[0] - size[0] * 0.5,
                center[1] - size[1] * 0.5,
                center[0] + size[0] * 0.5,
                center[1] + size[1] * 0.5,
            ];
            let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
            scene.layers.push(layer);
            let image = render(&mut renderer, &fx.project, &scene);
            let ink = ink_bounds(&image).unwrap_or_else(|| panic!("{what}: drew nothing"));
            for axis in 0..2 {
                assert!(
                    ink[axis] >= quad[axis] - 1.0 && ink[axis + 2] <= quad[axis + 2] + 1.0,
                    "{what}: ink {ink:?} spills out of the measured quad {quad:?}"
                );
            }
            if spec.cpu_raster.is_cpu() {
                let on_canvas = [
                    quad[0].max(0.0),
                    quad[1].max(0.0),
                    quad[2].min(canvas[0]),
                    quad[3].min(canvas[1]),
                ];
                for edge in 0..4 {
                    assert!(
                        (ink[edge] - on_canvas[edge]).abs() <= RASTER_SLACK,
                        "{what}: edge {edge} of the measured quad {quad:?} is off the ink {ink:?}"
                    );
                }
            }
        }
        measured.insert(topology.kind());
    }
    for spec in crate::scene::layer_source_specs() {
        assert!(
            !spec.cpu_raster.is_cpu() || measured.contains(spec.id),
            "the '{}' topology rasterizes on the CPU and no sample measured its quad",
            spec.id
        );
    }
}

// ---------------------------------------------------------------------------
// The registry itself
// ---------------------------------------------------------------------------

/// The registry this suite sweeps must be the capability table, not whatever a
/// loose match returns: if this drifts, every sweep above is measuring nothing.
///
/// A constructed value's own `kind()` is the authority on spelling, and it reads
/// the same row the table hands out, so a row whose id drifts from its variant
/// fails here before it can quietly shrink the sweep.
#[test]
fn the_registry_is_the_layer_source_table() {
    let declared = declared_kinds();
    for source in [
        LayerSource::CanvasPass,
        LayerSource::Solid([0, 0, 0, 0]),
        LayerSource::PathShape {
            path: square_path(1.0),
            fill: [0, 0, 0, 0],
            stroke: None,
            raster_scale: 1.0,
        },
    ] {
        assert!(
            declared.contains(source.kind()),
            "the topology table {declared:?} is missing '{}'",
            source.kind()
        );
    }
    // The table's own census reaches every row, so the two derivations of the
    // registry — rows read for their ids, and rows asked for a value of
    // themselves — must agree.
    let from_census: BTreeSet<String> = crate::scene::layer_source_specs()
        .iter()
        .map(|spec| (spec.census)().kind().to_owned())
        .collect();
    assert_eq!(
        declared, from_census,
        "a row's id and its census value's kind() disagree"
    );
}
