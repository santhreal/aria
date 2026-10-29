//! SQLite persistence for validated media moments.

use std::fs;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::ffi::ErrorCode;
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior, params};

use super::{
    AnalyzerIdentity, AnalyzerVersion, MAX_ANALYZER_IDENTITY_BYTES, MAX_MOMENT_KIND_BYTES,
    MAX_MOMENT_PAYLOAD_BYTES, MEDIA_CONTENT_KEY_BYTES, MediaContentKey, MomentBatchKey,
    MomentConfidence, MomentKind, MomentPayload, MomentQuery, MomentRecord, MomentReplacementBatch,
};
use crate::TimeSpan;

/// Filename used by [`MomentsIndex::open_in_cache_root`].
pub const MOMENTS_INDEX_DATABASE_FILENAME: &str = "moments.sqlite3";

/// Current SQLite schema version stored in `PRAGMA user_version`.
pub const MOMENTS_INDEX_SCHEMA_VERSION: u32 = 1;

/// Default maximum number of records returned by one query.
pub const DEFAULT_MOMENTS_QUERY_LIMIT: usize = 256;

/// Absolute upper bound for a configured or caller-requested query limit.
pub const MAX_MOMENTS_QUERY_LIMIT: usize = 4_096;

/// Default duration SQLite waits for a contended database lock.
pub const DEFAULT_MOMENTS_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Longest lock wait accepted by [`MomentsIndexConfig`].
pub const MAX_MOMENTS_BUSY_TIMEOUT: Duration = Duration::from_secs(60);

/// Largest time boundary accepted by the SQLite schema.
///
/// This is the largest finite `f64`, preserving the full validated
/// [`TimeSpan`] domain while excluding SQLite infinities.
pub const MAX_MOMENT_TIME_SECONDS: f64 = f64::MAX;

mod errors;

pub use errors::MomentsIndexError;

mod rows;
mod schema;

use rows::{hydrate_record, map_sqlite_error};
use schema::{
    DELETE_MOMENTS_SQL, INSERT_MOMENT_SQL, QUERY_MOMENTS_SQL, UPSERT_BATCH_SQL,
    initialize_connection,
};

/// Validated operational limits for a [`MomentsIndex`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MomentsIndexConfig {
    default_query_limit: usize,
    maximum_query_limit: usize,
    busy_timeout: Duration,
}

impl MomentsIndexConfig {
    /// Creates a validated index configuration.
    ///
    /// Query limits must be non-zero, the default must not exceed the
    /// configured maximum, and the maximum must not exceed
    /// [`MAX_MOMENTS_QUERY_LIMIT`]. `busy_timeout` may be zero and must not
    /// exceed [`MAX_MOMENTS_BUSY_TIMEOUT`].
    ///
    /// # Errors
    ///
    /// Returns [`MomentsIndexError::InvalidConfig`] for the first invalid
    /// field.
    pub fn new(
        default_query_limit: usize,
        maximum_query_limit: usize,
        busy_timeout: Duration,
    ) -> Result<Self, MomentsIndexError> {
        let config = Self {
            default_query_limit,
            maximum_query_limit,
            busy_timeout,
        };
        config.validate()?;
        Ok(config)
    }

    /// Number of records requested by [`MomentsIndex::query`].
    #[must_use]
    pub const fn default_query_limit(self) -> usize {
        self.default_query_limit
    }

    /// Largest limit accepted by [`MomentsIndex::query_with_limit`].
    #[must_use]
    pub const fn maximum_query_limit(self) -> usize {
        self.maximum_query_limit
    }

    /// Duration SQLite waits for a contended database lock.
    #[must_use]
    pub const fn busy_timeout(self) -> Duration {
        self.busy_timeout
    }

    fn validate(self) -> Result<(), MomentsIndexError> {
        if self.maximum_query_limit == 0 {
            return Err(MomentsIndexError::InvalidConfig {
                field: "maximum_query_limit",
                reason: "must be non-zero",
            });
        }
        if self.maximum_query_limit > MAX_MOMENTS_QUERY_LIMIT {
            return Err(MomentsIndexError::InvalidConfig {
                field: "maximum_query_limit",
                reason: "exceeds the absolute query limit",
            });
        }
        if self.default_query_limit == 0 {
            return Err(MomentsIndexError::InvalidConfig {
                field: "default_query_limit",
                reason: "must be non-zero",
            });
        }
        if self.default_query_limit > self.maximum_query_limit {
            return Err(MomentsIndexError::InvalidConfig {
                field: "default_query_limit",
                reason: "must not exceed maximum_query_limit",
            });
        }
        if self.busy_timeout > MAX_MOMENTS_BUSY_TIMEOUT {
            return Err(MomentsIndexError::InvalidConfig {
                field: "busy_timeout",
                reason: "exceeds the maximum busy timeout",
            });
        }
        Ok(())
    }
}

