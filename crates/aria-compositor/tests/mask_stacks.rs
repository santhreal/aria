//! Mask stacks: every mask combines with the coverage of the masks above it
//! by its op and opacity, from the coverage the stack's first op starts from,
//! and a stack of one, which draws through a coverage remap instead of the
//! stack passes, follows the same rule.
//!
//! The GPU cases read the coverage back from a white plate cut by a stack of
//! flat image masks, so each case is one number to compare with the CPU rule
//! in [`MaskOp::apply`] and [`MaskOp::start`]. The coverages are chosen so
//! every op gives a different result. Not caught: the shape of a mask's edge,
//! which the `masks` and `stencils` suites check.

use std::sync::Arc;

use aria_compositor::{
    ColorGrade, CompositeLayer, Compositor, CompositorConfig, CompositorError, CompositorLayer,
    CoverageMix, GpuContext, LayerEffects, LayerMask, LayerPlacement, LayerStencil, MaskOp,
    RgbaImage, StencilCoverage, StencilMask, mask_kind,
};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 16;
const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [220, 40, 30, 255];

/// The coverage above the mask under test, and the mask's own, as 8-bit
/// alphas. With these, every op gives a different result.
const ABOVE: u8 = 102;
const OWN: u8 = 179;

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping mask stack test: no GPU adapter ({e})");
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
    CompositorConfig::new(WIDTH, HEIGHT).with_background(BLACK)
}

fn full() -> LayerPlacement {
    LayerPlacement::full_canvas(&config())
}

/// Readback difference allowed, in 8-bit steps: each stack pass stores its
/// coverage at 8 bits.
fn tolerance(gpu: &GpuContext) -> f32 {
    if gpu.is_software() { 5.0 } else { 4.0 }
}

/// One mask of a case: op, opacity, invert, and its flat coverage as alpha.
type Case = (MaskOp, f32, bool, u8);

/// The stack rule: the coverage `masks` leave, top first, over the start
/// their first op sets. An empty stack covers everything.
fn rule(masks: &[(MaskOp, f32, bool, f32)]) -> f32 {
    let Some(&(top, ..)) = masks.first() else {
        return 1.0;
    };
    masks
        .iter()
        .fold(top.start(), |a, &(op, opacity, invert, c)| {
            let m = if invert { 1.0 - c } else { c };
            a + (op.apply(a, m) - a) * opacity
        })
}

fn rule_of(case: &[Case]) -> f32 {
    let masks: Vec<(MaskOp, f32, bool, f32)> = case
        .iter()
        .map(|&(op, opacity, invert, alpha)| (op, opacity, invert, f32::from(alpha) / 255.0))
        .collect();
    rule(&masks)
}

/// A canvas-sized image whose alpha is `alpha` everywhere.
fn flat(alpha: u8) -> RgbaImage {
    RgbaImage::new(
        WIDTH,
        HEIGHT,
        [255, 255, 255, alpha].repeat((WIDTH * HEIGHT) as usize),
    )
}

