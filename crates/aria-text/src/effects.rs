//! CPU paint for text stroke, background card, and drop shadow.
//!
//! Applied only by [`crate::TextRenderer::rasterize`] — [`crate::TextRenderer::shape`]
//! stays ink-tight so character-level animation can still place clusters freely.

use std::collections::VecDeque;

use crate::style::TextStyle;
use crate::{ShapedText, over_straight};
use aria_core::RgbaImage;

/// Extra margin (as a fraction of font size) between glyph ink and the
/// background card's edge — CapCut-ish breathing room.
const BG_INSET_FRAC: f32 = 0.18;
const BOX_BLUR_ITERATIONS: u32 = 2;

/// Defensive render-side ceiling for direct `aria-text` callers. The app's
/// model validation is tighter in reference space, but this crate also has a
/// public pixel-space API and must not turn a corrupt/hostile float into an
/// overflowing allocation or practically unbounded morphology loop.
const MAX_EFFECT_EXTENT_PX: f32 = 4096.0;

/// Compose stroke / background / shadow around an already-shaped run.
pub(crate) fn paint(shaped: &ShapedText, style: &TextStyle) -> RgbaImage {
    let pad = effect_padding(style);
    let Some((width, height, mut pixels)) = blank(shaped, pad) else {
        return RgbaImage::transparent(0, 0);
    };

    // Coverage of the fill glyphs — substrate for stroke dilation and shadow blur.
    let cover = glyph_coverage(shaped, style, pad, width, height);

    fill_card(&mut pixels, width, height, shaped, style, pad);

    // Stroke is the expensive mask operation. Compute it once: a shadow can
    // bloom from the stroked silhouette and the visible outline uses the same
    // pixels immediately afterwards.
    let stroke_radius = style
        .stroke
        .map_or(0.0, |stroke| effect_extent(stroke.width));
    let stroke_mask =
        (stroke_radius > 0.0).then(|| dilate_alpha(&cover, width, height, stroke_radius));

    if let Some(shadow) = style.shadow {
        // Glow/outline shadows should bloom from the stroked silhouette when
        // a stroke is present (neon / chrome presets).
        let shadow_src = stroke_mask.as_deref().unwrap_or(&cover);
        let blur_px = shadow_blur_px(style, shadow.blur);
        let blurred = (blur_px > 0.5).then(|| box_blur_alpha(shadow_src, width, height, blur_px));
        let shadow_mask = blurred.as_deref().unwrap_or(shadow_src);
        // CapCut-style 45° down-right offset.
        let offset = signed_effect_extent(shadow.distance) / std::f32::consts::SQRT_2;
        let dx = offset.round() as i32;
        let dy = dx;
        blit_tinted(&mut pixels, width, height, shadow_mask, dx, dy, shadow.rgba);
    }

    if let (Some(stroke), Some(dilated)) = (style.stroke, stroke_mask.as_deref()) {
        blit_tinted(&mut pixels, width, height, dilated, 0, 0, stroke.rgba);
    }

    blit_clusters(&mut pixels, width, height, shaped, pad);
    blit_underlines(&mut pixels, width, height, shaped, style, pad);
    RgbaImage::new(width, height, pixels)
}

/// Bitmap headroom needed so stroke / shadow / background don't clip.
pub(crate) fn effect_padding(style: &TextStyle) -> u32 {
    let stroke_radius = style
        .stroke
        .map_or(0.0, |stroke| effect_extent(stroke.width));
    let mut need = (style.padding as f32).min(MAX_EFFECT_EXTENT_PX);
    let has_underline = style.any_underline();
    let underline_extent = if has_underline {
        // The line sits just below the baseline. Capital-only runs otherwise
        // have no descender ink reserving that part of the bitmap.
        effect_extent(style.font_size * 0.18)
    } else {
        0.0
    };
    need = need.max(underline_extent);
    if stroke_radius > 0.0 {
        need = need.max(underline_extent + stroke_radius + 1.0);
    }
    if let Some(shadow) = style.shadow {
        let blur_px = shadow_blur_px(style, shadow.blur);
        let blur_support = box_blur_support(blur_px);
        let offset = effect_extent(shadow.distance.abs()) / std::f32::consts::SQRT_2;
        // The shadow source is the stroked silhouette, so these extents are
        // additive rather than alternatives. `max(stroke, blur + offset)`
        // clips combined outline/shadow treatments at the bitmap edge.
        need = need.max(underline_extent + stroke_radius + blur_support + offset + 1.0);
    }
    if style.background.is_some() {
        let inset = effect_extent(style.font_size * BG_INSET_FRAC).max(2.0);
        need = need.max(inset);
    }
    need.min(MAX_EFFECT_EXTENT_PX).ceil() as u32
}

