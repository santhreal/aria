//! The database this adapter will accept, and nothing else.
//!
//! One module owns the statements, the per-connection limits, the durability
//! pragmas, and the `CREATE` text every table and index is compared against on
//! open. The comparison is why the `CREATE` text lives beside the statements
//! that read it: a schema change that misses one of the two is a database this
//! build opens and then writes wrongly, and it fails to open instead.

use super::rows::is_locking_error;
use super::*;

const SQLITE_LIMIT_LENGTH_BYTES: i32 = 1_048_576;
const SQLITE_LIMIT_SQL_BYTES: i32 = 65_536;
const SQLITE_LIMIT_COLUMNS: i32 = 64;
const SQLITE_LIMIT_EXPRESSION_DEPTH: i32 = 100;
const SQLITE_LIMIT_COMPOUND_SELECTS: i32 = 8;
const SQLITE_LIMIT_VM_OPERATIONS: i32 = 100_000;
const SQLITE_LIMIT_FUNCTION_ARGUMENTS: i32 = 32;
const SQLITE_LIMIT_ATTACHED_DATABASES: i32 = 0;
const SQLITE_LIMIT_LIKE_PATTERN_BYTES: i32 = 1_024;
const SQLITE_LIMIT_VARIABLES: i32 = 32;
const SQLITE_LIMIT_TRIGGER_DEPTH: i32 = 8;
const SQLITE_LIMIT_WORKER_THREADS: i32 = 2;
pub(super) const UPSERT_BATCH_SQL: &str = "
    INSERT INTO moment_batches (
        content_key,
        analyzer_identity,
        analyzer_version
    ) VALUES (?1, ?2, ?3)
    ON CONFLICT (content_key, analyzer_identity, analyzer_version) DO NOTHING
";

pub(super) const DELETE_MOMENTS_SQL: &str = "
    DELETE FROM moments
    WHERE content_key = ?1
      AND analyzer_identity = ?2
      AND analyzer_version = ?3
";

pub(super) const INSERT_MOMENT_SQL: &str = "
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
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
";

pub(super) const QUERY_MOMENTS_SQL: &str = "
    SELECT
        content_key,
        start_seconds,
        end_seconds,
        kind,
        confidence,
        analyzer_identity,
        analyzer_version,
        payload_present,
        payload_text
    FROM moments
    WHERE content_key = ?1
      AND (?2 IS NULL OR kind = ?2)
      AND confidence >= ?3
      AND (
          ?4 = 0
          OR (
              ?5 < ?6
              AND (
                  (
                      start_seconds = end_seconds
                      AND ?5 <= start_seconds
                      AND start_seconds < ?6
                  )
                  OR (
                      start_seconds < end_seconds
                      AND start_seconds < ?6
                      AND end_seconds > ?5
                  )
              )
          )
      )
    ORDER BY
        start_seconds ASC,
        end_seconds ASC,
        kind ASC,
        analyzer_identity ASC,
        analyzer_version ASC,
        confidence DESC,
        payload_present ASC,
        payload_text ASC
    LIMIT ?7
";
pub(super) fn initialize_connection(
    connection: &mut Connection,
    config: MomentsIndexConfig,
) -> Result<(), MomentsIndexError> {
    connection
        .busy_timeout(config.busy_timeout)
        .map_err(map_sqlite_error)?;
    configure_sqlite_limits(connection)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite_error)?;
    let foreign_keys_enabled: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(map_sqlite_error)?;
    if foreign_keys_enabled != 1 {
        return Err(MomentsIndexError::SchemaMismatch {
            reason: "SQLite foreign keys could not be enabled",
        });
    }

    let schema_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(map_sqlite_error)?;
    if schema_version < 0 {
        return Err(MomentsIndexError::SchemaMismatch {
            reason: "schema version must not be negative",
        });
    }
    if schema_version > i64::from(MOMENTS_INDEX_SCHEMA_VERSION) {
        return Err(MomentsIndexError::UnsupportedSchema {
            found: schema_version,
            supported: MOMENTS_INDEX_SCHEMA_VERSION,
        });
    }
    configure_durability(connection)?;

    if schema_version == 0 {
        initialize_schema(connection)?;
    }
    validate_schema(connection)
}

