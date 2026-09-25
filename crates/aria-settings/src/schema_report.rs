//! What this build does with the schema versions a `config.toml` records.
//!
//! [`crate::load`] refuses a file whose recorded version it cannot read, and the
//! refusal names one table. That is the right behaviour and the wrong amount of
//! information: an operator holding a configuration written by another build
//! wants to know, per table, which version is on disk, which version this build
//! reads, and how far back it migrates. Answering that used to require reading
//! the source.
//!
//! [`report`] answers it from the same three columns loading uses
//! ([`PersistedShape::current_version`], [`PersistedShape::oldest_migratable_version`],
//! [`PersistedShape::decision`]), enumerated from [`PersistedShape::all`] at run
//! time. A shape added to the enum appears here by existing; nothing here holds
//! a list of tables.

use std::path::Path;

use toml_edit::DocumentMut;

use crate::schema::{PersistedShape, VersionDecision, recorded_version};

/// One persisted table, as a diagnostics row reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeStatus {
    /// The shape this row describes.
    pub shape: PersistedShape,
    /// The top-level table it owns, which is also its key in `[schema]`.
    pub table: &'static str,
    /// The version the file records, or `None` when the file itself could not be
    /// read far enough to say — a parse failure, or a `[schema]` entry that is
    /// not a version number.
    pub recorded: Option<u32>,
    /// The version this build reads and writes.
    pub current: u32,
    /// The oldest recorded version this build migrates.
    pub oldest_migratable: u32,
    /// What loading does with the recorded version, or `None` when there is no
    /// recorded version to decide about.
    pub decision: Option<VersionDecision>,
    /// Whether the file actually holds this table. A shape whose table is absent
    /// is served from defaults, which is not the same as a shape that is refused.
    pub present: bool,
}

impl ShapeStatus {
    /// One line an operator can act on.
    ///
    /// Phrased from the decision rather than from the numbers alone: "records 1,
    /// reads 2" leaves the reader to work out whether that is fine.
    pub fn summary(&self) -> String {
        let Some(recorded) = self.recorded else {
            return format!(
                "unreadable; this build reads version {} and migrates {} and newer",
                self.current, self.oldest_migratable
            );
        };
        let where_from = if self.present {
            "in the file"
        } else {
            "not in the file, so defaults are served"
        };
        match self.decision {
            Some(VersionDecision::Current) => {
                format!("version {recorded}, current — {where_from}")
            }
            Some(VersionDecision::Migrate { from }) => format!(
                "version {from}, migrated to {} on load — {where_from}",
                self.current
            ),
            Some(VersionDecision::Refuse(_)) | None => format!(
                "version {recorded} refused; this build reads {} and migrates {} and newer",
                self.current, self.oldest_migratable
            ),
        }
    }

    /// Whether this shape stops the file from loading.
    pub fn refused(&self) -> bool {
        self.recorded.is_none() || matches!(self.decision, Some(VersionDecision::Refuse(_)))
    }
}

/// Every persisted shape and what this build does with the version `path`
/// records for it.
///
/// A missing file is not an error: nothing is recorded, so every shape is at the
/// legacy version and reads as it always has. A file that cannot be parsed
/// reports every shape as unreadable rather than inventing versions for it.
pub fn report(path: &Path) -> Vec<ShapeStatus> {
    let doc = match std::fs::read_to_string(path) {
        Ok(raw) => raw.parse::<DocumentMut>().ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(DocumentMut::new()),
        Err(_) => None,
    };
    PersistedShape::all()
        .map(|shape| {
            let table = shape.table();
            let recorded = doc
                .as_ref()
                .and_then(|doc| recorded_version(doc, shape).ok());
            ShapeStatus {
                shape,
                table,
                recorded,
                current: shape.current_version(),
                oldest_migratable: shape.oldest_migratable_version(),
                decision: recorded.map(|recorded| shape.decision(recorded)),
                present: doc.as_ref().is_some_and(|doc| {
                    doc.get(table)
                        .is_some_and(|item| item.as_table_like().is_some())
                }),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "schema_report_tests.rs"]
mod tests;
