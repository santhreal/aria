use std::error::Error;
use std::fmt;
use std::io;

/// Failures produced by the SQLite moments adapter.
#[derive(Debug)]
#[non_exhaustive]
pub enum MomentsIndexError {
    /// An index configuration field violated its documented bounds.
    InvalidConfig {
        /// Stable configuration field name.
        field: &'static str,
        /// Stable explanation that does not contain caller data.
        reason: &'static str,
    },
    /// A requested query limit was zero or exceeded the configured maximum.
    InvalidQueryLimit {
        /// Caller-requested limit.
        requested: usize,
        /// Configured maximum accepted by this index.
        maximum: usize,
    },
    /// The database was created by a newer schema version.
    UnsupportedSchema {
        /// Version found in `PRAGMA user_version`.
        found: i64,
        /// Newest version supported by this adapter.
        supported: u32,
    },
    /// The declared schema version did not match the required schema objects.
    SchemaMismatch {
        /// Stable mismatch category without database-provided text.
        reason: &'static str,
    },
    /// A returned row could not be reconstructed through domain constructors.
    CorruptRow {
        /// Zero-based position within the bounded SQLite result.
        row_index: usize,
        /// Stable field name.
        field: &'static str,
        /// Stable reason that never includes stored field contents.
        reason: &'static str,
    },
    /// The connection mutex was poisoned by an earlier panic.
    MutexPoisoned,
    /// Creating the database parent directory failed.
    FileSystem {
        /// Stable filesystem operation name.
        operation: &'static str,
        /// Underlying standard-library error.
        source: io::Error,
    },
    /// SQLite could not proceed because the database or a table was locked.
    DatabaseLocked {
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// SQLite reported a malformed or non-database file.
    DatabaseCorrupt {
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// SQLite rejected a value, trigger, size, or table constraint.
    ConstraintViolation {
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// SQLite reported an operating-system, access, capacity, or open failure.
    DatabaseIo {
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// Another SQLite failure occurred.
    Database {
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
}

impl fmt::Display for MomentsIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig { field, reason } => {
                write!(
                    formatter,
                    "invalid moments index config `{field}`: {reason}"
                )
            }
            Self::InvalidQueryLimit { requested, maximum } => write!(
                formatter,
                "invalid moments query limit {requested}; expected 1..={maximum}"
            ),
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "moments index schema version {found} is newer than supported version {supported}"
            ),
            Self::SchemaMismatch { reason } => {
                write!(formatter, "moments index schema mismatch: {reason}")
            }
            Self::CorruptRow {
                row_index,
                field,
                reason,
            } => write!(
                formatter,
                "corrupt moments row {row_index}, field `{field}`: {reason}"
            ),
            Self::MutexPoisoned => {
                formatter.write_str("moments index connection mutex is poisoned")
            }
            Self::FileSystem { operation, source } => {
                write!(formatter, "moments index {operation} failed: {source}")
            }
            Self::DatabaseLocked { source } => {
                write!(formatter, "moments database is locked: {source}")
            }
            Self::DatabaseCorrupt { source } => {
                write!(formatter, "moments database is corrupt: {source}")
            }
            Self::ConstraintViolation { source } => {
                write!(formatter, "moments database constraint failed: {source}")
            }
            Self::DatabaseIo { source } => {
                write!(formatter, "moments database I/O failed: {source}")
            }
            Self::Database { source } => write!(formatter, "moments database failed: {source}"),
        }
    }
}

impl Error for MomentsIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            Self::DatabaseLocked { source }
            | Self::DatabaseCorrupt { source }
            | Self::ConstraintViolation { source }
            | Self::DatabaseIo { source }
            | Self::Database { source } => Some(source),
            Self::InvalidConfig { .. }
            | Self::InvalidQueryLimit { .. }
            | Self::UnsupportedSchema { .. }
            | Self::SchemaMismatch { .. }
            | Self::CorruptRow { .. }
            | Self::MutexPoisoned => None,
        }
    }
}
