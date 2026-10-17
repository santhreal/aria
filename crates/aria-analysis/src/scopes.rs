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
//! the compositor and the encoders already work in. Both are computed in fixed
//! point, so a GPU pass that counts the same frame gets the same bins:
//! `aria_compositor::GpuScopes` runs this arithmetic in WGSL.

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

/// One quantity a waveform column counts, by 8-bit code value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveformChannel {
    /// BT.709 luma, [`luma709`].
    Luma,
    /// The red code value.
    Red,
    /// The green code value.
    Green,
    /// The blue code value.
    Blue,
    /// BT.709 Cb as a code value, 128 for neutral, [`chroma_codes709`].
    Cb,
    /// BT.709 Cr as a code value, 128 for neutral, [`chroma_codes709`].
    Cr,
}

/// What a waveform monitor counts: one trace, or a parade of three laid side
/// by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WaveformTrace {
    /// Luma per column.
    #[default]
    Luma,
    /// Red, green, and blue per column, side by side.
    RgbParade,
    /// Luma, Cb, and Cr per column, side by side.
    YcbcrParade,
}

impl WaveformTrace {
    /// Every trace, in the order a picker offers them.
    pub const ALL: [Self; 3] = [Self::Luma, Self::RgbParade, Self::YcbcrParade];

    /// The channels counted, in the order a parade lays them out left to
    /// right. Each is a block of `columns × SCOPE_BINS` counts in that order.
    #[must_use]
    pub const fn channels(self) -> &'static [WaveformChannel] {
        match self {
            Self::Luma => &[WaveformChannel::Luma],
            Self::RgbParade => &[
                WaveformChannel::Red,
                WaveformChannel::Green,
                WaveformChannel::Blue,
            ],
            Self::YcbcrParade => &[
                WaveformChannel::Luma,
                WaveformChannel::Cb,
                WaveformChannel::Cr,
            ],
        }
    }
}

/// How much of the frame is read and how finely the scopes are resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeConfig {
    waveform_columns: usize,
    vectorscope_size: usize,
    maximum_samples: usize,
    waveform_trace: WaveformTrace,
}

impl Default for ScopeConfig {
    /// The shipped preview settings: 256 waveform columns of luma, a 128-pixel
    /// vectorscope, and a 200k-pixel budget, which is a whole 540p frame and a
    /// sixteenth of a 4K one.
    fn default() -> Self {
        Self {
            waveform_columns: 256,
            vectorscope_size: 128,
            maximum_samples: 200_000,
            waveform_trace: WaveformTrace::Luma,
        }
    }
}

impl ScopeConfig {
    /// Validates a scope configuration. The waveform counts luma; see
    /// [`Self::with_waveform_trace`].
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
            waveform_trace: WaveformTrace::Luma,
        })
    }

    /// This configuration with the waveform counting `trace`.
    #[must_use]
    pub const fn with_waveform_trace(self, trace: WaveformTrace) -> Self {
        Self {
            waveform_trace: trace,
            ..self
        }
    }

    /// What the waveform counts.
    #[must_use]
    pub const fn waveform_trace(self) -> WaveformTrace {
        self.waveform_trace
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

/// Code values per horizontal position, for each channel its
/// [`WaveformTrace`] counts: `columns × SCOPE_BINS` counts per channel, so a
/// column is the vertical smear a waveform monitor draws for that part of the
/// picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    trace: WaveformTrace,
    columns: usize,
    cells: Vec<u32>,
}

impl Waveform {
    /// An empty waveform of `columns` columns counting `trace`.
    fn empty(trace: WaveformTrace, columns: usize) -> Self {
        Self {
            trace,
            columns,
            cells: vec![0; trace.channels().len() * columns * SCOPE_BINS],
        }
    }

    /// What the waveform counted.
    #[must_use]
    pub const fn trace(&self) -> WaveformTrace {
        self.trace
    }

    /// Columns the frame width was folded into.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// The counts of `channel`, [`SCOPE_BINS`] per column, column after
    /// column, or `None` when the trace does not count it.
    #[must_use]
    pub fn channel_counts(&self, channel: WaveformChannel) -> Option<&[u32]> {
        let index = self
            .trace
            .channels()
            .iter()
            .position(|counted| *counted == channel)?;
        let len = self.columns * SCOPE_BINS;
        self.cells.get(index * len..(index + 1) * len)
    }

