//! The pixel kernels an effect is built from: dilation, blur, rounded card.
//!
//! These functions take and return raw alpha or RGBA buffers independent of
//! `TextStyle`; the paint passes in `effects.rs` select when to call them.

use super::*;

/// Morphological dilation of an alpha mask by `radius` px (circular kernel).
///
/// For each integer y-offset in the circle, a monotonic-queue horizontal max
/// filter produces that row's exact chord in O(width), then the chord is
/// merged into the two symmetric destination offsets. This preserves the old
/// circular result while reducing O(width * height * radius²) to
/// O(width * height * radius).
pub(super) fn dilate_alpha(src: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    let radius = effect_extent(radius);
    let r = radius.ceil() as usize;
    if r == 0 {
        return src.to_vec();
    }
    let r2 = radius * radius;
    let mut dst = vec![0u8; src.len()];
    let mut chord = vec![0u8; src.len()];
    let height = h as usize;
    let width = w as usize;

    for dy in 0..=r {
        let dy2 = (dy * dy) as f32;
        if dy2 > r2 {
            continue;
        }
        let half_chord = (r2 - dy2).sqrt().floor() as usize;
        horizontal_max_filter(src, width, height, half_chord, &mut chord);

        for y in 0..height {
            if let Some(source_y) = y.checked_add(dy).filter(|source_y| *source_y < height) {
                merge_mask_row(&mut dst, &chord, width, y, source_y);
            }
            if dy > 0
                && let Some(source_y) = y.checked_sub(dy)
            {
                merge_mask_row(&mut dst, &chord, width, y, source_y);
            }
        }
    }
    dst
}

/// Row-wise centered maximum filter with clipped edges, using a monotonic
/// queue so every input sample enters and leaves once.
pub(super) fn horizontal_max_filter(src: &[u8], w: usize, h: usize, radius: usize, dst: &mut [u8]) {
    if w == 0 || h == 0 {
        return;
    }
    let mut queue = VecDeque::<usize>::with_capacity(radius.saturating_mul(2).saturating_add(1));
    for y in 0..h {
        queue.clear();
        let row = y * w;
        let mut next = 0usize;
        for x in 0..w {
            let upper = x.saturating_add(radius).min(w - 1);
            while next <= upper {
                while queue
                    .back()
                    .is_some_and(|index| src[row + *index] <= src[row + next])
                {
                    queue.pop_back();
                }
                queue.push_back(next);
                next += 1;
            }
            let lower = x.saturating_sub(radius);
            while queue.front().is_some_and(|index| *index < lower) {
                queue.pop_front();
            }
            dst[row + x] = src[row + queue[0]];
        }
    }
}

pub(super) fn merge_mask_row(dst: &mut [u8], src: &[u8], width: usize, dst_y: usize, src_y: usize) {
    let dst_row = dst_y * width;
    let src_row = src_y * width;
    for x in 0..width {
        dst[dst_row + x] = dst[dst_row + x].max(src[src_row + x]);
    }
}

/// Approximate Gaussian blur with two separable box passes.
pub(super) fn box_blur_alpha(src: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    let r = radius.round().max(1.0) as i32;
    let mut tmp = vec![0u8; src.len()];
    let mut out = src.to_vec();
    for _ in 0..BOX_BLUR_ITERATIONS {
        box_blur_pass(&out, &mut tmp, w, h, r, true);
        box_blur_pass(&tmp, &mut out, w, h, r, false);
    }
    out
}

pub(super) fn box_blur_pass(src: &[u8], dst: &mut [u8], w: u32, h: u32, r: i32, horizontal: bool) {
    let (outer, inner) = if horizontal {
        (h as i32, w as i32)
    } else {
        (w as i32, h as i32)
    };
    let window = (2 * r + 1) as u32;
    for o in 0..outer {
        // Running sum for O(n) sliding window.
        let mut sum: u32 = 0;
        for i in -r..=r {
            sum += sample_1d(src, w, h, o, i.clamp(0, inner - 1), horizontal) as u32;
        }
        for i in 0..inner {
            dst[index_1d(w, o, i, horizontal)] = ((sum + window / 2) / window) as u8;
            let leave = i - r;
            let enter = i + r + 1;
            if leave >= 0 {
                sum -= sample_1d(src, w, h, o, leave, horizontal) as u32;
            } else {
                sum -= sample_1d(src, w, h, o, 0, horizontal) as u32;
            }
            if enter < inner {
                sum += sample_1d(src, w, h, o, enter, horizontal) as u32;
            } else {
                sum += sample_1d(src, w, h, o, inner - 1, horizontal) as u32;
            }
        }
    }
}

pub(super) fn sample_1d(
    src: &[u8],
    w: u32,
    h: u32,
    outer: i32,
    inner: i32,
    horizontal: bool,
) -> u8 {
    let (x, y) = if horizontal {
        (inner as u32, outer as u32)
    } else {
        (outer as u32, inner as u32)
    };
    debug_assert!(x < w && y < h);
    src[(y * w + x) as usize]
}

pub(super) fn index_1d(w: u32, outer: i32, inner: i32, horizontal: bool) -> usize {
    let (x, y) = if horizontal {
        (inner as u32, outer as u32)
    } else {
        (outer as u32, inner as u32)
    };
    (y * w + x) as usize
}

/// Axis-aligned card rectangle in bitmap space.
pub(super) struct CardRect {
    pub(super) x0: f32,
    pub(super) y0: f32,
    pub(super) x1: f32,
    pub(super) y1: f32,
}

/// Fill a rounded rectangle (axis-aligned) with analytic coverage.
pub(super) fn fill_rounded_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rect: CardRect,
    radius_frac: f32,
    color: [u8; 4],
) {
    let CardRect { x0, y0, x1, y1 } = rect;
    if color[3] == 0 || x1 <= x0 || y1 <= y0 {
        return;
    }
    let rw = x1 - x0;
    let rh = y1 - y0;
    let max_r = rw.min(rh) * 0.5;
    let radius = unit_fraction(radius_frac) * max_r;

    let min_x = x0.floor().max(0.0) as i32;
    let min_y = y0.floor().max(0.0) as i32;
    let max_x = x1.ceil().min(width as f32) as i32;
    let max_y = y1.ceil().min(height as f32) as i32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let cx = x as f32 + 0.5;
            let cy = y as f32 + 0.5;
            let d = sd_rounded_box(
                cx - (x0 + x1) * 0.5,
                cy - (y0 + y1) * 0.5,
                rw * 0.5,
                rh * 0.5,
                radius,
            );
            // 1px AA band.
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let a = ((cover * f32::from(color[3])).round() as u32).min(255) as u8;
            if a == 0 {
                continue;
            }
            let idx = ((y as u32 * width + x as u32) * 4) as usize;
            over_straight(&mut pixels[idx..idx + 4], [color[0], color[1], color[2], a]);
        }
    }
}

/// Signed distance to a rounded box centered at the origin with half-extents
/// `(hx, hy)` and corner radius `r` (Inigo Quilez).
pub(super) fn sd_rounded_box(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = px.abs() - hx + r;
    let qy = py.abs() - hy + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.min(0.0).max(qy.min(0.0)) - r
}
