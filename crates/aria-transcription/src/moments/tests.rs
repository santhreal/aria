//! Transcript-to-moments conversion cases.

use aria_analysis::moments::{
    AnalyzerIdentity, AnalyzerVersion, MAX_MOMENT_PAYLOAD_BYTES, MediaContentKey, MomentConfidence,
    MomentKind,
};
use serde_json::Value;

use super::*;
use crate::{TranscriptSegment, TranscriptWord};

fn content_key() -> MediaContentKey {
    MediaContentKey::from_bytes([0x5a; 32])
}

fn analyzer_identity() -> AnalyzerIdentity {
    AnalyzerIdentity::new("whisper/base.en").expect("fixture identity is valid")
}

fn analyzer_version() -> AnalyzerVersion {
    AnalyzerVersion::new(9).expect("fixture version is valid")
}

fn convert(
    transcript: &Transcript,
) -> Result<MomentReplacementBatch, TranscriptMomentConversionError> {
    transcript_to_moment_batch(
        content_key(),
        analyzer_identity(),
        analyzer_version(),
        transcript,
    )
}

#[test]
fn mixed_transcript_emits_sorted_records_with_caller_provenance() {
    let transcript = Transcript::test_fixture(vec![
        TranscriptSegment::test_fixture(
            100,
            200,
            "Alpha beta",
            vec![
                TranscriptWord::test_fixture(100, 150, "Alpha", 0.75),
                TranscriptWord::test_fixture(150, 200, "beta", 0.25),
            ],
        ),
        TranscriptSegment::test_fixture(200, 250, "", Vec::new()),
        TranscriptSegment::test_fixture(
            300,
            350,
            "Gamma",
            vec![TranscriptWord::test_fixture(300, 350, "Gamma", 0.875)],
        ),
    ]);

    let batch = convert(&transcript).expect("fixture transcript converts");
    let expected = [
        (
            "transcript_word",
            1.0,
            1.5,
            0.75,
            r#"{"v":1,"segment_index":0,"word_index":0,"text":"Alpha"}"#,
        ),
        (
            "transcript_segment",
            1.0,
            2.0,
            0.5,
            r#"{"v":1,"segment_index":0,"text":"Alpha beta"}"#,
        ),
        (
            "transcript_word",
            1.5,
            2.0,
            0.25,
            r#"{"v":1,"segment_index":0,"word_index":1,"text":"beta"}"#,
        ),
        (
            "transcript_segment",
            3.0,
            3.5,
            0.875,
            r#"{"v":1,"segment_index":2,"text":"Gamma"}"#,
        ),
        (
            "transcript_word",
            3.0,
            3.5,
            0.875,
            r#"{"v":1,"segment_index":2,"word_index":0,"text":"Gamma"}"#,
        ),
    ];

    assert_eq!(batch.key().content_key(), content_key());
    assert_eq!(batch.key().analyzer_identity(), &analyzer_identity());
    assert_eq!(batch.key().analyzer_version(), analyzer_version());
    assert_eq!(batch.records().len(), expected.len());

    for (record, (kind, start, end, confidence, payload)) in batch.records().iter().zip(expected) {
        assert_eq!(record.kind().as_str(), kind);
        assert_eq!(record.span().start_seconds(), start);
        assert_eq!(record.span().end_seconds(), end);
        assert_eq!(record.confidence().get(), confidence);
        assert_eq!(record.content_key(), content_key());
        assert_eq!(record.analyzer_identity(), &analyzer_identity());
        assert_eq!(record.analyzer_version(), analyzer_version());
        assert_eq!(
            record
                .payload()
                .expect("transcript records have payloads")
                .as_str(),
            payload
        );
    }
}

