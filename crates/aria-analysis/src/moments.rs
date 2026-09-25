//! Validated domain and query types for persistent media moments.
//!
//! The domain values and in-memory matching behavior remain dependency-free.
//! Enabling the `sqlite` feature also exposes the optional persistent
//! [`MomentsIndex`] adapter.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use crate::TimeSpan;

pub mod adapters;

pub use adapters::{
    AdapterRecordKind, MOMENT_ADAPTER_PAYLOAD_VERSION, MomentAdapterError,
    beat_events_to_moment_batch, shot_boundaries_to_moment_batch, silence_spans_to_moment_batch,
};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub use sqlite::{
    DEFAULT_MOMENTS_BUSY_TIMEOUT, DEFAULT_MOMENTS_QUERY_LIMIT, MAX_MOMENT_TIME_SECONDS,
    MAX_MOMENTS_BUSY_TIMEOUT, MAX_MOMENTS_QUERY_LIMIT, MOMENTS_INDEX_DATABASE_FILENAME,
    MOMENTS_INDEX_SCHEMA_VERSION, MomentsIndex, MomentsIndexConfig, MomentsIndexError,
    MomentsQueryResult,
};

/// Number of bytes in a [`MediaContentKey`].
pub const MEDIA_CONTENT_KEY_BYTES: usize = 32;

/// Number of ASCII bytes in the hexadecimal form of a [`MediaContentKey`].
pub const MEDIA_CONTENT_KEY_HEX_BYTES: usize = MEDIA_CONTENT_KEY_BYTES * 2;

/// Maximum UTF-8 byte length of a [`MomentKind`].
pub const MAX_MOMENT_KIND_BYTES: usize = 64;

/// Maximum UTF-8 byte length of an [`AnalyzerIdentity`].
pub const MAX_ANALYZER_IDENTITY_BYTES: usize = 128;

/// Maximum UTF-8 byte length of a [`MomentPayload`].
///
/// The payload is intentionally small enough for bounded tool and database
/// results. Larger analyzer artifacts should live outside the moments index.
pub const MAX_MOMENT_PAYLOAD_BYTES: usize = 16 * 1024;

#[path = "moments_key.rs"]
mod key;
#[path = "moments_names.rs"]
mod names;
#[path = "moments_values.rs"]
mod values;

pub use key::{MediaContentKey, MediaContentKeyError};
pub use names::{
    AnalyzerIdentity, AnalyzerIdentityError, AnalyzerVersion, AnalyzerVersionError, MomentKind,
    MomentKindError,
};
pub use values::{MomentConfidence, MomentConfidenceError, MomentPayload, MomentPayloadError};

/// One immutable, validated result produced by an analyzer.
///
/// `span` is half-open and may be empty. An empty span represents a point
/// event at its start boundary. All owned text enters through bounded,
/// validated domain types.
///
/// The natural ordering is deterministic: content key, start time, end time,
/// kind, analyzer identity, analyzer version, descending confidence, then
/// payload (`None` before text, text lexicographically).
#[derive(Clone, Debug, PartialEq)]
pub struct MomentRecord {
    content_key: MediaContentKey,
    span: TimeSpan,
    kind: MomentKind,
    confidence: MomentConfidence,
    analyzer_identity: AnalyzerIdentity,
    analyzer_version: AnalyzerVersion,
    payload: Option<MomentPayload>,
}

impl MomentRecord {
    /// Constructs a record from values that have already been validated by
    /// their domain types.
    #[must_use]
    pub fn new(
        content_key: MediaContentKey,
        span: TimeSpan,
        kind: MomentKind,
        confidence: MomentConfidence,
        analyzer_identity: AnalyzerIdentity,
        analyzer_version: AnalyzerVersion,
        payload: Option<MomentPayload>,
    ) -> Self {
        Self {
            content_key,
            span,
            kind,
            confidence,
            analyzer_identity,
            analyzer_version,
            payload,
        }
    }

