//! RGBA bitmap layers: premultiplied blending, layer opacity, placement,
//! chroma key, masks, and the undersized-bitmap rejection.

use super::*;

#[test]
fn rgba_opaque_layer_draws_over_background() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 0, 255]);
    let bmp = rgba_uniform(16, 16, [10, 200, 60, 255]);
    let layer = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    assert_px(&img, 8, 8, [10, 200, 60, 255], 1);
}

#[test]
fn rgba_fully_transparent_shows_background() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(16, 16).with_background([7, 8, 9, 255]);
    // Opaque-colored RGB but zero alpha: must not tint the background.
    let bmp = rgba_uniform(16, 16, [255, 0, 0, 0]);
    let layer = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    assert_px(&img, 8, 8, [7, 8, 9, 255], 1);
}

#[test]
fn rgba_semi_alpha_blends_premultiplied() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // 50%-alpha red over opaque blue → ~half-and-half (premultiplied src-over).
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 255, 255]);
    let bmp = rgba_uniform(16, 16, [255, 0, 0, 128]);
    let layer = CompositeLayer::rgba(&bmp, LayerPlacement::full_canvas(&config));
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    assert_px(&img, 8, 8, [128, 0, 127, 255], 3);
}

#[test]
fn rgba_layer_opacity_scales_coverage() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // Opaque red bitmap, but the *layer* opacity is 0.5 → same as 50% alpha.
    let config = CompositorConfig::new(16, 16).with_background([0, 0, 255, 255]);
    let bmp = rgba_uniform(16, 16, [255, 0, 0, 255]);
    let mut placement = LayerPlacement::full_canvas(&config);
    placement.opacity = 0.5;
    let layer = CompositeLayer::rgba(&bmp, placement);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    assert_px(&img, 8, 8, [128, 0, 127, 255], 3);
}

#[test]
fn rgba_placement_lands_in_the_right_quadrant() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(100, 100); // black background

    let bmp = rgba_uniform(8, 8, [255, 255, 255, 255]);
    let placement = LayerPlacement {
        center: [25.0, 25.0],
        size: [20.0, 20.0],
        rotation: 0.0,
        opacity: 1.0,
    };
    let layer = CompositeLayer::rgba(&bmp, placement);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    assert_px(&img, 25, 25, [255, 255, 255, 255], 1); // inside, top-left
    assert_px(&img, 75, 25, [0, 0, 0, 255], 1);
    assert_px(&img, 25, 75, [0, 0, 0, 255], 1);
}

#[test]
fn chroma_key_keys_green_nv12_to_transparent() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 255, 255]);
    // Full-range BT.709 green (#00FF00).
    let full = ColorSpace {
        range: ColorRange::Full,
        ..ColorSpace::BT709
    };
    let f = nv12_uniform(64, 64, 182, 30, 12, full);
    let placement = LayerPlacement::full_canvas(&config);
    let effects = LayerEffects {
        mask: None,
        chroma_key: Some(LayerChromaKey {
            rgb: [0.0, 1.0, 0.0],
            strength: 0.5,
            shadow: 0.0,
        }),
    };
    let layer = CompositeLayer::frame(&f, placement).with_fx(effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");
    // Keyed-out green reveals the blue background.
    assert_px(&img, 32, 32, [0, 0, 255, 255], 8);
}

#[test]
fn combined_mask_and_chroma_on_rgba_layer() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 255, 255]);
    let bmp = rgba_uniform(64, 64, [0, 255, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let effects = LayerEffects {
        mask: Some(LayerMask::new(mask_kind::CIRCLE)),
        chroma_key: Some(LayerChromaKey {
            rgb: [0.0, 1.0, 0.0],
            strength: 0.5,
            shadow: 0.0,
        }),
    };
    let layer = CompositeLayer::rgba(&bmp, placement).with_fx(effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    // Center: inside circle but green keyed out → background.
    assert_px(&img, 32, 32, [0, 0, 255, 255], 8);
    // Corner: outside circle → background regardless.
    assert_px(&img, 2, 2, [0, 0, 255, 255], 2);
}

#[test]
fn circle_mask_cuts_rgba_corners() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(64, 64).with_background([0, 0, 255, 255]);
    let bmp = rgba_uniform(64, 64, [255, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let effects = LayerEffects {
        mask: Some(LayerMask::new(mask_kind::CIRCLE)),
        chroma_key: None,
    };
    let layer = CompositeLayer::rgba(&bmp, placement).with_fx(effects);
    let img = comp.render(&gpu, &config, &[layer]).expect("render");

    // Center is inside the circle mask.
    assert_px(&img, 32, 32, [255, 0, 0, 255], 2);
    // Corners are outside the circle — background shows through.
    assert_px(&img, 2, 2, [0, 0, 255, 255], 2);
    assert_px(&img, 61, 61, [0, 0, 255, 255], 2);
}

#[test]
fn rgba_undersized_bitmap_is_rejected() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(8, 8);
    // Claims 4×4 (64 bytes) but only carries 16.
    let bad = RgbaImage::new(4, 4, vec![0u8; 16]);
    let layer = CompositeLayer::rgba(&bad, LayerPlacement::full_canvas(&config));
    assert!(comp.render(&gpu, &config, &[layer]).is_err());
}
