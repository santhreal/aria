//! GPU readback checks for the shipped generators.
//!
//! The class these close: a generator that compiles, dispatches and draws the
//! wrong picture, or draws nothing over the transparent solid that carries it.
//! Every generator in the shipped catalog is enumerated at run time and has to
//! have a case in [`CASES`], so a generator added to the catalog without a
//! check of what it draws fails [`every_shipped_generator_has_a_case`].
//!
//! Each case renders at a small size with the catalog's default parameters,
//! unless it states its own, and reads pixels whose value follows from the
//! shader's definition. Sizes are in pixels of a 1080-pixel-high frame, so the
//! expected cell and line positions are worked out from the frame height here.
//!
//! Not covered: how a generator looks on a monitor, and noise beyond its
//! determinism, range and dependence on its parameters.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, PassInstance,
    RgbaImage,
};
use aria_models::EffectParamKind;

const W: u32 = 128;
const H: u32 = 72;

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping generator test: no GPU adapter ({e})");
            None
        }
    }
}

/// The catalog's default for every parameter of `id`, flattened in catalog
/// order the way the engine packs an instance.
fn defaults(id: &str) -> Vec<f32> {
    let spec = aria_models::effect_spec(id).unwrap_or_else(|| panic!("{id} is in the catalog"));
    let mut floats = Vec::new();
    for param in &spec.params {
        match param.kind {
            EffectParamKind::Color => floats.extend(param.default_color_linear()),
            EffectParamKind::Vec2 => floats.extend(param.default_vec2),
            _ => floats.push(param.default),
        }
    }
    floats
}

/// One device and one compositor for every draw, so the pipelines are built
/// once.
struct Rig {
    gpu: GpuContext,
    comp: Compositor,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = try_gpu()?;
        let comp = Compositor::new(&gpu);
        Some(Self { gpu, comp })
    }

    /// Render `id` with `params` over a transparent solid on an opaque black
    /// `width` x `height` background.
    fn draw_at(
        &mut self,
        width: u32,
        height: u32,
        id: &'static str,
        params: Vec<f32>,
    ) -> RgbaImage {
        let config = CompositorConfig::new(width, height).with_background([0, 0, 0, 255]);
        let effects = [PassInstance::new(id, params)];
        let layer = CompositeLayer::solid([0, 0, 0, 0], LayerPlacement::full_canvas(&config))
            .with_effects(&effects);
        self.comp
            .render(&self.gpu, &config, &[layer])
            .unwrap_or_else(|err| panic!("{id} renders: {err:?}"))
    }

    fn draw(&mut self, id: &'static str, params: Vec<f32>) -> RgbaImage {
        self.draw_at(W, H, id, params)
    }
}

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32, what: &str) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "{what}: pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d})"
        );
    }
}

/// A grey level, opaque.
const fn grey(v: u8) -> [u8; 4] {
    [v, v, v, 255]
}

fn color_matte(rig: &mut Rig) {
    let img = rig.draw("color_matte", defaults("color_matte"));
    for (x, y) in [(0, 0), (W / 2, H / 2), (W - 1, H - 1)] {
        assert_px(&img, x, y, grey(128), 1, "the default matte");
    }
    // Half-transparent red over black: the matte keeps its alpha.
    let img = rig.draw("color_matte", vec![1.0, 0.0, 0.0, 0.5]);
    assert_px(
        &img,
        W / 2,
        H / 2,
        [128, 0, 0, 255],
        2,
        "a half-transparent matte",
    );
}