#[test]
fn repeated_identical_words_remain_distinct_by_ordinal() {
    let transcript = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        0,
        100,
        "echo echo",
        vec![
            TranscriptWord::test_fixture(10, 20, "echo", 0.5),
            TranscriptWord::test_fixture(10, 20, "echo", 0.5),
        ],
    )]);

    let batch = convert(&transcript).expect("ordinals prevent duplicate records");
    let word_payloads: Vec<_> = batch
        .records()
        .iter()
        .filter(|record| record.kind() == &MomentKind::TRANSCRIPT_WORD)
        .map(|record| {
            record
                .payload()
                .expect("word has payload")
                .as_str()
                .to_owned()
        })
        .collect();

    assert_eq!(word_payloads.len(), 2);
    assert_ne!(word_payloads[0], word_payloads[1]);
    assert!(word_payloads[0].contains(r#""word_index":0"#));
    assert!(word_payloads[1].contains(r#""word_index":1"#));
}

#[test]
fn arbitrary_text_round_trips_through_canonical_json_payloads() {
    let text = "quote \" backslash \\ tab\tcr\rlf\nUnicode: 雪 🦀";
    let transcript = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        10,
        20,
        text,
        vec![TranscriptWord::test_fixture(10, 20, text, 0.5)],
    )]);

    let batch = convert(&transcript).expect("escaped text converts");
    assert_eq!(batch.records().len(), 2);
    for record in batch.records() {
        let encoded = record.payload().expect("record has payload").as_str();
        let decoded: Value = serde_json::from_str(encoded).expect("payload is valid JSON");
        assert_eq!(decoded.get("v").and_then(Value::as_u64), Some(1));
        assert_eq!(decoded.get("text").and_then(Value::as_str), Some(text));
        assert!(!encoded.contains('\t'));
        assert!(!encoded.contains('\r'));
        assert!(!encoded.contains('\n'));
    }
}

#[test]
fn empty_transcript_yields_valid_empty_replacement_marker() {
    let transcript = Transcript::test_fixture(Vec::new());
    let batch = convert(&transcript).expect("empty transcript is valid");

    assert!(batch.is_empty());
    assert_eq!(batch.key().content_key(), content_key());
    assert_eq!(batch.key().analyzer_identity(), &analyzer_identity());
    assert_eq!(batch.key().analyzer_version(), analyzer_version());
}

#[test]
fn segment_without_words_uses_conservative_fallback_confidence() {
    let transcript = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        25,
        75,
        "No token timestamps",
        Vec::new(),
    )]);

    let batch = convert(&transcript).expect("no-word segment converts");
    assert_eq!(batch.records().len(), 1);
    assert_eq!(
        batch.records()[0].confidence(),
        TRANSCRIPT_SEGMENT_FALLBACK_CONFIDENCE
    );
    assert_eq!(batch.records()[0].confidence(), MomentConfidence::ZERO);
}

#[test]
fn oversized_payload_fails_with_bounded_non_leaking_error() {
    let secret = "private transcript marker";
    let text = format!("{secret}{}", "x".repeat(MAX_MOMENT_PAYLOAD_BYTES));
    let transcript = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        0,
        1,
        text,
        Vec::new(),
    )]);

    let error = convert(&transcript).expect_err("oversized payload must fail closed");
    assert_eq!(
        error.record_index(),
        Some(TranscriptMomentRecordIndex::Segment { segment_index: 0 })
    );
    assert_eq!(error.field(), TranscriptMomentErrorField::Payload);
    assert_eq!(
        error.reason(),
        TranscriptMomentErrorReason::PayloadExceedsLimit
    );
    let rendered = error.to_string();
    assert!(!rendered.contains(secret));
    assert!(rendered.len() < 200);
}

#[test]
fn output_is_repeatable_and_independent_of_string_capacity() {
    let compact_text = String::from("same allocation-independent text");
    let mut spare_capacity_text = String::with_capacity(4_096);
    spare_capacity_text.push_str(&compact_text);

    let compact = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        40,
        80,
        compact_text,
        vec![TranscriptWord::test_fixture(40, 80, "same", 0.625)],
    )]);
    let spare_capacity = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        40,
        80,
        spare_capacity_text,
        vec![TranscriptWord::test_fixture(40, 80, "same", 0.625)],
    )]);

    let first = convert(&compact).expect("first conversion succeeds");
    let repeated = convert(&compact).expect("repeated conversion succeeds");
    let reallocated = convert(&spare_capacity).expect("equivalent allocation converts");
    assert_eq!(first, repeated);
    assert_eq!(first, reallocated);
}

#[test]
fn unrepresentable_centiseconds_fail_closed() {
    let transcript = Transcript::test_fixture(vec![TranscriptSegment::test_fixture(
        MAX_EXACT_F64_CENTISECONDS + 1,
        MAX_EXACT_F64_CENTISECONDS + 1,
        "far future",
        Vec::new(),
    )]);

    let error = convert(&transcript).expect_err("precision-losing boundary must fail");
    assert_eq!(
        error.record_index(),
        Some(TranscriptMomentRecordIndex::Segment { segment_index: 0 })
    );
    assert_eq!(error.field(), TranscriptMomentErrorField::StartCentiseconds);
    assert_eq!(
        error.reason(),
        TranscriptMomentErrorReason::CentisecondsNotRepresentable
    );
}
