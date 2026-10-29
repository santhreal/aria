//! Core shot boundary detector and frame validation.

use std::borrow::Borrow;
use std::error::Error;
use std::fmt;

use super::histogram::{FrameFeatures, extract_features};
use super::metrics::{ShotScoreWeights, scene_change_score};
use crate::ConfigError;

/// Largest accepted horizontal sampling bound.
pub const MAX_SHOT_SAMPLE_WIDTH: usize = 512;

/// Largest accepted vertical sampling bound.
pub const MAX_SHOT_SAMPLE_HEIGHT: usize = 512;

/// Largest accepted luminance histogram size.
pub const MAX_SHOT_HISTOGRAM_BINS: usize = 256;

/// Validation errors for a borrowed RGBA8 frame view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rgba8FrameError {
    /// The declared width was zero.
    ZeroWidth,
    /// The declared height was zero.
    ZeroHeight,
    /// Multiplying the width by four bytes per pixel overflowed `usize`.
    RowBytesOverflow {
        /// Declared frame width in pixels.
        width: usize,
    },
    /// The row stride cannot hold one complete visible RGBA8 row.
    StrideTooSmall {
        /// Declared bytes between consecutive row starts.
        row_stride: usize,
        /// Minimum bytes required for one visible row.
        minimum_row_stride: usize,
    },
    /// Computing the byte offset of the final visible pixel overflowed `usize`.
    RequiredLengthOverflow {
        /// Declared frame height in pixels.
        height: usize,
        /// Declared bytes between consecutive row starts.
        row_stride: usize,
        /// Bytes occupied by visible pixels in one row.
        visible_row_bytes: usize,
    },
    /// The borrowed buffer does not reach the final visible pixel.
    BufferTooShort {
        /// Actual borrowed byte length.
        actual_length: usize,
        /// Minimum byte length implied by the dimensions and row stride.
        required_length: usize,
    },
    /// The frame timestamp was NaN or positive/negative infinity.
    NonFiniteTimestamp,
    /// The frame timestamp was finite but negative.
    NegativeTimestamp,
}

impl fmt::Display for Rgba8FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWidth => formatter.write_str("RGBA8 frame width must be non-zero"),
            Self::ZeroHeight => formatter.write_str("RGBA8 frame height must be non-zero"),
            Self::RowBytesOverflow { width } => {
                write!(
                    formatter,
                    "RGBA8 row byte count overflows for width {width}"
                )
            }
            Self::StrideTooSmall {
                row_stride,
                minimum_row_stride,
            } => write!(
                formatter,
                "RGBA8 row stride {row_stride} is smaller than required {minimum_row_stride}"
            ),
            Self::RequiredLengthOverflow {
                height,
                row_stride,
                visible_row_bytes,
            } => write!(
                formatter,
                "RGBA8 required length overflows for height {height}, stride {row_stride}, and \
                 visible row size {visible_row_bytes}"
            ),
            Self::BufferTooShort {
                actual_length,
                required_length,
            } => write!(
                formatter,
                "RGBA8 buffer has {actual_length} bytes but requires at least {required_length}"
            ),
            Self::NonFiniteTimestamp => formatter.write_str("RGBA8 frame timestamp must be finite"),
            Self::NegativeTimestamp => {
                formatter.write_str("RGBA8 frame timestamp must be non-negative")
            }
        }
    }
}

impl Error for Rgba8FrameError {}

/// A validated borrowed view of one row-strided RGBA8 frame.
#[derive(Debug, Clone, Copy)]
pub struct Rgba8Frame<'a> {
    bytes: &'a [u8],
    width: usize,
    height: usize,
    row_stride: usize,
    visible_row_bytes: usize,
    timestamp_seconds: f64,
}

