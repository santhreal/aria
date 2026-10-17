//! GPU readback checks for nested sequences: a nest draws its items into a
//! target of its own, and a layer shows that target as its picture.
//!
//! Catches: a nest that draws only some of its items, a nest layer that drops
//! its own placement or opacity, a canvas pass inside a nest that reaches the
//! canvas under it, a nest that cannot show another nest, and a layer showing
//! a nest no item draws that renders instead of failing. Not caught: the
//! resolver choosing what a nest holds, which `aria-render` checks.

use aria_compositor::{
    ColorGrade, CompositeLayer, Compositor, CompositorConfig, CompositorError, CompositorLayer,
    GpuContext, LayerPlacement, RgbaImage,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 210, 255];
const GREEN: [u8; 4] = [40, 200, 70, 255];

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping nest test: no GPU adapter ({e})");
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

/// The rectangle `x, y, w, h` in canvas pixels.
fn quad(x: f32, y: f32, w: f32, h: f32) -> LayerPlacement {
    LayerPlacement {
        center: [x + w / 2.0, y + h / 2.0],
        size: [w, h],
        rotation: 0.0,
        opacity: 1.0,
    }
}

fn render(
    comp: &mut Compositor,
    gpu: &GpuContext,
    items: &[CompositorLayer<'_>],
) -> Result<RgbaImage, CompositorError> {
    comp.render_compositor_layers(gpu, &config(), items)
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

/// A nest draws every item it holds, so the layer showing it at the full
/// canvas reads what the items draw on the canvas directly.
#[test]
fn a_layer_shows_every_item_its_nest_draws() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let under = CompositeLayer::solid(RED, full());
    let over = CompositeLayer::solid(BLUE, quad(0.0, 0.0, 32.0, 24.0));
    let items = [
        CompositorLayer::layer(&under),
        CompositorLayer::layer(&over),
    ];
    let direct = render(&mut comp, &gpu, &items).expect("render items");

    let shown = CompositeLayer::nest(1, full());
    let nested = render(
        &mut comp,
        &gpu,
        &[
            CompositorLayer::Nest {
                nest: 1,
                items: &items,
            },
            CompositorLayer::layer(&shown),
        ],
    )
    .expect("render nest");
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_px("nest", &nested, x, y, direct.pixel(x, y), 1);
        }
    }
    assert_px("upper item", &nested, 8, 8, BLUE, 1);
    assert_px("lower item", &nested, 48, 36, RED, 1);
}

/// The layer showing a nest places the whole picture in its own quad, at its
/// own opacity.
#[test]
fn a_nest_layer_takes_its_own_placement_and_opacity() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let plate = CompositeLayer::solid(RED, full());
    let items = [CompositorLayer::layer(&plate)];
    let placement = LayerPlacement {
        opacity: 0.5,
        ..quad(16.0, 12.0, 32.0, 24.0)
    };
    let shown = CompositeLayer::nest(1, placement);
    let image = render(
        &mut comp,
        &gpu,
        &[
            CompositorLayer::Nest {
                nest: 1,
                items: &items,
            },
            CompositorLayer::layer(&shown),
        ],
    )
    .expect("render nest");
    assert_px("inside the quad", &image, 32, 24, [110, 15, 20, 255], 2);
    assert_px("outside the quad", &image, 4, 4, BLACK, 0);
    assert_px("outside the quad", &image, 60, 44, BLACK, 0);
}

/// A canvas pass inside a nest grades what the nest has drawn, and nothing
/// the canvas holds under the layer that shows it.
#[test]
fn a_canvas_pass_inside_a_nest_grades_the_nest_alone() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let grade = ColorGrade {
        saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let plate = CompositeLayer::solid(RED, full());
    let items = [
        CompositorLayer::layer(&plate),
        CompositorLayer::CanvasPass {
            effects: &[],
            grade: Some(grade),
            lut: None,
            stencil: None,
        },
    ];
    let beside = CompositeLayer::solid(GREEN, quad(32.0, 0.0, 32.0, 48.0));
    let shown = CompositeLayer::nest(1, quad(0.0, 0.0, 32.0, 48.0));
    let image = render(
        &mut comp,
        &gpu,
        &[
            CompositorLayer::Nest {
                nest: 1,
                items: &items,
            },
            CompositorLayer::layer(&beside),
            CompositorLayer::layer(&shown),
        ],
    )
    .expect("render nest");
    assert_px("the canvas beside the nest", &image, 48, 24, GREEN, 0);
    let gray = image.pixel(16, 24);
    assert!(
        gray[0].abs_diff(gray[1]) <= 2 && gray[1].abs_diff(gray[2]) <= 2 && gray[0] > 20,
        "the nest reads {gray:?}, want the plate desaturated to gray"
    );
}

/// A nest shows another nest drawn before it, as a sequence nested in a
/// nested sequence does.
#[test]
fn a_nest_shows_a_nest_drawn_before_it() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let inner_plate = CompositeLayer::solid(BLUE, full());
    let inner_items = [CompositorLayer::layer(&inner_plate)];
    let outer_plate = CompositeLayer::solid(RED, full());
    let inner_shown = CompositeLayer::nest(2, quad(0.0, 0.0, 32.0, 24.0));
    let outer_items = [
        CompositorLayer::layer(&outer_plate),
        CompositorLayer::layer(&inner_shown),
    ];
    let shown = CompositeLayer::nest(1, full());
    let image = render(
        &mut comp,
        &gpu,
        &[
            CompositorLayer::Nest {
                nest: 2,
                items: &inner_items,
            },
            CompositorLayer::Nest {
                nest: 1,
                items: &outer_items,
            },
            CompositorLayer::layer(&shown),
        ],
    )
    .expect("render nests");
    assert_px("the inner nest", &image, 8, 8, BLUE, 1);
    assert_px("the outer nest", &image, 48, 36, RED, 1);
}

/// A layer showing a nest that no item of the frame draws fails, naming the
/// nest, instead of drawing whatever its target held.
#[test]
fn a_layer_showing_an_undrawn_nest_fails_naming_it() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let shown = CompositeLayer::nest(9, full());
    let error = render(&mut comp, &gpu, &[CompositorLayer::layer(&shown)])
        .expect_err("an undrawn nest renders");
    assert!(
        matches!(error, CompositorError::UndrawnNest(9)),
        "{error:?}"
    );
    assert!(error.to_string().contains("nest 9"), "{error}");
}