    /// Returns the media content key.
    #[must_use]
    pub const fn content_key(&self) -> MediaContentKey {
        self.content_key
    }

    /// Returns the half-open time span.
    #[must_use]
    pub const fn span(&self) -> TimeSpan {
        self.span
    }

    /// Returns the moment kind.
    #[must_use]
    pub const fn kind(&self) -> &MomentKind {
        &self.kind
    }

    /// Returns the confidence.
    #[must_use]
    pub const fn confidence(&self) -> MomentConfidence {
        self.confidence
    }

    /// Returns the analyzer/source identity.
    #[must_use]
    pub const fn analyzer_identity(&self) -> &AnalyzerIdentity {
        &self.analyzer_identity
    }

    /// Returns the analyzer result schema/version.
    #[must_use]
    pub const fn analyzer_version(&self) -> AnalyzerVersion {
        self.analyzer_version
    }

    /// Returns the optional uninterpreted payload.
    #[must_use]
    pub const fn payload(&self) -> Option<&MomentPayload> {
        self.payload.as_ref()
    }
}

impl Eq for MomentRecord {}

impl PartialOrd for MomentRecord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MomentRecord {
    fn cmp(&self, other: &Self) -> Ordering {
        self.content_key
            .cmp(&other.content_key)
            .then_with(|| cmp_valid_time(self.span.start_seconds(), other.span.start_seconds()))
            .then_with(|| cmp_valid_time(self.span.end_seconds(), other.span.end_seconds()))
            .then_with(|| self.kind.cmp(&other.kind))
            .then_with(|| self.analyzer_identity.cmp(&other.analyzer_identity))
            .then_with(|| self.analyzer_version.cmp(&other.analyzer_version))
            .then_with(|| other.confidence.cmp(&self.confidence))
            .then_with(|| self.payload.cmp(&other.payload))
    }
}

fn cmp_valid_time(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right)
        .expect("TimeSpan guarantees finite boundaries")
}

/// A validated in-memory moments query.
///
/// The content key is always required. Kind matching is exact. Minimum
/// confidence is inclusive. With a non-empty query range `[query_start,
/// query_end)`:
///
/// - a non-empty record `[record_start, record_end)` matches when the two
///   half-open spans overlap (`record_start < query_end` and
///   `record_end > query_start`);
/// - a point record `[point, point)` matches when
///   `query_start <= point < query_end`.
///
/// Consequently, a span ending at the query start does not match, a span
/// starting at the query end does not match, a point at the query start does
/// match, and a point at the query end does not. An empty query range matches
/// no records.
#[derive(Clone, Debug, PartialEq)]
pub struct MomentQuery {
    content_key: MediaContentKey,
    kind: Option<MomentKind>,
    time_range: Option<TimeSpan>,
    minimum_confidence: MomentConfidence,
}

impl MomentQuery {
    /// Constructs an unbounded query for one media content key.
    ///
    /// The initial query accepts every kind, every time, and confidence zero
    /// or greater.
    #[must_use]
    pub const fn new(content_key: MediaContentKey) -> Self {
        Self {
            content_key,
            kind: None,
            time_range: None,
            minimum_confidence: MomentConfidence::ZERO,
        }
    }

