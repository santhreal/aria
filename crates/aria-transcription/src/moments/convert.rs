//! Time span conversion and confidence calculations for transcript moments.

use aria_analysis::TimeSpan;
use aria_analysis::moments::{MomentBatchError, MomentConfidence, MomentConfidenceError};

use super::TRANSCRIPT_SEGMENT_FALLBACK_CONFIDENCE;
use super::errors::{
    TranscriptMomentConversionError, TranscriptMomentErrorField, TranscriptMomentErrorReason,
    TranscriptMomentRecordIndex,
};
use crate::TranscriptSegment;

// Below this inclusive limit, conversion to seconds retains enough f64
// resolution to distinguish every adjacent centisecond.
pub(super) const MAX_EXACT_F64_CENTISECONDS: u64 = 1_u64 << 52;

pub(super) fn segment_confidence(
    segment: &TranscriptSegment,
) -> Result<MomentConfidence, MomentConfidenceError> {
    match segment.mean_word_probability() {
        Some(mean) => MomentConfidence::new(mean),
        None => Ok(TRANSCRIPT_SEGMENT_FALLBACK_CONFIDENCE),
    }
}

pub(super) fn convert_span(
    record_index: TranscriptMomentRecordIndex,
    start_centiseconds: u64,
    end_centiseconds: u64,
) -> Result<TimeSpan, TranscriptMomentConversionError> {
    let start_seconds = convert_boundary(
        record_index,
        TranscriptMomentErrorField::StartCentiseconds,
        start_centiseconds,
    )?;
    let end_seconds = convert_boundary(
        record_index,
        TranscriptMomentErrorField::EndCentiseconds,
        end_centiseconds,
    )?;

    TimeSpan::new(start_seconds, end_seconds).map_err(|_| {
        TranscriptMomentConversionError::record(
            record_index,
            TranscriptMomentErrorField::Span,
            TranscriptMomentErrorReason::InvalidSpan,
        )
    })
}

fn convert_boundary(
    record_index: TranscriptMomentRecordIndex,
    field: TranscriptMomentErrorField,
    centiseconds: u64,
) -> Result<f64, TranscriptMomentConversionError> {
    if centiseconds > MAX_EXACT_F64_CENTISECONDS {
        return Err(TranscriptMomentConversionError::record(
            record_index,
            field,
            TranscriptMomentErrorReason::CentisecondsNotRepresentable,
        ));
    }

    let seconds = centiseconds as f64 / 100.0;
    let recovered_centiseconds = (seconds * 100.0).round();
    if !seconds.is_finite() || recovered_centiseconds != centiseconds as f64 {
        return Err(TranscriptMomentConversionError::record(
            record_index,
            field,
            TranscriptMomentErrorReason::CentisecondsNotRepresentable,
        ));
    }

    Ok(seconds)
}

pub(super) fn convert_batch_error(
    error: MomentBatchError,
    record_indices: &[TranscriptMomentRecordIndex],
) -> TranscriptMomentConversionError {
    let (record_position, reason) = match error {
        MomentBatchError::ContentKeyMismatch { record_index } => (
            record_index,
            TranscriptMomentErrorReason::BatchContentKeyMismatch,
        ),
        MomentBatchError::AnalyzerIdentityMismatch { record_index } => (
            record_index,
            TranscriptMomentErrorReason::BatchAnalyzerIdentityMismatch,
        ),
        MomentBatchError::AnalyzerVersionMismatch { record_index } => (
            record_index,
            TranscriptMomentErrorReason::BatchAnalyzerVersionMismatch,
        ),
        MomentBatchError::DuplicateRecord {
            duplicate_index, ..
        } => (
            duplicate_index,
            TranscriptMomentErrorReason::BatchDuplicateRecord,
        ),
    };

    TranscriptMomentConversionError::batch(record_indices.get(record_position).copied(), reason)
}