impl<'a> Rgba8Frame<'a> {
    /// Validates and borrows an RGBA8 frame.
    pub fn new(
        bytes: &'a [u8],
        width: usize,
        height: usize,
        row_stride: usize,
        timestamp_seconds: f64,
    ) -> Result<Self, Rgba8FrameError> {
        if width == 0 {
            return Err(Rgba8FrameError::ZeroWidth);
        }
        if height == 0 {
            return Err(Rgba8FrameError::ZeroHeight);
        }

        let visible_row_bytes = width
            .checked_mul(4)
            .ok_or(Rgba8FrameError::RowBytesOverflow { width })?;
        if row_stride < visible_row_bytes {
            return Err(Rgba8FrameError::StrideTooSmall {
                row_stride,
                minimum_row_stride: visible_row_bytes,
            });
        }

        let required_length = (height - 1)
            .checked_mul(row_stride)
            .and_then(|row_offset| row_offset.checked_add(visible_row_bytes))
            .ok_or(Rgba8FrameError::RequiredLengthOverflow {
                height,
                row_stride,
                visible_row_bytes,
            })?;
        if bytes.len() < required_length {
            return Err(Rgba8FrameError::BufferTooShort {
                actual_length: bytes.len(),
                required_length,
            });
        }
        if !timestamp_seconds.is_finite() {
            return Err(Rgba8FrameError::NonFiniteTimestamp);
        }
        if timestamp_seconds < 0.0 {
            return Err(Rgba8FrameError::NegativeTimestamp);
        }

        Ok(Self {
            bytes,
            width,
            height,
            row_stride,
            visible_row_bytes,
            timestamp_seconds,
        })
    }

    /// The original borrowed bytes, including row padding and any trailing data.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Visible frame width in pixels.
    #[must_use]
    pub const fn width(self) -> usize {
        self.width
    }

    /// Visible frame height in pixels.
    #[must_use]
    pub const fn height(self) -> usize {
        self.height
    }

    /// Bytes between consecutive row starts.
    #[must_use]
    pub const fn row_stride(self) -> usize {
        self.row_stride
    }

    /// Bytes occupied by visible RGBA pixels in one row.
    #[must_use]
    pub const fn visible_row_bytes(self) -> usize {
        self.visible_row_bytes
    }

    /// Finite, non-negative presentation timestamp in seconds.
    #[must_use]
    pub const fn timestamp_seconds(self) -> f64 {
        self.timestamp_seconds
    }
}

/// Validated deterministic shot-boundary detection parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotDetectionConfig {
    sample_width: usize,
    sample_height: usize,
    luma_histogram_bins: usize,
    score_weights: ShotScoreWeights,
    hard_cut_threshold: f32,
    minimum_shot_duration_seconds: f64,
}

impl ShotDetectionConfig {
    /// Constructs a validated shot-detection configuration.
    pub fn new(
        sample_width: usize,
        sample_height: usize,
        luma_histogram_bins: usize,
        score_weights: ShotScoreWeights,
        hard_cut_threshold: f32,
        minimum_shot_duration_seconds: f64,
    ) -> Result<Self, ConfigError> {
        if sample_width == 0 || sample_width > MAX_SHOT_SAMPLE_WIDTH {
            return Err(ConfigError::new(
                "sample_width",
                "must be between 1 and MAX_SHOT_SAMPLE_WIDTH",
            ));
        }
        if sample_height == 0 || sample_height > MAX_SHOT_SAMPLE_HEIGHT {
            return Err(ConfigError::new(
                "sample_height",
                "must be between 1 and MAX_SHOT_SAMPLE_HEIGHT",
            ));
        }
        if !(2..=MAX_SHOT_HISTOGRAM_BINS).contains(&luma_histogram_bins) {
            return Err(ConfigError::new(
                "luma_histogram_bins",
                "must be between 2 and MAX_SHOT_HISTOGRAM_BINS",
            ));
        }
        if !hard_cut_threshold.is_finite() || hard_cut_threshold <= 0.0 || hard_cut_threshold > 1.0
        {
            return Err(ConfigError::new(
                "hard_cut_threshold",
                "must be finite, positive, and at most 1",
            ));
        }
        if !minimum_shot_duration_seconds.is_finite() || minimum_shot_duration_seconds < 0.0 {
            return Err(ConfigError::new(
                "minimum_shot_duration_seconds",
                "must be finite and non-negative",
            ));
        }

        Ok(Self {
            sample_width,
            sample_height,
            luma_histogram_bins,
            score_weights,
            hard_cut_threshold,
            minimum_shot_duration_seconds,
        })
    }

    /// Maximum horizontal samples taken from a frame.
    #[must_use]
    pub const fn sample_width(self) -> usize {
        self.sample_width
    }

    /// Maximum vertical samples taken from a frame.
    #[must_use]
    pub const fn sample_height(self) -> usize {
        self.sample_height
    }

