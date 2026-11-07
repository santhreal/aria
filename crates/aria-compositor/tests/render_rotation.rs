//! Container rotation: a frame recorded on its side draws upright, filling
//! the upright quad its probed size places, under every crop and flip.

use super::*;

/// Decoded picture size: wider than tall, as a phone records portrait video.
const W: u32 = 16;
const H: u32 = 8;
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// A limited-range I420 frame, black except its top-left `W/2`×`H/2`
/// quadrant, which is white: the quadrant states which way the picture turned.
fn marked(rotation: Rotation) -> VideoFrame {
    let mut luma = vec![16u8; (W * H) as usize];
    for y in 0..H / 2 {
        for x in 0..W / 2 {
            luma[(y * W + x) as usize] = 235;
        }
    }
    let (cw, ch) = (W as usize / 2, H as usize / 2);
    let mut f = frame(
        PixelFormat::Yuv420p,
        ColorSpace::BT709,
        W,
        H,
        vec![
            Plane::new(luma, W as usize, H as usize),
            Plane::new(vec![128; cw * ch], cw, ch),
            Plane::new(vec![128; cw * ch], cw, ch),
        ],
    );
    f.rotation = rotation;
    f
}

/// The upright picture of [`marked`]: the decoded top-left corner turned
/// clockwise by `rotation`, as `(upright width, upright height, white
/// quadrant's column, white quadrant's row)` in halves.
fn upright(rotation: Rotation) -> (u32, u32, u32, u32) {
    match rotation {
        Rotation::None => (W, H, 0, 0),
        Rotation::Cw90 => (H, W, 1, 0),
        Rotation::Cw180 => (W, H, 1, 1),
        Rotation::Cw270 => (H, W, 0, 1),
    }
}

/// Render `f` filling a canvas of its upright size, with the layer's UV rect
/// set to `uv` and its mask state `fx`, and read back the color at the center
/// of each quadrant.
fn quadrants(
    gpu: &GpuContext,
    comp: &mut Compositor,
    f: &VideoFrame,
    fx: LayerEffects,
    uv: [f32; 4],
) -> [[u8; 4]; 4] {
    let (w, h, _, _) = upright(f.rotation);
    let config = CompositorConfig::new(w, h);
    let mut layer = CompositeLayer::frame(f, LayerPlacement::full_canvas(&config)).with_fx(fx);
    layer.uv = uv;
    let img = comp.render(gpu, &config, &[layer]).expect("render");
    assert_eq!((img.width, img.height), (w, h));
    let at = |col: u32, row: u32| img.pixel(w / 4 + col * w / 2, h / 4 + row * h / 2);
    [at(0, 0), at(1, 0), at(0, 1), at(1, 1)]
}

#[track_caller]
fn assert_white_at(got: [[u8; 4]; 4], col: u32, row: u32, what: &str) {
    for (i, px) in got.iter().enumerate() {
        let want = if i as u32 == row * 2 + col {
            WHITE
        } else {
            BLACK
        };
        for c in 0..4 {
            assert!(
                (i32::from(px[c]) - i32::from(want[c])).abs() <= 2,
                "{what}: quadrant {i} is {px:?}, expected {want:?}; got {got:?}"
            );
        }
    }
}

/// WHY: a container rotation was added to the quad's rotation, so a frame
/// recorded on its side drew turned inside a quad sized for the upright
/// picture: squashed, and overhanging a portrait canvas. This walks every
/// rotation, through the plain and the masked video pipelines, and checks
/// the upright picture fills its quad in the right turn, that a flip (`uv`
/// reversed on an axis) mirrors the upright picture rather than the decoded
/// one, and that a crop of the upright left half keeps the upright left half.
/// It does not cover a decoder reporting the rotation, which the decoder
/// tests cover.
#[test]
fn a_rotated_frame_draws_upright_under_crop_and_flip() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let masked = LayerEffects {
        mask: Some(LayerMask::new(mask_kind::RECTANGLE)),
        chroma_key: None,
    };
    for (pipeline, fx) in [("plain", LayerEffects::IDENTITY), ("masked", masked)] {
        for rotation in Rotation::ALL {
            let f = marked(rotation);
            let (_, _, col, row) = upright(rotation);
            let what = format!("{pipeline} {rotation:?}");
            let mut draw = |uv| quadrants(&gpu, &mut comp, &f, fx, uv);
            assert_white_at(
                draw([0.0, 0.0, 1.0, 1.0]),
                col,
                row,
                &format!("{what} upright"),
            );
            assert_white_at(
                draw([1.0, 0.0, 0.0, 1.0]),
                1 - col,
                row,
                &format!("{what} flipped horizontally"),
            );
            assert_white_at(
                draw([0.0, 1.0, 1.0, 0.0]),
                col,
                1 - row,
                &format!("{what} flipped vertically"),
            );
            // The upright left half stretched across the quad: its white
            // quadrant, if it has one, fills the whole of its row.
            let lit = draw([0.0, 0.0, 0.5, 1.0]).map(|px| px[0] > 128);
            let want = [0, 1].map(|r| col == 0 && r == row);
            assert_eq!(
                lit,
                [want[0], want[0], want[1], want[1]],
                "{what} cropped to the upright left half"
            );
        }
    }
}