fn configure_sqlite_limits(connection: &Connection) -> Result<(), MomentsIndexError> {
    let limits = [
        (Limit::SQLITE_LIMIT_LENGTH, SQLITE_LIMIT_LENGTH_BYTES),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, SQLITE_LIMIT_SQL_BYTES),
        (Limit::SQLITE_LIMIT_COLUMN, SQLITE_LIMIT_COLUMNS),
        (
            Limit::SQLITE_LIMIT_EXPR_DEPTH,
            SQLITE_LIMIT_EXPRESSION_DEPTH,
        ),
        (
            Limit::SQLITE_LIMIT_COMPOUND_SELECT,
            SQLITE_LIMIT_COMPOUND_SELECTS,
        ),
        (Limit::SQLITE_LIMIT_VDBE_OP, SQLITE_LIMIT_VM_OPERATIONS),
        (
            Limit::SQLITE_LIMIT_FUNCTION_ARG,
            SQLITE_LIMIT_FUNCTION_ARGUMENTS,
        ),
        (
            Limit::SQLITE_LIMIT_ATTACHED,
            SQLITE_LIMIT_ATTACHED_DATABASES,
        ),
        (
            Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
            SQLITE_LIMIT_LIKE_PATTERN_BYTES,
        ),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, SQLITE_LIMIT_VARIABLES),
        (
            Limit::SQLITE_LIMIT_TRIGGER_DEPTH,
            SQLITE_LIMIT_TRIGGER_DEPTH,
        ),
        (
            Limit::SQLITE_LIMIT_WORKER_THREADS,
            SQLITE_LIMIT_WORKER_THREADS,
        ),
    ];

    for (limit, maximum) in limits {
        connection
            .set_limit(limit, maximum)
            .map_err(map_sqlite_error)?;
        let effective = connection.limit(limit).map_err(map_sqlite_error)?;
        if effective > maximum {
            return Err(MomentsIndexError::SchemaMismatch {
                reason: "SQLite runtime limit could not be bounded",
            });
        }
    }
    Ok(())
}

fn configure_durability(connection: &Connection) -> Result<(), MomentsIndexError> {
    let wal_result = connection.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    });
    if let Err(source) = wal_result
        && !is_locking_error(&source)
    {
        return Err(map_sqlite_error(source));
    }

    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(map_sqlite_error)
}

fn require_empty_unversioned_database(connection: &Connection) -> Result<(), MomentsIndexError> {
    let object_count: i64 = connection
        .query_row(
            "
                SELECT COUNT(*)
                FROM sqlite_schema
                WHERE name NOT LIKE 'sqlite_%'
            ",
            [],
            |row| row.get(0),
        )
        .map_err(map_sqlite_error)?;
    if object_count != 0 {
        return Err(MomentsIndexError::SchemaMismatch {
            reason: "unversioned database is not empty",
        });
    }
    Ok(())
}

fn initialize_schema(connection: &mut Connection) -> Result<(), MomentsIndexError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sqlite_error)?;
    let schema_version: i64 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(map_sqlite_error)?;
    if schema_version > i64::from(MOMENTS_INDEX_SCHEMA_VERSION) {
        return Err(MomentsIndexError::UnsupportedSchema {
            found: schema_version,
            supported: MOMENTS_INDEX_SCHEMA_VERSION,
        });
    }
    if schema_version == i64::from(MOMENTS_INDEX_SCHEMA_VERSION) {
        transaction.commit().map_err(map_sqlite_error)?;
        return Ok(());
    }
    if schema_version != 0 {
        return Err(MomentsIndexError::SchemaMismatch {
            reason: "schema version is not supported",
        });
    }
    require_empty_unversioned_database(&transaction)?;

    transaction
        .execute(&create_batches_sql(), [])
        .map_err(map_sqlite_error)?;
    transaction
        .execute(&create_moments_sql(), [])
        .map_err(map_sqlite_error)?;
    transaction
        .execute(&create_query_index_sql(), [])
        .map_err(map_sqlite_error)?;
    transaction
        .pragma_update(
            None,
            "user_version",
            i64::from(MOMENTS_INDEX_SCHEMA_VERSION),
        )
        .map_err(map_sqlite_error)?;
    transaction.commit().map_err(map_sqlite_error)
}

fn validate_schema(connection: &Connection) -> Result<(), MomentsIndexError> {
    validate_schema_object(
        connection,
        "table",
        "moment_batches",
        &create_batches_sql(),
        "moment_batches table is missing or incompatible",
    )?;
    validate_schema_object(
        connection,
        "table",
        "moments",
        &create_moments_sql(),
        "moments table is missing or incompatible",
    )?;
    validate_schema_object(
        connection,
        "index",
        "moments_query_order",
        &create_query_index_sql(),
        "moments query index is missing or incompatible",
    )
}

