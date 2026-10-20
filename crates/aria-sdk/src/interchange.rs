//! Timeline importers and exporters: their specifications and what a run
//! returns.

use serde::{Deserialize, Serialize};

/// Specification of a timeline importer registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterSpec {
    /// Not empty, and unique among importers and exporters.
    pub id: String,
    /// The format as a menu lists it, such as `CMX 3600 EDL`.
    pub name: String,
    /// The file extensions it reads, without the dot: at least one, each ASCII
    /// letters and digits. Matched without regard to case.
    pub extensions: Vec<String>,
}

/// Specification of a timeline exporter registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExporterSpec {
    /// Not empty, and unique among importers and exporters.
    pub id: String,
    /// The format as a menu lists it.
    pub name: String,
    /// The extension of the file it writes, without the dot: ASCII letters and
    /// digits, stored lowercase.
    pub extension: String,
}

/// Result returned by [`Plugin::import`](crate::Plugin::import).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResult {
    /// Serde JSON of `Vec<aria_commands::Command>`, applied by the host as one transaction.
    pub commands_json: String,
    /// Items in the source file that the importer skipped.
    #[serde(default)]
    pub skipped: Vec<String>,
}

impl ImportResult {
    /// Build an import result from serialized commands JSON and skipped lines.
    pub fn new(commands_json: impl Into<String>, skipped: Vec<String>) -> Self {
        Self {
            commands_json: commands_json.into(),
            skipped,
        }
    }
}

/// Report returned by [`Plugin::export`](crate::Plugin::export).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReport {
    /// Items in the project that the exported format could not represent.
    #[serde(default)]
    pub skipped: Vec<String>,
}
