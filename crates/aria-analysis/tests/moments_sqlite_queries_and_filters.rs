//! `moments_sqlite` tests covering queries and filters. A `#[path]` child of
//! the `moments_sqlite` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn sqlite_time_matching_has_exact_in_memory_boundary_parity() {
    let directory = TempDir::new().expect("temporary directory");
    let index = MomentsIndex::open(database_path(&directory)).expect("index opens");
    let key = content_key(4);
    let records = vec![
        record(
            key,
            0.0,
            1.0,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("span-ending-at-start"),
        ),
        record(
            key,
            0.5,
            1.25,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("span-overlapping-start"),
        ),
        record(
            key,
            1.0,
            1.0,
            "beat",
            1.0,
            "aria/time",
            1,
            Some("point-at-start"),
        ),
        record(
            key,
            1.0,
            1.4,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("span-at-start"),
        ),
        record(
            key,
            1.5,
            1.5,
            "beat",
            1.0,
            "aria/time",
            1,
            Some("point-inside"),
        ),
        record(
            key,
            1.75,
            2.0,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("span-ending-at-end"),
        ),
        record(
            key,
            2.0,
            2.0,
            "beat",
            1.0,
            "aria/time",
            1,
            Some("point-at-end"),
        ),
        record(
            key,
            2.0,
            2.5,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("span-at-end"),
        ),
        record(
            key,
            0.0,
            3.0,
            "silence",
            1.0,
            "aria/time",
            1,
            Some("encloses-empty-query-point"),
        ),
    ];
    replace_grouped(&index, &records);

    let query =
        MomentQuery::new(key).with_time_range(TimeSpan::new(1.0, 2.0).expect("valid test range"));
    assert_eq!(
        query_records(&index, &query),
        expected_records(&records, &query)
    );

    let empty_query =
        MomentQuery::new(key).with_time_range(TimeSpan::new(1.0, 1.0).expect("valid empty range"));
    assert!(
        query_records(&index, &empty_query).is_empty(),
        "an empty query must not match even an enclosing span"
    );
    assert!(expected_records(&records, &empty_query).is_empty());
}

#[test]
fn kind_and_inclusive_confidence_filters_match_in_memory() {
    let directory = TempDir::new().expect("temporary directory");
    let index = MomentsIndex::open(database_path(&directory)).expect("index opens");
    let key = content_key(5);
    let other_key = content_key(6);
    let records = vec![
        record(
            key,
            0.0,
            0.0,
            "beat",
            0.5,
            "aria/filter",
            1,
            Some("inclusive"),
        ),
        record(key, 1.0, 1.0, "beat", 0.49, "aria/filter", 1, Some("below")),
        record(
            key,
            2.0,
            3.0,
            "silence",
            0.9,
            "aria/filter",
            1,
            Some("wrong-kind"),
        ),
        record(
            other_key,
            0.0,
            0.0,
            "beat",
            1.0,
            "aria/filter",
            1,
            Some("wrong-content"),
        ),
    ];
    replace_grouped(&index, &records);

    let query = MomentQuery::new(key)
        .with_kind(MomentKind::BEAT)
        .with_minimum_confidence(MomentConfidence::new(0.5).expect("valid threshold"));
    assert_eq!(
        query_records(&index, &query),
        expected_records(&records, &query)
    );
    assert_eq!(
        payload_label(&query_records(&index, &query)[0]),
        "inclusive"
    );
}

#[test]
fn sqlite_order_uses_every_moment_record_tie_breaker() {
    let directory = TempDir::new().expect("temporary directory");
    let index = MomentsIndex::open(database_path(&directory)).expect("index opens");
    let key = content_key(7);
    let records = vec![
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, Some("b")),
        record(key, 2.0, 3.0, "beat", 0.5, "source/a", 1, Some("late")),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.5,
            "source/b",
            1,
            Some("source-b"),
        ),
        record(
            key,
            0.0,
            1.0,
            "beat",
            0.5,
            "source/a",
            1,
            Some("early-span"),
        ),
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, None),
        record(key, 1.0, 2.0, "beat", 0.5, "source/z", 1, Some("kind-beat")),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.9,
            "source/a",
            2,
            Some("high-confidence"),
        ),
        record(
            key,
            0.0,
            0.0,
            "vision_moment",
            0.5,
            "source/a",
            1,
            Some("early-point"),
        ),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.5,
            "source/a",
            1,
            Some("version-1"),
        ),
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, Some("a")),
    ];
    replace_grouped(&index, &records);
    let query = MomentQuery::new(key);

    let actual = query_records(&index, &query);
    assert_eq!(actual, expected_records(&records, &query));
    let labels: Vec<_> = actual.iter().map(payload_label).collect();
    assert_eq!(
        labels,
        [
            "early-point",
            "early-span",
            "kind-beat",
            "version-1",
            "high-confidence",
            "<none>",
            "a",
            "b",
            "source-b",
            "late",
        ]
    );
}

