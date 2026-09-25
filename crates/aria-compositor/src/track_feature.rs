//! Following one patch of picture from frame to frame.
//!
//! A mask drawn on the preview can be keyframed by hand, one frame at a time,
//! which is the work an operator asks a tracker to do. The search is a plain
//! normalized-cross-correlation over a bounded window: no GPU, no model, no
//! external dependency, and deterministic, so a tracked result is reproducible
//! and testable without a device.
//!
//! Confidence is reported, never assumed. A tracker that keeps emitting
//! positions after it has lost the feature is worse than one that stops: the
//! keyframes it writes look deliberate and are wrong. Callers stop on a score
//! below their floor and say which frame lost it.

/// A rectangle of picture, in pixels, to follow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    /// Left edge in pixels.
    pub x: i32,
    /// Top edge in pixels.
    pub y: i32,
    /// Width in pixels; a zero or negative extent has nothing to match.
    pub w: i32,
    /// Height in pixels.
    pub h: i32,
}

impl Patch {
    /// A patch centred on `(cx, cy)` with extent `(w, h)`, rounded to pixels.
    pub fn centered(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self {
            x: (cx - w * 0.5).round() as i32,
            y: (cy - h * 0.5).round() as i32,
            w: w.round().max(1.0) as i32,
            h: h.round().max(1.0) as i32,
        }
    }

    /// Whether every pixel of the patch is inside a `width` × `height` frame.
    pub fn inside(&self, width: u32, height: u32) -> bool {
        self.w > 0
            && self.h > 0
            && self.x >= 0
            && self.y >= 0
            && (self.x + self.w) as i64 <= i64::from(width)
            && (self.y + self.h) as i64 <= i64::from(height)
    }
}

/// Where a patch went, and how sure the match is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    /// Pixel offset from the patch's old position to its new one.
    pub dx: i32,
    /// Vertical pixel offset, positive downward.
    pub dy: i32,
    /// Normalized cross-correlation of the winning position, `-1.0..=1.0`.
    /// `1.0` is an exact match; a flat patch on flat picture scores `1.0`
    /// wherever it lands, which is why callers also bound the search.
    pub score: f32,
}

/// Luminance of an RGBA frame, one byte per pixel.
///
/// Rec.709 weights, because that is what the compositor grades in; the tracker
/// only needs a monotone channel, but using the same luma keeps a tracked
/// feature the feature the operator can see.
fn luma(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let count = (width as usize) * (height as usize);
    let mut out = vec![0u8; count];
    for (i, out) in out.iter_mut().enumerate() {
        let p = i * 4;
        let r = f32::from(pixels[p]);
        let g = f32::from(pixels[p + 1]);
        let b = f32::from(pixels[p + 2]);
        *out = (0.2126 * r + 0.7152 * g + 0.0722 * b)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    out
}

/// Mean and standard deviation of one patch of a luma plane.
fn stats(plane: &[u8], width: u32, patch: Patch, dx: i32, dy: i32) -> (f32, f32) {
    let count = (patch.w * patch.h) as f32;
    let mut sum = 0.0f32;
    let mut sum_sq = 0.0f32;
    for row in 0..patch.h {
        let base = ((patch.y + dy + row) as usize) * (width as usize) + (patch.x + dx) as usize;
        for col in 0..patch.w {
            let v = f32::from(plane[base + col as usize]);
            sum += v;
            sum_sq += v * v;
        }
    }
    let mean = sum / count;
    let var = (sum_sq / count - mean * mean).max(0.0);
    (mean, var.sqrt())
}

/// Normalized cross-correlation of `patch` in `prev` against the same patch
/// displaced by `(dx, dy)` in `next`.
fn correlate(
    prev: &[u8],
    next: &[u8],
    width: u32,
    patch: Patch,
    prev_stats: (f32, f32),
    dx: i32,
    dy: i32,
) -> f32 {
    let (next_mean, next_dev) = stats(next, width, patch, dx, dy);
    let (prev_mean, prev_dev) = prev_stats;
    // A flat patch has no gradient to match on. Reporting 0 rather than
    // dividing by ~0 keeps a featureless region from scoring like a lock.
    if prev_dev < 1e-3 || next_dev < 1e-3 {
        return 0.0;
    }
    let count = (patch.w * patch.h) as f32;
    let mut acc = 0.0f32;
    for row in 0..patch.h {
        let a = ((patch.y + row) as usize) * (width as usize) + patch.x as usize;
        let b = ((patch.y + dy + row) as usize) * (width as usize) + (patch.x + dx) as usize;
        for col in 0..patch.w {
            let pa = f32::from(prev[a + col as usize]) - prev_mean;
            let pb = f32::from(next[b + col as usize]) - next_mean;
            acc += pa * pb;
        }
    }
    acc / (count * prev_dev * next_dev)
}

/// Where `patch` from `prev` sits in `next`, searched ±`radius` pixels.
///
/// `None` when the frames disagree in size, the patch is not wholly inside them,
/// or the radius leaves no legal position — the caller then reports that the
/// track could not start, instead of writing a keyframe it invented.
pub fn track_patch(
    prev: &aria_core::RgbaImage,
    next: &aria_core::RgbaImage,
    patch: Patch,
    radius: i32,
) -> Option<Match> {
    if prev.width != next.width || prev.height != next.height {
        return None;
    }
    let (width, height) = (prev.width, prev.height);
    if !patch.inside(width, height) || radius < 0 {
        return None;
    }
    let prev_luma = luma(&prev.pixels, width, height);
    let next_luma = luma(&next.pixels, width, height);
    let prev_stats = stats(&prev_luma, width, patch, 0, 0);

    let mut best: Option<Match> = None;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let moved = Patch {
                x: patch.x + dx,
                y: patch.y + dy,
                ..patch
            };
            if !moved.inside(width, height) {
                continue;
            }
            let score = correlate(&prev_luma, &next_luma, width, patch, prev_stats, dx, dy);
            let better = best.is_none_or(|held| {
                // Ties go to the smaller motion: a patch that did not move must
                // not be reported as having jumped to an equally good position.
                score > held.score
                    || (score == held.score
                        && dx * dx + dy * dy < held.dx * held.dx + held.dy * held.dy)
            });
            if better {
                best = Some(Match { dx, dy, score });
            }
        }
    }
    best
}

#[cfg(test)]
#[path = "track_feature_tests.rs"]
mod tests;