    /// Number of fixed luminance histogram bins.
    #[must_use]
    pub const fn luma_histogram_bins(self) -> usize {
        self.luma_histogram_bins
    }

    /// Relative component weights normalized during scoring.
    #[must_use]
    pub const fn score_weights(self) -> ShotScoreWeights {
        self.score_weights
    }

    /// Inclusive score threshold for a hard-cut candidate.
    #[must_use]
    pub const fn hard_cut_threshold(self) -> f32 {
        self.hard_cut_threshold
    }

    /// Minimum timestamp distance from the current shot's sampled start.
    #[must_use]
    pub const fn minimum_shot_duration_seconds(self) -> f64 {
        self.minimum_shot_duration_seconds
    }

    /// Maximum sampled pixels stored for one frame feature.
    #[must_use]
    pub const fn maximum_sampled_pixels(self) -> usize {
        self.sample_width * self.sample_height
    }
}

impl Default for ShotDetectionConfig {
    fn default() -> Self {
        Self {
            sample_width: 64,
            sample_height: 36,
            luma_histogram_bins: 32,
            score_weights: ShotScoreWeights::default(),
            hard_cut_threshold: 0.38,
            minimum_shot_duration_seconds: 0.5,
        }
    }
}

/// Why a shot boundary candidate was emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotBoundaryKind {
    /// Same-sized neighboring frames exceeded the configured visual threshold.
    VisualChange,
    /// Neighboring frames had different visible dimensions.
    DimensionChange,
}

/// A deterministic boundary between two neighboring sampled frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotBoundary {
    pub(crate) timestamp_seconds: f64,
    pub(crate) score: f32,
    pub(crate) previous_frame_timestamp_seconds: f64,
    pub(crate) current_frame_timestamp_seconds: f64,
    pub(crate) kind: ShotBoundaryKind,
}

impl ShotBoundary {
    /// Timestamp of the first sampled frame in the candidate new shot.
    #[must_use]
    pub const fn timestamp_seconds(self) -> f64 {
        self.timestamp_seconds
    }

    /// Finite normalized candidate strength in `[0, 1]`.
    #[must_use]
    pub const fn score(self) -> f32 {
        self.score
    }

    /// Timestamp of the sampled frame immediately before the transition.
    #[must_use]
    pub const fn previous_frame_timestamp_seconds(self) -> f64 {
        self.previous_frame_timestamp_seconds
    }

    /// Timestamp of the sampled frame immediately after the transition.
    #[must_use]
    pub const fn current_frame_timestamp_seconds(self) -> f64 {
        self.current_frame_timestamp_seconds
    }

    /// Whether visual statistics or a dimension change caused the boundary.
    #[must_use]
    pub const fn kind(self) -> ShotBoundaryKind {
        self.kind
    }
}

/// Sequence validation errors from shot detection.
#[derive(Debug, Clone, PartialEq)]
pub enum ShotDetectionError {
    /// A frame timestamp preceded the last successfully processed timestamp.
    OutOfOrderTimestamp {
        /// Timestamp of the last successfully processed frame.
        previous_timestamp_seconds: f64,
        /// Timestamp of the rejected frame.
        current_timestamp_seconds: f64,
    },
}

impl fmt::Display for ShotDetectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfOrderTimestamp {
                previous_timestamp_seconds,
                current_timestamp_seconds,
            } => write!(
                formatter,
                "frame timestamp {current_timestamp_seconds} precedes previous timestamp \
                 {previous_timestamp_seconds}"
            ),
        }
    }
}

impl Error for ShotDetectionError {}

#[derive(Debug, Clone, Copy)]
struct FrameMetadata {
    width: usize,
    height: usize,
    timestamp_seconds: f64,
}

/// Reusable deterministic streaming shot detector.
#[derive(Debug)]
pub struct ShotDetector {
    config: ShotDetectionConfig,
    previous_metadata: Option<FrameMetadata>,
    current_shot_start_seconds: Option<f64>,
    previous_features: FrameFeatures,
    scratch_features: FrameFeatures,
}

