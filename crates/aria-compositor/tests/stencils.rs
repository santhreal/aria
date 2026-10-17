//! GPU readback checks for layer stencils: a mask cuts any layer content before
//! its styles, keeps where its coverage is opaque, averages its motion-blur
//! samples, and limits a canvas pass to its coverage.

use aria_compositor::{
    ColorGrade, CompositeLayer, Compositor, CompositorConfig, CompositorLayer, GlyphsLayer,
    GpuContext, LayerContent, LayerEffects, LayerMask, LayerOutline, LayerPlacement, LayerStencil,
    LayerStyles, RgbaImage, SdfLayer, SdfParams, StencilCoverage, StencilMask, identity_instances,
    mask_kind,
};
use aria_core::{
    ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation,
    VideoFrame,
};
use aria_text::{TextRenderer, TextStyle};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const BLACK: [u8; 4] = [0, 0, 0, 255];
const PLATE: [u8; 4] = [200, 60, 30, 255];

/// Bundled OFL face so CI shapes real glyphs without system fonts.
const TEST_FONT: &[u8] = include_bytes!("../../aria-text/assets/Micro5-Regular.ttf");

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping stencil test: no GPU adapter ({e})");
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

fn render(comp: &mut Compositor, gpu: &GpuContext, items: &[CompositorLayer<'_>]) -> RgbaImage {
    comp.render_compositor_layers(gpu, &config(), items)
        .expect("render stencil case")
}

/// Readback difference allowed per channel: the cut content is stored at
/// 8 bits between the matte pass and the composite.
fn tolerance(gpu: &GpuContext) -> i32 {
    if gpu.is_software() { 4 } else { 3 }
}

/// A stencil drawing `masks` over the whole canvas, their image coverages
/// read from `images`.
fn full_stencil<'a>(masks: &'a [StencilMask], images: &'a [RgbaImage]) -> LayerStencil<'a> {
    LayerStencil {
        masks,
        images,
        placement: full(),
        blur_passes: &[],
    }
}

fn uniform(rgba: [u8; 4]) -> RgbaImage {
    RgbaImage::new(WIDTH, HEIGHT, rgba.repeat((WIDTH * HEIGHT) as usize))
}

/// Opaque white over the left half of the canvas, transparent over the right.
fn left_half() -> RgbaImage {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for _ in 0..HEIGHT {
        for x in 0..WIDTH {
            let alpha = if x < WIDTH / 2 { 255 } else { 0 };
            pixels.extend_from_slice(&[alpha, alpha, alpha, alpha]);
        }
    }
    RgbaImage::new(WIDTH, HEIGHT, pixels)
}

/// A `WIDTH×HEIGHT` NV12 frame with every pixel set to the same (Y, U, V).
fn nv12_uniform(y: u8, u: u8, v: u8) -> VideoFrame {
    let luma = Plane::new(
        vec![y; (WIDTH * HEIGHT) as usize],
        WIDTH as usize,
        HEIGHT as usize,
    );
    let chroma = Plane::new(
        [u, v].repeat((WIDTH / 2 * HEIGHT / 2) as usize),
        WIDTH as usize,
        (HEIGHT / 2) as usize,
    );
    VideoFrame::new(
        RationalTime::new(0, Rational::FPS_30),
        PixelFormat::Nv12,
        ColorSpace::BT709,
        (WIDTH, HEIGHT),
        Rect::from_size(WIDTH, HEIGHT),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![luma, chroma])),
    )
}

