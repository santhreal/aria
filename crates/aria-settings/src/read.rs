//! Reading the config document into [`Settings`], and reporting what it drops.
//!
//! Loading is tolerant: a hand edit must never stop the editor opening, so a
//! value this build cannot use keeps the default. Tolerant used to mean silent,
//! and silence was the defect. `root = "cache"` under `[storage]` is a relative
//! path, which resolves against wherever the process was started rather than
//! anywhere the file can mean; the loader dropped it and returned no root, so
//! the file said the caches had moved and the editor kept writing them under
//! the profile directory with nothing to read anywhere.
//!
//! So every read of an owned key goes through one of the helpers below, and
//! each one records what it stepped over in [`Rejected`]. The record travels on
//! the value, not through the signature: [`Settings::rejected`] is a field, so
//! no caller of [`crate::load`] had to change to carry diagnostics it does not
//! surface, and the reader that does surface them
//! (`aria-ui`'s storage report) reads them off the settings it already holds.
//!
//! The rule is enforced rather than remembered: a test reads this file and
//! fails if [`Settings::from_document`] pulls a value out of a table itself
//! instead of asking a helper.

use std::path::PathBuf;

use toml_edit::{DocumentMut, Item, TableLike};

use crate::schema::{self, PersistedShape};
use crate::{
    AiApiProtocol, AiSource, Autonomy, PluginLimitSettings, ProviderSettings, ReasoningSummary,
    Settings, StoragePathOverrides, StorageSettings, ThemeChoice, TranscriptionSettings,
};

/// A value the config file held that this build could not use.
///
/// The load succeeded and the key kept its default; this is the record of what
/// was in the file so the operator can be told which line of their own config
/// had no effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The dotted key, written the way the file writes it:
    /// `storage.paths.proxies`.
    pub key: String,
    /// What the file held, rendered as TOML: `"cache"`, `3`, `a table`.
    pub value: String,
    /// Why this build could not use it.
    pub why: Refusal,
}

impl Rejected {
    /// One line for an operator, naming the key, the value, and the reason.
    pub fn line(&self) -> String {
        format!("{} = {}: {}", self.key, self.value, self.why.reason())
    }
}

/// Why a value in the config file could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not the kind of value the key takes: `model = 3`.
    WrongType,
    /// A key with a fixed set of names holding something outside it:
    /// `theme = "purple"`.
    NotOneOf,
    /// A path key holding a relative path.
    NotAbsolute,
    /// A number outside the range the key accepts.
    OutOfRange,
}

impl Refusal {
    /// Every refusal, so a surface that formats one covers all of them.
    pub const ALL: [Refusal; 4] = [
        Refusal::WrongType,
        Refusal::NotOneOf,
        Refusal::NotAbsolute,
        Refusal::OutOfRange,
    ];

    /// The half-sentence that follows the value in a report.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::WrongType => "is not the kind of value this key takes",
            Refusal::NotOneOf => "is not one of the values this key accepts",
            Refusal::NotAbsolute => {
                "is not an absolute path, so it would resolve differently \
                 depending on where the editor was started"
            }
            Refusal::OutOfRange => "is outside the range this key accepts",
        }
    }
}

