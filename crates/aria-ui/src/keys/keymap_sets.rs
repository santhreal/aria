//! Named key sets: `<data dir>/keymaps/<name>.toml`.
//!
//! A set holds rebindings by binding id, the same rows `ui-settings.json` holds
//! under `keys`. Only the rows that differ from the shipped chords are written,
//! so a set saved by one build loads in the next, and a shipped chord changed
//! between the two is the new one.
//!
//! ```toml
//! version = 1
//!
//! [keys]
//! key-play = "Ctrl+Space"
//! key-tool-razor = ""
//! ```
//!
//! An empty chord is a binding cleared on purpose. A file with another version,
//! a field this format does not have, or a chord no bindable key carries is
//! rejected whole with the reason, because half a set is a keyboard nobody
//! chose. A binding id this build does not have is skipped when the set is
//! installed, and reported.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Chord, KeyHost, KeyOverrideError};

/// The directory under the data dir that holds named sets.
pub const KEYMAPS_DIR: &str = "keymaps";

/// The file format this build reads and writes.
pub const KEYMAP_VERSION: u32 = 1;

/// A set file larger than this is refused unread. Every shipped binding id with
/// its longest chord fits in a twentieth of it.
const MAX_KEYMAP_BYTES: u64 = 256 * 1024;

/// The longest set name, in bytes.
const MAX_NAME_BYTES: usize = 64;

/// Rebindings by binding id. `None` is a binding cleared on purpose.
pub type KeySet = BTreeMap<String, Option<Chord>>;

/// The file as written. `version` is optional here only so that a file without
/// one is refused with its own reason rather than a parser message.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    version: Option<u32>,
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

/// Why one row of a set was not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// No binding by that id in this build: a plugin that is not installed, or
    /// a binding that was renamed.
    Unknown,
    /// Another binding on the same route holds the chord, and that binding is
    /// not moved by the set.
    Taken { chord: Chord, held_by: &'static str },
}

/// One row of a set that was not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetSkip {
    pub id: String,
    pub reason: SkipReason,
}

/// Validate a set name and return `<data dir>/keymaps/<name>.toml`.
pub fn keymap_path(data_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a keymap needs a name".to_owned());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "a keymap name is at most {MAX_NAME_BYTES} bytes; `{name}` is {}",
            name.len()
        ));
    }
    if name.starts_with('.')
        || name
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | ':') || ch.is_control())
    {
        return Err(format!(
            "`{name}` cannot name a file: drop the `/`, `\\`, `:` and leading `.`"
        ));
    }
    Ok(data_dir.join(KEYMAPS_DIR).join(format!("{name}.toml")))
}

/// The sets in `<data dir>/keymaps`, by name, sorted without regard to case.
///
/// A missing directory holds none. A file whose stem cannot name a set is not
/// listed, because nothing could load it by that name.
pub fn keymap_names(data_dir: &Path) -> Result<Vec<String>, String> {
    let dir = data_dir.join(KEYMAPS_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("could not list {}: {err}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|err| format!("could not list {}: {err}", dir.display()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if keymap_path(data_dir, stem).is_ok() {
            names.push(stem.to_owned());
        }
    }
    names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
    Ok(names)
}

/// Read a set from the text of a keymap file.
pub fn parse_keymap(text: &str) -> Result<KeySet, String> {
    let file: KeymapFile =
        toml::from_str(text).map_err(|err| format!("not a keymap file: {err}"))?;
    match file.version {
        Some(KEYMAP_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "keymap version {other} is not one this build reads; it reads version {KEYMAP_VERSION}"
            ));
        }
        None => {
            return Err(format!(
                "the keymap has no version: add `version = {KEYMAP_VERSION}` as its first line"
            ));
        }
    }
    let mut set = KeySet::new();
    for (id, text) in file.keys {
        let chord = if text.trim().is_empty() {
            None
        } else {
            Some(Chord::parse(&text).ok_or_else(|| {
                format!("`{id}` is bound to `{text}`, which names no key a binding can use")
            })?)
        };
        set.insert(id, chord);
    }
    Ok(set)
}

