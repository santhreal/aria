//! Scope bitmaps: the measured frame drawn as a waveform monitor, a
//! vectorscope, and an RGB histogram.
//!
//! The measurement lives in `aria-analysis`; this module only turns counts
//! into pixels. Each scope is painted into an [`RgbaImage`] and handed to GPUI
//! as one texture, rather than drawn as thousands of elements: a 256-column
//! waveform is 65k cells, and a cell per element would cost more per frame
//! than the render that produced the picture.
//!
//! Intensity is the square root of the cell's share of the peak. A linear
//! ramp hides everything but the brightest trace, which is why hardware scopes
//! are gamma-lifted too.

use aria_analysis::{FrameScopes, Histogram, SCOPE_BINS, Vectorscope, Waveform};
use aria_core::RgbaImage;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Edge length of the drawn vectorscope, and the height of the waveform.
pub const SCOPE_PX: u32 = 192;

/// Width of the drawn histogram.
pub const HISTOGRAM_W: u32 = 256;

/// Height of the drawn histogram.
pub const HISTOGRAM_H: u32 = 96;

/// Graticule colour: dark enough to sit under a trace, bright enough to read.
const GRATICULE: [u8; 4] = [70, 78, 90, 255];
/// Scope background. Opaque, so a trace is not blended with the panel.
const BACKDROP: [u8; 4] = [12, 14, 18, 255];

/// Luma trace colour for the waveform monitor.
const TRACE: [u8; 4] = [120, 235, 170, 255];

/// The three scopes as textures, in the order they are shown.
pub struct ScopeImages {
    /// Luma per horizontal position.
    pub waveform: RgbaImage,
    /// Chroma on the Cb/Cr plane.
    pub vectorscope: RgbaImage,
    /// Code-value distribution per channel.
    pub histogram: RgbaImage,
}

impl ScopeImages {
    /// Paint every scope for one measured frame.
    pub fn paint(scopes: &FrameScopes) -> Self {
        let mut images = Self::default();
        images.repaint(scopes);
        images
    }

    /// Repaint into the existing pixel allocations.
    pub fn repaint(&mut self, scopes: &FrameScopes) {
        paint_waveform(&mut self.waveform, &scopes.waveform);
        paint_vectorscope(&mut self.vectorscope, &scopes.vectorscope);
        paint_histogram(&mut self.histogram, &scopes.histogram);
    }
}

impl Default for ScopeImages {
    fn default() -> Self {
        Self {
            waveform: canvas(HISTOGRAM_W, SCOPE_PX),
            vectorscope: canvas(SCOPE_PX, SCOPE_PX),
            histogram: canvas(HISTOGRAM_W, HISTOGRAM_H),
        }
    }
}

/// A blank scope canvas of `width × height`.
fn canvas(width: u32, height: u32) -> RgbaImage {
    let len = (width as usize) * (height as usize) * 4;
    let mut pixels = vec![0u8; len];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&BACKDROP);
    }
    RgbaImage::new(width, height, pixels)
}

/// Reset an existing canvas, reallocating only when its dimensions change.
fn reset_canvas(image: &mut RgbaImage, width: u32, height: u32) {
    let len = (width * height * 4) as usize;
    if image.width != width || image.height != height || image.pixels.len() != len {
        *image = canvas(width, height);
        return;
    }
    for pixel in image.pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&BACKDROP);
    }
}

/// Write one pixel, ignoring anything outside the canvas.
fn put(image: &mut RgbaImage, x: u32, y: u32, rgba: [u8; 4]) {
    if x >= image.width || y >= image.height {
        return;
    }
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].copy_from_slice(&rgba);
}

/// Add `rgba` to what is already there, so overlapping traces build up instead
/// of the last one winning.
fn add(image: &mut RgbaImage, x: u32, y: u32, rgba: [u8; 3], amount: f32) {
    if x >= image.width || y >= image.height {
        return;
    }
    let at = ((y * image.width + x) * 4) as usize;
    for channel in 0..3 {
        let target = f32::from(rgba[channel]) * amount.clamp(0.0, 1.0);
        let current = f32::from(image.pixels[at + channel]);
        image.pixels[at + channel] = (current + target).min(255.0) as u8;
    }
}

/// Perceptual lift for a cell holding `count` of `peak` samples.
fn intensity(count: u32, peak: u32) -> f32 {
    if count == 0 || peak == 0 {
        return 0.0;
    }
    (count as f32 / peak as f32).sqrt()
}

/// Horizontal graticule lines at 0, 25, 50, 75, and 100 percent.
fn draw_levels(image: &mut RgbaImage) {
    let last = image.height.saturating_sub(1);
    for step in 0..=4 {
        let y = last * step / 4;
        for x in 0..image.width {
            put(image, x, y, GRATICULE);
        }
    }
}