impl Default for MomentsIndexConfig {
    fn default() -> Self {
        Self {
            default_query_limit: DEFAULT_MOMENTS_QUERY_LIMIT,
            maximum_query_limit: MAX_MOMENTS_QUERY_LIMIT,
            busy_timeout: DEFAULT_MOMENTS_BUSY_TIMEOUT,
        }
    }
}

/// Result of one bounded moments query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MomentsQueryResult {
    records: Vec<MomentRecord>,
    truncated: bool,
}

impl MomentsQueryResult {
    /// Returned records in [`MomentRecord`]'s canonical order.
    #[must_use]
    pub fn records(&self) -> &[MomentRecord] {
        &self.records
    }

    /// Consumes the result and returns its records.
    #[must_use]
    pub fn into_records(self) -> Vec<MomentRecord> {
        self.records
    }

    /// Whether at least one additional matching record exists.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Number of returned records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no records were returned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Thread-safe SQLite persistence for complete analyzer moment batches.
///
/// A single rusqlite connection is serialized by a mutex. Replacement batches
/// use `BEGIN IMMEDIATE`, so readers never observe a partially replaced batch.
pub struct MomentsIndex {
    path: PathBuf,
    config: MomentsIndexConfig,
    connection: Mutex<Connection>,
}

impl std::fmt::Debug for MomentsIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MomentsIndex")
            .field("path", &self.path)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl MomentsIndex {
    /// Opens or creates an index at an explicit database path.
    ///
    /// Missing parent directories are created. Existing files are never
    /// deleted or replaced when opening, migrating, or reporting corruption.
    ///
    /// # Errors
    ///
    /// Returns a typed [`MomentsIndexError`] for filesystem, SQLite, schema,
    /// or configuration failures.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MomentsIndexError> {
        Self::open_with_config(path, MomentsIndexConfig::default())
    }

