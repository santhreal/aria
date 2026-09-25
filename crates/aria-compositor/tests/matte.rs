//! GPU readback checks for the track-matte pass.
//!
//! The class closed here is "the stencil decides where the content survives".
//! Each knob is checked by where pixels land: the read channel (an opaque black
//! plate cuts under luma but not under alpha), inversion (the same stencil keeps
//! the other side), and softness (a hard edge becomes a ramp, and zero softness
//! leaves it hard). A matte that merely dimmed the whole frame would pass a
//! "looks translucent" check and fail all of these.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, GpuContext, LayerPlacement,
    MatteRead, RgbaImage,
};

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping matte test: no GPU adapter ({e})");
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

const W: u32 = 64;
const H: u32 = 64;

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

fn solid(rgba: [u8; 4]) -> RgbaImage {
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..W * H {
        pixels.extend_from_slice(&rgba);
    }
    RgbaImage::new(W, H, pixels)
}

/// Opaque `left` for the first half of every row, `right` for the second: a hard
/// vertical edge at x = W/2.
fn split(left: [u8; 4], right: [u8; 4]) -> RgbaImage {
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for _y in 0..H {
        for x in 0..W {
            pixels.extend_from_slice(if x < W / 2 { &left } else { &right });
        }
    }
    RgbaImage::new(W, H, pixels)
}

/// Cut `content` by `stencil` and read the canvas back.
fn cut(
    gpu: &GpuContext,
    comp: &mut Compositor,
    content: &RgbaImage,
    stencil: &RgbaImage,
    read: MatteRead,
    invert: bool,
    softness: f32,
) -> RgbaImage {
    // A transparent background, so a cut pixel reads as a hole rather than as
    // whatever the plate happened to be.
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 0]);
    let placement = LayerPlacement::full_canvas(&config);
    let content_layer = CompositeLayer::rgba(content, placement);
    let stencil_layer = CompositeLayer::rgba(stencil, placement);
    let items = [CompositorLayer::Matte {
        content: &content_layer,
        stencil: &stencil_layer,
        read,
        invert,
        softness,
    }];
    comp.render_compositor_layers(gpu, &config, &items)
        .expect("matte")
}

#[test]
fn an_alpha_matte_keeps_the_content_only_where_the_stencil_is_opaque() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let content = solid([255, 0, 0, 255]);
    let stencil = split([0, 0, 0, 255], [0, 0, 0, 0]);

    let img = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Alpha,
        false,
        0.0,
    );
    assert_px(&img, 8, 32, [255, 0, 0, 255], 3);
    assert_px(&img, 56, 32, [0, 0, 0, 0], 3);
}

#[test]
fn a_luma_matte_reads_brightness_where_an_alpha_matte_reads_nothing() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let content = solid([255, 0, 0, 255]);
    // Both halves are opaque, so alpha cannot tell them apart; luma can.
    let stencil = split([255, 255, 255, 255], [0, 0, 0, 255]);

    let alpha = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Alpha,
        false,
        0.0,
    );
    assert_px(&alpha, 8, 32, [255, 0, 0, 255], 3);
    assert_px(&alpha, 56, 32, [255, 0, 0, 255], 3);

    let luma = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Luma,
        false,
        0.0,
    );
    assert_px(&luma, 8, 32, [255, 0, 0, 255], 3);
    assert_px(&luma, 56, 32, [0, 0, 0, 0], 3);
}

#[test]
fn a_mid_grey_luma_stencil_holds_the_content_half_way() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let content = solid([255, 255, 255, 255]);
    // 0.5 in linear-ish byte terms: Rec.709 luma of a neutral grey is the grey.
    let stencil = solid([128, 128, 128, 255]);

    let img = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Luma,
        false,
        0.0,
    );
    // Premultiplied output: colour and alpha are both scaled by coverage.
    assert_px(&img, 32, 32, [128, 128, 128, 128], 3);
}

#[test]
fn inverting_a_matte_keeps_the_other_side() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let content = solid([0, 255, 0, 255]);
    let stencil = split([0, 0, 0, 255], [0, 0, 0, 0]);

    let img = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Alpha,
        true,
        0.0,
    );
    assert_px(&img, 8, 32, [0, 0, 0, 0], 3);
    assert_px(&img, 56, 32, [0, 255, 0, 255], 3);
}

#[test]
fn softness_turns_a_hard_stencil_edge_into_a_ramp() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let content = solid([255, 255, 255, 255]);
    let stencil = split([0, 0, 0, 255], [0, 0, 0, 0]);

    let hard = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Alpha,
        false,
        0.0,
    );
    // Zero softness leaves the edge where the stencil put it.
    assert_px(&hard, W / 2 - 1, 32, [255, 255, 255, 255], 3);
    assert_px(&hard, W / 2, 32, [0, 0, 0, 0], 3);

    let soft = cut(
        &gpu,
        &mut comp,
        &content,
        &stencil,
        MatteRead::Alpha,
        false,
        1.0,
    );
    // The edge is now a ramp: partial on both sides of where it sat, and it
    // still resolves to solid and to nothing away from the edge.
    let inside = soft.pixel(W / 2 - 1, 32)[3];
    let outside = soft.pixel(W / 2, 32)[3];
    assert!(
        (1..255).contains(&inside),
        "inside the edge should be partial, got alpha {inside}"
    );
    assert!(
        (1..255).contains(&outside),
        "outside the edge should be partial, got alpha {outside}"
    );
    assert!(
        inside > outside,
        "the ramp must still fall from the kept side to the cut side ({inside} vs {outside})"
    );
    assert_px(&soft, 4, 32, [255, 255, 255, 255], 4);
    assert_px(&soft, W - 5, 32, [0, 0, 0, 0], 4);
}