/// The background card and underlines [`paint`] draws for `shaped` under
/// `style`, in a bitmap of the size it draws, without the glyphs, stroke, or
/// shadow: what character animation draws behind glyphs that move.
pub(crate) fn paint_card(shaped: &ShapedText, style: &TextStyle) -> RgbaImage {
    let pad = effect_padding(style);
    let Some((width, height, mut pixels)) = blank(shaped, pad) else {
        return RgbaImage::transparent(0, 0);
    };
    fill_card(&mut pixels, width, height, shaped, style, pad);
    blit_underlines(&mut pixels, width, height, shaped, style, pad);
    RgbaImage::new(width, height, pixels)
}

/// A transparent RGBA buffer the size of `shaped`'s extent with `pad` on every
/// side, or `None` when its byte length overflows.
fn blank(shaped: &ShapedText, pad: u32) -> Option<(u32, u32, Vec<u8>)> {
    let width = shaped.extent.0.saturating_add(pad.saturating_mul(2));
    let height = shaped.extent.1.saturating_add(pad.saturating_mul(2));
    let len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    Some((width, height, vec![0u8; len]))
}

/// Fill the background card of `style`, if it has one, around the ink of
/// `shaped` drawn `pad` pixels in.
fn fill_card(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    shaped: &ShapedText,
    style: &TextStyle,
    pad: u32,
) {
    let Some(bg) = style.background else {
        return;
    };
    let inset = effect_extent(style.font_size * BG_INSET_FRAC).max(2.0);
    let rect = CardRect {
        x0: pad as f32 - inset,
        y0: pad as f32 - inset,
        x1: pad as f32 + shaped.extent.0 as f32 + inset,
        y1: pad as f32 + shaped.extent.1 as f32 + inset,
    };
    fill_rounded_rect(pixels, width, height, rect, bg.radius, bg.rgba);
}

fn effect_extent(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_EFFECT_EXTENT_PX)
    } else {
        0.0
    }
}

fn signed_effect_extent(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-MAX_EFFECT_EXTENT_PX, MAX_EFFECT_EXTENT_PX)
    } else {
        0.0
    }
}

fn unit_fraction(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn shadow_blur_px(style: &TextStyle, blur: f32) -> f32 {
    effect_extent(unit_fraction(blur) * effect_extent(style.font_size))
}

fn box_blur_support(radius: f32) -> f32 {
    if radius > 0.5 {
        BOX_BLUR_ITERATIONS as f32 * radius.round().max(1.0)
    } else {
        0.0
    }
}

fn glyph_coverage(
    shaped: &ShapedText,
    style: &TextStyle,
    pad: u32,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut cover = vec![0u8; (width as usize) * (height as usize)];
    for cluster in &shaped.clusters {
        let (cw, ch) = (cluster.image.width, cluster.image.height);
        if cw == 0 || ch == 0 {
            continue;
        }
        let ox = cluster.offset[0].round() as i64 + i64::from(pad);
        let oy = cluster.offset[1].round() as i64 + i64::from(pad);
        for row in 0..ch {
            for col in 0..cw {
                let src = cluster.image.pixel(col, row);
                if src[3] == 0 {
                    continue;
                }
                let (px, py) = (ox + i64::from(col), oy + i64::from(row));
                if px < 0 || py < 0 || px >= i64::from(width) || py >= i64::from(height) {
                    continue;
                }
                let idx = (py as u32 * width + px as u32) as usize;
                cover[idx] = cover[idx].max(src[3]);
            }
        }
    }
    for rect in underline_rects(shaped, style, pad, width, height) {
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                cover[(y * width + x) as usize] = u8::MAX;
            }
        }
    }
    cover
}