    /// Opens or creates the standard database beneath a cache root.
    ///
    /// The path is `cache_root /` [`MOMENTS_INDEX_DATABASE_FILENAME`].
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`MomentsIndex::open`].
    pub fn open_in_cache_root(cache_root: impl AsRef<Path>) -> Result<Self, MomentsIndexError> {
        Self::open_in_cache_root_with_config(cache_root, MomentsIndexConfig::default())
    }

    /// Opens an explicit database path with validated operational limits.
    ///
    /// # Errors
    ///
    /// Returns a typed [`MomentsIndexError`] for filesystem, SQLite, schema,
    /// or configuration failures.
    pub fn open_with_config(
        path: impl AsRef<Path>,
        config: MomentsIndexConfig,
    ) -> Result<Self, MomentsIndexError> {
        config.validate()?;
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().is_empty() {
            return Err(MomentsIndexError::SchemaMismatch {
                reason: "database path must not be empty",
            });
        }

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| MomentsIndexError::FileSystem {
                operation: "parent directory creation",
                source,
            })?;
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection = Connection::open_with_flags(&path, flags).map_err(map_sqlite_error)?;
        initialize_connection(&mut connection, config)?;

        Ok(Self {
            path,
            config,
            connection: Mutex::new(connection),
        })
    }

    /// Opens the standard cache-root database with explicit configuration.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`MomentsIndex::open_with_config`].
    pub fn open_in_cache_root_with_config(
        cache_root: impl AsRef<Path>,
        config: MomentsIndexConfig,
    ) -> Result<Self, MomentsIndexError> {
        Self::open_with_config(
            cache_root.as_ref().join(MOMENTS_INDEX_DATABASE_FILENAME),
            config,
        )
    }

    /// Exact database path supplied to the open operation.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Operational configuration used by this index.
    #[must_use]
    pub const fn config(&self) -> MomentsIndexConfig {
        self.config
    }

    /// Atomically replaces one analyzer's complete result batch.
    ///
    /// The batch marker is retained even when `batch` contains no records.
    /// Any failure before commit rolls back the marker, deletion, and all
    /// inserts, leaving the prior complete batch unchanged.
    ///
    /// # Errors
    ///
    /// Returns a typed lock, SQLite, constraint, or mutex error.
    pub fn replace_batch(&self, batch: &MomentReplacementBatch) -> Result<(), MomentsIndexError> {
        let key = batch.key();
        let content_key = key.content_key();
        let analyzer_version = i64::from(key.analyzer_version().get());
        let mut connection = self.lock_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;

        transaction
            .execute(
                UPSERT_BATCH_SQL,
                params![
                    &content_key.as_bytes()[..],
                    key.analyzer_identity().as_str(),
                    analyzer_version,
                ],
            )
            .map_err(map_sqlite_error)?;
        transaction
            .execute(
                DELETE_MOMENTS_SQL,
                params![
                    &content_key.as_bytes()[..],
                    key.analyzer_identity().as_str(),
                    analyzer_version,
                ],
            )
            .map_err(map_sqlite_error)?;

        {
            let mut insert = transaction
                .prepare_cached(INSERT_MOMENT_SQL)
                .map_err(map_sqlite_error)?;
            for record in batch.records() {
                let (payload_present, payload_text) = record
                    .payload()
                    .map_or((0_i64, ""), |payload| (1_i64, payload.as_str()));
                insert
                    .execute(params![
                        &content_key.as_bytes()[..],
                        key.analyzer_identity().as_str(),
                        analyzer_version,
                        record.span().start_seconds(),
                        record.span().end_seconds(),
                        record.kind().as_str(),
                        f64::from(record.confidence().get()),
                        payload_present,
                        payload_text,
                    ])
                    .map_err(map_sqlite_error)?;
            }
        }

        transaction.commit().map_err(map_sqlite_error)
    }

    /// Returns whether a completed batch marker exists for `key`.
    ///
    /// This returns `true` for a successfully persisted empty replacement.
    ///
    /// # Errors
    ///
    /// Returns a typed SQLite or mutex error.
    pub fn batch_exists(&self, key: &MomentBatchKey) -> Result<bool, MomentsIndexError> {
        let content_key = key.content_key();
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "
                    SELECT EXISTS (
                        SELECT 1
                        FROM moment_batches
                        WHERE content_key = ?1
                          AND analyzer_identity = ?2
                          AND analyzer_version = ?3
                    )
                ",
                params![
                    &content_key.as_bytes()[..],
                    key.analyzer_identity().as_str(),
                    i64::from(key.analyzer_version().get()),
                ],
                |row| row.get::<_, bool>(0),
            )
            .map_err(map_sqlite_error)
    }

    /// Deletes a completed batch marker and its cascading child rows.
    ///
    /// The returned boolean is `true` when a marker existed.
    ///
    /// # Errors
    ///
    /// Returns a typed SQLite or mutex error.
    pub fn delete_batch(&self, key: &MomentBatchKey) -> Result<bool, MomentsIndexError> {
        let content_key = key.content_key();
        let connection = self.lock_connection()?;
        let deleted = connection
            .execute(
                "
                    DELETE FROM moment_batches
                    WHERE content_key = ?1
                      AND analyzer_identity = ?2
                      AND analyzer_version = ?3
                ",
                params![
                    &content_key.as_bytes()[..],
                    key.analyzer_identity().as_str(),
                    i64::from(key.analyzer_version().get()),
                ],
            )
            .map_err(map_sqlite_error)?;
        Ok(deleted != 0)
    }

    /// Runs a bounded query using the configured default limit.
    ///
    /// # Errors
    ///
    /// Returns a typed SQLite, corrupt-row, or mutex error.
    pub fn query(&self, query: &MomentQuery) -> Result<MomentsQueryResult, MomentsIndexError> {
        self.query_with_limit(query, self.config.default_query_limit)
    }

    /// Runs a bounded query with an explicit positive limit.
    ///
    /// One sentinel row beyond `limit` is read and validated. `truncated` is
    /// therefore set only when an additional matching row actually exists.
    ///
    /// # Errors
    ///
    /// Returns [`MomentsIndexError::InvalidQueryLimit`] for zero or a value
    /// above this index's configured maximum. SQLite, corrupt-row, and mutex
    /// failures are also returned.
    pub fn query_with_limit(
        &self,
        query: &MomentQuery,
        limit: usize,
    ) -> Result<MomentsQueryResult, MomentsIndexError> {
        if limit == 0 || limit > self.config.maximum_query_limit {
            return Err(MomentsIndexError::InvalidQueryLimit {
                requested: limit,
                maximum: self.config.maximum_query_limit,
            });
        }
        let fetch_limit = limit
            .checked_add(1)
            .ok_or(MomentsIndexError::InvalidQueryLimit {
                requested: limit,
                maximum: self.config.maximum_query_limit,
            })?;
        let fetch_limit_sql =
            i64::try_from(fetch_limit).map_err(|_| MomentsIndexError::InvalidQueryLimit {
                requested: limit,
                maximum: self.config.maximum_query_limit,
            })?;

        let (has_time_range, query_start, query_end) =
            query.time_range().map_or((0_i64, 0.0, 0.0), |range| {
                (1_i64, range.start_seconds(), range.end_seconds())
            });
        let content_key = query.content_key();
        let kind = query.kind().map(MomentKind::as_str);
        let minimum_confidence = f64::from(query.minimum_confidence().get());

        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare_cached(QUERY_MOMENTS_SQL)
            .map_err(map_sqlite_error)?;
        let mut rows = statement
            .query(params![
                &content_key.as_bytes()[..],
                kind,
                minimum_confidence,
                has_time_range,
                query_start,
                query_end,
                fetch_limit_sql,
            ])
            .map_err(map_sqlite_error)?;

        let mut records = Vec::with_capacity(fetch_limit);
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            records.push(hydrate_record(row, records.len())?);
        }

        let truncated = records.len() > limit;
        if truncated {
            records.pop();
        }
        Ok(MomentsQueryResult { records, truncated })
    }

    fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>, MomentsIndexError> {
        self.connection
            .lock()
            .map_err(|_| MomentsIndexError::MutexPoisoned)
    }
}
