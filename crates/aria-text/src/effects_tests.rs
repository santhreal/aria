//! Stroke, shadow, underline and background-card cases.
//!
//! A pure move out of `effects.rs`; the module reaches its fixtures through
//! `use super::*` exactly as it did inline.

use super::*;
use crate::style::{TextBackground, TextShadow, TextStroke};
use crate::{TextRenderer, TextStyle};

const TEST_FONT: &[u8] = include_bytes!("../assets/Micro5-Regular.ttf");

fn renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    assert!(r.load_font(TEST_FONT.to_vec()) > 0);
    r
}

fn covered(img: &RgbaImage) -> usize {
    img.pixels.chunks_exact(4).filter(|p| p[3] != 0).count()
}

fn count_near(img: &RgbaImage, rgb: [u8; 3], tol: i16) -> usize {
    img.pixels
        .chunks_exact(4)
        .filter(|p| {
            p[3] > 32
                && (i16::from(p[0]) - i16::from(rgb[0])).abs() <= tol
                && (i16::from(p[1]) - i16::from(rgb[1])).abs() <= tol
                && (i16::from(p[2]) - i16::from(rgb[2])).abs() <= tol
        })
        .count()
}

fn dilate_alpha_reference(src: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    let r = radius.ceil().max(0.0) as i32;
    if r == 0 {
        return src.to_vec();
    }
    let r2 = radius * radius;
    let mut dst = vec![0u8; src.len()];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut max_a = 0u8;
            for dy in -r..=r {
                for dx in -r..=r {
                    if (dx * dx + dy * dy) as f32 > r2 {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 {
                        max_a = max_a.max(src[(ny as u32 * w + nx as u32) as usize]);
                    }
                }
            }
            dst[(y as u32 * w + x as u32) as usize] = max_a;
        }
    }
    dst
}

#[test]
fn fast_dilation_matches_circular_reference() {
    let (w, h) = (9, 7);
    let mut src = vec![0u8; (w * h) as usize];
    for (index, alpha) in [(0, 17), (7, 93), (22, 255), (41, 128), (62, 211)] {
        src[index] = alpha;
    }
    for radius in [0.25, 1.0, 1.75, 2.0, 3.4, 5.0] {
        assert_eq!(
            dilate_alpha(&src, w, h, radius),
            dilate_alpha_reference(&src, w, h, radius),
            "radius {radius}"
        );
    }
}

#[test]
fn combined_stroke_shadow_padding_adds_every_extent() {
    let style = TextStyle::new(100.0)
        .with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: 10.0,
        })
        .with_shadow(TextShadow {
            rgba: [0, 0, 0, 255],
            blur: 0.2,
            distance: 10.0 * std::f32::consts::SQRT_2,
        });
    // stroke 10 + two 20px blur iterations + per-axis offset 10 + 1px AA guard.
    assert_eq!(effect_padding(&style), 61);
}

#[test]
fn non_finite_effect_metrics_are_bounded() {
    let style = TextStyle::new(48.0)
        .with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: f32::INFINITY,
        })
        .with_shadow(TextShadow {
            rgba: [0, 0, 0, 255],
            blur: f32::NAN,
            distance: f32::NEG_INFINITY,
        });
    assert_eq!(effect_padding(&style), 1);
    let img = renderer().rasterize("Safe", &style);
    assert!(img.width > 0 && img.height > 0);
}

#[test]
fn stroke_adds_outline_pixels_beyond_fill() {
    let mut r = renderer();
    let plain = r.rasterize("A", &TextStyle::new(48.0).with_color([255, 255, 255, 255]));
    let stroked = r.rasterize(
        "A",
        &TextStyle::new(48.0)
            .with_color([255, 255, 255, 255])
            .with_stroke(TextStroke {
                rgba: [255, 0, 0, 255],
                width: 4.0,
            }),
    );
    assert!(stroked.width > plain.width && stroked.height > plain.height);
    assert!(covered(&stroked) > covered(&plain));
    assert!(
        count_near(&stroked, [255, 0, 0], 40) > 20,
        "expected red stroke pixels"
    );
}

#[test]
fn inspector_max_stroke_rasterizes_a_title() {
    let mut r = renderer();
    let img = r.rasterize(
        "Responsive stroke preview",
        &TextStyle::new(96.0)
            .with_color([255, 255, 255, 255])
            .with_stroke(TextStroke {
                rgba: [0, 0, 0, 255],
                width: 40.0,
            }),
    );
    assert!(img.width > 0 && img.height > 0);
}

#[test]
fn background_fills_card_behind_glyphs() {
    let mut r = renderer();
    let img = r.rasterize(
        "Hi",
        &TextStyle::new(40.0)
            .with_color([255, 255, 255, 255])
            .with_background(TextBackground {
                rgba: [0, 0, 255, 255],
                radius: 0.0,
            }),
    );
    assert!(
        count_near(&img, [0, 0, 255], 20) > 100,
        "expected blue card"
    );
    // A pixel well inside the card (just outside the glyph ink on the left)
    // should be solid blue — not the AA fringe at the bitmap edge.
    let sample = img.pixel(2, img.height / 2);
    assert_eq!(&sample[..3], &[0, 0, 255]);
    assert!(
        sample[3] > 200,
        "interior card pixel should be opaque: {sample:?}"
    );
}

#[test]
fn shadow_tints_offset_pixels() {
    let mut r = renderer();
    let img = r.rasterize(
        "A",
        &TextStyle::new(48.0)
            .with_color([255, 255, 255, 255])
            .with_shadow(TextShadow {
                rgba: [0, 255, 0, 255],
                blur: 0.0,
                distance: 10.0,
            }),
    );
    assert!(
        count_near(&img, [0, 255, 0], 40) > 10,
        "expected green shadow pixels"
    );
}

#[test]
fn underline_adds_a_fill_colored_rule_below_the_glyphs() {
    let mut r = renderer();
    let plain = r.rasterize("HH", &TextStyle::new(48.0).with_color([255, 0, 0, 255]));
    let underlined = r.rasterize(
        "HH",
        &TextStyle::new(48.0)
            .with_color([255, 0, 0, 255])
            .with_underline(true),
    );
    assert!(underlined.height > plain.height);
    assert!(covered(&underlined) > covered(&plain));
    assert!(count_near(&underlined, [255, 0, 0], 20) > covered(&plain));
}
