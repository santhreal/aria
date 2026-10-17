//! Measurement of media the editor already holds: shots, levels, and scopes.
//!
//! Five things live here, and each one turns a decoded frame or a sampled
//! signal into numbers a surface can show or a command can act on:
//!
//! - [`shot_detection`](detect_shots): where one shot ends and the next begins.
//! - [`gain`]: decibels against the linear multiplier a mixer applies.
//! - [`scopes`]: histogram, waveform, and vectorscope over one frame.
//! - [`moments`]: the searchable index of what was found, optionally persistent.
//! - [`mod@align`]: where one signal envelope sits against another in time.
//!
//! Nothing here decodes, renders, or touches a GPU. Every entry point takes
//! already-sampled data and a validated configuration, so a measurement is
//! reproducible on any platform and testable without a device. Configuration is
//! validated at construction and reports the field it rejected
//! ([`ConfigError`]), because a silently clamped threshold reads as a detector
//! that does not work.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::error::Error;
use std::fmt;

pub mod align;
pub mod gain;
pub mod moments;
#[path = "params.rs"]
mod params;
#[path = "pcm.rs"]
mod pcm;
pub mod scopes;
mod shot_detection;

pub use params::{BeatConfig, LoudnessConfig, SilenceConfig};
pub use pcm::{
    BeatEvent, InterleavedPcm, LoudnessPoint, SilenceSpan, detect_beats, detect_silence,
    loudness_contour,
};

pub use align::{
    ALIGN_BUDGET, AlignConfig, Alignment, MAX_ALIGN_LAG, MAX_ALIGN_SAMPLES, RIVAL_GAP, align,
    align_anywhere, align_near,
};

pub use gain::{
    MAX_NORMALISATION_BOOST_DB, NORMALISATION_FLOOR_DBFS, db_from_linear, linear_from_db,
    normalisation_gain,
};
pub use scopes::measure as measure_scopes;
pub use scopes::{
    FrameScopes, Histogram, MAX_SCOPE_SAMPLES, MAX_VECTORSCOPE_SIZE, MAX_WAVEFORM_COLUMNS,
    SCOPE_BINS, ScopeConfig, Vectorscope, Waveform, WaveformChannel, WaveformTrace,
    chroma_codes709, sample_stride as scope_sample_stride, vectorscope_cell,
};
pub use shot_detection::{
    MAX_SHOT_HISTOGRAM_BINS, MAX_SHOT_SAMPLE_HEIGHT, MAX_SHOT_SAMPLE_WIDTH, Rgba8Frame,
    Rgba8FrameError, ShotBoundary, ShotBoundaryKind, ShotDetectionConfig, ShotDetectionError,
    ShotDetector, ShotScoreWeights, detect_shots,
};

/// A failure to construct a validated analysis configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    field: &'static str,
    reason: &'static str,
}

impl ConfigError {
    fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }

    /// The configuration field that failed validation.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// A stable, human-readable explanation of the failed constraint.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid analysis configuration `{}`: {}",
            self.field, self.reason
        )
    }
}

impl Error for ConfigError {}

/// A failure to construct a valid seconds-based [`TimeSpan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSpanError {
    reason: &'static str,
}

impl TimeSpanError {
    fn new(reason: &'static str) -> Self {
        Self { reason }
    }

    /// A stable, human-readable explanation of the failed constraint.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for TimeSpanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid time span: {}", self.reason)
    }
}

impl Error for TimeSpanError {}

/// Validation errors for borrowed interleaved PCM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioInputError {
    /// The sample rate was zero.
    ZeroSampleRate,
    /// The channel count was zero.
    ZeroChannels,
    /// The sample count was not divisible by the channel count.
    NonFrameAligned {
        /// Number of interleaved samples in the input.
        sample_count: usize,
        /// Declared number of channels per frame.
        channels: usize,
    },
    /// A sample was NaN or positive/negative infinity.
    NonFiniteSample {
        /// Zero-based sample index of the first invalid value.
        index: usize,
    },
}

impl fmt::Display for AudioInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSampleRate => formatter.write_str("sample rate must be non-zero"),
            Self::ZeroChannels => formatter.write_str("channel count must be non-zero"),
            Self::NonFrameAligned {
                sample_count,
                channels,
            } => write!(
                formatter,
                "{sample_count} interleaved samples do not form complete {channels}-channel frames"
            ),
            Self::NonFiniteSample { index } => {
                write!(formatter, "PCM sample at index {index} is not finite")
            }
        }
    }
}

impl Error for AudioInputError {}

/// A validated half-open time span, measured in seconds.
///
/// The span is `[start_seconds, end_seconds)`. Both boundaries are finite and
/// non-negative, and the end is never before the start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeSpan {
    start_seconds: f64,
    end_seconds: f64,
}

impl TimeSpan {
    /// Constructs a validated half-open span from absolute second values.
    ///
    /// # Errors
    ///
    /// Returns [`TimeSpanError`] for non-finite or negative boundaries, or when
    /// `end_seconds < start_seconds`.
    pub fn new(start_seconds: f64, end_seconds: f64) -> Result<Self, TimeSpanError> {
        if !start_seconds.is_finite() || !end_seconds.is_finite() {
            return Err(TimeSpanError::new("boundaries must be finite"));
        }
        if start_seconds < 0.0 || end_seconds < 0.0 {
            return Err(TimeSpanError::new("boundaries must be non-negative"));
        }
        if end_seconds < start_seconds {
            return Err(TimeSpanError::new(
                "end boundary must not precede start boundary",
            ));
        }

        Ok(Self {
            start_seconds,
            end_seconds,
        })
    }

    fn from_frames(start_frame: usize, end_frame: usize, sample_rate: u32) -> Self {
        Self {
            start_seconds: seconds_at_frame(start_frame, sample_rate),
            end_seconds: seconds_at_frame(end_frame, sample_rate),
        }
    }

    /// The inclusive start boundary in seconds.
    #[must_use]
    pub const fn start_seconds(self) -> f64 {
        self.start_seconds
    }

    /// The exclusive end boundary in seconds.
    #[must_use]
    pub const fn end_seconds(self) -> f64 {
        self.end_seconds
    }

    /// The span duration in seconds.
    #[must_use]
    pub fn duration_seconds(self) -> f64 {
        self.end_seconds - self.start_seconds
    }

    /// Whether the span has zero duration.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start_seconds == self.end_seconds
    }
}

pub(crate) fn seconds_at_frame(frame: usize, sample_rate: u32) -> f64 {
    frame as f64 / f64::from(sample_rate)
}

pub(crate) fn duration_to_frames(seconds: f64, sample_rate: u32, frame_count: usize) -> usize {
    if frame_count == 0 {
        return 0;
    }

    let frames = seconds * f64::from(sample_rate);
    if frames >= frame_count as f64 {
        frame_count
    } else {
        (frames.round() as usize).max(1)
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
