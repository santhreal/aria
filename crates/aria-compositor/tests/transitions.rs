//! GPU readback checks for the transition passes.
//!
//! Every id in `shipped_transition_ids()` renders its own way — a dip passes through a
//! solid frame, a wipe has a hard edge, a slide moves both frames — so each one
//! is checked by where its pixels land at a known progress.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, GpuContext, LayerPlacement,
    RgbaImage,
};

/// Try to bring up a headless GPU; `None` (skip) if the host has no adapter.
fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping transition test: no GPU adapter ({e})");
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

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&rgba);
    }
    RgbaImage::new(width, height, pixels)
}

/// Dark on the left, bright on the right, so a horizontal move is measurable.
fn horizontal_gradient(width: u32, height: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            let v = ((x as f32 / (width - 1) as f32) * 255.0).round() as u8;
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    RgbaImage::new(width, height, pixels)
}

const W: u32 = 64;
const H: u32 = 64;

/// Blend `outgoing` into `incoming` through `id` at `progress`.
fn blend(
    gpu: &GpuContext,
    comp: &mut Compositor,
    id: &str,
    outgoing: &RgbaImage,
    incoming: &RgbaImage,
    progress: f32,
) -> RgbaImage {
    let config = CompositorConfig::new(W, H).with_background([0, 0, 0, 255]);
    let placement = LayerPlacement::full_canvas(&config);
    let out_layer = CompositeLayer::rgba(outgoing, placement);
    let in_layer = CompositeLayer::rgba(incoming, placement);
    let items = [CompositorLayer::Transition {
        outgoing: &out_layer,
        incoming: &in_layer,
        transition_id: id,
        progress,
    }];
    comp.render_compositor_layers(gpu, &config, &items)
        .expect(id)
}

#[test]
fn crossfade_mixes_both_frames_at_the_midpoint() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let black = solid(W, H, [0, 0, 0, 255]);
    let white = solid(W, H, [255, 255, 255, 255]);

    let img = blend(&gpu, &mut comp, "crossfade", &black, &white, 0.5);
    assert_px(&img, 32, 32, [128, 128, 128, 255], 3);
}

#[test]
fn dip_to_black_passes_through_an_opaque_black_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let grey = solid(W, H, [200, 200, 200, 255]);
    let red = solid(W, H, [255, 0, 0, 255]);

    // Midpoint is the dip colour itself, not a mix of the two clips.
    let mid = blend(&gpu, &mut comp, "dip_to_black", &grey, &red, 0.5);
    assert_px(&mid, 32, 32, [0, 0, 0, 255], 3);

    // Quarter in, the outgoing frame is half way to the dip colour.
    let early = blend(&gpu, &mut comp, "dip_to_black", &grey, &red, 0.25);
    assert_px(&early, 32, 32, [100, 100, 100, 255], 4);

    // Three quarters in, the incoming frame is half way out of it.
    let late = blend(&gpu, &mut comp, "dip_to_black", &grey, &red, 0.75);
    assert_px(&late, 32, 32, [128, 0, 0, 255], 4);
}

#[test]
fn dip_to_white_passes_through_an_opaque_white_frame() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let black = solid(W, H, [0, 0, 0, 255]);
    let red = solid(W, H, [255, 0, 0, 255]);

    let mid = blend(&gpu, &mut comp, "dip_to_white", &black, &red, 0.5);
    assert_px(&mid, 32, 32, [255, 255, 255, 255], 3);

    // The two dips differ, which is the whole point of having both ids.
    let dark = blend(&gpu, &mut comp, "dip_to_black", &black, &red, 0.5);
    assert_px(&dark, 32, 32, [0, 0, 0, 255], 3);
}

#[test]
fn each_wipe_direction_reveals_from_its_own_edge() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let out = solid(W, H, [0, 0, 0, 255]);
    let inc = solid(W, H, [255, 255, 255, 255]);
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];

    // Half way through, exactly half the frame is the incoming clip, and which
    // half is the only difference between these four ids.
    let left = blend(&gpu, &mut comp, "wipe_left", &out, &inc, 0.5);
    assert_px(&left, 8, 32, white, 3);
    assert_px(&left, 56, 32, black, 3);

    let right = blend(&gpu, &mut comp, "wipe_right", &out, &inc, 0.5);
    assert_px(&right, 8, 32, black, 3);
    assert_px(&right, 56, 32, white, 3);

    // Row 0 is the top of the frame.
    let up = blend(&gpu, &mut comp, "wipe_up", &out, &inc, 0.5);
    assert_px(&up, 32, 8, black, 3);
    assert_px(&up, 32, 56, white, 3);

    let down = blend(&gpu, &mut comp, "wipe_down", &out, &inc, 0.5);
    assert_px(&down, 32, 8, white, 3);
    assert_px(&down, 32, 56, black, 3);
}

#[test]
fn wipe_endpoints_are_the_two_clips() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let out = solid(W, H, [0, 0, 0, 255]);
    let inc = solid(W, H, [255, 255, 255, 255]);

    let start = blend(&gpu, &mut comp, "wipe_left", &out, &inc, 0.0);
    assert_px(&start, 32, 32, [0, 0, 0, 255], 3);
    let end = blend(&gpu, &mut comp, "wipe_left", &out, &inc, 1.0);
    assert_px(&end, 32, 32, [255, 255, 255, 255], 3);
}

#[test]
fn slide_moves_both_frames_instead_of_revealing_one() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let ramp = horizontal_gradient(W, H);
    let red = solid(W, H, [255, 0, 0, 255]);

    let img = blend(&gpu, &mut comp, "slide", &ramp, &red, 0.5);

    // Left of the seam is the outgoing ramp shifted half a frame left, so the
    // value at x is the ramp's value at x + 32 — a wipe would leave it at x.
    let at_16 = img.pixel(16, 32)[0];
    let ramp_at_48 = ramp.pixel(48, 32)[0];
    let ramp_at_16 = ramp.pixel(16, 32)[0];
    assert!(
        i32::from(at_16) - i32::from(ramp_at_48) <= 3
            && i32::from(ramp_at_48) - i32::from(at_16) <= 3,
        "outgoing frame did not slide: got {at_16}, moved {ramp_at_48}, unmoved {ramp_at_16}"
    );

    // Right of the seam is the incoming frame, which has entered from the right.
    assert_px(&img, 48, 32, [255, 0, 0, 255], 3);
}
