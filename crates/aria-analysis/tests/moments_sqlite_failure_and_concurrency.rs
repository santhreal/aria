//! `moments_sqlite` tests covering failure and concurrency. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `moments_sqlite` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn malformed_database_is_typed_and_never_replaced() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    std::fs::create_dir_all(path.parent().expect("database parent")).expect("parent creates");
    let original = b"this is not a SQLite database";
    std::fs::write(&path, original).expect("malformed database fixture writes");

    let error = MomentsIndex::open(&path)
        .err()
        .expect("malformed database must be rejected");
    assert!(matches!(error, MomentsIndexError::DatabaseCorrupt { .. }));
    assert_eq!(
        std::fs::read(&path).expect("malformed database remains readable"),
        original,
        "opening corruption must not replace or delete the file"
    );
}

#[test]
fn external_write_contention_maps_to_database_locked() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let config =
        MomentsIndexConfig::new(1, 1, Duration::ZERO).expect("zero-timeout config is valid");
    let index = MomentsIndex::open_with_config(&path, config).expect("index opens");
    let key = content_key(16);
    let batch_key = batch_key(key, "source/locked", 1);
    index
        .replace_batch(&replacement(batch_key.clone(), Vec::new()))
        .expect("prior marker persists");

    let raw = raw_connection(&path);
    raw.execute_batch("BEGIN IMMEDIATE")
        .expect("raw connection acquires write lock");
    let error = index
        .replace_batch(&replacement(batch_key.clone(), Vec::new()))
        .expect_err("contended replacement must fail");
    assert!(matches!(error, MomentsIndexError::DatabaseLocked { .. }));
    raw.execute_batch("ROLLBACK")
        .expect("raw write lock releases");
    assert!(
        index
            .batch_exists(&batch_key)
            .expect("prior marker remains")
    );
}

#[test]
fn corrupt_rows_fail_closed_with_bounded_errors() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let index = MomentsIndex::open(&path).expect("index opens");
    let key = content_key(14);
    let marker = batch_key(key, "source/corrupt", 1);
    index
        .replace_batch(&replacement(marker, Vec::new()))
        .expect("parent marker persists");

    let raw = raw_connection(&path);
    raw.pragma_update(None, "ignore_check_constraints", "ON")
        .expect("test corruption injection enables");
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
            ) VALUES (?1, 'source/corrupt', 1, 'not-a-time', 1.0, 'beat', 1.0, 0, '')
        ",
        params![&key.as_bytes()[..]],
    )
    .expect("type-invalid row is injected");

    let error = index
        .query(&MomentQuery::new(key))
        .expect_err("type-invalid row must fail closed");
    assert!(matches!(
        error,
        MomentsIndexError::CorruptRow {
            row_index: 0,
            field: "start_seconds",
            ..
        }
    ));

    raw.execute("DELETE FROM moments", [])
        .expect("first corrupt row deletes");
    let hostile_payload = format!("secret-prefix-{}", "x".repeat(100_000));
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
            ) VALUES (?1, 'source/corrupt', 1, 1.0, 1.0, 'beat', 1.0, 1, ?2)
        ",
        params![&key.as_bytes()[..], hostile_payload],
    )
    .expect("domain-invalid payload is injected");

    let error = index
        .query(&MomentQuery::new(key))
        .expect_err("oversized payload must fail closed");
    assert!(matches!(
        error,
        MomentsIndexError::CorruptRow {
            row_index: 0,
            field: "payload_text",
            ..
        }
    ));
    let display = error.to_string();
    assert!(display.len() < 256);
    assert!(!display.contains("secret-prefix"));
}

#[test]
fn failed_mid_insert_replacement_rolls_back_prior_complete_batch() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    let index = MomentsIndex::open(&path).expect("index opens");
    let key = content_key(15);
    let batch_key = batch_key(key, "source/rollback", 1);
    let old = record(
        key,
        0.0,
        0.0,
        "beat",
        1.0,
        "source/rollback",
        1,
        Some("old-complete"),
    );
    index
        .replace_batch(&replacement(batch_key.clone(), vec![old.clone()]))
        .expect("prior complete batch persists");

    let raw = raw_connection(&path);
    raw.execute_batch(
        "
            CREATE TRIGGER reject_test_moment
            BEFORE INSERT ON moments
            WHEN NEW.payload_text = 'abort'
            BEGIN
                SELECT RAISE(ABORT, 'injected test failure');
            END;
        ",
    )
    .expect("failure trigger installs");
    drop(raw);

    let replacement_records = vec![
        record(
            key,
            1.0,
            1.0,
            "beat",
            1.0,
            "source/rollback",
            1,
            Some("inserted-before-failure"),
        ),
        record(
            key,
            2.0,
            2.0,
            "beat",
            1.0,
            "source/rollback",
            1,
            Some("abort"),
        ),
    ];
    let error = index
        .replace_batch(&replacement(batch_key.clone(), replacement_records))
        .expect_err("trigger aborts replacement");
    assert!(matches!(
        error,
        MomentsIndexError::ConstraintViolation { .. }
    ));

    assert!(index.batch_exists(&batch_key).expect("old marker remains"));
    assert_eq!(
        query_records(&index, &MomentQuery::new(key)),
        [old],
        "delete and earlier inserts roll back with the failed transaction"
    );
}

#[test]
fn mutex_serializes_concurrent_replacements_and_queries() {
    const THREADS: usize = 6;
    const ITERATIONS: usize = 8;

    let directory = TempDir::new().expect("temporary directory");
    let index =
        Arc::new(MomentsIndex::open(database_path(&directory)).expect("shared index opens"));
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();

    for worker in 0..THREADS {
        let index = Arc::clone(&index);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let marker = u8::try_from(worker + 20).expect("worker marker fits u8");
            let key = content_key(marker);
            let analyzer_identity = format!("source/concurrent-{worker}");
            let batch_key = batch_key(key, &analyzer_identity, 1);
            barrier.wait();

            for iteration in 0..ITERATIONS {
                let start =
                    f64::from(u32::try_from(iteration).expect("test iteration count fits u32"));
                let record = record(
                    key,
                    start,
                    start,
                    "beat",
                    1.0,
                    &analyzer_identity,
                    1,
                    Some("current"),
                );
                index
                    .replace_batch(&replacement(batch_key.clone(), vec![record.clone()]))
                    .expect("concurrent replacement succeeds");
                let result = index
                    .query_with_limit(&MomentQuery::new(key), 1)
                    .expect("concurrent query succeeds");
                assert_eq!(result.records(), &[record]);
                assert!(!result.truncated());
            }
        }));
    }

    for handle in handles {
        handle.join().expect("worker does not panic");
    }
}
