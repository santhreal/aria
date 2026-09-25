//! `moments` tests covering replacement batches. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `moments` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn replacement_batch_rejects_each_scope_mismatch() {
    let key = content_key(4);
    let source = analyzer("aria/shots");
    let source_version = version(2);
    let batch_key = MomentBatchKey::new(key, source.clone(), source_version);
    let valid = record(
        key,
        0.0,
        0.0,
        "shot_boundary",
        1.0,
        source.as_str(),
        source_version.get(),
        None,
    );

    let wrong_content = record(
        content_key(5),
        1.0,
        1.0,
        "shot_boundary",
        1.0,
        source.as_str(),
        source_version.get(),
        None,
    );
    assert_eq!(
        MomentReplacementBatch::new(batch_key.clone(), vec![valid.clone(), wrong_content]),
        Err(MomentBatchError::ContentKeyMismatch { record_index: 1 })
    );

    let wrong_analyzer = record(
        key,
        1.0,
        1.0,
        "shot_boundary",
        1.0,
        "other/shots",
        source_version.get(),
        None,
    );
    assert_eq!(
        MomentReplacementBatch::new(batch_key.clone(), vec![valid.clone(), wrong_analyzer]),
        Err(MomentBatchError::AnalyzerIdentityMismatch { record_index: 1 })
    );

    let wrong_version = record(
        key,
        1.0,
        1.0,
        "shot_boundary",
        1.0,
        source.as_str(),
        3,
        None,
    );
    assert_eq!(
        MomentReplacementBatch::new(batch_key, vec![valid, wrong_version]),
        Err(MomentBatchError::AnalyzerVersionMismatch { record_index: 1 })
    );
}

#[test]
fn replacement_batch_rejects_exact_duplicates_and_canonicalizes_records() {
    let key = content_key(6);
    let batch_key = MomentBatchKey::new(key, analyzer("aria/vision"), version(1));
    let first = record(
        key,
        2.0,
        3.0,
        "vision_moment",
        0.8,
        "aria/vision",
        1,
        Some("second"),
    );
    let second = record(
        key,
        1.0,
        2.0,
        "vision_moment",
        0.8,
        "aria/vision",
        1,
        Some("first"),
    );

    assert_eq!(
        MomentReplacementBatch::new(
            batch_key.clone(),
            vec![first.clone(), second.clone(), first.clone()],
        ),
        Err(MomentBatchError::DuplicateRecord {
            first_index: 0,
            duplicate_index: 2,
        })
    );

    let same_time_different_payload = record(
        key,
        2.0,
        3.0,
        "vision_moment",
        0.8,
        "aria/vision",
        1,
        Some("third"),
    );
    let batch = MomentReplacementBatch::new(
        batch_key.clone(),
        vec![first, same_time_different_payload, second],
    )
    .expect("distinct matching records form a batch");
    assert_eq!(batch.key(), &batch_key);
    assert_eq!(batch.len(), 3);
    let labels: Vec<_> = batch.records().iter().map(payload_label).collect();
    assert_eq!(labels, ["first", "second", "third"]);

    let (returned_key, returned_records) = batch.into_parts();
    assert_eq!(returned_key, batch_key);
    assert_eq!(returned_records.len(), 3);
}

#[test]
fn empty_queries_and_empty_replacement_batches_are_valid_and_deterministic() {
    let key = content_key(7);
    let query = MomentQuery::new(key);
    assert!(filter_and_sort_moments(&[], &query).is_empty());
    assert!(filter_and_sort_moments(&[], &query).is_empty());

    let batch_key = MomentBatchKey::new(key, analyzer("aria/empty"), version(1));
    let batch = MomentReplacementBatch::new(batch_key.clone(), Vec::new())
        .expect("empty complete result sets are valid");
    assert!(batch.is_empty());
    assert_eq!(batch.len(), 0);
    assert_eq!(batch.key(), &batch_key);
    assert!(batch.records().is_empty());
}