/// A canvas-sized image opaque over columns `from..to` and clear elsewhere.
fn columns(from: u32, to: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for _ in 0..HEIGHT {
        for x in 0..WIDTH {
            let alpha = if (from..to).contains(&x) { 255 } else { 0 };
            pixels.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    RgbaImage::new(WIDTH, HEIGHT, pixels)
}

fn image_masks(case: &[Case]) -> (Vec<StencilMask>, Vec<Arc<RgbaImage>>) {
    case.iter()
        .enumerate()
        .map(|(index, &(op, opacity, invert, alpha))| {
            let mask = StencilMask {
                coverage: StencilCoverage::Image(index),
                invert,
                softness: 0.0,
                op,
                opacity,
            };
            (mask, Arc::new(flat(alpha)))
        })
        .unzip()
}

/// The coverage a white plate cut by `case` reads back with, in 8-bit steps.
fn stack_coverage(comp: &mut Compositor, gpu: &GpuContext, case: &[Case]) -> f32 {
    let (masks, images) = image_masks(case);
    let plate = CompositeLayer::solid(WHITE, full()).with_stencil(LayerStencil {
        masks: &masks,
        images: &images,
        placement: full(),
        blur_passes: &[],
    });
    let got = comp
        .render_compositor_layers(gpu, &config(), &[CompositorLayer::layer(&plate)])
        .expect("render a mask stack");
    f32::from(got.pixel(WIDTH / 2, HEIGHT / 2)[0])
}

#[track_caller]
fn assert_coverage(what: &str, got: f32, want: f32, tol: f32) {
    let want = want * 255.0;
    assert!(
        (got - want).abs() <= tol,
        "{what}: reads coverage {got}/255, the stack rule gives {want}/255"
    );
}

/// Every op, as a mask under another, combines with the coverage above it at
/// full and partial opacity, upright and inverted.
#[test]
fn every_op_combines_with_the_coverage_above_it() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let tol = tolerance(&gpu);
    for op in MaskOp::ALL {
        for opacity in [1.0, 0.4] {
            for invert in [false, true] {
                let case = [(MaskOp::Add, 1.0, false, ABOVE), (op, opacity, invert, OWN)];
                let what = format!("{op:?} at opacity {opacity}, inverted {invert}");
                let got = stack_coverage(&mut comp, &gpu, &case);
                assert_coverage(&what, got, rule_of(&case), tol);
            }
        }
    }
}

/// The stack starts full under a first op that cuts and empty under one that
/// adds: a first Subtract, Intersect or Darken cuts the layer rather than
/// emptying it.
#[test]
fn the_first_op_sets_the_coverage_the_stack_starts_from() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let tol = tolerance(&gpu);
    for op in MaskOp::ALL {
        // A clear Add under it keeps what the first mask left.
        let case = [(op, 1.0, false, ABOVE), (MaskOp::Add, 1.0, false, 0)];
        let got = stack_coverage(&mut comp, &gpu, &case);
        assert_coverage(&format!("{op:?} first"), got, rule_of(&case), tol);
    }
}

/// A stack of one draws through the matte pass's coverage remap and agrees
/// with the stack rule for every op, opacity and inversion.
#[test]
fn a_stack_of_one_follows_the_stack_rule() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let tol = tolerance(&gpu);
    for op in MaskOp::ALL {
        for opacity in [1.0, 0.4] {
            for invert in [false, true] {
                let case = [(op, opacity, invert, OWN)];
                let what = format!("lone {op:?} at opacity {opacity}, inverted {invert}");
                let got = stack_coverage(&mut comp, &gpu, &case);
                assert_coverage(&what, got, rule_of(&case), tol);
            }
        }
    }
}

/// The fx pipeline's shape mask remaps its coverage by its mix: inside the
/// shape it reads the remap of full coverage, outside the remap of none.
#[test]
fn the_fx_mask_remaps_its_coverage_by_its_mix() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let tol = tolerance(&gpu);
    let white = flat(255);
    for op in MaskOp::ALL {
        for invert in [false, true] {
            let mix = CoverageMix::single(op, 0.4, invert);
            let layer = CompositeLayer::rgba(&white, full()).with_fx(LayerEffects {
                mask: Some(LayerMask {
                    size: [0.5, 0.5],
                    mix,
                    ..LayerMask::new(mask_kind::RECTANGLE)
                }),
                chroma_key: None,
            });
            let got = comp
                .render_compositor_layers(&gpu, &config(), &[CompositorLayer::layer(&layer)])
                .expect("render an fx mask");
            let what = format!("{op:?}, inverted {invert}");
            let inside = f32::from(got.pixel(WIDTH / 2, HEIGHT / 2)[0]);
            let outside = f32::from(got.pixel(1, 1)[0]);
            assert_coverage(&format!("{what} inside"), inside, mix.apply(1.0), tol);
            assert_coverage(&format!("{what} outside"), outside, mix.apply(0.0), tol);
        }
    }
}