fn linear_gradient(rig: &mut Rig) {
    let img = rig.draw("linear_gradient", defaults("linear_gradient"));
    // Black at the left edge to white at the right, the same down a column.
    // Pixel centres sit half a pixel in from each edge.
    let at = |x: u32| ((x as f32 + 0.5) / W as f32 * 255.0).round() as u8;
    for x in [0, W / 4, W / 2, W - 1] {
        assert_px(&img, x, 0, grey(at(x)), 2, "along the default ramp");
        assert_px(&img, x, H - 1, grey(at(x)), 2, "down a column of the ramp");
    }
    // A vertical ramp, white at the top: the points are fractions of the frame.
    let mut params = defaults("linear_gradient");
    params[8..12].copy_from_slice(&[0.5, 1.0, 0.5, 0.0]);
    let img = rig.draw("linear_gradient", params);
    assert_px(&img, 0, 0, grey(253), 3, "the end of a vertical ramp");
    assert_px(
        &img,
        W - 1,
        H - 1,
        grey(2),
        3,
        "the start of a vertical ramp",
    );
    // Start and end at one point: no direction, the start colour throughout.
    let mut params = defaults("linear_gradient");
    params[8..12].copy_from_slice(&[0.5, 0.5, 0.5, 0.5]);
    let img = rig.draw("linear_gradient", params);
    assert_px(&img, W - 1, 0, grey(0), 1, "a ramp of no length");
}

fn radial_gradient(rig: &mut Rig) {
    let img = rig.draw("radial_gradient", defaults("radial_gradient"));
    // White at the centre, black from half the frame height out.
    assert_px(&img, W / 2, H / 2, grey(255), 6, "the center");
    assert_px(&img, 0, 0, grey(0), 1, "a corner past the radius");
    // Halfway out along x: 18 px of a 36 px radius.
    assert_px(&img, W / 2 + 18, H / 2, grey(128), 6, "half the radius out");
    // Round on a wide frame: the same distance up reads the same.
    assert_px(&img, W / 2, H / 2 - 18, grey(128), 6, "half the radius up");
}

fn checkerboard(rig: &mut Rig) {
    let img = rig.draw("checkerboard", defaults("checkerboard"));
    // 60 px cells of a 1080 px frame are 4 px at 72, counted from the centre.
    let (cx, cy) = (W / 2, H / 2);
    assert_px(
        &img,
        cx,
        cy,
        grey(255),
        1,
        "the cell right of and below the anchor",
    );
    assert_px(&img, cx + 4, cy, grey(0), 1, "the next cell right");
    assert_px(&img, cx - 1, cy, grey(0), 1, "the cell left of the anchor");
    assert_px(&img, cx, cy + 4, grey(0), 1, "the next cell down");
    assert_px(&img, cx + 4, cy + 4, grey(255), 1, "diagonally across");
    // A larger cell moves the edge.
    let mut params = defaults("checkerboard");
    params[8] = 120.0;
    let img = rig.draw("checkerboard", params);
    assert_px(
        &img,
        cx + 4,
        cy,
        grey(255),
        1,
        "an 8 px cell still covers 4 px right",
    );
}

fn grid(rig: &mut Rig) {
    // 270 px spacing and 60 px lines of a 1080 px frame: 18 px and 4 px at 72.
    let mut params = defaults("grid");
    params[4] = 270.0;
    params[5] = 60.0;
    let img = rig.draw("grid", params);
    let (cx, cy) = (W / 2, H / 2);
    assert_px(&img, cx, 5, grey(255), 1, "on the line through the anchor");
    assert_px(&img, cx + 18, 5, grey(255), 1, "on the next vertical line");
    assert_px(&img, 5, cy + 18, grey(255), 1, "on a horizontal line");
    assert_px(
        &img,
        cx + 9,
        cy + 9,
        grey(0),
        1,
        "between lines, the layer is clear",
    );
    // No width, no lines.
    let mut params = defaults("grid");
    params[4] = 270.0;
    params[5] = 0.0;
    let img = rig.draw("grid", params);
    assert_px(&img, cx, cy, grey(0), 1, "a line of no width");
}