impl Settings {
    /// Read every owned table out of `doc`.
    ///
    /// Refuses a shape whose recorded version this build cannot read, and reads
    /// a shape recording an older version through its migration. A value this
    /// build cannot use keeps its default and lands in
    /// [`Settings::rejected`].
    pub(crate) fn from_document(doc: &DocumentMut) -> Result<Self, String> {
        schema::validate(doc)?;
        let ai_version = schema::shape_decision(doc, PersistedShape::Ai)?;
        let cloud_version = schema::shape_decision(doc, PersistedShape::Cloud)?;
        let mut s = Settings::default();
        let out = &mut s.rejected;

        if let Some(t) = section_at(doc, "ai", out) {
            if let Some(v) = text_at(t, "base_url", "ai", out) {
                s.ai.base_url = v;
            }
            if let Some(v) = text_at(t, "model", "ai", out) {
                s.ai.model = v;
            }
            if let Some(protocol) = choice_at(t, "api_protocol", "ai", out, AiApiProtocol::from_key)
            {
                s.ai.api_protocol = protocol;
            }
            if let Some(summary) = choice_at(
                t,
                "reasoning_summary",
                "ai",
                out,
                ReasoningSummary::from_key,
            ) {
                s.ai.reasoning_summary = summary;
            }
            s.ai.api_key = text_at(t, "api_key", "ai", out);
            s.ai.api_key_env = text_at(t, "api_key_env", "ai", out);
            // Legacy `use_account` is ignored (Aria account removed).
            if let Some(autonomy) = choice_at(t, "autonomy", "ai", out, Autonomy::from_key) {
                s.ai.autonomy = autonomy;
            }
            // Explicit `source` wins. Version 1 had no `source` key, so a
            // version 1 config with URL+model was the freeform endpoint; from
            // version 2 an absent `source` is the Local default, which is what
            // a Local configuration writes.
            if let Some(source) = choice_at(t, "source", "ai", out, AiSource::from_key) {
                s.ai.source = source;
            } else if ai_version.is_migration()
                && !s.ai.base_url.trim().is_empty()
                && !s.ai.model.trim().is_empty()
            {
                s.ai.source = AiSource::Custom;
            }
        }

        if let Some(t) = section_at(doc, "appearance", out)
            && let Some(theme) = choice_at(t, "theme", "appearance", out, ThemeChoice::from_key)
        {
            s.appearance.theme = theme;
        }

        if let Some(t) = section_at(doc, "storage", out) {
            s.storage.root = absolute_path_at(t, "root", "storage", out);
            if let Some(quota) = bounded_at(
                t,
                "download_quota_mib",
                "storage",
                out,
                StorageSettings::is_valid_download_quota_mib,
            ) {
                s.storage.download_quota_mib = quota;
            }

            if let Some(paths) = table_at(t, "paths", "storage", out) {
                // Driven by the key list, so an override added to
                // `StoragePathOverrides` cannot be writable and unreadable.
                for key in StoragePathOverrides::KEYS {
                    if let Some(slot) = s.storage.paths.slot(key) {
                        *slot = absolute_path_at(paths, key, "storage.paths", out);
                    }
                }
            }
        }

        if let Some(t) = section_at(doc, "transcription", out) {
            // An out-of-range value keeps the default rather than failing the load: a
            // hand-edited zero must not make the editor refuse to open.
            for (key, slot) in [
                (
                    "connect_timeout_secs",
                    &mut s.transcription.connect_timeout_secs,
                ),
                ("read_timeout_secs", &mut s.transcription.read_timeout_secs),
                (
                    "total_timeout_secs",
                    &mut s.transcription.total_timeout_secs,
                ),
            ] {
                if let Some(secs) = bounded_at(
                    t,
                    key,
                    "transcription",
                    out,
                    TranscriptionSettings::is_valid_timeout_secs,
                ) {
                    *slot = secs;
                }
            }
        }

        if let Some(t) = section_at(doc, "plugin_limits", out) {
            for (key, slot, ok) in [
                (
                    "wasm_fuel_per_call",
                    &mut s.plugin_limits.wasm_fuel_per_call,
                    PluginLimitSettings::is_valid_fuel_per_call as fn(u64) -> bool,
                ),
                (
                    "wasm_memory_mib",
                    &mut s.plugin_limits.wasm_memory_mib,
                    PluginLimitSettings::is_valid_memory_mib,
                ),
            ] {
                if let Some(value) = bounded_at(t, key, "plugin_limits", out, ok) {
                    *slot = value;
                }
            }
        }

        if let Some(t) = section_at(doc, "providers", out) {
            for (name, item) in t.iter() {
                let at = format!("providers.{name}");
                if let Some(entry) = table_item(item, at.clone(), out) {
                    s.providers.insert(
                        name.to_string(),
                        ProviderSettings {
                            api_key: text_at(entry, "api_key", &at, out),
                            api_key_env: text_at(entry, "api_key_env", &at, out),
                        },
                    );
                }
            }
        }

        // Version 2 keeps the host in `[cloud]`. A version 1 file kept it in
        // `[account].base_url`, so that table is read only while migrating; in
        // a current file an `[account]` table belongs to something else.
        if let Some(t) = section_at(doc, "cloud", out) {
            if let Some(v) = text_at(t, "base_url", "cloud", out) {
                s.cloud.base_url = v;
            }
        } else if cloud_version.is_migration()
            && let Some(t) = section_at(doc, "account", out)
            && let Some(v) = text_at(t, "base_url", "account", out)
        {
            s.cloud.base_url = v;
        }

        // Plugin-declared values. Read as opaque strings: this crate cannot know
        // what a plugin's key means, and a non-string value is a hand edit the
        // declaring plugin never wrote.
        if let Some(t) = section_at(doc, "plugins", out) {
            for (key, item) in t.iter() {
                if let Some(value) = text_item(item, dotted("plugins", key), out) {
                    s.plugins.insert(key.to_string(), value.to_string());
                }
            }
        }

        Ok(s)
    }
}

