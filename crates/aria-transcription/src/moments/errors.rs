//! Why a transcript could not become a moments batch.
//!
//! Four types: which record failed, which field of it, why, and the error that
//! carries all three. They are separate from the conversion because the
//! conversion is the only thing that builds them and every caller only reads them.

use std::error::Error;
use std::fmt;

/// Stable source ordinal for a transcript moment record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptMomentRecordIndex {
    /// A segment record.
    Segment {
        /// Zero-based index in [`crate::Transcript::segments`].
        segment_index: usize,
    },
    /// A word record.
    Word {
        /// Zero-based index in [`crate::Transcript::segments`].
        segment_index: usize,
        /// Zero-based index in [`crate::TranscriptSegment::words`].
        word_index: usize,
    },
}

impl fmt::Display for TranscriptMomentRecordIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Segment { segment_index } => write!(formatter, "segment[{segment_index}]"),
            Self::Word {
                segment_index,
                word_index,
            } => write!(formatter, "segment[{segment_index}].word[{word_index}]"),
        }
    }
}

/// Stable field category reported by [`TranscriptMomentConversionError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptMomentErrorField {
    /// A record's start timestamp in centiseconds.
    StartCentiseconds,
    /// A record's end timestamp in centiseconds.
    EndCentiseconds,
    /// A converted half-open time span.
    Span,
    /// A segment or word confidence.
    Confidence,
    /// A versioned record payload.
    Payload,
    /// Final atomic replacement-batch validation.
    Batch,
}

impl fmt::Display for TranscriptMomentErrorField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StartCentiseconds => "start_centiseconds",
            Self::EndCentiseconds => "end_centiseconds",
            Self::Span => "span",
            Self::Confidence => "confidence",
            Self::Payload => "payload",
            Self::Batch => "batch",
        })
    }
}

/// Stable failure reason reported by [`TranscriptMomentConversionError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptMomentErrorReason {
    /// A centisecond boundary could not retain centisecond resolution in
    /// seconds-based `f64` storage.
    CentisecondsNotRepresentable,
    /// Converted boundaries did not form a valid moments time span.
    InvalidSpan,
    /// A normalized probability could not form a moments confidence.
    InvalidConfidence,
    /// Canonical JSON would exceed
    /// [`aria_analysis::moments::MAX_MOMENT_PAYLOAD_BYTES`].
    PayloadExceedsLimit,
    /// Canonical JSON unexpectedly failed moments payload validation.
    InvalidPayload,
    /// Final validation found a record with another media content key.
    BatchContentKeyMismatch,
    /// Final validation found a record with another analyzer identity.
    BatchAnalyzerIdentityMismatch,
    /// Final validation found a record with another analyzer version.
    BatchAnalyzerVersionMismatch,
    /// Final validation found two exactly equal records.
    BatchDuplicateRecord,
}

impl fmt::Display for TranscriptMomentErrorReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CentisecondsNotRepresentable => {
                "centisecond boundary is not exactly representable at centisecond resolution"
            }
            Self::InvalidSpan => "converted boundaries do not form a valid time span",
            Self::InvalidConfidence => "probability does not form a valid confidence",
            Self::PayloadExceedsLimit => "canonical payload exceeds the maximum encoded size",
            Self::InvalidPayload => "canonical payload failed domain validation",
            Self::BatchContentKeyMismatch => {
                "record content key does not match the replacement scope"
            }
            Self::BatchAnalyzerIdentityMismatch => {
                "record analyzer identity does not match the replacement scope"
            }
            Self::BatchAnalyzerVersionMismatch => {
                "record analyzer version does not match the replacement scope"
            }
            Self::BatchDuplicateRecord => "record duplicates another record in the batch",
        })
    }
}

/// Bounded, non-text-leaking failure to convert a transcript into moments.
///
/// Record failures carry stable transcript ordinals. Batch failures carry the
/// ordinal of the rejected input record when final validation supplied one.
/// Display output contains only that ordinal, a typed field, and a stable
/// reason; transcript text is never included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscriptMomentConversionError {
    record_index: Option<TranscriptMomentRecordIndex>,
    field: TranscriptMomentErrorField,
    reason: TranscriptMomentErrorReason,
}

impl TranscriptMomentConversionError {
    /// Returns the stable source ordinal, or `None` for a batch-wide failure.
    #[must_use]
    pub const fn record_index(&self) -> Option<TranscriptMomentRecordIndex> {
        self.record_index
    }

    /// Returns the field category that failed conversion or validation.
    #[must_use]
    pub const fn field(&self) -> TranscriptMomentErrorField {
        self.field
    }

    /// Returns the stable, typed failure reason.
    #[must_use]
    pub const fn reason(&self) -> TranscriptMomentErrorReason {
        self.reason
    }

    pub(super) const fn record(
        record_index: TranscriptMomentRecordIndex,
        field: TranscriptMomentErrorField,
        reason: TranscriptMomentErrorReason,
    ) -> Self {
        Self {
            record_index: Some(record_index),
            field,
            reason,
        }
    }

    pub(super) fn batch(
        record_index: Option<TranscriptMomentRecordIndex>,
        reason: TranscriptMomentErrorReason,
    ) -> Self {
        Self {
            record_index,
            field: TranscriptMomentErrorField::Batch,
            reason,
        }
    }
}

impl fmt::Display for TranscriptMomentConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(record_index) = self.record_index {
            write!(
                formatter,
                "transcript moment {record_index} field `{}`: {}",
                self.field, self.reason
            )
        } else {
            write!(
                formatter,
                "transcript moment batch field `{}`: {}",
                self.field, self.reason
            )
        }
    }
}

impl Error for TranscriptMomentConversionError {}
