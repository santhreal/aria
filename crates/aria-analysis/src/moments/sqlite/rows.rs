//! One row of `moments` into one [`MomentRecord`], or a named corruption.
//!
//! Every column is read through a typed accessor that says which field and
//! which row failed, because a database this adapter wrote can still be edited
//! by hand, truncated by a full disk, or written by an older build. A row that
//! does not answer the schema is a refusal naming the field, never a record
//! with a plausible default in it.

use super::*;

pub(super) fn hydrate_record(
    row: &Row<'_>,
    row_index: usize,
) -> Result<MomentRecord, MomentsIndexError> {
    let content_key =
        MediaContentKey::from_slice(blob_column(row, 0, row_index, "content_key")?)
            .map_err(|_| corrupt_row(row_index, "content_key", "invalid media content key"))?;

    let start_seconds = real_column(row, 1, row_index, "start_seconds")?;
    let end_seconds = real_column(row, 2, row_index, "end_seconds")?;
    let span = TimeSpan::new(start_seconds, end_seconds)
        .map_err(|_| corrupt_row(row_index, "time_span", "invalid time boundaries"))?;

    let kind_text = text_column(row, 3, row_index, "kind")?;
    let kind = MomentKind::new(kind_text)
        .map_err(|_| corrupt_row(row_index, "kind", "invalid moment kind"))?;

    let stored_confidence = real_column(row, 4, row_index, "confidence")?;
    if !stored_confidence.is_finite() || !(0.0..=1.0).contains(&stored_confidence) {
        return Err(corrupt_row(
            row_index,
            "confidence",
            "value is outside the finite domain",
        ));
    }
    #[allow(clippy::cast_possible_truncation)]
    let confidence_f32 = stored_confidence as f32;
    if f64::from(confidence_f32).to_bits() != stored_confidence.to_bits() {
        return Err(corrupt_row(
            row_index,
            "confidence",
            "value is not an exact f32",
        ));
    }
    let confidence = MomentConfidence::new(confidence_f32)
        .map_err(|_| corrupt_row(row_index, "confidence", "invalid confidence"))?;

    let analyzer_text = text_column(row, 5, row_index, "analyzer_identity")?;
    let analyzer_identity = AnalyzerIdentity::new(analyzer_text)
        .map_err(|_| corrupt_row(row_index, "analyzer_identity", "invalid analyzer identity"))?;

    let stored_version = integer_column(row, 6, row_index, "analyzer_version")?;
    let version_u32 = u32::try_from(stored_version)
        .map_err(|_| corrupt_row(row_index, "analyzer_version", "value is outside u32"))?;
    let analyzer_version = AnalyzerVersion::new(version_u32)
        .map_err(|_| corrupt_row(row_index, "analyzer_version", "version must be non-zero"))?;

    let payload_present = integer_column(row, 7, row_index, "payload_present")?;
    let payload_text = text_column(row, 8, row_index, "payload_text")?;
    let payload = match payload_present {
        0 if payload_text.is_empty() => None,
        0 => {
            return Err(corrupt_row(
                row_index,
                "payload_text",
                "absent payload must use the empty sentinel",
            ));
        }
        1 => Some(
            MomentPayload::new(payload_text)
                .map_err(|_| corrupt_row(row_index, "payload_text", "invalid moment payload"))?,
        ),
        _ => {
            return Err(corrupt_row(
                row_index,
                "payload_present",
                "marker must be zero or one",
            ));
        }
    };

    Ok(MomentRecord::new(
        content_key,
        span,
        kind,
        confidence,
        analyzer_identity,
        analyzer_version,
        payload,
    ))
}

fn blob_column<'row>(
    row: &'row Row<'_>,
    column: usize,
    row_index: usize,
    field: &'static str,
) -> Result<&'row [u8], MomentsIndexError> {
    match row
        .get_ref(column)
        .map_err(|_| corrupt_row(row_index, field, "column could not be read"))?
    {
        ValueRef::Blob(value) => Ok(value),
        _ => Err(corrupt_row(row_index, field, "unexpected SQLite type")),
    }
}

fn text_column<'row>(
    row: &'row Row<'_>,
    column: usize,
    row_index: usize,
    field: &'static str,
) -> Result<&'row str, MomentsIndexError> {
    match row
        .get_ref(column)
        .map_err(|_| corrupt_row(row_index, field, "column could not be read"))?
    {
        ValueRef::Text(value) => str::from_utf8(value)
            .map_err(|_| corrupt_row(row_index, field, "text is not valid UTF-8")),
        _ => Err(corrupt_row(row_index, field, "unexpected SQLite type")),
    }
}

fn real_column(
    row: &Row<'_>,
    column: usize,
    row_index: usize,
    field: &'static str,
) -> Result<f64, MomentsIndexError> {
    match row
        .get_ref(column)
        .map_err(|_| corrupt_row(row_index, field, "column could not be read"))?
    {
        ValueRef::Real(value) => Ok(value),
        _ => Err(corrupt_row(row_index, field, "unexpected SQLite type")),
    }
}

fn integer_column(
    row: &Row<'_>,
    column: usize,
    row_index: usize,
    field: &'static str,
) -> Result<i64, MomentsIndexError> {
    match row
        .get_ref(column)
        .map_err(|_| corrupt_row(row_index, field, "column could not be read"))?
    {
        ValueRef::Integer(value) => Ok(value),
        _ => Err(corrupt_row(row_index, field, "unexpected SQLite type")),
    }
}

fn corrupt_row(row_index: usize, field: &'static str, reason: &'static str) -> MomentsIndexError {
    MomentsIndexError::CorruptRow {
        row_index,
        field,
        reason,
    }
}

pub(super) fn map_sqlite_error(source: rusqlite::Error) -> MomentsIndexError {
    match sqlite_error_code(&source) {
        Some(
            ErrorCode::DatabaseBusy
            | ErrorCode::DatabaseLocked
            | ErrorCode::FileLockingProtocolFailed,
        ) => MomentsIndexError::DatabaseLocked { source },
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            MomentsIndexError::DatabaseCorrupt { source }
        }
        Some(ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch | ErrorCode::TooBig) => {
            MomentsIndexError::ConstraintViolation { source }
        }
        Some(
            ErrorCode::PermissionDenied
            | ErrorCode::ReadOnly
            | ErrorCode::SystemIoFailure
            | ErrorCode::DiskFull
            | ErrorCode::CannotOpen
            | ErrorCode::NoLargeFileSupport,
        ) => MomentsIndexError::DatabaseIo { source },
        _ => MomentsIndexError::Database { source },
    }
}

fn sqlite_error_code(source: &rusqlite::Error) -> Option<ErrorCode> {
    match source {
        rusqlite::Error::SqliteFailure(error, _) => Some(error.code),
        rusqlite::Error::SqlInputError { error, .. } => Some(error.code),
        _ => None,
    }
}

pub(super) fn is_locking_error(source: &rusqlite::Error) -> bool {
    matches!(
        sqlite_error_code(source),
        Some(
            ErrorCode::DatabaseBusy
                | ErrorCode::DatabaseLocked
                | ErrorCode::FileLockingProtocolFailed
        )
    )
}