// --- reading helpers --------------------------------------------------------
//
// Every one of these reports what it could not use. Nothing in
// `from_document` reads a table itself, and a test enforces that.

/// The dotted key a section's child is written as.
fn dotted(at: &str, key: &str) -> String {
    format!("{at}.{key}")
}

/// Record a value that was present and unusable.
fn refuse(out: &mut Vec<Rejected>, key: String, value: String, why: Refusal) {
    out.push(Rejected { key, value, why });
}

/// What an item held, rendered the way the file writes it, so the operator can
/// match the report against a line in their own config.
fn item_text(item: &Item) -> String {
    if let Some(text) = item.as_str() {
        return format!("{text:?}");
    }
    if item.is_table_like() {
        return "a table".to_owned();
    }
    if item.is_array() || item.is_array_of_tables() {
        return "a list".to_owned();
    }
    item.to_string().trim().to_owned()
}

/// A string item, or nothing plus a rejection when it holds another kind.
fn text_item<'i>(item: &'i Item, key: String, out: &mut Vec<Rejected>) -> Option<&'i str> {
    match item.as_str() {
        Some(text) => Some(text),
        None => {
            refuse(out, key, item_text(item), Refusal::WrongType);
            None
        }
    }
}

/// A table item, standard or inline, or nothing plus a rejection.
fn table_item<'i>(
    item: &'i Item,
    key: String,
    out: &mut Vec<Rejected>,
) -> Option<&'i dyn TableLike> {
    match item.as_table_like() {
        Some(table) => Some(table),
        None => {
            refuse(out, key, item_text(item), Refusal::WrongType);
            None
        }
    }
}

/// A whole number inside the range the owning type accepts. A negative or
/// oversized number is out of range, not the wrong type: the file did write a
/// number.
fn int_item(item: &Item, key: String, out: &mut Vec<Rejected>, ok: fn(u64) -> bool) -> Option<u64> {
    let Some(raw) = item.as_integer() else {
        refuse(out, key, item_text(item), Refusal::WrongType);
        return None;
    };
    match u64::try_from(raw).ok().filter(|v| ok(*v)) {
        Some(v) => Some(v),
        None => {
            refuse(out, key, item_text(item), Refusal::OutOfRange);
            None
        }
    }
}

/// A top-level table of the document.
fn section_at<'d>(
    doc: &'d DocumentMut,
    key: &str,
    out: &mut Vec<Rejected>,
) -> Option<&'d dyn TableLike> {
    table_item(doc.as_table().get(key)?, key.to_owned(), out)
}

/// A nested table of a section.
fn table_at<'t>(
    table: &'t dyn TableLike,
    key: &str,
    at: &str,
    out: &mut Vec<Rejected>,
) -> Option<&'t dyn TableLike> {
    table_item(table.get(key)?, dotted(at, key), out)
}

/// The string at `key`.
fn text_at(table: &dyn TableLike, key: &str, at: &str, out: &mut Vec<Rejected>) -> Option<String> {
    text_item(table.get(key)?, dotted(at, key), out).map(str::to_owned)
}

/// One of a fixed set of names, parsed by the type that owns the set.
fn choice_at<T>(
    table: &dyn TableLike,
    key: &str,
    at: &str,
    out: &mut Vec<Rejected>,
    parse: fn(&str) -> Option<T>,
) -> Option<T> {
    let text = text_at(table, key, at, out)?;
    match parse(&text) {
        Some(value) => Some(value),
        None => {
            refuse(out, dotted(at, key), format!("{text:?}"), Refusal::NotOneOf);
            None
        }
    }
}

/// An absolute path. A relative one is refused rather than resolved: the file
/// cannot mean "next to wherever the editor was started". An empty value is
/// "unset", which the writer produces for a cleared field, so it is not a
/// rejection.
fn absolute_path_at(
    table: &dyn TableLike,
    key: &str,
    at: &str,
    out: &mut Vec<Rejected>,
) -> Option<PathBuf> {
    let raw = text_at(table, key, at, out)?;
    if raw.trim().is_empty() {
        return None;
    }
    let path = PathBuf::from(&raw);
    if path.is_absolute() {
        return Some(path);
    }
    refuse(
        out,
        dotted(at, key),
        format!("{raw:?}"),
        Refusal::NotAbsolute,
    );
    None
}

/// A whole number at `key` inside the range `ok` accepts.
fn bounded_at(
    table: &dyn TableLike,
    key: &str,
    at: &str,
    out: &mut Vec<Rejected>,
    ok: fn(u64) -> bool,
) -> Option<u64> {
    int_item(table.get(key)?, dotted(at, key), out, ok)
}
