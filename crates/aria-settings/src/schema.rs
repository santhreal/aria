//! Schema versions for the shapes `config.toml` persists.
//!
//! Loading `config.toml` is tolerant: an absent key keeps its default, so a
//! file written by an older build still opens and a hand-edited file still
//! loads. Tolerance covers absence, not meaning. When the meaning of a table
//! changes, a file written before the change must not be read as if it had been
//! written after it. Every table this crate owns is therefore a
//! [`PersistedShape`] with a version, and the version each shape has in a file
//! is recorded in one table:
//!
//! ```toml
//! [schema]
//! ai = 2
//! appearance = 1
//! ```
//!
//! A shape whose version the file does not record is at [`LEGACY_VERSION`]:
//! the shape as it was before versions were written. Loading migrates a
//! recorded version below the current one, and refuses one this build cannot
//! read — a version above the current one, a version older than the shape
//! migrates from, or a record that is not a version number. A refused shape is
//! not served: [`load`](crate::load) returns an error naming the table and the
//! versions, so the caller surfaces the problem or falls back to defaults.
//! [`save`](crate::save) refuses the same file rather than overwriting a
//! configuration it cannot read.
//!
//! The version is recorded for a shape whose table is present in the file, and
//! dropped for one whose table is not, so a configuration that holds nothing
//! but defaults still holds no empty sections.

use toml_edit::{DocumentMut, Item, TableLike};

/// The table recording the schema version of every persisted shape.
pub const SCHEMA_TABLE: &str = "schema";

/// The version of a shape whose version the file does not record: the shape as
/// it was before `[schema]` was written.
pub const LEGACY_VERSION: u32 = 1;

/// One versioned shape in `config.toml`.
///
/// A row is a table name, the version this build reads and writes, and the
/// oldest version this build migrates. Those three decide how every stale file
/// of that shape is read, so a table cannot be persisted without a recorded
/// version and a decision for its stale copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PersistedShape {
    /// The `[ai]` table.
    Ai,
    /// The `[appearance]` table.
    Appearance,
    /// The `[cloud]` table.
    Cloud,
    /// The `[plugins]` table.
    Plugins,
    /// The `[providers]` table and its per-provider children.
    Providers,
    /// The `[storage]` table and its `[storage.paths]` child.
    Storage,
    /// The `[transcription]` table.
    Transcription,
}

impl PersistedShape {
    /// The first shape in registry order.
    pub const FIRST: Self = Self::Ai;

    /// The next shape in registry order, or `None` after the last one.
    ///
    /// [`all`](Self::all) walks this, so the enumeration is the enum itself: a
    /// new variant makes this match non-exhaustive and the crate stops
    /// compiling until the variant is placed in the order.
    fn successor(self) -> Option<Self> {
        Some(match self {
            Self::Ai => Self::Appearance,
            Self::Appearance => Self::Cloud,
            Self::Cloud => Self::Plugins,
            Self::Plugins => Self::Providers,
            Self::Providers => Self::Storage,
            Self::Storage => Self::Transcription,
            Self::Transcription => return None,
        })
    }

    /// Every shape, in registry order.
    pub fn all() -> impl Iterator<Item = Self> {
        std::iter::successors(Some(Self::FIRST), |shape| shape.successor())
    }

    /// The top-level table the shape owns, which is also its key in
    /// [`SCHEMA_TABLE`].
    pub fn table(self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Appearance => "appearance",
            Self::Cloud => "cloud",
            Self::Plugins => "plugins",
            Self::Providers => "providers",
            Self::Storage => "storage",
            Self::Transcription => "transcription",
        }
    }

    /// The version this build reads and writes.
    pub fn current_version(self) -> u32 {
        match self {
            // Version 2 records `source` explicitly. In version 1 there was no
            // `source` key and a base URL with a model meant the freeform
            // endpoint; in version 2 an absent `source` means the default local
            // server, which is what a Local configuration writes.
            Self::Ai => 2,
            // Version 2 keeps the API host in `[cloud]`. Version 1 kept it in
            // `[account].base_url`, alongside managed-account keys that no
            // longer exist.
            Self::Cloud => 2,
            Self::Appearance
            | Self::Plugins
            | Self::Providers
            | Self::Storage
            | Self::Transcription => 1,
        }
    }

    /// The oldest recorded version this build migrates to the current shape.
    /// A file recording anything older is refused rather than served.
    pub fn oldest_migratable_version(self) -> u32 {
        match self {
            Self::Ai
            | Self::Appearance
            | Self::Cloud
            | Self::Plugins
            | Self::Providers
            | Self::Storage
            | Self::Transcription => LEGACY_VERSION,
        }
    }

    /// What loading does with the version `recorded` for this shape.
    pub fn decision(self, recorded: u32) -> VersionDecision {
        let current = self.current_version();
        if recorded == current {
            return VersionDecision::Current;
        }
        if recorded > current {
            return VersionDecision::Refuse(RefusalReason::FromNewerBuild);
        }
        if recorded < self.oldest_migratable_version() {
            return VersionDecision::Refuse(RefusalReason::TooOld);
        }
        VersionDecision::Migrate { from: recorded }
    }
}

