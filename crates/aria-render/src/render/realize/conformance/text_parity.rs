//! One text run draws the same pixels as one bitmap and as instanced glyphs,
//! wherever its quad lands and at any bitmap scale.
//!
//! The two draws sample different textures. A static run, and every run drawn
//! as a transition or matte side, is one bitmap; an animated run is cluster
//! images packed in the glyph atlas, each inside a transparent pad. Each quad
//! was drawn to its texture rect's edges, which clips the bilinear footprint
//! half a texel short, and the bitmap's sampler repeated its outermost texel
//! where the atlas read transparent. So at a quad edge between pixels the
//! glyph quads dropped texels the one bitmap kept between clusters, and the
//! bitmap doubled the texels at its own edge. A run changed at its edges when
//! it moved half a pixel, when its scale changed, and when it became a side,
//! and the extent the host's font gives decided whether its quad edges fell
//! between pixels. Under animation the card and underlines are a bitmap of
//! their own behind the glyphs, which once held the glyphs as well. The sweep
//! crosses sub-pixel offsets with bitmap scales, plain, on a card, and
//! underlined.
//!
//! What this does not catch: clusters closer than a texel, where two glyph
//! quads blend where the bitmap interpolates, so the sweep's letters stand a
//! space apart under any host font; a rotated run; stroke and shadow looks.

use aria_models::AnimationSlot;
use aria_text::{TextBackground, TextStyle};

use crate::scene::{LayerSource, Scene, SizeSpec, TextAnimation};

use super::masks::assert_same;
use super::samples::{placed, text_style};
use super::{H, W, harness, render};

/// Two letters a space apart. Letters set closer, such as `HI` in Nimbus Sans,
/// can put two clusters' ink within a texel of each other.
const SPACED: &str = "H I";

/// The sample style, small enough that the spaced text on a card stays on the
/// canvas at the largest scale.
fn spaced_style() -> TextStyle {
    TextStyle {
        font_size: 20.0,
        ..text_style()
    }
}

/// Quad center offsets from the canvas center, in pixels: on the pixel grid,
/// halfway between pixels, and a quarter off on each axis.
const OFFSETS: [[f32; 2]; 3] = [[0.0, 0.0], [0.5, 0.5], [0.25, 0.75]];

/// Bitmap scales: drawn at its texels, larger, and smaller.
const SCALES: [f32; 3] = [1.0, 1.5, 0.75];

/// The sample text alone on the canvas, its quad center moved by `offset`.
fn run(style: &TextStyle, animation: Option<TextAnimation>, offset: [f32; 2], scale: f32) -> Scene {
    let mut layer = placed(
        LayerSource::Text {
            content: SPACED.into(),
            style: style.clone(),
            animation,
        },
        SizeSpec::BitmapScaled([scale, scale]),
    );
    layer.center = [layer.center[0] + offset[0], layer.center[1] + offset[1]];
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene.layers.push(layer);
    scene
}

#[test]
fn a_text_run_draws_the_same_as_one_bitmap_and_as_glyphs() {
    let (fx, mut renderer) = harness_or_skip!();
    // Fully in: no residual fade, so every glyph draws at full opacity in
    // place.
    let glyphs = TextAnimation {
        id: "char_fade_in".into(),
        slot: AnimationSlot::In,
        t: 1.0,
        intensity: 1.0,
        stagger: 1.0,
    };
    let card = TextStyle {
        background: Some(TextBackground {
            rgba: [0, 0, 255, 255],
            radius: 0.0,
        }),
        ..spaced_style()
    };
    let underlined = TextStyle {
        underline: true,
        ..spaced_style()
    };
    let widest = renderer.text.rasterize(SPACED, &card);
    let largest = SCALES.into_iter().fold(0.0, f32::max);
    assert!(
        widest.width as f32 * largest < W as f32 && widest.height as f32 * largest < H as f32,
        "the spaced text on a card, {}x{} at scale {largest}, leaves the {W}x{H} canvas",
        widest.width,
        widest.height
    );
    for (look, style) in [
        ("plain", spaced_style()),
        ("on a card", card),
        ("underlined", underlined),
    ] {
        for scale in SCALES {
            for offset in OFFSETS {
                let bitmap = render(
                    &mut renderer,
                    &fx.project,
                    &run(&style, None, offset, scale),
                );
                let drawn = render(
                    &mut renderer,
                    &fx.project,
                    &run(&style, Some(glyphs.clone()), offset, scale),
                );
                assert_same(
                    &format!(
                        "text {look} at scale {scale}, offset {offset:?}, drawn as glyphs \
                         against one bitmap"
                    ),
                    &drawn,
                    &bitmap,
                );
            }
        }
    }
}
