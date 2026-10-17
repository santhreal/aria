//! GPU readback checks that every side of a transition or matte draws as the
//! same layer drawn alone.
//!
//! The sides of a transition, and the content and stencil of a matte, were
//! drawn from their base content only: a clip's effect chain, LUT, motion blur
//! and styles vanished for the length of a transition and for the whole of a
//! matte. Each case below renders one layer through all four sides, then alone
//! on a transparent canvas, and requires each side to read back as the layer
//! alone: pixel for pixel for the outgoing, incoming and content sides, and as
//! the coverage the alone image yields for the stencil side. Each case starts a
//! compositor and draws the pairs first, so the pool holds only the slots the
//! pairs asked for, and each pair draws over side textures the previous pair
//! filled, so a side that is not cleared shows the previous frame's layer.
//! The every-treatment case draws a blend mode over the layer's own glow: a
//! side drawn as Normal into its texture and composited with the mode only
//! afterwards lost the blend of the content over its own styles.
//!
//! Not caught: a defect in a pair's own pass, which `transitions.rs` and
//! `matte.rs` check, and a side drawn wrong in the same way the canvas draws
//! the layer wrong, which the per-feature suites check.

use aria_compositor::{
    BlendMode, CompositeLayer, Compositor, CompositorConfig, CompositorLayer, CubeLut, GpuContext,
    LayerBackground, LayerGlow, LayerLut, LayerMask, LayerOutline, LayerPlacement, LayerShadow,
    LayerStencil, LayerStyles, MatteRead, PassInstance, RgbaImage, StencilCoverage, StencilMask,
    mask_kind,
};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 64;

/// Readback difference allowed per channel: 8-bit storage of a side texture
/// and the coverage arithmetic of the matte pass round differently.
const TOLERANCE: i32 = 2;

/// Rec. 709 luma weights, as the matte pass reads a luma stencil.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping pair side test: no GPU adapter ({e})");
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

fn config() -> CompositorConfig {
    CompositorConfig::new(WIDTH, HEIGHT).with_background([0, 0, 0, 0])
}

/// A plate inside the canvas, with room on every side for styles and blur.
fn plate_placement() -> LayerPlacement {
    LayerPlacement {
        center: [WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0],
        size: [40.0, 24.0],
        rotation: 0.0,
        opacity: 1.0,
    }
}

/// A plate whose luma moves under every case's treatment.
fn plate<'a>() -> CompositeLayer<'a> {
    CompositeLayer::solid([200, 60, 30, 255], plate_placement())
}

/// Horizontal sub-placements of the plate, as a fast move samples them.
fn shutter() -> Vec<LayerPlacement> {
    [-6.0, -3.0, 0.0, 3.0, 6.0]
        .into_iter()
        .map(|dx| LayerPlacement {
            center: [WIDTH as f32 / 2.0 + dx, HEIGHT as f32 / 2.0],
            ..plate_placement()
        })
        .collect()
}

fn shadow_and_glow() -> LayerStyles {
    LayerStyles {
        shadow: Some(LayerShadow {
            rgba: [240, 240, 240, 200],
            offset: [5.0, 4.0],
            blur: 3.0,
        }),
        glow: Some(LayerGlow {
            rgba: [0, 255, 255, 200],
            radius: 4.0,
            intensity: 1.2,
        }),
        ..LayerStyles::default()
    }
}

fn outline() -> LayerStyles {
    LayerStyles {
        outline: Some(LayerOutline {
            rgba: [255, 255, 0, 255],
            width: 3.0,
        }),
        ..LayerStyles::default()
    }
}

/// An ellipse inside the plate.
const ELLIPSE: [StencilMask; 1] = [StencilMask::keep(StencilCoverage::Shape(LayerMask {
    size: [0.7, 0.7],
    ..LayerMask::new(mask_kind::CIRCLE)
}))];