/// `image` with its premultiplied readback divided back to straight alpha, as
/// a bitmap layer reads it.
fn straight(image: &RgbaImage) -> RgbaImage {
    let mut pixels = image.pixels.clone();
    for px in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(px[3]);
        if alpha > 0 {
            for ch in &mut px[..3] {
                *ch = ((u32::from(*ch) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    RgbaImage::new(image.width, image.height, pixels)
}

#[track_caller]
fn assert_px(what: &str, image: &RgbaImage, x: u32, y: u32, want: [u8; 4], tol: i32) {
    let got = image.pixel(x, y);
    let off = (0..4).any(|ch| (i32::from(got[ch]) - i32::from(want[ch])).abs() > tol);
    assert!(
        !off,
        "{what}: ({x},{y}) reads {got:?}, want {want:?} within {tol}"
    );
}

#[track_caller]
fn assert_same(what: &str, got: &RgbaImage, want: &RgbaImage, tol: i32) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_px(what, got, x, y, want.pixel(x, y), tol);
        }
    }
}

/// The content kinds [`a_mask_cuts_every_content_kind_as_it_cuts_a_bitmap`]
/// draws, one per pipeline family. [`kind_of`] has no wildcard arm: a new kind
/// does not compile until it has a name, and fails the test until it has a
/// case.
const KINDS: [&str; 6] = ["frame", "bitmap", "solid", "shape", "glyphs", "nest"];

fn kind_of(content: &LayerContent<'_>) -> &'static str {
    match content {
        LayerContent::Frame(_) => "frame",
        LayerContent::Rgba(_) => "bitmap",
        LayerContent::Solid(_) => "solid",
        LayerContent::Sdf(_) => "shape",
        LayerContent::Glyphs(_) => "glyphs",
        LayerContent::Nest(_) => "nest",
    }
}

/// A mask cuts every kind of layer content as the fx mask cuts a bitmap of
/// the content, including content drawn by a pipeline with no fx variant.
///
/// A mask rode the fx pipeline alone, so a solid, shape or text layer drew
/// uncut. Not caught: a mask whose geometry differs from the fx mask's in a
/// way a centered ellipse does not show.
#[test]
fn a_mask_cuts_every_content_kind_as_it_cuts_a_bitmap() {
    let gpu = gpu_or_skip!();
    let full = full();
    let frame = nv12_uniform(150, 90, 170);
    let bitmap = uniform([40, 200, 90, 255]);
    let mut text = TextRenderer::new();
    assert!(text.load_font(TEST_FONT.to_vec()) > 0);
    let shaped = text.shape("HiW", &TextStyle::new(40.0).with_color([255, 220, 0, 255]));
    let glyphs: Vec<RgbaImage> = shaped.clusters.iter().map(|c| c.image.clone()).collect();
    let offsets: Vec<[f32; 2]> = shaped.clusters.iter().map(|c| c.offset).collect();
    let instances = identity_instances(&glyphs, &offsets, [2.0, 2.0], 1.0);
    let shape = SdfLayer {
        shape: SdfParams::RoundedRect { radius: 0.0 }
            .with_half([WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0]),
        fill: [30, 90, 220, 255],
        stroke: None,
    };
    // The nested picture is a plate the nest draws, which the canvas samples.
    let nested = CompositeLayer::solid([220, 40, 160, 255], full);
    let nest_items = [CompositorLayer::layer(&nested)];
    let nest = CompositorLayer::Nest {
        nest: 1,
        items: &nest_items,
    };
    let layers = [
        CompositeLayer::frame(&frame, full),
        CompositeLayer::rgba(&bitmap, full),
        CompositeLayer::solid(PLATE, full),
        CompositeLayer::sdf(shape, full),
        CompositeLayer::glyphs(
            GlyphsLayer {
                atlas_key: 1,
                glyphs: &glyphs,
                instances: &instances,
            },
            full,
        ),
        CompositeLayer::nest(1, full),
    ];
    let covered: Vec<&str> = layers.iter().map(|layer| kind_of(&layer.content)).collect();
    assert_eq!(covered, KINDS, "one case per content kind");

    let mask = LayerMask::new(mask_kind::CIRCLE);
    let masks = [StencilMask::keep(StencilCoverage::Shape(mask))];
    let mut comp = Compositor::new(&gpu);
    for layer in &layers {
        let what = kind_of(&layer.content);
        // A layer showing a nest renders after the nest it shows.
        let lead: &[CompositorLayer<'_>] = match layer.content {
            LayerContent::Nest(_) => std::slice::from_ref(&nest),
            _ => &[],
        };
        let alone = render(
            &mut comp,
            &gpu,
            &[lead, &[CompositorLayer::layer(layer)][..]].concat(),
        );
        let cut = layer.clone().with_stencil(full_stencil(&masks, &[]));
        let got = render(
            &mut comp,
            &gpu,
            &[lead, &[CompositorLayer::layer(&cut)][..]].concat(),
        );
        assert_ne!(got.pixels, alone.pixels, "{what}: the mask cut nothing");
        let reference = CompositeLayer::rgba(&alone, full).with_fx(LayerEffects {
            mask: Some(mask),
            chroma_key: None,
        });
        let want = render(&mut comp, &gpu, &[CompositorLayer::layer(&reference)]);
        assert_same(what, &got, &want, tolerance(&gpu));
    }
}

/// A mask cuts a layer before its styles: the outline follows the mask's
/// edge, and the plate outside the mask draws nothing.
///
/// Not caught: the order of a mask against the effect chain, which no style
/// shows.
#[test]
fn a_mask_cuts_before_the_layers_styles() {
    let gpu = gpu_or_skip!();
    let plate = LayerPlacement {
        center: [WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0],
        size: [40.0, 24.0],
        rotation: 0.0,
        opacity: 1.0,
    };
    let masks = [StencilMask::keep(StencilCoverage::Shape(LayerMask::new(
        mask_kind::CIRCLE,
    )))];
    let ellipse = LayerStencil {
        placement: plate,
        ..full_stencil(&masks, &[])
    };
    let styles = LayerStyles {
        outline: Some(LayerOutline {
            rgba: [255, 255, 0, 255],
            width: 3.0,
        }),
        ..LayerStyles::default()
    };
    let mut comp = Compositor::new(&gpu);
    let cut = CompositeLayer::solid(PLATE, plate).with_stencil(ellipse);
    let silhouette = comp
        .render_compositor_layers(
            &gpu,
            &CompositorConfig::new(WIDTH, HEIGHT).with_background([0, 0, 0, 0]),
            &[CompositorLayer::layer(&cut)],
        )
        .expect("render cut plate");
    let silhouette = straight(&silhouette);

    let styled = cut.clone().with_styles(styles);
    let got = render(&mut comp, &gpu, &[CompositorLayer::layer(&styled)]);
    // A plate corner, outside the ellipse and its outline.
    assert_px("plate corner", &got, 12, 12, BLACK, 0);
    let reference = CompositeLayer::rgba(&silhouette, full()).with_styles(styles);
    let want = render(&mut comp, &gpu, &[CompositorLayer::layer(&reference)]);
    assert_same("outlined cut", &got, &want, tolerance(&gpu));
}

/// An image mask keeps the layer where its alpha is opaque, or where it is
/// transparent when inverted.
#[test]
fn an_image_mask_keeps_where_its_alpha_is_opaque() {
    let gpu = gpu_or_skip!();
    let images = [left_half()];
    let mut comp = Compositor::new(&gpu);
    for invert in [false, true] {
        let masks = [StencilMask {
            invert,
            ..StencilMask::keep(StencilCoverage::Image(0))
        }];
        let stencil = full_stencil(&masks, &images);
        let layer = CompositeLayer::solid(PLATE, full()).with_stencil(stencil);
        let got = render(&mut comp, &gpu, &[CompositorLayer::layer(&layer)]);
        let (kept, cut) = if invert { (56, 8) } else { (8, 56) };
        let what = if invert { "inverted" } else { "upright" };
        assert_px(what, &got, kept, 24, PLATE, 1);
        assert_px(what, &got, cut, 24, BLACK, 1);
    }
}

/// A mask's motion-blur samples average its coverage: a mask sliding across
/// the canvas cuts a ramp, and a still mask a hard edge.
#[test]
fn a_masks_blur_samples_average_its_coverage() {
    let gpu = gpu_or_skip!();
    let images = [left_half()];
    let masks = [StencilMask::keep(StencilCoverage::Image(0))];
    let samples: Vec<LayerPlacement> = [-8.0, -4.0, 0.0, 4.0, 8.0]
        .into_iter()
        .map(|dx| LayerPlacement {
            center: [WIDTH as f32 / 2.0 + dx, HEIGHT as f32 / 2.0],
            ..full()
        })
        .collect();
    let white = [255, 255, 255, 255];
    let mut comp = Compositor::new(&gpu);
    let sliding = LayerStencil {
        blur_passes: &samples,
        ..full_stencil(&masks, &images)
    };
    let layer = CompositeLayer::solid(white, full()).with_stencil(sliding);
    let got = render(&mut comp, &gpu, &[CompositorLayer::layer(&layer)]);
    let tol = tolerance(&gpu);
    // The sampled edges sit at x = 24, 28, 32, 36 and 40: pixel 30 is kept by
    // three samples of five, pixel 20 by all, and pixel 44 by none.
    assert_px("ramp", &got, 30, 24, [153, 153, 153, 255], tol);
    assert_px("ramp start", &got, 20, 24, white, tol);
    assert_px("ramp end", &got, 44, 24, BLACK, tol);

    let still = CompositeLayer::solid(white, full()).with_stencil(full_stencil(&masks, &images));
    let got = render(&mut comp, &gpu, &[CompositorLayer::layer(&still)]);
    assert_px("still edge", &got, 30, 24, white, tol);
}

/// A masked canvas pass replaces the canvas inside its mask, keeps the canvas
/// outside it, and mixes the two across a soft edge without darkening.
#[test]
fn a_masked_canvas_pass_keeps_the_canvas_outside_its_mask() {
    let gpu = gpu_or_skip!();
    let images = [left_half()];
    let plate = CompositeLayer::solid(PLATE, full());
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let pass = |stencil| CompositorLayer::CanvasPass {
        effects: &[],
        grade: Some(grade),
        lut: None,
        stencil,
    };
    let mut comp = Compositor::new(&gpu);
    let plain = render(&mut comp, &gpu, &[CompositorLayer::layer(&plate)]);
    let graded = render(
        &mut comp,
        &gpu,
        &[CompositorLayer::layer(&plate), pass(None)],
    );
    assert_ne!(
        plain.pixel(8, 24),
        graded.pixel(8, 24),
        "the grade moved nothing"
    );
    let tol = tolerance(&gpu);

    let hard_masks = [StencilMask::keep(StencilCoverage::Image(0))];
    let hard = full_stencil(&hard_masks, &images);
    let got = render(
        &mut comp,
        &gpu,
        &[CompositorLayer::layer(&plate), pass(Some(&hard))],
    );
    assert_px("inside", &got, 8, 24, graded.pixel(8, 24), tol);
    assert_px("outside", &got, 56, 24, plain.pixel(56, 24), tol);

    let soft_masks = [StencilMask {
        softness: 0.5,
        ..hard_masks[0]
    }];
    let soft = full_stencil(&soft_masks, &images);
    let got = render(
        &mut comp,
        &gpu,
        &[CompositorLayer::layer(&plate), pass(Some(&soft))],
    );
    let mut mixed = false;
    for x in 0..WIDTH {
        let (have, a, b) = (got.pixel(x, 24), plain.pixel(x, 24), graded.pixel(x, 24));
        for ch in 0..3 {
            let (low, high) = (i32::from(a[ch].min(b[ch])), i32::from(a[ch].max(b[ch])));
            let value = i32::from(have[ch]);
            assert!(
                (low - tol..=high + tol).contains(&value),
                "soft edge at x {x} reads {have:?}, outside the canvas {a:?} and the pass {b:?}"
            );
        }
        assert_eq!(
            have[3], 255,
            "soft edge at x {x} reads {have:?}: the canvas lost alpha"
        );
        mixed |= (0..3).all(|ch| have[ch] != a[ch] && have[ch] != b[ch]);
    }
    assert!(
        mixed,
        "the soft edge mixed no pixel of the canvas and the pass"
    );
}
