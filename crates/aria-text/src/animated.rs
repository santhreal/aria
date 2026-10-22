//! Per-cluster effect paint for character-level animation.
//!
//! Stroke and shadow are folded into each cluster's bitmap so outlines move
//! with their letter. The background card and the underlines stay a
//! whole-run quad drawn behind the glyphs (CapCut-style).

use crate::effects::{effect_padding, paint as paint_run, paint_card};
use crate::style::TextStyle;
use crate::{ClusterBox, ShapedText};
use aria_core::RgbaImage;

/// Shaped text with per-cluster stroke/shadow folded in, plus an optional
/// whole-run background card.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedText {
    /// Ink-tight extent of the unstyled shaped run (alignment reference).
    pub extent: (u32, u32),
    /// Clusters with stroke/shadow painted into their images. Offsets are
    /// relative to the ink-tight origin (same space as [`ShapedText`]); a
    /// cluster's image may extend into negative offset when padded.
    pub clusters: Vec<ClusterBox>,
    /// Whole-run background card, or `None`.
    pub background: Option<RgbaImage>,
    /// Top-left of `background` relative to the ink-tight origin.
    pub background_offset: [f32; 2],
}

impl AnimatedText {
    pub fn has_ink(&self) -> bool {
        self.extent.0 > 0 && self.extent.1 > 0
    }
}

/// Paint stroke / shadow per cluster, and the background card with the run's
/// underlines, for character-level animation. When the style has no stroke,
/// shadow, card, or underline, this is equivalent to cloning the shaped
/// clusters.
pub fn paint_animated(shaped: &ShapedText, style: &TextStyle) -> AnimatedText {
    if !shaped.has_ink() {
        return AnimatedText {
            extent: (0, 0),
            clusters: Vec::new(),
            background: None,
            background_offset: [0.0, 0.0],
        };
    }

    // Underlines need line context, so they go on the card; each cluster
    // paints its own stroke and shadow.
    let clusters = if style.stroke.is_some() || style.shadow.is_some() {
        let local = cluster_style(style);
        let pad = effect_padding(&local);
        shaped
            .clusters
            .iter()
            .map(|c| paint_cluster(c, &local, pad))
            .collect()
    } else {
        shaped.clusters.clone()
    };

    let (background, background_offset) = paint_background(shaped, style);

    AnimatedText {
        extent: shaped.extent,
        clusters,
        background,
        background_offset,
    }
}

/// `style` for painting one cluster alone: its stroke and shadow, without the
/// run's padding, card, or underlines.
fn cluster_style(style: &TextStyle) -> TextStyle {
    let mut local = style.clone();
    local.background = None;
    local.padding = 0;
    local.underline = false;
    for run in &mut local.runs {
        run.underline = None;
    }
    local
}

/// `cluster` painted alone under `style`, a [`cluster_style`], into a bitmap
/// `pad` pixels larger on every side, the padding `style` paints.
fn paint_cluster(cluster: &ClusterBox, style: &TextStyle, pad: u32) -> ClusterBox {
    if cluster.image.width == 0 || cluster.image.height == 0 || pad == 0 {
        return cluster.clone();
    }

    // Build a one-cluster shaped run so we can reuse the whole-run painter's
    // stroke/shadow path (minus background).
    let local = ShapedText {
        extent: (cluster.image.width, cluster.image.height),
        clusters: vec![ClusterBox {
            text_range: cluster.text_range.clone(),
            line: 0,
            offset: [0.0, 0.0],
            baseline: cluster.baseline - cluster.offset[1],
            image: cluster.image.clone(),
        }],
    };
    let painted = paint_run(&local, style);
    ClusterBox {
        text_range: cluster.text_range.clone(),
        line: cluster.line,
        offset: [
            cluster.offset[0] - pad as f32,
            cluster.offset[1] - pad as f32,
        ],
        baseline: cluster.baseline,
        image: painted,
    }
}

fn paint_background(shaped: &ShapedText, style: &TextStyle) -> (Option<RgbaImage>, [f32; 2]) {
    if style.background.is_none() && !style.any_underline() {
        return (None, [0.0, 0.0]);
    }

    // The card and the underlines alone, padded as a run without stroke or
    // shadow: the glyphs draw over it wherever they move.
    let mut card = style.clone();
    card.stroke = None;
    card.shadow = None;
    let painted = paint_card(shaped, &card);
    if painted.width == 0 || painted.height == 0 {
        return (None, [0.0, 0.0]);
    }
    // The padded bitmap's origin relative to the ink-tight extent is
    // (−pad, −pad).
    let pad = effect_padding(&card) as f32;
    // Strip fully-transparent results (e.g. underline-only with no coverage).
    let lit = painted.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0);
    if !lit {
        return (None, [0.0, 0.0]);
    }
    (Some(painted), [-pad, -pad])
}

