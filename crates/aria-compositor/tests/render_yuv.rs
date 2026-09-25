//! Video-frame layers: NV12 and I420 upload, range handling, and the
//! YUV→RGB matrix, each against the CPU reference in the parent file.

use super::*;

#[test]
fn nv12_neutral_is_gray() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    // Full-range Y=128, neutral chroma → mid gray, alpha opaque.
    let full_gray = ColorSpace {
        range: ColorRange::Full,
        ..ColorSpace::BT709
    };
    let f = nv12_uniform(16, 16, 128, 128, 128, full_gray);
    let img = render_frame(&gpu, &mut comp, 16, 16, &f);
    assert_px(&img, 8, 8, [128, 128, 128, 255], 2);
}

#[test]
fn nv12_limited_luma_endpoints() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);

    // Limited range: Y=16 is black, Y=235 is white (neutral chroma).
    let black = nv12_uniform(16, 16, 16, 128, 128, ColorSpace::BT709);
    let white = nv12_uniform(16, 16, 235, 128, 128, ColorSpace::BT709);
    assert_px(
        &render_frame(&gpu, &mut comp, 16, 16, &black),
        8,
        8,
        [0, 0, 0, 255],
        2,
    );
    assert_px(
        &render_frame(&gpu, &mut comp, 16, 16, &white),
        8,
        8,
        [255, 255, 255, 255],
        2,
    );
}

#[test]
fn nv12_color_matches_cpu_reference() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);

    // A saturated, non-neutral sample under several color spaces.
    let (y, u, v) = (150u8, 60u8, 200u8);
    let spaces = [
        ColorSpace::BT709,
        ColorSpace::BT601,
        ColorSpace {
            range: ColorRange::Full,
            ..ColorSpace::BT709
        },
        ColorSpace {
            range: ColorRange::Full,
            ..ColorSpace::BT601
        },
    ];
    for color in spaces {
        let f = nv12_uniform(16, 16, y, u, v, color);
        let img = render_frame(&gpu, &mut comp, 16, 16, &f);
        let [r, g, b] = yuv_ref(y, u, v, color);
        assert_px(&img, 8, 8, [r, g, b, 255], 2);
    }
}

#[test]
fn nv12_and_i420_agree() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let (y, u, v) = (90u8, 200u8, 70u8);
    let color = ColorSpace::BT709;

    let from_nv12 = render_frame(
        &gpu,
        &mut comp,
        16,
        16,
        &nv12_uniform(16, 16, y, u, v, color),
    );
    let from_i420 = render_frame(
        &gpu,
        &mut comp,
        16,
        16,
        &i420_uniform(16, 16, y, u, v, color),
    );

    let a = from_nv12.pixel(8, 8);
    assert_px(&from_i420, 8, 8, a, 1);
}

#[test]
fn matrix_choice_actually_changes_output() {
    // Guards against the YUV matrix being hardcoded: BT.601 and BT.709 must
    // produce visibly different RGB for chroma-heavy input.
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let (y, u, v) = (128u8, 128u8, 210u8); // strong Cr, neutral Cb

    let bt709 = render_frame(
        &gpu,
        &mut comp,
        16,
        16,
        &nv12_uniform(16, 16, y, u, v, ColorSpace::BT709),
    );
    let bt601 = render_frame(
        &gpu,
        &mut comp,
        16,
        16,
        &nv12_uniform(16, 16, y, u, v, ColorSpace::BT601),
    );

    let p709 = bt709.pixel(8, 8);
    let p601 = bt601.pixel(8, 8);
    let max_diff = (0..3)
        .map(|c| (i32::from(p709[c]) - i32::from(p601[c])).abs())
        .max()
        .unwrap();
    assert!(
        max_diff > 2,
        "BT.601 and BT.709 should differ: 709={p709:?} 601={p601:?}"
    );
}