    /// Adds an exact kind filter.
    #[must_use]
    pub fn with_kind(mut self, kind: MomentKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Adds a half-open time-range filter.
    #[must_use]
    pub const fn with_time_range(mut self, time_range: TimeSpan) -> Self {
        self.time_range = Some(time_range);
        self
    }

    /// Adds an inclusive minimum-confidence filter.
    #[must_use]
    pub const fn with_minimum_confidence(mut self, minimum_confidence: MomentConfidence) -> Self {
        self.minimum_confidence = minimum_confidence;
        self
    }

    /// Returns the required media content key.
    #[must_use]
    pub const fn content_key(&self) -> MediaContentKey {
        self.content_key
    }

    /// Returns the optional exact kind filter.
    #[must_use]
    pub const fn kind(&self) -> Option<&MomentKind> {
        self.kind.as_ref()
    }

    /// Returns the optional half-open time range.
    #[must_use]
    pub const fn time_range(&self) -> Option<TimeSpan> {
        self.time_range
    }

    /// Returns the inclusive minimum confidence.
    #[must_use]
    pub const fn minimum_confidence(&self) -> MomentConfidence {
        self.minimum_confidence
    }

    /// Returns whether a record satisfies every query filter.
    ///
    /// Time matching follows the point-versus-span boundary semantics
    /// documented on [`MomentQuery`].
    #[must_use]
    pub fn matches(&self, record: &MomentRecord) -> bool {
        record.content_key == self.content_key
            && self.kind.as_ref().is_none_or(|kind| record.kind == *kind)
            && record.confidence >= self.minimum_confidence
            && self
                .time_range
                .is_none_or(|range| span_matches_range(record.span, range))
    }
}

impl Eq for MomentQuery {}

fn span_matches_range(record: TimeSpan, query: TimeSpan) -> bool {
    if query.is_empty() {
        return false;
    }

    if record.is_empty() {
        query.start_seconds() <= record.start_seconds()
            && record.start_seconds() < query.end_seconds()
    } else {
        record.start_seconds() < query.end_seconds() && record.end_seconds() > query.start_seconds()
    }
}

/// Filters borrowed records and returns them in canonical value order.
///
/// Results are independent of input order. Within the required content key,
/// ordering is start time, end time, kind, analyzer identity, analyzer version,
/// descending confidence, and payload (`None` before lexicographic text).
///
/// For `n` input records and `m` matches, complexity is `O(n + m log m)` time
/// and `O(m)` output space. Filtering is one linear pass; no pairwise scan is
/// performed.
#[must_use]
pub fn filter_and_sort_moments<'a>(
    records: &'a [MomentRecord],
    query: &MomentQuery,
) -> Vec<&'a MomentRecord> {
    let mut matches: Vec<_> = records
        .iter()
        .filter(|record| query.matches(record))
        .collect();
    matches.sort_unstable();
    matches
}

/// The replacement scope for one analyzer's complete result set.
///
/// Storage adapters should use the full tuple `(content key, analyzer
/// identity, analyzer version)` as the delete-and-insert scope. Results from
/// another analyzer or version are outside this key and must remain untouched.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MomentBatchKey {
    content_key: MediaContentKey,
    analyzer_identity: AnalyzerIdentity,
    analyzer_version: AnalyzerVersion,
}

impl MomentBatchKey {
    /// Constructs a replacement key from validated components.
    #[must_use]
    pub const fn new(
        content_key: MediaContentKey,
        analyzer_identity: AnalyzerIdentity,
        analyzer_version: AnalyzerVersion,
    ) -> Self {
        Self {
            content_key,
            analyzer_identity,
            analyzer_version,
        }
    }

    /// Returns the media content key.
    #[must_use]
    pub const fn content_key(&self) -> MediaContentKey {
        self.content_key
    }

    /// Returns the analyzer/source identity.
    #[must_use]
    pub const fn analyzer_identity(&self) -> &AnalyzerIdentity {
        &self.analyzer_identity
    }

    /// Returns the analyzer result schema/version.
    #[must_use]
    pub const fn analyzer_version(&self) -> AnalyzerVersion {
        self.analyzer_version
    }
}

/// A validated complete replacement batch for one [`MomentBatchKey`].
///
/// Construction rejects records outside the key and duplicate exact records,
/// then stores accepted records in [`MomentRecord`]'s canonical order. An empty
/// batch is valid and means the analyzer completed with no results, allowing a
/// storage transaction to remove its previous rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MomentReplacementBatch {
    key: MomentBatchKey,
    records: Vec<MomentRecord>,
}

