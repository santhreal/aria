//! Conversion from normalized transcripts to analyzer-scoped media moments.

mod convert;
mod payload;

#[cfg(test)]
use convert::MAX_EXACT_F64_CENTISECONDS;
use convert::{convert_batch_error, convert_span, segment_confidence};
use payload::encode_payload;

use aria_analysis::moments::{
    AnalyzerIdentity, AnalyzerVersion, MediaContentKey, MomentBatchKey, MomentConfidence,
    MomentKind, MomentRecord, MomentReplacementBatch,
};

use crate::{Transcript, TranscriptSegment};

/// Schema version embedded in every transcript moment payload.
///
/// Version 1 is canonical compact JSON with fields in this exact order:
///
/// - segment: `{"v":1,"segment_index":N,"text":"..."}`
/// - word: `{"v":1,"segment_index":N,"word_index":M,"text":"..."}`
///
/// Indices are unpadded decimal integers. Quotes, backslashes, and Unicode
/// control characters are escaped; all other Unicode is emitted directly as
/// UTF-8. There is no insignificant whitespace. The segment and word indices
/// preserve source ordinals and therefore distinguish otherwise identical
/// records.
pub const TRANSCRIPT_MOMENT_PAYLOAD_VERSION: u32 = 1;

/// Confidence assigned to a transcript segment that has no word probabilities.
///
/// Zero is intentionally conservative: segment-level confidence is not
/// supplied by the normalized transcript, so the adapter does not imply model
/// certainty when token timestamps or probabilities are unavailable.
pub const TRANSCRIPT_SEGMENT_FALLBACK_CONFIDENCE: MomentConfidence = MomentConfidence::ZERO;

/// Converts one normalized transcript into an atomic moments replacement batch.
///
/// The caller supplies the validated content key, analyzer identity, and
/// analyzer version so model and configuration provenance remains an
/// application decision. The transcript is borrowed.
///
/// One `TRANSCRIPT_SEGMENT` record is emitted for every non-empty segment and
/// one `TRANSCRIPT_WORD` record for every word. Centisecond boundaries are
/// converted directly to seconds after verifying that `f64` retains
/// centisecond resolution. Word confidence is the normalized Whisper
/// probability. Segment confidence is the duration-independent arithmetic mean
/// of its word probabilities, computed in `f64`; a segment without words uses
/// [`TRANSCRIPT_SEGMENT_FALLBACK_CONFIDENCE`].
///
/// Payloads use the canonical representation documented by
/// [`TRANSCRIPT_MOMENT_PAYLOAD_VERSION`]. Text is never truncated. An empty
/// transcript returns a valid empty batch, which is an atomic marker that prior
/// results in the same replacement scope should be removed.
///
/// # Errors
///
/// Returns [`TranscriptMomentConversionError`] if a timestamp cannot retain
/// centisecond resolution, confidence or span domain validation fails, an
/// encoded payload would exceed
/// [`aria_analysis::moments::MAX_MOMENT_PAYLOAD_BYTES`], or final replacement
/// batch validation fails.
pub fn transcript_to_moment_batch(
    content_key: MediaContentKey,
    analyzer_identity: AnalyzerIdentity,
    analyzer_version: AnalyzerVersion,
    transcript: &Transcript,
) -> Result<MomentReplacementBatch, TranscriptMomentConversionError> {
    let mut records = Vec::new();
    let mut record_indices = Vec::new();

    for (segment_index, segment) in transcript.segments().iter().enumerate() {
        if !segment.text().is_empty() {
            let record_index = TranscriptMomentRecordIndex::Segment { segment_index };
            records.push(segment_record(
                content_key,
                &analyzer_identity,
                analyzer_version,
                segment,
                record_index,
            )?);
            record_indices.push(record_index);
        }

        for (word_index, word) in segment.words().iter().enumerate() {
            let record_index = TranscriptMomentRecordIndex::Word {
                segment_index,
                word_index,
            };
            let span = convert_span(
                record_index,
                word.start_centiseconds(),
                word.end_centiseconds(),
            )?;
            let confidence = MomentConfidence::new(word.probability()).map_err(|_| {
                TranscriptMomentConversionError::record(
                    record_index,
                    TranscriptMomentErrorField::Confidence,
                    TranscriptMomentErrorReason::InvalidConfidence,
                )
            })?;
            let payload = encode_payload(record_index, word.text())?;

            records.push(MomentRecord::new(
                content_key,
                span,
                MomentKind::TRANSCRIPT_WORD,
                confidence,
                analyzer_identity.clone(),
                analyzer_version,
                Some(payload),
            ));
            record_indices.push(record_index);
        }
    }

    let key = MomentBatchKey::new(content_key, analyzer_identity, analyzer_version);
    MomentReplacementBatch::new(key, records)
        .map_err(|error| convert_batch_error(error, &record_indices))
}

fn segment_record(
    content_key: MediaContentKey,
    analyzer_identity: &AnalyzerIdentity,
    analyzer_version: AnalyzerVersion,
    segment: &TranscriptSegment,
    record_index: TranscriptMomentRecordIndex,
) -> Result<MomentRecord, TranscriptMomentConversionError> {
    let span = convert_span(
        record_index,
        segment.start_centiseconds(),
        segment.end_centiseconds(),
    )?;
    let confidence = segment_confidence(segment).map_err(|_| {
        TranscriptMomentConversionError::record(
            record_index,
            TranscriptMomentErrorField::Confidence,
            TranscriptMomentErrorReason::InvalidConfidence,
        )
    })?;
    let payload = encode_payload(record_index, segment.text())?;

    Ok(MomentRecord::new(
        content_key,
        span,
        MomentKind::TRANSCRIPT_SEGMENT,
        confidence,
        analyzer_identity.clone(),
        analyzer_version,
        Some(payload),
    ))
}

mod errors;

pub use errors::{
    TranscriptMomentConversionError, TranscriptMomentErrorField, TranscriptMomentErrorReason,
    TranscriptMomentRecordIndex,
};

#[cfg(test)]
mod tests;