/// The text of a keymap file holding `set`.
pub fn render_keymap(set: &KeySet) -> Result<String, String> {
    let file = KeymapFile {
        version: Some(KEYMAP_VERSION),
        keys: set
            .iter()
            .map(|(id, chord)| {
                (
                    id.clone(),
                    chord.map(|chord| chord.label()).unwrap_or_default(),
                )
            })
            .collect(),
    };
    toml::to_string(&file).map_err(|err| format!("could not write the keymap: {err}"))
}

/// Read and check the keymap file at `path`.
pub fn read_keymap(path: &Path) -> Result<KeySet, String> {
    let file = std::fs::File::open(path)
        .map_err(|err| format!("could not open {}: {err}", path.display()))?;
    let mut text = String::new();
    file.take(MAX_KEYMAP_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    if text.len() as u64 > MAX_KEYMAP_BYTES {
        return Err(format!(
            "{} is larger than {} KiB; a keymap holds one line per rebound key",
            path.display(),
            MAX_KEYMAP_BYTES / 1024
        ));
    }
    parse_keymap(&text).map_err(|reason| format!("{}: {reason}", path.display()))
}

/// Write `set` to `path`, creating the directory it is in.
pub fn write_keymap(path: &Path, set: &KeySet) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    let text = render_keymap(set)?;
    std::fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

/// Delete `<data dir>/keymaps/<name>.toml`.
pub fn delete_keymap(data_dir: &Path, name: &str) -> Result<(), String> {
    let path = keymap_path(data_dir, name)?;
    std::fs::remove_file(&path).map_err(|err| format!("could not delete {}: {err}", path.display()))
}

impl KeyHost {
    /// The rebindings in force, as a set.
    pub fn current_set(&self) -> KeySet {
        self.overrides()
            .into_iter()
            .map(|(id, chord)| (id.to_owned(), chord))
            .collect()
    }

    /// Replace every rebinding with `set`, and report the rows that were not
    /// installed.
    ///
    /// Every binding the set names is cleared first and bound second. Applied
    /// one row at a time instead, a set that trades two chords fails on
    /// whichever row comes first, because the other binding still holds its
    /// shipped chord at that moment. After the clearing pass the only chords
    /// still held are held by bindings the set does not move, so a refusal in
    /// the second pass is a real conflict.
    ///
    /// A row that is refused puts its binding back on its shipped chord when
    /// that chord is free, and leaves it with no key when the set gave that
    /// chord to another binding.
    pub fn install_set(&self, set: &KeySet) -> Vec<KeySetSkip> {
        self.reset_overrides();
        let mut skipped = Vec::new();
        let mut placed: Vec<(&str, Chord)> = Vec::with_capacity(set.len());
        for (id, chord) in set {
            match self.set_override(id, None) {
                Ok(()) => {
                    if let Some(chord) = chord {
                        placed.push((id, *chord));
                    }
                }
                Err(KeyOverrideError::Unknown) => skipped.push(KeySetSkip {
                    id: id.clone(),
                    reason: SkipReason::Unknown,
                }),
                // Clearing names no chord, so nothing can hold it.
                Err(KeyOverrideError::Taken(_)) => {}
            }
        }
        let mut refused = Vec::new();
        for (id, chord) in placed {
            match self.set_override(id, Some(chord)) {
                Ok(()) => {}
                Err(KeyOverrideError::Taken(conflict)) => {
                    refused.push(id);
                    skipped.push(KeySetSkip {
                        id: id.to_owned(),
                        reason: SkipReason::Taken {
                            chord,
                            held_by: conflict.held_by,
                        },
                    });
                }
                Err(KeyOverrideError::Unknown) => skipped.push(KeySetSkip {
                    id: id.to_owned(),
                    reason: SkipReason::Unknown,
                }),
            }
        }
        for id in refused {
            // Placing the shipped chord through `set_override` is the conflict
            // check; the record it leaves is then dropped, because a binding on
            // its shipped chord is not a rebinding.
            let restored = self
                .shipped_chord(id)
                .is_none_or(|shipped| self.set_override(id, Some(shipped)).is_ok());
            if restored {
                self.remove_override(id);
            }
        }
        skipped
    }

    /// The chord a placed binding was registered with, before any rebinding.
    fn shipped_chord(&self, id: &str) -> Option<Chord> {
        self.bindings
            .read()
            .iter()
            .find(|binding| binding.id == id)
            .map(|binding| binding.chord)
    }
}

#[cfg(test)]
#[path = "keymap_sets_tests.rs"]
mod tests;