static VECTORSCOPE_GRATICULES: LazyLock<Mutex<HashMap<u32, Arc<[usize]>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Pixel offsets for one vectorscope centre. A size is rendered once, then each
/// frame copies its static cross and saturation ring into the reused canvas.
fn vectorscope_graticule(centre: u32) -> Arc<[usize]> {
    let mut cached = VECTORSCOPE_GRATICULES
        .lock()
        .expect("vectorscope graticule cache");
    Arc::clone(cached.entry(centre).or_insert_with(|| {
        let mut pixels = Vec::with_capacity(SCOPE_PX as usize * 2 + 360);
        for i in 0..SCOPE_PX {
            pixels.push(((centre * SCOPE_PX + i) * 4) as usize);
            pixels.push(((i * SCOPE_PX + centre) * 4) as usize);
        }
        let radius = centre as f32 * 0.75;
        for degrees in 0..360 {
            let angle = degrees as f32 * std::f32::consts::PI / 180.0;
            let x = centre as f32 + radius * angle.cos();
            let y = centre as f32 + radius * angle.sin();
            let (x, y) = (x.round() as u32, y.round() as u32);
            if x < SCOPE_PX && y < SCOPE_PX {
                pixels.push(((y * SCOPE_PX + x) * 4) as usize);
            }
        }
        pixels.sort_unstable();
        pixels.dedup();
        Arc::from(pixels)
    }))
}

/// Luma per horizontal position: columns across, code value up.
pub fn waveform_image(waveform: &Waveform) -> RgbaImage {
    let mut image = canvas(1, SCOPE_PX);
    paint_waveform(&mut image, waveform);
    image
}

fn paint_waveform(target: &mut RgbaImage, waveform: &Waveform) {
    let width = u32::try_from(waveform.columns()).unwrap_or(SCOPE_PX).max(1);
    reset_canvas(target, width, SCOPE_PX);
    let mut image = target;
    draw_levels(&mut image);
    let peak = waveform.peak();
    let last = SCOPE_PX - 1;
    for column in 0..waveform.columns() {
        for bin in 0..SCOPE_BINS {
            let hits = waveform.cell(column, bin);
            if hits == 0 {
                continue;
            }
            // Code 255 is the top of the display, code 0 the bottom.
            let y = last - (bin as u32 * last / (SCOPE_BINS as u32 - 1));
            add(
                &mut image,
                column as u32,
                y,
                [TRACE[0], TRACE[1], TRACE[2]],
                intensity(hits, peak),
            );
        }
    }
}

/// Chroma on the Cb/Cr plane, with a centre cross and a saturation ring.
pub fn vectorscope_image(vectorscope: &Vectorscope) -> RgbaImage {
    let mut image = canvas(SCOPE_PX, SCOPE_PX);
    paint_vectorscope(&mut image, vectorscope);
    image
}

fn paint_vectorscope(target: &mut RgbaImage, vectorscope: &Vectorscope) {
    reset_canvas(target, SCOPE_PX, SCOPE_PX);
    let mut image = target;
    let last = SCOPE_PX - 1;
    let size = vectorscope.size();
    // Grid cell to pixel. The graticule is placed through the same mapping as
    // the trace, so neutral chroma lands on the cross rather than a pixel
    // beside it.
    let map = |cell: usize| -> u32 {
        match size {
            0 | 1 => last / 2,
            _ => ((cell as u32 * last) / (size as u32 - 1)).min(last),
        }
    };
    let centre = map(vectorscope.centre());
    // Neutral cross and 75% saturation ring.
    for at in vectorscope_graticule(centre).iter().copied() {
        image.pixels[at..at + 4].copy_from_slice(&GRATICULE);
    }

    let peak = vectorscope.peak();
    for y in 0..size {
        for x in 0..size {
            let hits = vectorscope.cell(x, y);
            if hits == 0 {
                continue;
            }
            add(
                &mut image,
                map(x),
                map(y),
                [235, 235, 120],
                intensity(hits, peak),
            );
        }
    }
}

/// Code-value distribution: red, green, and blue traces over one another.
pub fn histogram_image(histogram: &Histogram) -> RgbaImage {
    let mut image = canvas(HISTOGRAM_W, HISTOGRAM_H);
    paint_histogram(&mut image, histogram);
    image
}

fn paint_histogram(target: &mut RgbaImage, histogram: &Histogram) {
    reset_canvas(target, HISTOGRAM_W, HISTOGRAM_H);
    let mut image = target;
    let last = HISTOGRAM_H - 1;
    // Quarter-tone verticals, so an operator can see where a clip sits.
    for step in 1..4 {
        let x = (HISTOGRAM_W - 1) * step / 4;
        for y in 0..HISTOGRAM_H {
            put(&mut image, x, y, GRATICULE);
        }
    }
    let peak = histogram.peak();
    let channels: [(&Vec<u32>, [u8; 3]); 3] = [
        (&histogram.red, [235, 80, 80]),
        (&histogram.green, [80, 235, 110]),
        (&histogram.blue, [90, 130, 245]),
    ];
    for (bins, colour) in channels {
        for (bin, count) in bins.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let x = bin as u32 * (HISTOGRAM_W - 1) / (SCOPE_BINS as u32 - 1);
            let height = (intensity(*count, peak) * last as f32).round() as u32;
            for step in 0..=height {
                add(&mut image, x, last - step, colour, 0.7);
            }
        }
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
