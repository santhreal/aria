//! Scope bitmaps: the measured frame drawn as a waveform monitor (luma, or an
//! RGB or YCbCr parade), a vectorscope, and an RGB histogram.
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

use aria_analysis::{
    FrameScopes, Histogram, SCOPE_BINS, Vectorscope, Waveform, WaveformChannel, WaveformTrace,
    vectorscope_cell,
};
use aria_core::RgbaImage;
use serde::{Deserialize, Serialize};
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

/// Red trace colour, in the waveform parade and the histogram.
const RED: [u8; 3] = [235, 80, 80];
/// Green trace colour, in the waveform parade and the histogram.
const GREEN: [u8; 3] = [80, 235, 110];
/// Blue trace colour, in the waveform parade and the histogram.
const BLUE: [u8; 3] = [90, 130, 245];

/// The vectorscope's skin tone line, in degrees counter-clockwise from the Cb
/// axis: the hue most skin falls on, whatever its tone.
pub const SKIN_TONE_DEGREES: f32 = 123.0;

/// The vectorscope targets: the primaries and secondaries, each at 75 % and
/// 100 % of full code value.
pub const TARGET_COLOURS: [[u8; 3]; 6] = [
    [255, 0, 0],
    [255, 255, 0],
    [0, 255, 0],
    [0, 255, 255],
    [0, 0, 255],
    [255, 0, 255],
];

/// The levels of [`TARGET_COLOURS`] a target marks: 75 % and 100 %.
pub const TARGET_LEVELS: [u8; 2] = [191, 255];

/// Half the edge of a target box, in pixels: a 75 % box is drawn larger than
/// a 100 % one, so the two read apart.
const TARGET_HALF: [u32; 2] = [4, 2];

/// What the waveform monitor shows. Saved with the UI settings, so a colourist
/// finds the parade they left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WaveformMode {
    /// Luma per column.
    #[default]
    Luma,
    /// Red, green, and blue side by side.
    RgbParade,
    /// Luma, Cb, and Cr side by side.
    YcbcrParade,
}

impl WaveformMode {
    /// Every mode, in the order the chips and the View menu offer them.
    pub const ALL: [Self; 3] = [Self::Luma, Self::RgbParade, Self::YcbcrParade];

    /// What the chips and the View menu call the mode.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Luma => "Luma",
            Self::RgbParade => "RGB Parade",
            Self::YcbcrParade => "YCbCr Parade",
        }
    }

    /// What the scope's own chips call the mode, where the caption already says
    /// it is the waveform.
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Luma => "Luma",
            Self::RgbParade => "RGB",
            Self::YcbcrParade => "YCbCr",
        }
    }

    /// The mode's chip id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Luma => "waveform-luma",
            Self::RgbParade => "waveform-rgb",
            Self::YcbcrParade => "waveform-ycbcr",
        }
    }

    /// The mode's View menu row id.
    pub const fn menu_id(self) -> &'static str {
        match self {
            Self::Luma => "view-waveform-luma",
            Self::RgbParade => "view-waveform-rgb",
            Self::YcbcrParade => "view-waveform-ycbcr",
        }
    }

    /// What the measurement counts for the mode.
    pub const fn trace(self) -> WaveformTrace {
        match self {
            Self::Luma => WaveformTrace::Luma,
            Self::RgbParade => WaveformTrace::RgbParade,
            Self::YcbcrParade => WaveformTrace::YcbcrParade,
        }
    }
}

/// The trace colour of one waveform channel.
const fn channel_colour(channel: WaveformChannel) -> [u8; 3] {
    match channel {
        WaveformChannel::Luma => [TRACE[0], TRACE[1], TRACE[2]],
        WaveformChannel::Red | WaveformChannel::Cr => RED,
        WaveformChannel::Green => GREEN,
        WaveformChannel::Blue | WaveformChannel::Cb => BLUE,
    }
}

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

static VECTORSCOPE_GRATICULES: LazyLock<Mutex<HashMap<usize, Arc<[usize]>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The drawn pixel of vectorscope grid cell `cell` on a grid `size` cells
/// wide. The graticule is placed through the same mapping as the trace, so
/// neutral chroma lands on the cross and a primary on its target rather than a
/// pixel beside them.
fn cell_pixel(cell: usize, size: usize) -> u32 {
    let last = SCOPE_PX - 1;
    match size {
        0 | 1 => last / 2,
        _ => ((cell as u32 * last) / (size as u32 - 1)).min(last),
    }
}