/// [`ELLIPSE`] on the plate.
fn ellipse<'a>() -> LayerStencil<'a> {
    LayerStencil {
        masks: &ELLIPSE,
        images: &[],
        placement: plate_placement(),
        blur_passes: &[],
    }
}

fn background() -> LayerStyles {
    LayerStyles {
        background: Some(LayerBackground {
            rgba: [180, 180, 180, 255],
            padding: 6.0,
            radius: 4.0,
        }),
        ..LayerStyles::default()
    }
}

/// Render `items` on `comp` at the test canvas.
fn render(comp: &mut Compositor, gpu: &GpuContext, items: &[CompositorLayer<'_>]) -> RgbaImage {
    comp.render_compositor_layers(gpu, &config(), items)
        .expect("render pair")
}

/// The coverage the matte pass reads from a premultiplied luma stencil pixel,
/// as the byte an opaque white content pixel scaled by it reads back as.
fn luma_coverage(pixel: [u8; 4]) -> u8 {
    if pixel[3] == 0 {
        return 0;
    }
    let alpha = f32::from(pixel[3]) / 255.0;
    let luma: f32 = (0..3)
        .map(|ch| f32::from(pixel[ch]) / 255.0 / alpha * LUMA[ch])
        .sum();
    (luma.clamp(0.0, 1.0) * alpha * 255.0).round() as u8
}

#[track_caller]
fn assert_matches(case: &str, side: &str, got: &RgbaImage, want: impl Fn(u32, u32) -> [u8; 4]) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (have, expect) = (got.pixel(x, y), want(x, y));
            let off =
                (0..4).any(|ch| (i32::from(have[ch]) - i32::from(expect[ch])).abs() > TOLERANCE);
            assert!(
                !off,
                "{case}: the {side} side at ({x},{y}) reads {have:?}, the layer alone reads {expect:?}"
            );
        }
    }
}

#[test]
fn every_side_of_a_pair_draws_as_its_layer_alone() {
    let gpu = gpu_or_skip!();
    let config = config();
    let overlay = [PassInstance::new(
        "color_overlay",
        vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
    )];
    let blur = [PassInstance::new("gaussian_blur", vec![4.0])];
    let invert = CubeLut::from_fn(9, |[r, g, b]| [1.0 - r, 1.0 - g, 1.0 - b]);
    let lut = Some(LayerLut {
        key: "pair-sides.invert",
        lut: &invert,
        intensity: 1.0,
    });

    let shutter = shutter();
    let cases = [
        ("background", plate().with_styles(background())),
        ("shadow and glow", plate().with_styles(shadow_and_glow())),
        ("motion blur", plate().with_blur_passes(&shutter)),
        ("mask", plate().with_stencil(ellipse())),
        ("separable effect", plate().with_effects(&blur)),
        ("outline", plate().with_styles(outline())),
        ("effect chain", plate().with_effects(&overlay)),
        ("lut", plate().with_lut(lut)),
        ("blend mode", plate().with_blend_mode(BlendMode::Screen)),
        (
            "every treatment",
            plate()
                .with_effects(&overlay)
                .with_lut(lut)
                .with_styles(LayerStyles {
                    outline: outline().outline,
                    ..shadow_and_glow()
                })
                .with_blur_passes(&shutter)
                .with_stencil(ellipse())
                .with_blend_mode(BlendMode::Multiply),
        ),
    ];
    let other = CompositeLayer::solid([20, 40, 200, 255], LayerPlacement::full_canvas(&config));
    let white = CompositeLayer::solid([255, 255, 255, 255], LayerPlacement::full_canvas(&config));

    for (case, layer) in &cases {
        let mut comp = Compositor::new(&gpu);
        let sides = [
            (
                "outgoing",
                CompositorLayer::Transition {
                    outgoing: layer,
                    incoming: &other,
                    transition_id: "crossfade",
                    progress: 0.0,
                },
            ),
            (
                "incoming",
                CompositorLayer::Transition {
                    outgoing: &other,
                    incoming: layer,
                    transition_id: "crossfade",
                    progress: 1.0,
                },
            ),
            (
                "content",
                CompositorLayer::Matte {
                    content: layer,
                    stencil: &white,
                    read: MatteRead::Alpha,
                    invert: false,
                    softness: 0.0,
                },
            ),
            (
                "stencil",
                CompositorLayer::Matte {
                    content: &white,
                    stencil: layer,
                    read: MatteRead::Luma,
                    invert: false,
                    softness: 0.0,
                },
            ),
        ]
        .map(|(side, item)| (side, render(&mut comp, &gpu, &[item])));
        let alone = render(&mut comp, &gpu, &[CompositorLayer::layer(layer)]);

        for (side, got) in &sides {
            if *side == "stencil" {
                assert_matches(case, side, got, |x, y| {
                    [luma_coverage(alone.pixel(x, y)); 4]
                });
            } else {
                assert_matches(case, side, got, |x, y| alone.pixel(x, y));
            }
        }
    }
}

