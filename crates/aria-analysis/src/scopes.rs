//! Video scopes over one decoded frame: histogram, waveform monitor, and
//! vectorscope.
//!
//! Colour work done by eye drifts. These three read the frame the same way a
//! broadcast monitor does, so exposure, contrast, and saturation are numbers
//! instead of impressions.
//!
//! All three are built in one pass over the same sampled pixels, because they
//! answer questions about the same frame and a second pass would double the
//! cost for no extra information. Sampling is a fixed stride derived from the
//! pixel count and the budget, so the result is deterministic: the same frame
//! sampled twice gives identical counts, and a scope never depends on when it
//! was asked for.
//!
//! Fully transparent pixels are skipped. A frame padded to a pane, or a title
//! rendered over nothing, would otherwise pile alpha-zero black into bin 0 and
//! report an exposure the picture does not have.
//!
//! Luma and chroma are BT.709 (`0.2126 / 0.7152 / 0.0722`), the coefficients
//! the compositor and the encoders already work in.

use crate::ConfigError;
use crate::shot_detection::Rgba8Frame;

/// Bins per channel: one per 8-bit code value, so a histogram is exact rather
/// than rebucketed.
pub const SCOPE_BINS: usize = 256;

/// Largest waveform column count a [`ScopeConfig`] accepts. Beyond a few
/// hundred columns the display cannot show the difference, and the cell buffer
/// grows with it.
pub const MAX_WAVEFORM_COLUMNS: usize = 1024;

/// Largest vectorscope grid edge a [`ScopeConfig`] accepts.
pub const MAX_VECTORSCOPE_SIZE: usize = 512;

/// Largest sample budget a [`ScopeConfig`] accepts. A 4K frame holds 8.3M
/// pixels; reading all of them per playhead move would cost more than the
/// render that produced the frame.
pub const MAX_SCOPE_SAMPLES: usize = 4_000_000;

/// How much of the frame is read and how finely the scopes are resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeConfig {
    waveform_columns: usize,
    vectorscope_size: usize,
    maximum_samples: usize,
}

impl Default for ScopeConfig {
    /// The shipped preview settings: 256 waveform columns, a 128-pixel
    /// vectorscope, and a 200k-pixel budget, which is a whole 540p frame and a
    /// sixteenth of a 4K one.
    fn default() -> Self {
        Self {
            waveform_columns: 256,
            vectorscope_size: 128,
            maximum_samples: 200_000,
        }
    }
}

impl ScopeConfig {
    /// Validates a scope configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] naming the first field that is zero or over its
    /// documented ceiling.
    pub fn new(
        waveform_columns: usize,
        vectorscope_size: usize,
        maximum_samples: usize,
    ) -> Result<Self, ConfigError> {
        if waveform_columns == 0 || waveform_columns > MAX_WAVEFORM_COLUMNS {
            return Err(ConfigError::new(
                "waveform_columns",
                "must be between 1 and MAX_WAVEFORM_COLUMNS",
            ));
        }
        if vectorscope_size == 0 || vectorscope_size > MAX_VECTORSCOPE_SIZE {
            return Err(ConfigError::new(
                "vectorscope_size",
                "must be between 1 and MAX_VECTORSCOPE_SIZE",
            ));
        }
        if maximum_samples == 0 || maximum_samples > MAX_SCOPE_SAMPLES {
            return Err(ConfigError::new(
                "maximum_samples",
                "must be between 1 and MAX_SCOPE_SAMPLES",
            ));
        }
        Ok(Self {
            waveform_columns,
            vectorscope_size,
            maximum_samples,
        })
    }

    /// Waveform columns the frame's width is folded into.
    #[must_use]
    pub const fn waveform_columns(self) -> usize {
        self.waveform_columns
    }

    /// Edge length of the square vectorscope grid.
    #[must_use]
    pub const fn vectorscope_size(self) -> usize {
        self.vectorscope_size
    }

