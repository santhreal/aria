use std::str::FromStr;

use aria_analysis::TimeSpan;
use aria_analysis::moments::{
    AnalyzerIdentity, AnalyzerIdentityError, AnalyzerVersion, AnalyzerVersionError,
    MAX_ANALYZER_IDENTITY_BYTES, MAX_MOMENT_KIND_BYTES, MAX_MOMENT_PAYLOAD_BYTES,
    MEDIA_CONTENT_KEY_BYTES, MEDIA_CONTENT_KEY_HEX_BYTES, MediaContentKey, MediaContentKeyError,
    MomentBatchError, MomentBatchKey, MomentConfidence, MomentConfidenceError, MomentKind,
    MomentKindError, MomentPayload, MomentPayloadError, MomentQuery, MomentRecord,
    MomentReplacementBatch, filter_and_sort_moments,
};

fn content_key(marker: u8) -> MediaContentKey {
    MediaContentKey::from_bytes([marker; MEDIA_CONTENT_KEY_BYTES])
}

fn analyzer(identity: &str) -> AnalyzerIdentity {
    AnalyzerIdentity::new(identity).expect("test analyzer identity is valid")
}

fn version(value: u32) -> AnalyzerVersion {
    AnalyzerVersion::new(value).expect("test analyzer version is valid")
}

#[allow(clippy::too_many_arguments)]
fn record(
    key: MediaContentKey,
    start: f64,
    end: f64,
    kind: &str,
    confidence: f32,
    analyzer_identity: &str,
    analyzer_version: u32,
    payload: Option<&str>,
) -> MomentRecord {
    MomentRecord::new(
        key,
        TimeSpan::new(start, end).expect("test time span is valid"),
        MomentKind::new(kind).expect("test moment kind is valid"),
        MomentConfidence::new(confidence).expect("test confidence is valid"),
        analyzer(analyzer_identity),
        version(analyzer_version),
        payload.map(|text| MomentPayload::new(text).expect("test payload is valid")),
    )
}

fn payload_label(record: &MomentRecord) -> &str {
    record.payload().map_or("<none>", MomentPayload::as_str)
}

#[path = "moments_values_and_identities.rs"]
mod values_and_identities;

#[path = "moments_query_and_order.rs"]
mod query_and_order;

#[path = "moments_replacement_batches.rs"]
mod replacement_batches;