fn fractal_noise(rig: &mut Rig) {
    let params = defaults("fractal_noise");
    let img = rig.draw("fractal_noise", params.clone());
    let again = rig.draw("fractal_noise", params.clone());
    assert_eq!(img.pixels, again.pixels, "one setting draws one picture");
    let mut levels = std::collections::BTreeSet::new();
    for pixel in img.pixels.as_chunks::<4>().0 {
        assert_eq!(pixel[3], 255, "the noise is opaque");
        assert!(
            pixel[0] == pixel[1] && pixel[1] == pixel[2],
            "between black and white the noise is gray: {pixel:?}"
        );
        levels.insert(pixel[0]);
    }
    assert!(
        levels.len() > 32,
        "the noise varies: {} levels",
        levels.len()
    );

    for (slot, value, what) in [
        (9, 0.5, "evolution"),
        (12, 0.3, "offset"),
        (8, 60.0, "scale"),
    ] {
        let mut moved = params.clone();
        moved[slot] = value;
        assert_ne!(
            rig.draw("fractal_noise", moved).pixels,
            img.pixels,
            "{what} changes the picture"
        );
    }
    // No contrast: every pixel is the midpoint of the two colours.
    let mut flat = params;
    flat[11] = 0.0;
    let img = rig.draw("fractal_noise", flat);
    assert_px(&img, 3, 3, grey(128), 1, "noise with no contrast");
    assert_px(&img, W - 3, H - 3, grey(128), 1, "noise with no contrast");
}

fn color_bars(rig: &mut Rig) {
    let img = rig.draw("color_bars", defaults("color_bars"));
    let bar = |index: u32| index * W / 7 + W / 14;
    let top = H / 3;
    // 75% bars: 0.75 of 255.
    let l = 191;
    for (index, rgb) in [
        (0, [l, l, l]),
        (1, [l, l, 0]),
        (2, [0, l, l]),
        (3, [0, l, 0]),
        (4, [l, 0, l]),
        (5, [l, 0, 0]),
        (6, [0, 0, l]),
    ] {
        assert_px(
            &img,
            bar(index),
            top,
            [rgb[0], rgb[1], rgb[2], 255],
            2,
            "a top bar",
        );
    }
    // The reversed row under the bars, and full white in the bottom quarter.
    assert_px(
        &img,
        bar(0),
        H * 17 / 24,
        [0, 0, l, 255],
        2,
        "the reversed row",
    );
    assert_px(&img, bar(1), H * 17 / 24, grey(0), 1, "the reversed row");
    assert_px(&img, W * 15 / 56, H - 2, grey(255), 1, "full white");
    // 100% bars.
    let img = rig.draw("color_bars", vec![1.0]);
    assert_px(
        &img,
        bar(1),
        top,
        [255, 255, 0, 255],
        1,
        "a 100% yellow bar",
    );
}

/// One check per shipped generator, by catalog id.
const CASES: &[(&str, fn(&mut Rig))] = &[
    ("color_matte", color_matte),
    ("linear_gradient", linear_gradient),
    ("radial_gradient", radial_gradient),
    ("checkerboard", checkerboard),
    ("grid", grid),
    ("fractal_noise", fractal_noise),
    ("color_bars", color_bars),
];

/// The shipped catalog's generators.
fn shipped_generators() -> Vec<String> {
    aria_models::shipped_effect_catalog()
        .iter()
        .filter(|spec| spec.generator)
        .map(|spec| spec.id.clone())
        .collect()
}

#[test]
fn every_shipped_generator_has_a_case() {
    let shipped = shipped_generators();
    assert!(!shipped.is_empty(), "the catalog ships generators");
    let cased: Vec<&str> = CASES.iter().map(|(id, _)| *id).collect();
    for id in &shipped {
        assert!(
            cased.contains(&id.as_str()),
            "{id} ships with no check of what it draws: add a case"
        );
    }
    for id in cased {
        assert!(
            shipped.iter().any(|shipped| shipped == id),
            "{id} has a case but is no shipped generator"
        );
    }
}

#[test]
fn every_shipped_generator_draws_its_picture() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    for (_, case) in CASES {
        case(&mut rig);
    }
}

#[test]
fn a_generator_draws_the_same_picture_at_any_size() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    // The same checkerboard at twice the size: the cell doubles with it.
    let big = rig.draw_at(W * 2, H * 2, "checkerboard", defaults("checkerboard"));
    let small = rig.draw("checkerboard", defaults("checkerboard"));
    for (x, y) in [(10, 10), (W / 2 + 5, H / 2 - 3), (W - 3, H - 3)] {
        assert_eq!(
            big.pixel(x * 2, y * 2),
            small.pixel(x, y),
            "({x},{y}) at twice the size"
        );
    }
}
