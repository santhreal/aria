//! `moments_sqlite` tests covering schema and persistence. A `#[path]` child of
//! the `moments_sqlite` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn fresh_cache_root_creates_schema_and_version_one() {
    let directory = TempDir::new().expect("temporary directory");
    let cache_root = directory.path().join("new").join("analysis-cache");
    let expected_path = cache_root.join(MOMENTS_INDEX_DATABASE_FILENAME);

    let index = MomentsIndex::open_in_cache_root(&cache_root).expect("fresh index opens");
    assert_eq!(index.path(), expected_path);
    assert_eq!(
        index.config().default_query_limit(),
        DEFAULT_MOMENTS_QUERY_LIMIT
    );
    drop(index);

    let raw = raw_connection(&expected_path);
    let user_version: u32 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user_version is readable");
    assert_eq!(user_version, MOMENTS_INDEX_SCHEMA_VERSION);

    let schemas: BTreeMap<String, String> = {
        let mut statement = raw
            .prepare(
                "
                    SELECT name, sql
                    FROM sqlite_schema
                    WHERE name IN (
                        'moment_batches',
                        'moments',
                        'moments_query_order'
                    )
                    ORDER BY name
                ",
            )
            .expect("schema query prepares");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("schema query runs")
            .collect::<Result<_, _>>()
            .expect("schema rows decode")
    };
    assert_eq!(schemas.len(), 3);
    assert!(schemas["moment_batches"].contains("WITHOUT ROWID"));
    assert!(schemas["moments"].contains("WITHOUT ROWID"));
    assert!(schemas["moments"].contains("payload_present INTEGER NOT NULL"));
    assert!(
        schemas["moments"].contains("length(CAST(payload_text AS BLOB))"),
        "payload bounds must count UTF-8 bytes"
    );

    let journal_mode: String = raw
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("journal mode is readable");
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
}

#[test]
fn concurrent_fresh_opens_converge_on_one_schema() {
    const THREADS: usize = 8;
    const ROUNDS: usize = 4;

    let directory = TempDir::new().expect("temporary directory");
    for round in 0..ROUNDS {
        let path = Arc::new(
            directory
                .path()
                .join(format!("concurrent-open-{round}.sqlite3")),
        );
        let barrier = Arc::new(Barrier::new(THREADS));
        let mut handles = Vec::with_capacity(THREADS);

        for _ in 0..THREADS {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                MomentsIndex::open(path.as_ref())
            }));
        }

        for handle in handles {
            handle
                .join()
                .expect("open worker does not panic")
                .expect("concurrent fresh open converges on the initialized schema");
        }

        let reopened = MomentsIndex::open(path.as_ref()).expect("initialized index reopens");
        assert!(reopened.query(&MomentQuery::new(content_key(0))).is_ok());
    }
}

#[test]
fn persisted_records_and_f32_confidence_survive_reopen() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let key = content_key(1);
    let batch_key = batch_key(key, "aria/reopen", 7);
    let confidence = 0.123_456_79_f32;
    let saved = record(
        key,
        1.25,
        2.5,
        "vision_moment",
        confidence,
        "aria/reopen",
        7,
        Some("persisted"),
    );

    {
        let index = MomentsIndex::open(&path).expect("index opens");
        index
            .replace_batch(&replacement(batch_key.clone(), vec![saved.clone()]))
            .expect("record persists");
    }

    let reopened = MomentsIndex::open(&path).expect("index reopens");
    assert!(reopened.batch_exists(&batch_key).expect("marker query"));
    let records = query_records(&reopened, &MomentQuery::new(key));
    assert_eq!(records, [saved]);
    assert_eq!(
        records[0].confidence().get().to_bits(),
        confidence.to_bits()
    );
}

#[test]
fn empty_replacement_retains_marker_and_delete_removes_it() {
    let directory = TempDir::new().expect("temporary directory");
    let index = MomentsIndex::open(database_path(&directory)).expect("index opens");
    let key = content_key(2);
    let batch_key = batch_key(key, "aria/empty", 1);

    index
        .replace_batch(&replacement(batch_key.clone(), Vec::new()))
        .expect("empty replacement persists");
    assert!(index.batch_exists(&batch_key).expect("marker query"));
    assert!(query_records(&index, &MomentQuery::new(key)).is_empty());

    assert!(index.delete_batch(&batch_key).expect("batch deletes"));
    assert!(!index.batch_exists(&batch_key).expect("marker query"));
    assert!(
        !index
            .delete_batch(&batch_key)
            .expect("second delete is a no-op")
    );
}

#[test]
fn analyzer_and_version_batches_replace_and_delete_in_isolation() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let index = MomentsIndex::open(&path).expect("index opens");
    let key = content_key(3);
    let a1 = batch_key(key, "source/a", 1);
    let a2 = batch_key(key, "source/a", 2);
    let b1 = batch_key(key, "source/b", 1);

    for (batch, label) in [(&a1, "a1-old"), (&a2, "a2"), (&b1, "b1")] {
        index
            .replace_batch(&replacement(
                batch.clone(),
                vec![record(
                    key,
                    0.0,
                    0.0,
                    "beat",
                    0.5,
                    batch.analyzer_identity().as_str(),
                    batch.analyzer_version().get(),
                    Some(label),
                )],
            ))
            .expect("isolated batch persists");
    }

    let a1_new = record(key, 1.0, 1.0, "beat", 0.75, "source/a", 1, Some("a1-new"));
    index
        .replace_batch(&replacement(a1.clone(), vec![a1_new]))
        .expect("one batch replaces");

    let queried = query_records(&index, &MomentQuery::new(key));
    let labels: Vec<_> = queried.iter().map(payload_label).collect();
    assert_eq!(labels, ["a2", "b1", "a1-new"]);
    assert!(index.batch_exists(&a1).expect("a1 marker"));
    assert!(index.batch_exists(&a2).expect("a2 marker"));
    assert!(index.batch_exists(&b1).expect("b1 marker"));

    assert!(index.delete_batch(&a2).expect("a2 deletes"));
    let queried = query_records(&index, &MomentQuery::new(key));
    let labels: Vec<_> = queried.iter().map(payload_label).collect();
    assert_eq!(labels, ["b1", "a1-new"]);
    assert!(index.batch_exists(&a1).expect("a1 remains"));
    assert!(!index.batch_exists(&a2).expect("a2 is gone"));
    assert!(index.batch_exists(&b1).expect("b1 remains"));

    let raw = raw_connection(&path);
    let deleted_children: i64 = raw
        .query_row(
            "
                SELECT COUNT(*)
                FROM moments
                WHERE content_key = ?1
                  AND analyzer_identity = 'source/a'
                  AND analyzer_version = 2
            ",
            params![&key.as_bytes()[..]],
            |row| row.get(0),
        )
        .expect("child count query");
    assert_eq!(deleted_children, 0, "foreign-key cascade removes rows");
}