fn validate_schema_object(
    connection: &Connection,
    object_type: &str,
    name: &str,
    expected_sql: &str,
    reason: &'static str,
) -> Result<(), MomentsIndexError> {
    let actual_sql = connection
        .query_row(
            "
                SELECT sql
                FROM sqlite_schema
                WHERE type = ?1 AND name = ?2
            ",
            params![object_type, name],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some(actual_sql) = actual_sql else {
        return Err(MomentsIndexError::SchemaMismatch { reason });
    };
    if normalize_schema_sql(&actual_sql) != normalize_schema_sql(expected_sql) {
        return Err(MomentsIndexError::SchemaMismatch { reason });
    }
    Ok(())
}

fn normalize_schema_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn create_batches_sql() -> String {
    format!(
        "
        CREATE TABLE moment_batches (
            content_key BLOB NOT NULL
                CHECK (
                    typeof(content_key) = 'blob'
                    AND length(content_key) = {content_key_bytes}
                ),
            analyzer_identity TEXT NOT NULL
                CHECK (
                    typeof(analyzer_identity) = 'text'
                    AND length(CAST(analyzer_identity AS BLOB))
                        BETWEEN 1 AND {maximum_identity_bytes}
                    AND substr(analyzer_identity, 1, 1) GLOB '[a-z0-9]'
                    AND analyzer_identity NOT GLOB '*[^a-z0-9._/-]*'
                ),
            analyzer_version INTEGER NOT NULL
                CHECK (
                    typeof(analyzer_version) = 'integer'
                    AND analyzer_version BETWEEN 1 AND {maximum_analyzer_version}
                ),
            PRIMARY KEY (
                content_key,
                analyzer_identity,
                analyzer_version
            )
        ) WITHOUT ROWID
        ",
        content_key_bytes = MEDIA_CONTENT_KEY_BYTES,
        maximum_identity_bytes = MAX_ANALYZER_IDENTITY_BYTES,
        maximum_analyzer_version = u32::MAX,
    )
}

fn create_moments_sql() -> String {
    format!(
        "
        CREATE TABLE moments (
            content_key BLOB NOT NULL
                CHECK (
                    typeof(content_key) = 'blob'
                    AND length(content_key) = {content_key_bytes}
                ),
            analyzer_identity TEXT NOT NULL
                CHECK (
                    typeof(analyzer_identity) = 'text'
                    AND length(CAST(analyzer_identity AS BLOB))
                        BETWEEN 1 AND {maximum_identity_bytes}
                    AND substr(analyzer_identity, 1, 1) GLOB '[a-z0-9]'
                    AND analyzer_identity NOT GLOB '*[^a-z0-9._/-]*'
                ),
            analyzer_version INTEGER NOT NULL
                CHECK (
                    typeof(analyzer_version) = 'integer'
                    AND analyzer_version BETWEEN 1 AND {maximum_analyzer_version}
                ),
            start_seconds REAL NOT NULL
                CHECK (
                    typeof(start_seconds) = 'real'
                    AND start_seconds >= 0.0
                    AND start_seconds <= {maximum_time:e}
                ),
            end_seconds REAL NOT NULL
                CHECK (
                    typeof(end_seconds) = 'real'
                    AND end_seconds >= start_seconds
                    AND end_seconds <= {maximum_time:e}
                ),
            kind TEXT NOT NULL
                CHECK (
                    typeof(kind) = 'text'
                    AND length(CAST(kind AS BLOB))
                        BETWEEN 1 AND {maximum_kind_bytes}
                    AND substr(kind, 1, 1) GLOB '[a-z]'
                    AND kind NOT GLOB '*[^a-z0-9._-]*'
                ),
            confidence REAL NOT NULL
                CHECK (
                    typeof(confidence) = 'real'
                    AND confidence >= 0.0
                    AND confidence <= 1.0
                ),
            payload_present INTEGER NOT NULL
                CHECK (
                    typeof(payload_present) = 'integer'
                    AND payload_present IN (0, 1)
                ),
            payload_text TEXT NOT NULL
                CHECK (
                    typeof(payload_text) = 'text'
                    AND (
                        (
                            payload_present = 0
                            AND length(CAST(payload_text AS BLOB)) = 0
                        )
                        OR (
                            payload_present = 1
                            AND length(CAST(payload_text AS BLOB))
                                BETWEEN 1 AND {maximum_payload_bytes}
                        )
                    )
                ),
            PRIMARY KEY (
                content_key,
                analyzer_identity,
                analyzer_version,
                start_seconds,
                end_seconds,
                kind,
                confidence,
                payload_present,
                payload_text
            ),
            FOREIGN KEY (
                content_key,
                analyzer_identity,
                analyzer_version
            ) REFERENCES moment_batches (
                content_key,
                analyzer_identity,
                analyzer_version
            ) ON DELETE CASCADE
        ) WITHOUT ROWID
        ",
        content_key_bytes = MEDIA_CONTENT_KEY_BYTES,
        maximum_identity_bytes = MAX_ANALYZER_IDENTITY_BYTES,
        maximum_analyzer_version = u32::MAX,
        maximum_time = MAX_MOMENT_TIME_SECONDS,
        maximum_kind_bytes = MAX_MOMENT_KIND_BYTES,
        maximum_payload_bytes = MAX_MOMENT_PAYLOAD_BYTES,
    )
}

fn create_query_index_sql() -> String {
    "
        CREATE INDEX moments_query_order ON moments (
            content_key,
            start_seconds,
            end_seconds,
            kind,
            analyzer_identity,
            analyzer_version,
            confidence DESC,
            payload_present,
            payload_text
        )
    "
    .to_owned()
}