/// Composite animated clusters at identity into a single bitmap — test helper
/// and documentation of the placement contract.
#[cfg(test)]
pub(crate) fn composite_animated(anim: &AnimatedText) -> RgbaImage {
    if !anim.has_ink() {
        return RgbaImage::transparent(0, 0);
    }
    // Bound = extent union every cluster image (which may overhang).
    let mut min_x = 0.0f32;
    let mut min_y = 0.0f32;
    let mut max_x = anim.extent.0 as f32;
    let mut max_y = anim.extent.1 as f32;
    for c in &anim.clusters {
        min_x = min_x.min(c.offset[0]);
        min_y = min_y.min(c.offset[1]);
        max_x = max_x.max(c.offset[0] + c.image.width as f32);
        max_y = max_y.max(c.offset[1] + c.image.height as f32);
    }
    if let Some(bg) = &anim.background {
        min_x = min_x.min(anim.background_offset[0]);
        min_y = min_y.min(anim.background_offset[1]);
        max_x = max_x.max(anim.background_offset[0] + bg.width as f32);
        max_y = max_y.max(anim.background_offset[1] + bg.height as f32);
    }
    let origin = [min_x, min_y];
    let width = (max_x - min_x).ceil().max(0.0) as u32;
    let height = (max_y - min_y).ceil().max(0.0) as u32;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];

    if let Some(bg) = &anim.background {
        blit(
            &mut pixels,
            width,
            height,
            bg,
            anim.background_offset[0] - origin[0],
            anim.background_offset[1] - origin[1],
        );
    }
    for c in &anim.clusters {
        if c.image.width == 0 {
            continue;
        }
        blit(
            &mut pixels,
            width,
            height,
            &c.image,
            c.offset[0] - origin[0],
            c.offset[1] - origin[1],
        );
    }
    RgbaImage::new(width, height, pixels)
}

