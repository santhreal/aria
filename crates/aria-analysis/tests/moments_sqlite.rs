#![cfg(feature = "sqlite")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use aria_analysis::TimeSpan;
use aria_analysis::moments::{
    AnalyzerIdentity, AnalyzerVersion, DEFAULT_MOMENTS_QUERY_LIMIT, MAX_MOMENT_PAYLOAD_BYTES,
    MAX_MOMENTS_BUSY_TIMEOUT, MAX_MOMENTS_QUERY_LIMIT, MEDIA_CONTENT_KEY_BYTES,
    MOMENTS_INDEX_DATABASE_FILENAME, MOMENTS_INDEX_SCHEMA_VERSION, MediaContentKey, MomentBatchKey,
    MomentConfidence, MomentKind, MomentPayload, MomentQuery, MomentRecord, MomentReplacementBatch,
    MomentsIndex, MomentsIndexConfig, MomentsIndexError, filter_and_sort_moments,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn content_key(marker: u8) -> MediaContentKey {
    MediaContentKey::from_bytes([marker; MEDIA_CONTENT_KEY_BYTES])
}

fn analyzer(identity: &str) -> AnalyzerIdentity {
    AnalyzerIdentity::new(identity).expect("test analyzer identity is valid")
}

fn version(value: u32) -> AnalyzerVersion {
    AnalyzerVersion::new(value).expect("test analyzer version is valid")
}

fn batch_key(
    key: MediaContentKey,
    analyzer_identity: &str,
    analyzer_version: u32,
) -> MomentBatchKey {
    MomentBatchKey::new(key, analyzer(analyzer_identity), version(analyzer_version))
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

fn replacement(key: MomentBatchKey, records: Vec<MomentRecord>) -> MomentReplacementBatch {
    MomentReplacementBatch::new(key, records).expect("test replacement batch is valid")
}

fn replace_grouped(index: &MomentsIndex, records: &[MomentRecord]) {
    let mut groups: BTreeMap<MomentBatchKey, Vec<MomentRecord>> = BTreeMap::new();
    for record in records {
        let key = MomentBatchKey::new(
            record.content_key(),
            record.analyzer_identity().clone(),
            record.analyzer_version(),
        );
        groups.entry(key).or_default().push(record.clone());
    }
    for (key, records) in groups {
        index
            .replace_batch(&replacement(key, records))
            .expect("grouped test batch persists");
    }
}

fn query_records(index: &MomentsIndex, query: &MomentQuery) -> Vec<MomentRecord> {
    index
        .query_with_limit(query, MAX_MOMENTS_QUERY_LIMIT)
        .expect("test query succeeds")
        .into_records()
}

fn expected_records(records: &[MomentRecord], query: &MomentQuery) -> Vec<MomentRecord> {
    filter_and_sort_moments(records, query)
        .into_iter()
        .cloned()
        .collect()
}

fn payload_label(record: &MomentRecord) -> &str {
    record.payload().map_or("<none>", MomentPayload::as_str)
}

fn database_path(directory: &TempDir) -> PathBuf {
    directory.path().join("nested").join("moments.sqlite3")
}

fn raw_connection(path: &Path) -> Connection {
    Connection::open(path).expect("raw test SQLite connection opens")
}

#[path = "moments_sqlite_schema_and_persistence.rs"]
mod schema_and_persistence;

#[path = "moments_sqlite_queries_and_filters.rs"]
mod queries_and_filters;

#[path = "moments_sqlite_failure_and_concurrency.rs"]
mod failure_and_concurrency;