impl ShotDetector {
    /// Creates an empty streaming detector with reusable bounded buffers.
    #[must_use]
    pub fn new(config: ShotDetectionConfig) -> Self {
        let maximum_sampled_pixels = config.maximum_sampled_pixels();
        let histogram_bins = config.luma_histogram_bins();
        Self {
            config,
            previous_metadata: None,
            current_shot_start_seconds: None,
            previous_features: FrameFeatures::new(maximum_sampled_pixels, histogram_bins),
            scratch_features: FrameFeatures::new(maximum_sampled_pixels, histogram_bins),
        }
    }

    /// The validated configuration used by this detector.
    #[must_use]
    pub const fn config(&self) -> ShotDetectionConfig {
        self.config
    }

    /// Logical upper bound on sampled pixel records across both reusable buffers.
    #[must_use]
    pub const fn sampled_pixel_storage_limit(&self) -> usize {
        self.config.maximum_sampled_pixels() * 2
    }

    /// Logical upper bound on histogram bins across both reusable buffers.
    #[must_use]
    pub const fn histogram_bin_storage_limit(&self) -> usize {
        self.config.luma_histogram_bins() * 2
    }

    /// Number of sampled pixel records currently retained in reusable buffers.
    #[must_use]
    pub fn retained_sampled_pixels(&self) -> usize {
        self.previous_features.sampled_rgba.len() + self.scratch_features.sampled_rgba.len()
    }

    /// Clears sequence history while retaining allocated feature buffers.
    pub fn reset(&mut self) {
        self.previous_metadata = None;
        self.current_shot_start_seconds = None;
        self.previous_features.clear();
        self.scratch_features.clear();
    }

    /// Processes one validated frame and returns at most one boundary.
    pub fn push_frame(
        &mut self,
        frame: Rgba8Frame<'_>,
    ) -> Result<Option<ShotBoundary>, ShotDetectionError> {
        if let Some(previous) = self.previous_metadata
            && frame.timestamp_seconds() < previous.timestamp_seconds
        {
            return Err(ShotDetectionError::OutOfOrderTimestamp {
                previous_timestamp_seconds: previous.timestamp_seconds,
                current_timestamp_seconds: frame.timestamp_seconds(),
            });
        }

        extract_features(frame, self.config, &mut self.scratch_features);
        let current_metadata = FrameMetadata {
            width: frame.width(),
            height: frame.height(),
            timestamp_seconds: frame.timestamp_seconds(),
        };

        let Some(previous_metadata) = self.previous_metadata else {
            std::mem::swap(&mut self.previous_features, &mut self.scratch_features);
            self.previous_metadata = Some(current_metadata);
            self.current_shot_start_seconds = Some(frame.timestamp_seconds());
            return Ok(None);
        };

        let (score, kind) = if frame.width() != previous_metadata.width
            || frame.height() != previous_metadata.height
        {
            (1.0, ShotBoundaryKind::DimensionChange)
        } else {
            (
                scene_change_score(
                    &self.previous_features,
                    &self.scratch_features,
                    self.config.score_weights(),
                ),
                ShotBoundaryKind::VisualChange,
            )
        };

        let duration_elapsed = self.current_shot_start_seconds.is_none_or(|start| {
            frame.timestamp_seconds() - start >= self.config.minimum_shot_duration_seconds()
        });
        let boundary = (score >= self.config.hard_cut_threshold() && duration_elapsed).then_some(
            ShotBoundary {
                timestamp_seconds: frame.timestamp_seconds(),
                score,
                previous_frame_timestamp_seconds: previous_metadata.timestamp_seconds,
                current_frame_timestamp_seconds: frame.timestamp_seconds(),
                kind,
            },
        );

        std::mem::swap(&mut self.previous_features, &mut self.scratch_features);
        self.previous_metadata = Some(current_metadata);
        if boundary.is_some() {
            self.current_shot_start_seconds = Some(frame.timestamp_seconds());
        }

        Ok(boundary)
    }
}

/// Detects shot boundaries over validated borrowed RGBA8 frame views.
pub fn detect_shots<'a, I>(
    frames: I,
    config: ShotDetectionConfig,
) -> Result<Vec<ShotBoundary>, ShotDetectionError>
where
    I: IntoIterator,
    I::Item: Borrow<Rgba8Frame<'a>>,
{
    let mut detector = ShotDetector::new(config);
    let mut boundaries = Vec::new();
    for frame in frames {
        if let Some(boundary) = detector.push_frame(*frame.borrow())? {
            boundaries.push(boundary);
        }
    }
    Ok(boundaries)
}