    /// Ceiling on sampled pixels per frame.
    #[must_use]
    pub const fn maximum_samples(self) -> usize {
        self.maximum_samples
    }
}

/// Code-value distribution per channel, plus luma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    /// Count per red code value.
    pub red: Vec<u32>,
    /// Count per green code value.
    pub green: Vec<u32>,
    /// Count per blue code value.
    pub blue: Vec<u32>,
    /// Count per BT.709 luma code value.
    pub luma: Vec<u32>,
}

impl Histogram {
    fn empty() -> Self {
        Self {
            red: vec![0; SCOPE_BINS],
            green: vec![0; SCOPE_BINS],
            blue: vec![0; SCOPE_BINS],
            luma: vec![0; SCOPE_BINS],
        }
    }

    /// The tallest bin across every channel, for scaling a drawn histogram.
    /// Zero only when nothing was sampled.
    #[must_use]
    pub fn peak(&self) -> u32 {
        self.red
            .iter()
            .chain(&self.green)
            .chain(&self.blue)
            .chain(&self.luma)
            .copied()
            .max()
            .unwrap_or(0)
    }
}

/// Luma per horizontal position: `columns × SCOPE_BINS` counts, so a column is
/// the vertical smear a waveform monitor draws for that part of the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    columns: usize,
    cells: Vec<u32>,
}

impl Waveform {
    /// Columns the frame width was folded into.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// Sample count at `column` with luma code `bin`, or zero out of bounds.
    #[must_use]
    pub fn cell(&self, column: usize, bin: usize) -> u32 {
        if column >= self.columns || bin >= SCOPE_BINS {
            return 0;
        }
        self.cells[column * SCOPE_BINS + bin]
    }

    /// The tallest cell, for scaling drawn intensity.
    #[must_use]
    pub fn peak(&self) -> u32 {
        self.cells.iter().copied().max().unwrap_or(0)
    }

    /// Mean luma code of `column`, or `None` when nothing landed there.
    #[must_use]
    pub fn column_mean(&self, column: usize) -> Option<f32> {
        if column >= self.columns {
            return None;
        }
        let row = &self.cells[column * SCOPE_BINS..(column + 1) * SCOPE_BINS];
        let count: u32 = row.iter().sum();
        if count == 0 {
            return None;
        }
        let total: u64 = row
            .iter()
            .enumerate()
            .map(|(bin, hits)| bin as u64 * u64::from(*hits))
            .sum();
        Some(total as f32 / count as f32)
    }
}

/// Chroma distribution on the Cb/Cr plane: a square grid with neutral at the
/// centre, Cb increasing to the right and Cr upward, which is the orientation
/// a vectorscope graticule is read in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vectorscope {
    size: usize,
    cells: Vec<u32>,
}

impl Vectorscope {
    /// Edge length of the grid.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Sample count in cell `(x, y)`, or zero out of bounds.
    #[must_use]
    pub fn cell(&self, x: usize, y: usize) -> u32 {
        if x >= self.size || y >= self.size {
            return 0;
        }
        self.cells[y * self.size + x]
    }

    /// The tallest cell, for scaling drawn intensity.
    #[must_use]
    pub fn peak(&self) -> u32 {
        self.cells.iter().copied().max().unwrap_or(0)
    }

    /// Grid coordinate of neutral chroma. A monochrome frame puts every sample
    /// here, which is what "no saturation" looks like on a scope.
    #[must_use]
    pub const fn centre(&self) -> usize {
        self.size / 2
    }
}

/// The three scopes for one frame, plus what they were measured from.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameScopes {
    /// Code-value distribution.
    pub histogram: Histogram,
    /// Luma per horizontal position.
    pub waveform: Waveform,
    /// Chroma distribution.
    pub vectorscope: Vectorscope,
    /// Pixels that contributed. Lower than the frame's pixel count when the
    /// budget forced a stride, or when the frame carries transparency.
    pub sampled: u32,
    /// Frame timestamp the scopes belong to, so a display cannot show one
    /// frame's numbers under another frame's picture.
    pub timestamp_seconds: f64,
}