fn blit_clusters(pixels: &mut [u8], width: u32, height: u32, shaped: &ShapedText, pad: u32) {
    for cluster in &shaped.clusters {
        let (cw, ch) = (cluster.image.width, cluster.image.height);
        if cw == 0 || ch == 0 {
            continue;
        }
        let ox = cluster.offset[0].round() as i64 + i64::from(pad);
        let oy = cluster.offset[1].round() as i64 + i64::from(pad);
        for row in 0..ch {
            for col in 0..cw {
                let src = cluster.image.pixel(col, row);
                if src[3] == 0 {
                    continue;
                }
                let (px, py) = (ox + i64::from(col), oy + i64::from(row));
                if px < 0 || py < 0 || px >= i64::from(width) || py >= i64::from(height) {
                    continue;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                over_straight(&mut pixels[idx..idx + 4], src);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct UnderlineRect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    color: [u8; 4],
}

/// Ink-width underline segments per visual line (or per underlined run).
fn underline_rects(
    shaped: &ShapedText,
    style: &TextStyle,
    pad: u32,
    width: u32,
    height: u32,
) -> Vec<UnderlineRect> {
    if !style.any_underline() {
        return Vec::new();
    }
    let mut spans: Vec<(usize, f32, f32, f32, [u8; 4])> = Vec::new();
    for cluster in shaped.clusters.iter().filter(|c| c.image.width > 0) {
        if !style.underline_at(cluster.text_range.start) {
            continue;
        }
        let color = style.color_at(cluster.text_range.start);
        let left = cluster.offset[0];
        let right = left + cluster.image.width as f32;
        if let Some((_, x0, x1, baseline, span_color)) = spans.last_mut().filter(|s| {
            s.0 == cluster.line && s.4 == color && !style.runs.iter().any(|r| r.underline.is_some())
        }) {
            *x0 = x0.min(left);
            *x1 = x1.max(right);
            *baseline = baseline.max(cluster.baseline);
            let _ = span_color;
        } else if let Some((_, _x0, x1, baseline, _)) = spans.last_mut().filter(|s| {
            s.0 == cluster.line && s.4 == color && (left - s.2).abs() <= style.font_size * 0.6
        }) {
            *x1 = x1.max(right);
            *baseline = baseline.max(cluster.baseline);
        } else {
            spans.push((cluster.line, left, right, cluster.baseline, color));
        }
    }

    let offset = effect_extent(style.font_size * 0.08);
    let thickness = effect_extent(style.font_size * 0.06).round().max(1.0);
    spans
        .into_iter()
        .filter_map(|(_, left, right, baseline, color)| {
            let x0 = (left + pad as f32).floor().clamp(0.0, width as f32) as u32;
            let x1 = (right + pad as f32).ceil().clamp(0.0, width as f32) as u32;
            let y0 = (baseline + offset + pad as f32)
                .round()
                .clamp(0.0, height as f32) as u32;
            let y1 = (y0 as f32 + thickness).clamp(0.0, height as f32) as u32;
            (x1 > x0 && y1 > y0).then_some(UnderlineRect {
                x0,
                y0,
                x1,
                y1,
                color,
            })
        })
        .collect()
}

fn blit_underlines(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    shaped: &ShapedText,
    style: &TextStyle,
    pad: u32,
) {
    for rect in underline_rects(shaped, style, pad, width, height) {
        if rect.color[3] == 0 {
            continue;
        }
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let index = ((y * width + x) * 4) as usize;
                over_straight(&mut pixels[index..index + 4], rect.color);
            }
        }
    }
}

/// Tint an alpha mask with `color` and composite it (source-over) at `(dx, dy)`.
fn blit_tinted(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    cover: &[u8],
    dx: i32,
    dy: i32,
    color: [u8; 4],
) {
    let color_a = u32::from(color[3]);
    if color_a == 0 {
        return;
    }
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let sx = x - dx;
            let sy = y - dy;
            if sx < 0 || sy < 0 || sx >= width as i32 || sy >= height as i32 {
                continue;
            }
            let a = cover[(sy as u32 * width + sx as u32) as usize];
            if a == 0 {
                continue;
            }
            let out_a = ((u32::from(a) * color_a + 127) / 255) as u8;
            if out_a == 0 {
                continue;
            }
            let idx = ((y as u32 * width + x as u32) * 4) as usize;
            over_straight(
                &mut pixels[idx..idx + 4],
                [color[0], color[1], color[2], out_a],
            );
        }
    }
}
// The pixel kernels and the test module live beside this file. Pure moves.
#[path = "effects_filters.rs"]
mod filters;

use filters::{CardRect, box_blur_alpha, dilate_alpha, fill_rounded_rect};

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