#[test]
fn truncation_is_exact_for_n_and_n_plus_one_rows() {
    let directory = TempDir::new().expect("temporary directory");
    let config =
        MomentsIndexConfig::new(2, 3, Duration::from_millis(250)).expect("test config is valid");
    let index =
        MomentsIndex::open_with_config(database_path(&directory), config).expect("index opens");
    let two_key = content_key(8);
    let three_key = content_key(9);
    let two = vec![
        record(two_key, 0.0, 0.0, "beat", 1.0, "source/limit", 1, Some("0")),
        record(two_key, 1.0, 1.0, "beat", 1.0, "source/limit", 1, Some("1")),
    ];
    let three = vec![
        record(
            three_key,
            0.0,
            0.0,
            "beat",
            1.0,
            "source/limit",
            1,
            Some("0"),
        ),
        record(
            three_key,
            1.0,
            1.0,
            "beat",
            1.0,
            "source/limit",
            1,
            Some("1"),
        ),
        record(
            three_key,
            2.0,
            2.0,
            "beat",
            1.0,
            "source/limit",
            1,
            Some("2"),
        ),
    ];
    replace_grouped(&index, &two);
    replace_grouped(&index, &three);

    let exact = index
        .query(&MomentQuery::new(two_key))
        .expect("exact-N query succeeds");
    assert_eq!(exact.len(), 2);
    assert!(!exact.truncated());

    let extra = index
        .query(&MomentQuery::new(three_key))
        .expect("N+1 query succeeds");
    assert_eq!(extra.len(), 2);
    assert!(extra.truncated());
    assert_eq!(
        extra
            .records()
            .iter()
            .map(payload_label)
            .collect::<Vec<_>>(),
        ["0", "1"]
    );

    let all = index
        .query_with_limit(&MomentQuery::new(three_key), 3)
        .expect("configured hard maximum is accepted");
    assert_eq!(all.len(), 3);
    assert!(!all.truncated());
}

#[test]
fn invalid_config_and_query_limits_are_rejected() {
    assert!(matches!(
        MomentsIndexConfig::new(0, 1, Duration::ZERO),
        Err(MomentsIndexError::InvalidConfig {
            field: "default_query_limit",
            ..
        })
    ));
    assert!(matches!(
        MomentsIndexConfig::new(2, 1, Duration::ZERO),
        Err(MomentsIndexError::InvalidConfig {
            field: "default_query_limit",
            ..
        })
    ));
    assert!(matches!(
        MomentsIndexConfig::new(1, MAX_MOMENTS_QUERY_LIMIT + 1, Duration::ZERO),
        Err(MomentsIndexError::InvalidConfig {
            field: "maximum_query_limit",
            ..
        })
    ));
    assert!(matches!(
        MomentsIndexConfig::new(1, 1, MAX_MOMENTS_BUSY_TIMEOUT + Duration::from_millis(1)),
        Err(MomentsIndexError::InvalidConfig {
            field: "busy_timeout",
            ..
        })
    ));

    let directory = TempDir::new().expect("temporary directory");
    let config = MomentsIndexConfig::new(1, 2, Duration::ZERO).expect("test config is valid");
    let index =
        MomentsIndex::open_with_config(database_path(&directory), config).expect("index opens");
    let query = MomentQuery::new(content_key(10));
    for requested in [0, 3, usize::MAX] {
        assert!(matches!(
            index.query_with_limit(&query, requested),
            Err(MomentsIndexError::InvalidQueryLimit {
                requested: actual,
                maximum: 2,
            }) if actual == requested
        ));
    }
}