/// BT.709 luma of one 8-bit RGB triple, rounded to a code value.
#[must_use]
pub fn luma709(red: u8, green: u8, blue: u8) -> u8 {
    let luma = 0.212_6 * f32::from(red) + 0.715_2 * f32::from(green) + 0.072_2 * f32::from(blue);
    luma.round().clamp(0.0, 255.0) as u8
}

/// BT.709 chroma of one 8-bit RGB triple, each in `[-0.5, 0.5]`.
#[must_use]
pub fn chroma709(red: u8, green: u8, blue: u8) -> (f32, f32) {
    let r = f32::from(red) / 255.0;
    let g = f32::from(green) / 255.0;
    let b = f32::from(blue) / 255.0;
    let cb = -0.114_572 * r - 0.385_428 * g + 0.5 * b;
    let cr = 0.5 * r - 0.454_153 * g - 0.045_847 * b;
    (cb, cr)
}

/// Measures every scope for one frame in a single pass.
///
/// Sampling walks a fixed stride in both axes, chosen so the sampled count
/// stays within `config.maximum_samples()`. The first row and column are
/// always sampled, so a frame smaller than the budget is read exactly.
#[must_use]
pub fn measure(frame: Rgba8Frame<'_>, config: ScopeConfig) -> FrameScopes {
    let mut histogram = Histogram::empty();
    let columns = config.waveform_columns();
    let size = config.vectorscope_size();
    let mut waveform = Waveform {
        columns,
        cells: vec![0; columns * SCOPE_BINS],
    };
    let mut vectorscope = Vectorscope {
        size,
        cells: vec![0; size * size],
    };

    let stride = sample_stride(frame.width(), frame.height(), config.maximum_samples());
    let bytes = frame.bytes();
    let mut sampled: u32 = 0;

    let mut y = 0;
    while y < frame.height() {
        let row = y * frame.row_stride();
        let mut x = 0;
        while x < frame.width() {
            let at = row + x * 4;
            let alpha = bytes[at + 3];
            if alpha != 0 {
                let (red, green, blue) = (bytes[at], bytes[at + 1], bytes[at + 2]);
                let luma = luma709(red, green, blue);
                histogram.red[usize::from(red)] += 1;
                histogram.green[usize::from(green)] += 1;
                histogram.blue[usize::from(blue)] += 1;
                histogram.luma[usize::from(luma)] += 1;

                let column = x * columns / frame.width();
                waveform.cells[column * SCOPE_BINS + usize::from(luma)] += 1;

                let (cb, cr) = chroma709(red, green, blue);
                let last = size.saturating_sub(1);
                let vx = (((cb + 0.5) * last as f32).round() as i64).clamp(0, last as i64) as usize;
                let vy = (((0.5 - cr) * last as f32).round() as i64).clamp(0, last as i64) as usize;
                vectorscope.cells[vy * size + vx] += 1;

                sampled += 1;
            }
            x += stride;
        }
        y += stride;
    }

    FrameScopes {
        histogram,
        waveform,
        vectorscope,
        sampled,
        timestamp_seconds: frame.timestamp_seconds(),
    }
}

/// Stride that keeps `ceil(w/s) * ceil(h/s)` within `budget`.
///
/// Grown one step at a time from the ideal square-root stride: the closed form
/// undershoots on extreme aspect ratios, and a scope that quietly reads more
/// than its budget on a 16384×16 frame is the cost this exists to bound.
fn sample_stride(width: usize, height: usize, budget: usize) -> usize {
    let pixels = width.saturating_mul(height);
    if pixels <= budget {
        return 1;
    }
    let mut stride = ((pixels as f64 / budget as f64).sqrt().floor() as usize).max(1);
    loop {
        let taken = width
            .div_ceil(stride)
            .saturating_mul(height.div_ceil(stride));
        if taken <= budget {
            return stride;
        }
        stride += 1;
    }
}

#[cfg(test)]
mod tests;