impl MomentReplacementBatch {
    /// Validates and canonically orders a complete analyzer result set.
    ///
    /// # Complexity
    ///
    /// For `n` records, construction takes `O(n log n)` time and `O(n)` space
    /// for the owned batch. Validation itself is one linear pass; sorting also
    /// enables adjacent duplicate detection without an `O(n²)` scan.
    ///
    /// # Errors
    ///
    /// Returns [`MomentBatchError`] for the first record whose content key,
    /// analyzer identity, or analyzer version differs from `key`, or for a
    /// duplicate exact-record pair. Duplicate errors report original input
    /// positions.
    pub fn new(key: MomentBatchKey, records: Vec<MomentRecord>) -> Result<Self, MomentBatchError> {
        for (record_index, record) in records.iter().enumerate() {
            if record.content_key != key.content_key {
                return Err(MomentBatchError::ContentKeyMismatch { record_index });
            }
            if record.analyzer_identity != key.analyzer_identity {
                return Err(MomentBatchError::AnalyzerIdentityMismatch { record_index });
            }
            if record.analyzer_version != key.analyzer_version {
                return Err(MomentBatchError::AnalyzerVersionMismatch { record_index });
            }
        }

        let mut indexed: Vec<_> = records.into_iter().enumerate().collect();
        indexed.sort_unstable_by(|(left_index, left_record), (right_index, right_record)| {
            left_record
                .cmp(right_record)
                .then_with(|| left_index.cmp(right_index))
        });

        for pair in indexed.windows(2) {
            if pair[0].1 == pair[1].1 {
                return Err(MomentBatchError::DuplicateRecord {
                    first_index: pair[0].0,
                    duplicate_index: pair[1].0,
                });
            }
        }

        Ok(Self {
            key,
            records: indexed.into_iter().map(|(_, record)| record).collect(),
        })
    }

    /// Returns the atomic replacement scope.
    #[must_use]
    pub const fn key(&self) -> &MomentBatchKey {
        &self.key
    }

    /// Returns the canonically ordered complete result set.
    #[must_use]
    pub fn records(&self) -> &[MomentRecord] {
        &self.records
    }

    /// Returns the number of records in the complete result set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns whether the complete result set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Consumes the batch into its replacement key and canonical records.
    #[must_use]
    pub fn into_parts(self) -> (MomentBatchKey, Vec<MomentRecord>) {
        (self.key, self.records)
    }
}

/// Errors produced while validating a [`MomentReplacementBatch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MomentBatchError {
    /// A record belonged to another media content key.
    ContentKeyMismatch {
        /// Zero-based position in the supplied records.
        record_index: usize,
    },
    /// A record belonged to another analyzer identity.
    AnalyzerIdentityMismatch {
        /// Zero-based position in the supplied records.
        record_index: usize,
    },
    /// A record belonged to another analyzer version.
    AnalyzerVersionMismatch {
        /// Zero-based position in the supplied records.
        record_index: usize,
    },
    /// Two supplied records were exactly equal in every domain field.
    DuplicateRecord {
        /// Original position of the first equal record.
        first_index: usize,
        /// Original position of the repeated equal record.
        duplicate_index: usize,
    },
}

impl fmt::Display for MomentBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentKeyMismatch { record_index } => write!(
                formatter,
                "moment at index {record_index} has a different media content key"
            ),
            Self::AnalyzerIdentityMismatch { record_index } => write!(
                formatter,
                "moment at index {record_index} has a different analyzer identity"
            ),
            Self::AnalyzerVersionMismatch { record_index } => write!(
                formatter,
                "moment at index {record_index} has a different analyzer version"
            ),
            Self::DuplicateRecord {
                first_index,
                duplicate_index,
            } => write!(
                formatter,
                "moment at index {duplicate_index} duplicates moment at index {first_index}"
            ),
        }
    }
}

impl Error for MomentBatchError {}