    /// Sample count at `column` with luma code `bin`, or zero out of bounds
    /// or when the trace counts no luma.
    #[must_use]
    pub fn cell(&self, column: usize, bin: usize) -> u32 {
        self.channel_cell(WaveformChannel::Luma, column, bin)
    }

    /// Sample count of `channel` at `column` with code `bin`, or zero out of
    /// bounds or when the trace does not count `channel`.
    #[must_use]
    pub fn channel_cell(&self, channel: WaveformChannel, column: usize, bin: usize) -> u32 {
        if column >= self.columns || bin >= SCOPE_BINS {
            return 0;
        }
        self.channel_counts(channel)
            .map_or(0, |plane| plane[column * SCOPE_BINS + bin])
    }

    /// The tallest cell across every counted channel, for scaling drawn
    /// intensity. One peak for a parade, so its channels compare at one
    /// brightness.
    #[must_use]
    pub fn peak(&self) -> u32 {
        self.cells.iter().copied().max().unwrap_or(0)
    }

    /// Mean luma code of `column`, or `None` when nothing landed there or the
    /// trace counts no luma.
    #[must_use]
    pub fn column_mean(&self, column: usize) -> Option<f32> {
        if column >= self.columns {
            return None;
        }
        let plane = self.channel_counts(WaveformChannel::Luma)?;
        let row = &plane[column * SCOPE_BINS..(column + 1) * SCOPE_BINS];
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
    /// Code values per horizontal position.
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

/// Luma weights in 16-bit fixed point. They sum to 65536, so a grey reads
/// its own code value.
const LUMA_WEIGHTS: [u32; 3] = [13_933, 46_871, 4_732];

/// One 8-bit channel's full span in the 15-bit chroma fixed point.
const CHROMA_ONE: u64 = 32_768 * 255;

/// Neutral chroma, half of [`CHROMA_ONE`].
const CHROMA_HALF: u64 = CHROMA_ONE / 2;

impl FrameScopes {
    /// Scopes counted outside [`measure`], such as by a GPU pass, read under
    /// `config`.
    ///
    /// `histogram` is red, green, blue, and luma, [`SCOPE_BINS`] counts each.
    /// `waveform` holds, for each channel of `config.waveform_trace()` in
    /// order, `config.waveform_columns()` columns of [`SCOPE_BINS`] counts,
    /// column after column. `vectorscope` holds
    /// `config.vectorscope_size()` rows of that many counts, top row first.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] naming the first buffer whose length does not
    /// match `config`.
    pub fn from_counts(
        config: ScopeConfig,
        histogram: [Vec<u32>; 4],
        waveform: Vec<u32>,
        vectorscope: Vec<u32>,
        sampled: u32,
        timestamp_seconds: f64,
    ) -> Result<Self, ConfigError> {
        if histogram.iter().any(|bins| bins.len() != SCOPE_BINS) {
            return Err(ConfigError::new(
                "histogram",
                "every channel must hold SCOPE_BINS counts",
            ));
        }
        let columns = config.waveform_columns();
        let trace = config.waveform_trace();
        if waveform.len() != trace.channels().len() * columns * SCOPE_BINS {
            return Err(ConfigError::new(
                "waveform",
                "must hold waveform_columns times SCOPE_BINS counts per traced channel",
            ));
        }
        let size = config.vectorscope_size();
        if vectorscope.len() != size * size {
            return Err(ConfigError::new(
                "vectorscope",
                "must hold vectorscope_size squared counts",
            ));
        }
        let [red, green, blue, luma] = histogram;
        Ok(Self {
            histogram: Histogram {
                red,
                green,
                blue,
                luma,
            },
            waveform: Waveform {
                trace,
                columns,
                cells: waveform,
            },
            vectorscope: Vectorscope {
                size,
                cells: vectorscope,
            },
            sampled,
            timestamp_seconds,
        })
    }
}

/// BT.709 luma of one 8-bit RGB triple, rounded to a code value.
///
/// Fixed point: `0.2126 / 0.7152 / 0.0722` are `13933 / 46871 / 4732` over
/// 65536, rounded half up.
#[must_use]
pub fn luma709(red: u8, green: u8, blue: u8) -> u8 {
    let [wr, wg, wb] = LUMA_WEIGHTS;
    let weighted = wr * u32::from(red) + wg * u32::from(green) + wb * u32::from(blue);
    // At most 255 * 65536 + 32768, so the shift leaves at most 255.
    ((weighted + 32_768) >> 16) as u8
}

/// The vectorscope cell `(x, y)` of one 8-bit RGB triple on a grid `size`
/// cells wide: Cb to the right, Cr upward, neutral at [`Vectorscope::centre`].
///
/// BT.709 chroma in 15-bit fixed point: Cb is `-3754 R - 12630 G + 16384 B`
/// and Cr is `16384 R - 14882 G - 1502 B` over 32768, so every grey lands on
/// the centre cell. Positions round half up. For every `size` up to
/// [`MAX_VECTORSCOPE_SIZE`] each intermediate fits in a `u32`, which is what
/// the GPU pass computes in.
#[must_use]
pub fn vectorscope_cell(red: u8, green: u8, blue: u8, size: usize) -> (usize, usize) {
    let (right, up) = chroma_offsets(red, green, blue);
    offsets_cell(right, up, size)
}

/// The vectorscope cell of chroma offsets from [`chroma_offsets`].
fn offsets_cell(right: u64, up: u64, size: usize) -> (usize, usize) {
    let last = size.saturating_sub(1) as u64;
    let cell = |offset: u64| ((offset * last + CHROMA_HALF) / CHROMA_ONE).min(last) as usize;
    (cell(right), cell(CHROMA_ONE - up))
}

/// BT.709 Cb and Cr of one 8-bit RGB triple as code values: 0 at -1/2, 255 at
/// +1/2, and 128 for every grey. The fixed point of [`vectorscope_cell`],
/// rounded half up to 255 steps; each intermediate fits in a `u32`, which is
/// what the GPU pass computes in.
#[must_use]
pub fn chroma_codes709(red: u8, green: u8, blue: u8) -> (u8, u8) {
    let (right, up) = chroma_offsets(red, green, blue);
    (offset_code(right), offset_code(up))
}

/// The code value of one chroma offset from [`chroma_offsets`]. At most
/// `CHROMA_ONE * 255 + CHROMA_HALF` over `CHROMA_ONE`, so at most 255.
fn offset_code(offset: u64) -> u8 {
    ((offset * 255 + CHROMA_HALF) / CHROMA_ONE) as u8
}

/// Cb + 1/2 and Cr + 1/2 of one 8-bit RGB triple, each in `[0, CHROMA_ONE]`.
fn chroma_offsets(red: u8, green: u8, blue: u8) -> (u64, u64) {
    let (r, g, b) = (u64::from(red), u64::from(green), u64::from(blue));
    let right = (16_384 * b + CHROMA_HALF) - (3_754 * r + 12_630 * g);
    let up = (16_384 * r + CHROMA_HALF) - (14_882 * g + 1_502 * b);
    (right, up)
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
    let channels = config.waveform_trace().channels();
    let mut waveform = Waveform::empty(config.waveform_trace(), columns);
    let plane = columns * SCOPE_BINS;
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
                let (right, up) = chroma_offsets(red, green, blue);
                for (index, channel) in channels.iter().enumerate() {
                    let code = match channel {
                        WaveformChannel::Luma => luma,
                        WaveformChannel::Red => red,
                        WaveformChannel::Green => green,
                        WaveformChannel::Blue => blue,
                        WaveformChannel::Cb => offset_code(right),
                        WaveformChannel::Cr => offset_code(up),
                    };
                    waveform.cells[index * plane + column * SCOPE_BINS + usize::from(code)] += 1;
                }

                let (vx, vy) = offsets_cell(right, up, size);
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

/// Stride that keeps `ceil(w/s) * ceil(h/s)` within `budget`: [`measure`]
/// reads every `stride`-th pixel of every `stride`-th row, starting at the
/// first, and so does a GPU pass that counts the same frame.
///
/// Grown one step at a time from the ideal square-root stride: the closed form
/// undershoots on extreme aspect ratios, and a scope that quietly reads more
/// than its budget on a 16384×16 frame is the cost this exists to bound.
#[must_use]
pub fn sample_stride(width: usize, height: usize, budget: usize) -> usize {
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
