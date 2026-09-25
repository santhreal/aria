//! Writing [`Settings`] back into the config document.
//!
//! Format-preserving by construction: the document is the one parsed from
//! disk, only the keys this crate owns are touched, and an unchanged value is
//! left alone so its inline comment and spacing survive. Everything a newer
//! build wrote stays where it was.
//!
//! Values are validated before anything is patched. A relative storage path or
//! an out-of-range quota is refused by name and the file is left exactly as it
//! was: a half-patched config is worse than a rejected save. That is the
//! counterpart to the loader, which keeps the default and reports the value it
//! could not use (see [`crate::Rejected`]).

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, value};

use crate::{
    AiApiProtocol, AiSource, Autonomy, DEFAULT_DOWNLOAD_QUOTA_MIB, MAX_DOWNLOAD_QUOTA_MIB,
    MAX_DOWNLOAD_TIMEOUT_SECS, MIN_DOWNLOAD_QUOTA_MIB, ReasoningSummary, Settings,
    StoragePathOverrides, StorageSettings, TranscriptionSettings, schema,
};

impl Settings {
    pub(crate) fn write_into(&self, doc: &mut DocumentMut) -> Result<(), String> {
        if !StorageSettings::is_valid_download_quota_mib(self.storage.download_quota_mib) {
            return Err(format!(
                "storage.download_quota_mib must be between \
                 {MIN_DOWNLOAD_QUOTA_MIB} and {MAX_DOWNLOAD_QUOTA_MIB} MiB"
            ));
        }
        let storage_root = storage_path_for_save(self.storage.root.as_deref(), "root")?;
        // Validated in key order, and every value is refused before anything is
        // written: a relative path in one override must not leave the file half
        // patched. The list is the registry, so an override cannot be readable
        // and unsaveable.
        let mut storage_paths = Vec::with_capacity(StoragePathOverrides::KEYS.len());
        for key in StoragePathOverrides::KEYS {
            let value =
                storage_path_for_save(self.storage.paths.get(key), &format!("paths.{key}"))?;
            storage_paths.push((key, value));
        }

        {
            let t = ensure_table(doc, "ai");
            if self.ai.source == AiSource::default() {
                t.remove("source");
            } else {
                set_str(t, "source", self.ai.source.key());
            }
            set_str(t, "base_url", &self.ai.base_url);
            set_str(t, "model", &self.ai.model);
            if self.ai.api_protocol == AiApiProtocol::default() {
                t.remove("api_protocol");
            } else {
                set_str(t, "api_protocol", self.ai.api_protocol.key());
            }
            if self.ai.reasoning_summary == ReasoningSummary::default() {
                t.remove("reasoning_summary");
            } else {
                set_str(t, "reasoning_summary", self.ai.reasoning_summary.key());
            }
            set_optional(t, "api_key", self.ai.api_key.as_deref());
            set_optional(t, "api_key_env", self.ai.api_key_env.as_deref());
            // Drop legacy managed-account key if present.
            t.remove("use_account");
            // Default autonomy is absence, so a fresh config stays minimal.
            if self.ai.autonomy == Autonomy::default() {
                t.remove("autonomy");
            } else {
                set_str(t, "autonomy", self.ai.autonomy.key());
            }
        }
        {
            let t = ensure_table(doc, "appearance");
            set_str(t, "theme", self.appearance.theme.key());
        }
        {
            let has_path_overrides = storage_paths.iter().any(|(_, value)| value.is_some());
            let has_storage_values = storage_root.is_some()
                || self.storage.download_quota_mib != DEFAULT_DOWNLOAD_QUOTA_MIB
                || has_path_overrides;

            if has_storage_values {
                ensure_table(doc, "storage");
            }
            if let Some(t) = doc.get_mut("storage").and_then(Item::as_table_mut) {
                set_optional(t, "root", storage_root);
                if self.storage.download_quota_mib == DEFAULT_DOWNLOAD_QUOTA_MIB {
                    t.remove("download_quota_mib");
                } else {
                    set_integer(
                        t,
                        "download_quota_mib",
                        self.storage.download_quota_mib as i64,
                    );
                }

                if has_path_overrides {
                    ensure_child_table(t, "paths");
                }
                let remove_paths =
                    if let Some(paths) = t.get_mut("paths").and_then(Item::as_table_mut) {
                        for (key, value) in storage_paths {
                            set_optional(paths, key, value);
                        }
                        paths.is_empty()
                    } else {
                        false
                    };
                if remove_paths {
                    t.remove("paths");
                }
            }

            let remove_storage = doc
                .get("storage")
                .and_then(Item::as_table)
                .is_some_and(Table::is_empty);
            if remove_storage {
                doc.remove("storage");
            }
        }
        {
            // Only write providers we hold; hand-added entries under
            // `[providers.*]` that we loaded are re-written unchanged, and
            // ones we never parsed (non-table junk) are left alone.
            for (name, provider) in &self.providers {
                let t = ensure_table(doc, "providers");
                if t.get(name).and_then(Item::as_table).is_none() {
                    t.insert(name, Item::Table(Table::new()));
                }
                let entry = t
                    .get_mut(name)
                    .and_then(Item::as_table_mut)
                    .expect("provider table ensured above");
                set_optional(entry, "api_key", provider.api_key.as_deref());
                set_optional(entry, "api_key_env", provider.api_key_env.as_deref());
            }
            // Dropping a provider from the map removes its table.
            if let Some(t) = doc.get_mut("providers").and_then(Item::as_table_mut) {
                let stale: Vec<String> = t
                    .iter()
                    .filter(|(name, item)| {
                        item.as_table().is_some() && !self.providers.contains_key(*name)
                    })
                    .map(|(name, _)| name.to_string())
                    .collect();
                for name in stale {
                    t.remove(&name);
                }
                if t.is_empty() {
                    doc.remove("providers");
                }
            }
        }
        {
            // Empty override removes the key (and a now-empty table).
            // Migrate off legacy `[account]` when writing.
            doc.remove("account");
            if self.cloud.base_url.is_empty() {
                if let Some(t) = doc.get_mut("cloud").and_then(Item::as_table_mut) {
                    t.remove("base_url");
                }
            } else {
                let t = ensure_table(doc, "cloud");
                set_str(t, "base_url", &self.cloud.base_url);
            }
            if let Some(t) = doc.get_mut("cloud").and_then(Item::as_table_mut)
                && t.is_empty()
            {
                doc.remove("cloud");
            }
        }
        {
            // Refuse an invalid timeout at the save rather than writing a file the
            // downloader will reject: the value came from a control that can clamp.
            for (key, value) in [
                (
                    "connect_timeout_secs",
                    self.transcription.connect_timeout_secs,
                ),
                ("read_timeout_secs", self.transcription.read_timeout_secs),
                ("total_timeout_secs", self.transcription.total_timeout_secs),
            ] {
                if !TranscriptionSettings::is_valid_timeout_secs(value) {
                    return Err(format!(
                        "transcription.{key} must be between 1 and {MAX_DOWNLOAD_TIMEOUT_SECS} seconds"
                    ));
                }
            }
            let defaults = TranscriptionSettings::default();
            if self.transcription == defaults {
                doc.remove("transcription");
            } else {
                let t = ensure_table(doc, "transcription");
                set_integer(
                    t,
                    "connect_timeout_secs",
                    self.transcription.connect_timeout_secs as i64,
                );
                set_integer(
                    t,
                    "read_timeout_secs",
                    self.transcription.read_timeout_secs as i64,
                );
                set_integer(
                    t,
                    "total_timeout_secs",
                    self.transcription.total_timeout_secs as i64,
                );
            }
        }
        {
            // Plugin values. This crate owns only the string entries under
            // `[plugins]`: a string key dropped from the map is removed from the
            // file so clearing a field in the dialog does not leave the old value
            // behind, while a non-string entry is somebody else's and is left
            // exactly as written. The table itself is removed only when it would
            // otherwise be left empty.
            let owned_keys: Vec<String> = doc
                .get("plugins")
                .and_then(|item| item.as_table_like())
                .map(|t| {
                    t.iter()
                        .filter(|(key, item)| {
                            item.as_str().is_some() && !self.plugins.contains_key(*key)
                        })
                        .map(|(key, _)| key.to_string())
                        .collect()
                })
                .unwrap_or_default();
            if !owned_keys.is_empty() || !self.plugins.is_empty() {
                let t = ensure_table(doc, "plugins");
                for key in owned_keys {
                    t.remove(&key);
                }
                for (key, value) in &self.plugins {
                    set_str(t, key, value);
                }
            }
            if doc
                .get("plugins")
                .and_then(|item| item.as_table_like())
                .is_some_and(|t| t.is_empty())
            {
                doc.remove("plugins");
            }
        }
        // Last, so the record follows the tables the file ends up with.
        schema::record_versions(doc);
        Ok(())
    }
}