/// What loading does with the version a file records for one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionDecision {
    /// The recorded version is the current one; the table is read as written.
    Current,
    /// The recorded version is older; the table is read through its migration.
    Migrate {
        /// The version the file records.
        from: u32,
    },
    /// The recorded version cannot be read; the shape is not served.
    Refuse(RefusalReason),
}

impl VersionDecision {
    /// Whether the table must be read through its migration path.
    pub fn is_migration(self) -> bool {
        matches!(self, Self::Migrate { .. })
    }
}

/// Why a recorded version is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// The version is above the one this build reads.
    FromNewerBuild,
    /// The version is below the oldest one this build migrates.
    TooOld,
}

/// The `[schema]` table, accepting both the standard and the inline form.
fn version_table(doc: &DocumentMut) -> Result<Option<&dyn TableLike>, String> {
    match doc.get(SCHEMA_TABLE) {
        None => Ok(None),
        Some(item) => item.as_table_like().map(Some).ok_or_else(|| {
            format!(
                "[{SCHEMA_TABLE}] is not a table, so no persisted setting has a readable version"
            )
        }),
    }
}

/// The version `doc` records for `shape`, or [`LEGACY_VERSION`] when it records
/// none. A record that is not a version number this build can read is an error
/// rather than a fallback: it decides how the table is interpreted.
pub(crate) fn recorded_version(doc: &DocumentMut, shape: PersistedShape) -> Result<u32, String> {
    let Some(table) = version_table(doc)? else {
        return Ok(LEGACY_VERSION);
    };
    let Some(item) = table.get(shape.table()) else {
        return Ok(LEGACY_VERSION);
    };
    item.as_integer()
        .and_then(|recorded| u32::try_from(recorded).ok())
        .ok_or_else(|| {
            format!(
                "[{SCHEMA_TABLE}].{} is not a schema version this build can read",
                shape.table()
            )
        })
}

/// The decision for one shape in `doc`, or the reason it is refused.
pub(crate) fn shape_decision(
    doc: &DocumentMut,
    shape: PersistedShape,
) -> Result<VersionDecision, String> {
    let recorded = recorded_version(doc, shape)?;
    match shape.decision(recorded) {
        VersionDecision::Refuse(reason) => Err(refusal(shape, recorded, reason)),
        decision => Ok(decision),
    }
}

/// Refuse every shape in `doc` whose recorded version this build cannot read.
///
/// Every shape is checked, including one whose table is absent: a record that
/// names a version this build does not have describes the whole file, not only
/// the sections that happen to be populated.
pub(crate) fn validate(doc: &DocumentMut) -> Result<(), String> {
    for shape in PersistedShape::all() {
        shape_decision(doc, shape)?;
    }
    Ok(())
}

fn refusal(shape: PersistedShape, recorded: u32, reason: RefusalReason) -> String {
    match reason {
        RefusalReason::FromNewerBuild => format!(
            "[{}] records schema version {recorded}, and this build reads version {}",
            shape.table(),
            shape.current_version()
        ),
        RefusalReason::TooOld => format!(
            "[{}] records schema version {recorded}, and this build migrates version {} and newer",
            shape.table(),
            shape.oldest_migratable_version()
        ),
    }
}

/// Record the current version of every shape whose table `doc` holds, and drop
/// the record of every shape whose table it does not.
///
/// Called after the owned tables are patched, so the record follows what the
/// file actually contains.
pub(crate) fn record_versions(doc: &mut DocumentMut) {
    let present: Vec<(&'static str, i64)> = PersistedShape::all()
        .filter(|shape| doc.contains_key(shape.table()))
        .map(|shape| (shape.table(), i64::from(shape.current_version())))
        .collect();
    let absent: Vec<&'static str> = PersistedShape::all()
        .filter(|shape| !doc.contains_key(shape.table()))
        .map(PersistedShape::table)
        .collect();

    if !present.is_empty() {
        let table = crate::ensure_table(doc, SCHEMA_TABLE);
        for (key, version) in present {
            crate::set_integer(table, key, version);
        }
    }
    if let Some(table) = doc.get_mut(SCHEMA_TABLE).and_then(Item::as_table_mut) {
        for key in absent {
            table.remove(key);
        }
        if table.is_empty() {
            doc.remove(SCHEMA_TABLE);
        }
    }
}

#[cfg(test)]
mod stale_tests;