#[test]
fn unsupported_future_schema_is_rejected_without_deleting_file() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    std::fs::create_dir_all(path.parent().expect("database parent")).expect("parent creates");
    let raw = raw_connection(&path);
    raw.pragma_update(
        None,
        "user_version",
        i64::from(MOMENTS_INDEX_SCHEMA_VERSION) + 1,
    )
    .expect("future version is set");
    drop(raw);

    let error = MomentsIndex::open(&path)
        .err()
        .expect("future schema must be rejected");
    assert!(matches!(
        error,
        MomentsIndexError::UnsupportedSchema {
            found,
            supported: MOMENTS_INDEX_SCHEMA_VERSION,
        } if found == i64::from(MOMENTS_INDEX_SCHEMA_VERSION) + 1
    ));
    assert!(path.exists(), "unsupported database is never auto-deleted");
    let raw = raw_connection(&path);
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("version remains readable");
    assert_eq!(version, i64::from(MOMENTS_INDEX_SCHEMA_VERSION) + 1);
}

#[test]
fn claimed_current_version_without_schema_is_rejected() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    std::fs::create_dir_all(path.parent().expect("database parent")).expect("parent creates");
    let raw = raw_connection(&path);
    raw.pragma_update(
        None,
        "user_version",
        i64::from(MOMENTS_INDEX_SCHEMA_VERSION),
    )
    .expect("claimed version is set");
    drop(raw);

    let error = MomentsIndex::open(&path)
        .err()
        .expect("missing schema must be rejected");
    assert!(matches!(error, MomentsIndexError::SchemaMismatch { .. }));
    assert!(path.exists(), "invalid database is never auto-deleted");
}

#[test]
fn schema_checks_storage_types_ascii_bounds_and_payload_bytes() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let index = MomentsIndex::open(&path).expect("index opens");
    let key = content_key(11);
    let marker = batch_key(key, "source/checks", 1);
    index
        .replace_batch(&replacement(marker, Vec::new()))
        .expect("parent marker persists");

    let raw = raw_connection(&path);
    raw.pragma_update(None, "foreign_keys", "ON")
        .expect("foreign keys enable");

    let invalid_identity = raw.execute(
        "
            INSERT INTO moment_batches (
                content_key,
                analyzer_identity,
                analyzer_version
            ) VALUES (?1, 'Source/Uppercase', 1)
        ",
        params![&content_key(12).as_bytes()[..]],
    );
    assert!(invalid_identity.is_err());

    let invalid_version = raw.execute(
        "
            INSERT INTO moment_batches (
                content_key,
                analyzer_identity,
                analyzer_version
            ) VALUES (?1, 'source/version', 0)
        ",
        params![&content_key(13).as_bytes()[..]],
    );
    assert!(invalid_version.is_err());

    let invalid_time = raw.execute(
        "
            INSERT INTO moments (
                content_key,
                analyzer_identity,
                analyzer_version,
                start_seconds,
                end_seconds,
                kind,
                confidence,
                payload_present,
                payload_text
            ) VALUES (?1, 'source/checks', 1, ?2, ?2, 'beat', 1.0, 0, '')
        ",
        params![&key.as_bytes()[..], f64::INFINITY],
    );
    assert!(invalid_time.is_err());

    let maximum_multibyte_payload = "é".repeat(MAX_MOMENT_PAYLOAD_BYTES / "é".len());
    assert_eq!(maximum_multibyte_payload.len(), MAX_MOMENT_PAYLOAD_BYTES);
    raw.execute(
        "
            INSERT INTO moments (
                content_key,
                analyzer_identity,
                analyzer_version,
                start_seconds,
                end_seconds,
                kind,
                confidence,
                payload_present,
                payload_text
            ) VALUES (?1, 'source/checks', 1, 1.0, 1.0, 'beat', 1.0, 1, ?2)
        ",
        params![&key.as_bytes()[..], maximum_multibyte_payload],
    )
    .expect("payload at the UTF-8 byte bound is accepted");

    let oversized_multibyte_payload = "é".repeat(MAX_MOMENT_PAYLOAD_BYTES / "é".len() + 1);
    let oversized = raw.execute(
        "
            INSERT INTO moments (
                content_key,
                analyzer_identity,
                analyzer_version,
                start_seconds,
                end_seconds,
                kind,
                confidence,
                payload_present,
                payload_text
            ) VALUES (?1, 'source/checks', 1, 2.0, 2.0, 'beat', 1.0, 1, ?2)
        ",
        params![&key.as_bytes()[..], oversized_multibyte_payload],
    );
    assert!(oversized.is_err());
}
