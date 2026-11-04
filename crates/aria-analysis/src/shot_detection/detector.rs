//! Core shot boundary detector and frame validation.

use std::borrow::Borrow;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use super::histogram::{FrameFeatures, extract_features};
use super::metrics::{ShotScoreWeights, scene_change_score};
use super::neighborhood::{MAX_SHOT_CONTEXT_FRAMES, ShotNeighborhood};
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
    neighborhood: ShotNeighborhood,
    hard_cut_threshold: f32,
    minimum_shot_duration_seconds: f64,
}

impl ShotDetectionConfig {
    /// Constructs a validated shot-detection configuration with the default
    /// [`ShotNeighborhood`]; [`Self::with_neighborhood`] replaces it.
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
            neighborhood: ShotNeighborhood::default(),
            hard_cut_threshold,
            minimum_shot_duration_seconds,
        })
    }

    /// This configuration with `neighborhood` as its context test.
    #[must_use]
    pub const fn with_neighborhood(self, neighborhood: ShotNeighborhood) -> Self {
        Self {
            neighborhood,
            ..self
        }
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

    /// The context test a candidate passes after clearing the threshold.
    #[must_use]
    pub const fn neighborhood(self) -> ShotNeighborhood {
        self.neighborhood
    }

    /// Inclusive score a frame-to-frame change must reach to be a cut candidate.
    #[must_use]
    pub const fn hard_cut_threshold(self) -> f32 {
        self.hard_cut_threshold
    }

    /// Minimum timestamp distance from the current shot's sampled start to the
    /// next boundary.
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
            neighborhood: ShotNeighborhood::default(),
            hard_cut_threshold: 0.28,
            minimum_shot_duration_seconds: 0.1,
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

/// One retained sampled frame.
#[derive(Debug)]
struct FrameSlot {
    features: FrameFeatures,
    width: usize,
    height: usize,
    timestamp_seconds: f64,
    /// Score and kind of the change from the frame before; `None` for the first
    /// frame of a sequence.
    change: Option<(f32, ShotBoundaryKind)>,
}

impl FrameSlot {
    fn new(config: ShotDetectionConfig) -> Self {
        Self {
            features: FrameFeatures::new(
                config.maximum_sampled_pixels(),
                config.luma_histogram_bins(),
            ),
            width: 0,
            height: 0,
            timestamp_seconds: 0.0,
            change: None,
        }
    }

    /// The change from this frame to `later`: a scored visual change for frames
    /// of one size, and a dimension change scoring 1 otherwise.
    fn change_to(&self, later: &Self, weights: ShotScoreWeights) -> (f32, ShotBoundaryKind) {
        if self.width == later.width && self.height == later.height {
            (
                scene_change_score(&self.features, &later.features, weights),
                ShotBoundaryKind::VisualChange,
            )
        } else {
            (1.0, ShotBoundaryKind::DimensionChange)
        }
    }
}

/// Reusable deterministic streaming shot detector.
///
/// A frame's candidate resolves once [`ShotNeighborhood::lookahead_frames`] later
/// frames have arrived, so [`Self::push_frame`] reports a boundary that many
/// frames late, and [`Self::finish`] resolves the frames still waiting at the end
/// of a sequence.
#[derive(Debug)]
pub struct ShotDetector {
    config: ShotDetectionConfig,
    /// The newest frames, oldest first; at most the neighborhood's retained count.
    history: VecDeque<FrameSlot>,
    /// Cleared slots kept by [`Self::reset`] for reuse.
    spare: Vec<FrameSlot>,
    /// Frames pushed since the sequence started.
    frames_pushed: usize,
    /// Index of the oldest frame whose candidate is unresolved.
    next_candidate: usize,
    /// Timestamp of the frame that started the current shot; the first frame of
    /// a sequence sets it.
    shot_start_seconds: f64,
}

impl ShotDetector {
    /// Creates an empty streaming detector whose frame buffers are allocated on
    /// first use and reused after.
    #[must_use]
    pub fn new(config: ShotDetectionConfig) -> Self {
        Self {
            config,
            history: VecDeque::with_capacity(config.neighborhood().retained_frames()),
            spare: Vec::new(),
            frames_pushed: 0,
            next_candidate: 1,
            shot_start_seconds: 0.0,
        }
    }

    /// The validated configuration used by this detector.
    #[must_use]
    pub const fn config(&self) -> ShotDetectionConfig {
        self.config
    }

    /// Logical upper bound on sampled pixel records across the retained frames.
    #[must_use]
    pub const fn sampled_pixel_storage_limit(&self) -> usize {
        self.config.maximum_sampled_pixels() * self.config.neighborhood().retained_frames()
    }

    /// Number of sampled pixel records currently retained in frame buffers.
    #[must_use]
    pub fn retained_sampled_pixels(&self) -> usize {
        self.history
            .iter()
            .chain(&self.spare)
            .map(|slot| slot.features.sampled_rgba.len())
            .sum()
    }

    /// Clears sequence history while retaining allocated frame buffers.
    pub fn reset(&mut self) {
        self.spare.extend(self.history.drain(..).map(|mut slot| {
            slot.features.clear();
            slot
        }));
        self.frames_pushed = 0;
        self.next_candidate = 1;
    }

    /// Processes one validated frame and returns the boundary, if any, at the
    /// frame [`ShotNeighborhood::lookahead_frames`] frames back.
    ///
    /// # Errors
    ///
    /// Returns [`ShotDetectionError::OutOfOrderTimestamp`] when `frame` precedes
    /// the last processed frame, and leaves the detector unchanged.
    pub fn push_frame(
        &mut self,
        frame: Rgba8Frame<'_>,
    ) -> Result<Option<ShotBoundary>, ShotDetectionError> {
        if let Some(previous) = self.history.back()
            && frame.timestamp_seconds() < previous.timestamp_seconds
        {
            return Err(ShotDetectionError::OutOfOrderTimestamp {
                previous_timestamp_seconds: previous.timestamp_seconds,
                current_timestamp_seconds: frame.timestamp_seconds(),
            });
        }

        let neighborhood = self.config.neighborhood();
        let reused = if self.history.len() == neighborhood.retained_frames() {
            self.history.pop_front()
        } else {
            self.spare.pop()
        };
        let mut slot = reused.unwrap_or_else(|| FrameSlot::new(self.config));
        extract_features(frame, self.config, &mut slot.features);
        slot.width = frame.width();
        slot.height = frame.height();
        slot.timestamp_seconds = frame.timestamp_seconds();
        slot.change = self
            .history
            .back()
            .map(|previous| previous.change_to(&slot, self.config.score_weights()));
        if self.frames_pushed == 0 {
            self.shot_start_seconds = frame.timestamp_seconds();
        }
        self.history.push_back(slot);
        self.frames_pushed += 1;

        let newest = self.frames_pushed - 1;
        if newest >= self.next_candidate + neighborhood.lookahead_frames() {
            return Ok(self.resolve_next(newest));
        }
        Ok(None)
    }

    /// Resolves the frames still waiting for lookahead at the end of a sequence,
    /// with the context that exists. Frames pushed afterwards continue the same
    /// sequence; [`Self::reset`] starts a new one.
    pub fn finish(&mut self) -> impl Iterator<Item = ShotBoundary> + '_ {
        std::iter::from_fn(move || {
            let newest = self.frames_pushed.checked_sub(1)?;
            while self.next_candidate <= newest {
                if let Some(boundary) = self.resolve_next(newest) {
                    return Some(boundary);
                }
            }
            None
        })
    }

    /// The retained frame at sequence index `frame`.
    fn slot(&self, frame: usize) -> &FrameSlot {
        &self.history[frame + self.history.len() - self.frames_pushed]
    }

    /// Resolves the oldest unresolved candidate with the frames through `newest`.
    fn resolve_next(&mut self, newest: usize) -> Option<ShotBoundary> {
        let index = self.next_candidate;
        self.next_candidate += 1;
        let candidate = self.slot(index);
        let (score, kind) = candidate.change?;
        let timestamp_seconds = candidate.timestamp_seconds;
        let previous_frame_timestamp_seconds = self.slot(index - 1).timestamp_seconds;

        let duration_elapsed = timestamp_seconds - self.shot_start_seconds
            >= self.config.minimum_shot_duration_seconds();
        let is_cut = duration_elapsed
            && match kind {
                ShotBoundaryKind::DimensionChange => true,
                ShotBoundaryKind::VisualChange => self.stands_out(index, newest, score),
            };
        if !is_cut {
            return None;
        }
        self.shot_start_seconds = timestamp_seconds;
        Some(ShotBoundary {
            timestamp_seconds,
            score,
            previous_frame_timestamp_seconds,
            current_frame_timestamp_seconds: timestamp_seconds,
            kind,
        })
    }

    /// Whether the visual change scoring `score` at frame `index` clears the
    /// threshold and the neighborhood test, given the frames through `newest`.
    fn stands_out(&self, index: usize, newest: usize, score: f32) -> bool {
        if score < self.config.hard_cut_threshold() {
            return false;
        }
        let rule = self.config.neighborhood();
        let radius = rule.radius_frames();
        let mut before = [0.0_f32; MAX_SHOT_CONTEXT_FRAMES];
        let mut after = [0.0_f32; MAX_SHOT_CONTEXT_FRAMES];
        let before_count = self.collect_scores(index.saturating_sub(radius)..index, &mut before);
        let after_count = self.collect_scores(index + 1..=(index + radius).min(newest), &mut after);
        if !rule.stands_out(
            score,
            &mut before[..before_count],
            &mut after[..after_count],
        ) {
            return false;
        }
        let span = rule.persistence_frames();
        if span == 0 {
            return true;
        }
        let weights = self.config.score_weights();
        let persists = |from: usize, to: usize| {
            let (lasting, _) = self.slot(from).change_to(self.slot(to), weights);
            rule.persists(score, lasting)
        };
        persists(index - 1, (index + span).min(newest))
            && persists(index.saturating_sub(span + 1), index)
    }

    /// Writes the change score of each frame in `frames` that has one into
    /// `scores`, in order, and returns how many it wrote.
    fn collect_scores(&self, frames: impl Iterator<Item = usize>, scores: &mut [f32]) -> usize {
        let mut count = 0;
        for frame in frames {
            if let Some((score, _)) = self.slot(frame).change {
                scores[count] = score;
                count += 1;
            }
        }
        count
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
        boundaries.extend(detector.push_frame(*frame.borrow())?);
    }
    boundaries.extend(detector.finish());
    Ok(boundaries)
}