/// A canvas pass replaces the canvas where its whole stack covers it: a left
/// half less its left quarter grades the second quarter only.
#[test]
fn a_canvas_pass_replaces_the_canvas_under_its_whole_stack() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let plate = CompositeLayer::solid(RED, full());
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let images = [columns(0, WIDTH / 2), columns(0, WIDTH / 4)].map(Arc::new);
    let masks = [
        StencilMask::keep(StencilCoverage::Image(0)),
        StencilMask {
            op: MaskOp::Subtract,
            ..StencilMask::keep(StencilCoverage::Image(1))
        },
    ];
    let stencil = LayerStencil {
        masks: &masks,
        images: &images,
        placement: full(),
        blur_passes: &[],
    };
    let pass = |stencil| CompositorLayer::CanvasPass {
        effects: &[],
        grade: Some(grade),
        lut: None,
        stencil,
    };
    let render = |comp: &mut Compositor, items: &[CompositorLayer<'_>]| {
        comp.render_compositor_layers(&gpu, &config(), items)
            .expect("render a canvas pass")
    };
    let plain = render(&mut comp, &[CompositorLayer::layer(&plate)]);
    let graded = render(&mut comp, &[CompositorLayer::layer(&plate), pass(None)]);
    assert_ne!(
        plain.pixel(0, 0),
        graded.pixel(0, 0),
        "the grade moved nothing"
    );
    let got = render(
        &mut comp,
        &[CompositorLayer::layer(&plate), pass(Some(&stencil))],
    );
    let y = HEIGHT / 2;
    for (x, want, what) in [
        (WIDTH / 8, plain.pixel(0, 0), "the subtracted quarter"),
        (WIDTH * 3 / 8, graded.pixel(0, 0), "the second quarter"),
        (WIDTH * 3 / 4, plain.pixel(0, 0), "the uncovered half"),
    ] {
        let have = got.pixel(x, y);
        let off = (0..4).any(|ch| (i32::from(have[ch]) - i32::from(want[ch])).abs() > 3);
        assert!(!off, "{what} at x {x} reads {have:?}, expected {want:?}");
    }
}

/// A mask naming a coverage image its stencil does not hold fails the frame
/// with the index and the count, rather than drawing without it.
#[test]
fn a_mask_naming_a_missing_image_fails_the_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let images = [Arc::new(flat(255))];
    let masks = [
        StencilMask::keep(StencilCoverage::Image(0)),
        StencilMask::keep(StencilCoverage::Image(1)),
    ];
    let plate = CompositeLayer::solid(WHITE, full()).with_stencil(LayerStencil {
        masks: &masks,
        images: &images,
        placement: full(),
        blur_passes: &[],
    });
    let error = comp
        .render_compositor_layers(&gpu, &config(), &[CompositorLayer::layer(&plate)])
        .expect_err("a missing coverage image fails the frame");
    assert!(
        matches!(
            error,
            CompositorError::StencilImage {
                index: 1,
                images: 1
            }
        ),
        "unexpected error: {error}"
    );
}

/// A new op fails to compile here until it has a place in [`MaskOp::ALL`].
fn place(op: MaskOp) -> usize {
    match op {
        MaskOp::Add => 0,
        MaskOp::Subtract => 1,
        MaskOp::Intersect => 2,
        MaskOp::Lighten => 3,
        MaskOp::Darken => 4,
        MaskOp::Difference => 5,
    }
}

/// [`CoverageMix::single`] folds a lone mask's op, opacity and inversion into
/// a remap equal to the stack rule, and its complement keeps what it cuts.
#[test]
fn a_single_mix_is_the_stack_rule_for_one_mask() {
    for (index, op) in MaskOp::ALL.into_iter().enumerate() {
        assert_eq!(place(op), index, "{op:?} is out of place in MaskOp::ALL");
        assert_eq!(op.id() as usize, index, "{op:?} has the id of another op");
        for opacity in [0.0, 0.3, 1.0] {
            for invert in [false, true] {
                let mix = CoverageMix::single(op, opacity, invert);
                for c in [0.0, 0.25, 0.5, 1.0] {
                    let what = format!("{op:?} at opacity {opacity}, inverted {invert}, c {c}");
                    let want = rule(&[(op, opacity, invert, c)]);
                    assert!((mix.apply(c) - want).abs() < 1e-6, "{what}: {mix:?}");
                    assert!(
                        (mix.complement().apply(c) - (1.0 - want)).abs() < 1e-6,
                        "{what}: complement of {mix:?}"
                    );
                }
            }
        }
    }
    assert_eq!(MaskOp::ALL.len(), 6);
}