/// Pixel offsets of the graticule for a grid `size` cells wide: the neutral
/// cross, the 75 % saturation ring, the skin tone line, and a target box for
/// each primary and secondary at 75 % and 100 %. A size is rendered once, then
/// each frame copies the offsets into the reused canvas.
fn vectorscope_graticule(size: usize) -> Arc<[usize]> {
    let mut cached = VECTORSCOPE_GRATICULES
        .lock()
        .expect("vectorscope graticule cache");
    Arc::clone(cached.entry(size).or_insert_with(|| {
        let centre = cell_pixel(size / 2, size);
        let mut pixels = Vec::with_capacity(SCOPE_PX as usize * 3 + 360 + 12 * 32);
        let mut mark = |x: f32, y: f32| {
            let (x, y) = (x.round(), y.round());
            if (0.0..SCOPE_PX as f32).contains(&x) && (0.0..SCOPE_PX as f32).contains(&y) {
                pixels.push(((y as u32 * SCOPE_PX + x as u32) * 4) as usize);
            }
        };
        let middle = centre as f32;
        for i in 0..SCOPE_PX {
            mark(middle, i as f32);
            mark(i as f32, middle);
        }
        let radius = middle * 0.75;
        for degrees in 0..360 {
            let angle = (degrees as f32).to_radians();
            mark(middle + radius * angle.cos(), middle + radius * angle.sin());
        }
        // Cr is up, so a counter-clockwise angle climbs the canvas.
        let (sin, cos) = SKIN_TONE_DEGREES.to_radians().sin_cos();
        for step in 0..=centre {
            let t = step as f32;
            mark(middle + t * cos, middle - t * sin);
        }
        for (level, half) in TARGET_LEVELS.into_iter().zip(TARGET_HALF) {
            for [red, green, blue] in TARGET_COLOURS {
                let scale = |code: u8| if code == 0 { 0 } else { level };
                let (x, y) = vectorscope_cell(scale(red), scale(green), scale(blue), size);
                let (x, y) = (cell_pixel(x, size) as f32, cell_pixel(y, size) as f32);
                let edge = half as f32;
                for step in 0..=2 * half {
                    let offset = step as f32 - edge;
                    mark(x + offset, y - edge);
                    mark(x + offset, y + edge);
                    mark(x - edge, y + offset);
                    mark(x + edge, y + offset);
                }
            }
        }
        pixels.sort_unstable();
        pixels.dedup();
        Arc::from(pixels)
    }))
}

/// Code value per horizontal position: columns across, code value up. A
/// parade lays its channels left to right, a graticule column between each.
pub fn waveform_image(waveform: &Waveform) -> RgbaImage {
    let mut image = canvas(1, SCOPE_PX);
    paint_waveform(&mut image, waveform);
    image
}

fn paint_waveform(target: &mut RgbaImage, waveform: &Waveform) {
    let channels = waveform.trace().channels();
    let columns = u32::try_from(waveform.columns()).unwrap_or(SCOPE_PX).max(1);
    let count = channels.len() as u32;
    // Each channel after the first starts one graticule column further on.
    let pitch = columns + 1;
    reset_canvas(target, pitch * count - 1, SCOPE_PX);
    let mut image = target;
    draw_levels(&mut image);
    for gap in 1..count {
        for y in 0..SCOPE_PX {
            put(&mut image, gap * pitch - 1, y, GRATICULE);
        }
    }
    let peak = waveform.peak();
    let last = SCOPE_PX - 1;
    for (index, channel) in channels.iter().enumerate() {
        let Some(counts) = waveform.channel_counts(*channel) else {
            continue;
        };
        let left = index as u32 * pitch;
        let colour = channel_colour(*channel);
        for (column, bins) in counts.chunks_exact(SCOPE_BINS).enumerate() {
            for (bin, hits) in bins.iter().enumerate() {
                if *hits == 0 {
                    continue;
                }
                // Code 255 is the top of the display, code 0 the bottom.
                let y = last - (bin as u32 * last / (SCOPE_BINS as u32 - 1));
                add(
                    &mut image,
                    left + column as u32,
                    y,
                    colour,
                    intensity(*hits, peak),
                );
            }
        }
    }
}

/// Chroma on the Cb/Cr plane, with a centre cross, a saturation ring, the skin
/// tone line, and the 75 % and 100 % colour targets.
pub fn vectorscope_image(vectorscope: &Vectorscope) -> RgbaImage {
    let mut image = canvas(SCOPE_PX, SCOPE_PX);
    paint_vectorscope(&mut image, vectorscope);
    image
}

fn paint_vectorscope(target: &mut RgbaImage, vectorscope: &Vectorscope) {
    reset_canvas(target, SCOPE_PX, SCOPE_PX);
    let mut image = target;
    let size = vectorscope.size();
    for at in vectorscope_graticule(size).iter().copied() {
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
                cell_pixel(x, size),
                cell_pixel(y, size),
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
        (&histogram.red, RED),
        (&histogram.green, GREEN),
        (&histogram.blue, BLUE),
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
