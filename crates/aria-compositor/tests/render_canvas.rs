//! Canvas basics: a cleared background, solid fills, placement, opacity,
//! and the reuse of the cached render target across frames.

use super::*;

#[test]
fn solid_fill_covers_canvas() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(32, 32); // default black background
    let red = CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config));
    let img = comp.render(&gpu, &config, &[red]).expect("render");

    assert_px(&img, 0, 0, [255, 0, 0, 255], 1);
    assert_px(&img, 16, 16, [255, 0, 0, 255], 1);
    assert_px(&img, 31, 31, [255, 0, 0, 255], 1);
}

#[test]
fn empty_scene_is_background() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(8, 8).with_background([10, 20, 30, 255]);
    let img = comp.render(&gpu, &config, &[]).expect("render");
    assert_px(&img, 4, 4, [10, 20, 30, 255], 1);
}

#[test]
fn repeated_renders_reuse_the_cached_target() {
    // `render` caches the canvas texture + readback buffer across calls (the
    // preview renders one size every frame). Same-size renders must stay
    // independent — the pass clears, so no state leaks between frames — and a
    // size change must rebuild the target at the new dimensions.
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);

    let config = CompositorConfig::new(32, 32).with_background([10, 20, 30, 255]);
    let first = comp.render(&gpu, &config, &[]).expect("render 1");
    assert_px(&first, 16, 16, [10, 20, 30, 255], 1);

    // Second render at the same size (cache hit) with different content.
    let red = CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&config));
    let second = comp.render(&gpu, &config, &[red]).expect("render 2");
    assert_px(&second, 16, 16, [255, 0, 0, 255], 1);

    // Third render back to an empty scene: nothing of the red frame survives.
    let third = comp.render(&gpu, &config, &[]).expect("render 3");
    assert_px(&third, 16, 16, [10, 20, 30, 255], 1);

    // Size change (also exercises a non-256-multiple row pitch): the target
    // is rebuilt and the readback matches the new dimensions.
    let smaller = CompositorConfig::new(17, 9).with_background([1, 2, 3, 255]);
    let resized = comp.render(&gpu, &smaller, &[]).expect("render 4");
    assert_eq!((resized.width, resized.height), (17, 9));
    assert_px(&resized, 16, 8, [1, 2, 3, 255], 1);

    // And back to the original size.
    let again = comp.render(&gpu, &config, &[]).expect("render 5");
    assert_eq!((again.width, again.height), (32, 32));
    assert_px(&again, 31, 31, [10, 20, 30, 255], 1);
}

#[test]
fn solid_placement_lands_in_the_right_quadrant() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(100, 100); // black background

    // A 20×20 red square centered in the top-left quadrant at (25, 25).
    let placement = LayerPlacement {
        center: [25.0, 25.0],
        size: [20.0, 20.0],
        rotation: 0.0,
        opacity: 1.0,
    };
    let layer = CompositeLayer::solid([255, 0, 0, 255], placement);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    assert_px(&img, 25, 25, [255, 0, 0, 255], 1); // inside, top-left
    assert_px(&img, 75, 25, [0, 0, 0, 255], 1); // top-right: background
    assert_px(&img, 25, 75, [0, 0, 0, 255], 1); // bottom-left: background
    assert_px(&img, 75, 75, [0, 0, 0, 255], 1); // bottom-right: background
}

#[test]
fn opacity_blends_over_background() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // Opaque blue canvas, half-opacity red on top → ~purple (gamma-space blend).
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 255, 255]);
    let mut placement = LayerPlacement::full_canvas(&config);
    placement.opacity = 0.5;
    let layer = CompositeLayer::solid([255, 0, 0, 255], placement);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    assert_px(&img, 8, 8, [128, 0, 128, 255], 2);
}