#[cfg(test)]
fn blit(dst: &mut [u8], dw: u32, dh: u32, src: &RgbaImage, ox: f32, oy: f32) {
    use crate::over_straight;
    let ox = ox.round() as i64;
    let oy = oy.round() as i64;
    for row in 0..src.height {
        for col in 0..src.width {
            let px = src.pixel(col, row);
            if px[3] == 0 {
                continue;
            }
            let x = ox + i64::from(col);
            let y = oy + i64::from(row);
            if x < 0 || y < 0 || x >= i64::from(dw) || y >= i64::from(dh) {
                continue;
            }
            let idx = ((y as u32 * dw + x as u32) * 4) as usize;
            over_straight(&mut dst[idx..idx + 4], px);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{TextBackground, TextRun, TextShadow, TextStroke};
    use crate::{TextRenderer, TextStyle};

    const TEST_FONT: &[u8] = include_bytes!("../assets/Micro5-Regular.ttf");

    fn renderer() -> TextRenderer {
        let mut r = TextRenderer::new();
        assert!(r.load_font(TEST_FONT.to_vec()) > 0);
        r
    }

    #[test]
    fn animated_stroke_grows_cluster_images() {
        let mut r = renderer();
        let style = TextStyle::new(48.0).with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: 4.0,
        });
        let shaped = r.shape("Hi", &style);
        let anim = paint_animated(&shaped, &style);
        assert_eq!(anim.clusters.len(), shaped.clusters.len());
        for (a, b) in anim.clusters.iter().zip(shaped.clusters.iter()) {
            if b.image.width == 0 {
                continue;
            }
            assert!(
                a.image.width > b.image.width && a.image.height > b.image.height,
                "stroke should pad the cluster bitmap"
            );
        }
    }

    #[test]
    fn animated_background_is_separate() {
        let mut r = renderer();
        let style = TextStyle::new(40.0).with_background(TextBackground {
            rgba: [20, 20, 20, 200],
            radius: 0.4,
        });
        let shaped = r.shape("A", &style);
        let anim = paint_animated(&shaped, &style);
        assert!(anim.background.is_some());
        // Fill glyphs stay ink-tight (no stroke/shadow).
        assert_eq!(anim.clusters[0].image.width, shaped.clusters[0].image.width);
    }

    /// Closes the class where the parts a character animation draws, the card
    /// with its underlines and then each glyph over it, composite to other
    /// pixels than the one bitmap a static run draws: a card that also held
    /// the glyphs drew every letter twice, and a card painted with the fill's
    /// alpha zeroed dropped the underlines.
    ///
    /// Not caught: stroke and shadow, which a static run paints around the
    /// whole run and an animated run around each letter; an underline over a
    /// descender, which a static run draws over the glyph and an animated run
    /// under it.
    #[test]
    fn animated_parts_composite_to_the_static_raster() {
        let mut r = renderer();
        // A translucent fill shows a letter drawn twice.
        let base = TextStyle::new(40.0).with_color([255, 220, 0, 200]);
        let card = TextBackground {
            rgba: [20, 40, 200, 255],
            radius: 0.3,
        };
        let underlined = TextStyle {
            underline: true,
            ..base.clone()
        };
        let first_underlined = base.clone().with_runs(vec![TextRun {
            range: 0..1,
            underline: Some(true),
            ..TextRun::default()
        }]);
        let styles = [
            ("plain", base.clone()),
            ("on a card", base.with_background(card)),
            ("underlined", underlined.clone()),
            ("underlined on a card", underlined.with_background(card)),
            ("one letter underlined", first_underlined.clone()),
            (
                "one letter underlined on a card",
                first_underlined.with_background(card),
            ),
        ];
        for (what, style) in styles {
            let shaped = r.shape("HI", &style);
            let parts = composite_animated(&paint_animated(&shaped, &style));
            let whole = r.rasterize("HI", &style);
            assert_eq!(
                (parts.width, parts.height),
                (whole.width, whole.height),
                "{what}: size"
            );
            let worst = parts
                .pixels
                .iter()
                .zip(&whole.pixels)
                .map(|(a, b)| a.abs_diff(*b))
                .max();
            assert!(
                worst <= Some(1),
                "{what}: the animated parts differ from the static raster by {worst:?}"
            );
        }
    }

    /// Closes the class where an underline, which a character animation draws
    /// on the card, also reaches a letter's own bitmap or its padding: each
    /// letter painted with its stroke or shadow is the same under a run-wide
    /// or a per-run underline as with none. A letter padded for an underline
    /// its bitmap did not hold drew up and to the left of its place.
    #[test]
    fn underlines_stay_off_the_letters() {
        let mut r = renderer();
        let stroked = TextStyle::new(40.0).with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: 3.0,
        });
        let shadowed = TextStyle::new(40.0).with_shadow(TextShadow {
            rgba: [0, 0, 0, 160],
            blur: 0.1,
            distance: 4.0,
        });
        for (what, plain) in [("stroked", stroked), ("shadowed", shadowed)] {
            let shaped = r.shape("HI", &plain);
            let want = paint_animated(&shaped, &plain).clusters;
            let underlined = TextStyle {
                underline: true,
                ..plain.clone()
            };
            let first_underlined = plain.clone().with_runs(vec![TextRun {
                range: 0..1,
                underline: Some(true),
                ..TextRun::default()
            }]);
            for (how, style) in [
                ("run-wide", underlined),
                ("on one letter", first_underlined),
            ] {
                let got = paint_animated(&r.shape("HI", &style), &style).clusters;
                assert_eq!(got, want, "{what}, underlined {how}");
            }
        }
    }

    /// Closes the class where a letter's stroke or shadow, which a character
    /// animation paints around each letter, also pads or moves the card and
    /// underline drawn behind them: a static run draws its card at one place
    /// around the ink whatever the letters' effects.
    #[test]
    fn stroke_and_shadow_leave_the_card_in_place() {
        let mut r = renderer();
        let plain = TextStyle {
            underline: true,
            ..TextStyle::new(40.0).with_background(TextBackground {
                rgba: [20, 40, 200, 255],
                radius: 0.3,
            })
        };
        let shaped = r.shape("HI", &plain);
        let want = paint_animated(&shaped, &plain);
        assert!(want.background.is_some(), "the card is drawn");
        let stroked = plain.clone().with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: 3.0,
        });
        let shadowed = plain.clone().with_shadow(TextShadow {
            rgba: [0, 0, 0, 160],
            blur: 0.1,
            distance: 4.0,
        });
        for (what, style) in [("stroked", stroked), ("shadowed", shadowed)] {
            let got = paint_animated(&r.shape("HI", &style), &style);
            assert_eq!(
                (got.background, got.background_offset),
                (want.background.clone(), want.background_offset),
                "{what}"
            );
        }
    }
}