// --- toml_edit helpers ------------------------------------------------------

pub(crate) fn storage_path_for_save<'a>(
    path: Option<&'a Path>,
    key: &str,
) -> Result<Option<&'a str>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.is_absolute() {
        return Err(format!("storage.{key} must be an absolute path"));
    }
    path.to_str()
        .map(Some)
        .ok_or_else(|| format!("storage.{key} is not valid UTF-8 and cannot be written to TOML"))
}

/// Borrow (creating if absent or if the existing item isn't a table) the
/// named top-level table. Replacing a non-table is the only way a corrupt
/// hand-edit (`ai = 3`) could otherwise wedge the writer.
pub(crate) fn ensure_table<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Table {
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc.insert(key, Item::Table(Table::new()));
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .expect("table ensured above")
}

/// Borrow a real child table, replacing a wrongly-typed item only when a
/// caller has a value that must be written there.
pub(crate) fn ensure_child_table<'a>(table: &'a mut Table, key: &str) -> &'a mut Table {
    if table.get(key).and_then(Item::as_table).is_none() {
        table.insert(key, Item::Table(Table::new()));
    }
    table
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .expect("child table ensured above")
}

/// Write a string only when it differs from what's there. Skipping an
/// unchanged key leaves its decor (inline comments, spacing) untouched — the
/// core of the format-preserving promise.
pub(crate) fn set_str(table: &mut Table, key: &str, v: &str) {
    if table.get(key).and_then(Item::as_str) != Some(v) {
        table[key] = value(v);
    }
}

pub(crate) fn set_integer(table: &mut Table, key: &str, v: i64) {
    if table.get(key).and_then(Item::as_integer) != Some(v) {
        table[key] = value(v);
    }
}

/// Set `key` to `val`, or remove it entirely when `None`, so a cleared field
/// leaves no stale literal behind.
pub(crate) fn set_optional(table: &mut Table, key: &str, val: Option<&str>) {
    match val {
        Some(v) => set_str(table, key, v),
        None => {
            table.remove(key);
        }
    }
}