/// Every field of a layer, destructured without `..`: a field added to
/// `CompositeLayer` stops this file compiling until a case above draws it on a
/// side. Content, placement, UV, fixed-function effects and the color grade are
/// drawn by the layer pass every case uses.
#[allow(dead_code)]
fn every_field_has_a_case(layer: &CompositeLayer<'_>) {
    let CompositeLayer {
        content: _,
        placement: _,
        uv: _,
        effects: _,
        fx: _,
        color_grade: _,
        lut: _,
        blend_mode: _,
        styles: _,
        blur_passes: _,
        stencil: _,
    } = layer;
}

/// A pair on the canvas composites as its layers do alone: a matte with its
/// content's blend mode, a transition with its sides' modes, whether the
/// sides share one or not.
///
/// A side drawn into a cleared texture blends over nothing, which is Normal,
/// and the pair then composited Normal: every mode inside a pair was lost.
/// Not caught: a transition whose sides have different modes and whose pass
/// moves pixels, which blends the canvas copies under the sides with them.
#[test]
fn a_pair_composites_with_its_layers_blend_modes() {
    let gpu = gpu_or_skip!();
    let config = config();
    let full = LayerPlacement::full_canvas(&config);
    let ground = CompositeLayer::solid([40, 120, 220, 255], full);
    let white = CompositeLayer::solid([255, 255, 255, 255], full);
    let normal = CompositeLayer::solid([20, 40, 200, 255], full);
    let screen = plate().with_blend_mode(BlendMode::Screen);
    let multiply = plate().with_blend_mode(BlendMode::Multiply);
    let multiply_full =
        CompositeLayer::solid([90, 200, 90, 255], full).with_blend_mode(BlendMode::Multiply);
    let matte = |content| CompositorLayer::Matte {
        content,
        stencil: &white,
        read: MatteRead::Alpha,
        invert: false,
        softness: 0.0,
    };
    let crossfade = |outgoing, incoming, progress| CompositorLayer::Transition {
        outgoing,
        incoming,
        transition_id: "crossfade",
        progress,
    };
    let cases = [
        ("matte content", matte(&screen), &screen),
        (
            "transition sides sharing a mode",
            crossfade(&multiply, &multiply_full, 0.0),
            &multiply,
        ),
        (
            "outgoing side of different modes",
            crossfade(&screen, &normal, 0.0),
            &screen,
        ),
        (
            "incoming side of different modes",
            crossfade(&normal, &multiply, 1.0),
            &multiply,
        ),
    ];
    for (case, pair, alone) in cases {
        let mut comp = Compositor::new(&gpu);
        let got = render(&mut comp, &gpu, &[CompositorLayer::layer(&ground), pair]);
        let want = render(
            &mut comp,
            &gpu,
            &[
                CompositorLayer::layer(&ground),
                CompositorLayer::layer(alone),
            ],
        );
        assert_matches(case, "pair", &got, |x, y| want.pixel(x, y));
    }
}
