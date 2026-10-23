//! Named key sets, the keyboard image, and moving a chord off its holder, from
//! the Settings side.
//!
//! The registry holds the chords ([`crate::keys::KeyHost`]) and
//! [`crate::keys::keymap_sets`] holds the file format; this holds what Settings
//! does with them. Every change goes through the registry first and is then
//! mirrored into `ui-settings.json`, the same order `rebind` keeps, so
//! a set the registry refused a row of is not saved as if it had installed.

use std::path::Path;
use std::sync::Arc;

use aria_core::count::counted;
use gpui::SharedString;

use super::AppState;
use crate::keys::keyboard_image::{CommandKind, Modifier, Mods};
use crate::keys::keymap_sets::{self, KeySet, KeySetSkip, SkipReason};
use crate::keys::{self, BindingRow, Chord, KeyOverrideError};

/// The Keys page's own state. Session state: none of it is saved.
#[derive(Debug, Default)]
pub struct KeyForm {
    /// The sets in `<data dir>/keymaps`, by name. Read when Settings opens and
    /// after every write, never during a frame.
    sets: Arc<[SharedString]>,
    /// A chord refused because another binding holds it, waiting for Move or
    /// Keep.
    pending_move: Option<KeyMove>,
    /// The modifiers the keyboard image shows chords for.
    image: Mods,
}

impl KeyForm {
    /// End the question a taken chord asked, unanswered.
    pub(super) fn forget_move(&mut self) {
        self.pending_move = None;
    }
}

/// The Keys section's rows under the Settings filter.
pub struct KeyListing {
    /// Each listed binding with what it is called, in registration order.
    pub rows: Vec<(BindingRow, String)>,
    /// Whether the filter matched the section's title, so every binding is
    /// listed and the sets, the keyboard image and the refusals are drawn too.
    pub whole: bool,
}

/// A chord one binding asked for and another holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyMove {
    /// The binding that asked.
    pub id: &'static str,
    pub chord: Chord,
    /// The binding that holds the chord, and loses it on Move.
    pub holder: &'static str,
}

impl AppState {
    /// The sets in `<data dir>/keymaps`, as last read.
    pub fn keymap_sets(&self) -> Arc<[SharedString]> {
        self.key_form.sets.clone()
    }

    /// The set last loaded or saved, by name.
    pub fn active_keymap(&self) -> Option<&str> {
        self.ui_settings.keymap.as_deref()
    }

    /// The bindings the Keys section lists under the Settings filter, or
    /// nothing when it lists none.
    ///
    /// A filter that is part of the section's title lists every binding. Any
    /// other filter lists the bindings whose name or chord contains it, without
    /// regard to case, so `razor` and `ctrl+k` both find a row.
    pub fn listed_binding_rows(&self) -> Option<KeyListing> {
        let host = self.key_host();
        let rows = host.as_ref().map(|host| host.rows()).unwrap_or_default();
        let named = rows.into_iter().map(|row| {
            let label = host.as_ref().map_or_else(
                || keys::binding_label(row.binding.id),
                |host| host.label_of(row.binding.id),
            );
            (row, label)
        });
        if self.settings_section_visible("Keys") {
            return Some(KeyListing {
                rows: named.collect(),
                whole: true,
            });
        }
        let filter = self.settings_form.filter.trim().to_lowercase();
        let rows: Vec<(BindingRow, String)> = named
            .filter(|(row, label)| {
                label.to_lowercase().contains(&filter)
                    || row
                        .chord
                        .is_some_and(|chord| chord.label().to_lowercase().contains(&filter))
            })
            .collect();
        (!rows.is_empty()).then_some(KeyListing { rows, whole: false })
    }

    /// What one key of the keyboard image holds with the image's modifiers
    /// down, for its tooltip: the chord, then each binding on it and its kind.
    pub fn key_cap_tip(&self, key: &'static str) -> String {
        let chord = self.key_form.image.chord(key);
        let held: Vec<String> = self
            .binding_rows()
            .into_iter()
            .filter(|row| row.chord == Some(chord))
            .map(|row| {
                format!(
                    "{} ({})",
                    self.binding_name(row.binding.id),
                    CommandKind::of(&row.binding).label()
                )
            })
            .collect();
        if held.is_empty() {
            format!("{}: no binding", chord.label())
        } else {
            format!("{}: {}", chord.label(), held.join(", "))
        }
    }

    /// Read the set names again.
    pub(crate) fn refresh_keymap_sets(&mut self) {
        match keymap_sets::keymap_names(&self.data_dir) {
            Ok(names) => {
                self.key_form.sets = names.into_iter().map(SharedString::from).collect();
            }
            Err(err) => self.report_error(err),
        }
    }

    /// Save the rebindings in force as `<data dir>/keymaps/<name>.toml`, over
    /// a set of that name if there is one.
    pub fn save_keymap_as(&mut self, name: &str) -> Result<(), String> {
        let path = keymap_sets::keymap_path(&self.data_dir, name)?;
        let set = self.current_key_set();
        keymap_sets::write_keymap(&path, &set)?;
        let name = name.trim().to_owned();
        self.status_message = Some(format!("Saved the keys as {name}"));
        self.ui_settings.keymap = Some(name);
        self.refresh_keymap_sets();
        Ok(())
    }

    /// Replace every rebinding with the set called `name`.
    pub fn load_keymap(&mut self, name: &str) -> Result<(), String> {
        let path = keymap_sets::keymap_path(&self.data_dir, name)?;
        let set = keymap_sets::read_keymap(&path)?;
        self.install_key_set(name.trim(), &set);
        Ok(())
    }

    /// Delete the set called `name`. The rebindings in force stay.
    pub fn delete_keymap(&mut self, name: &str) -> Result<(), String> {
        keymap_sets::delete_keymap(&self.data_dir, name)?;
        if self.active_keymap() == Some(name.trim()) {
            self.ui_settings.keymap = None;
        }
        self.status_message = Some(format!("Deleted the keymap {}", name.trim()));
        self.refresh_keymap_sets();
        Ok(())
    }

    /// Ask for a keymap file, add it to the sets, and install it.
    pub fn import_keymap(&mut self) {
        use super::picker::{Ask, FileSpec};
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Import a Keymap").filter("Aria keymap", &["toml"])),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                match state.import_keymap_from(&path) {
                    Ok(()) => state.save_settings_now(),
                    Err(err) => state.report_error(err),
                }
            },
        );
    }

    /// Ask where to write the rebindings in force as a keymap file.
    pub fn export_keymap(&mut self) {
        use super::picker::{Ask, FileSpec};
        let name = format!("{}.toml", self.active_keymap().unwrap_or("Aria keys"));
        self.request_pick(
            Ask::SaveFile(
                FileSpec::new("Export the Keymap")
                    .filter("Aria keymap", &["toml"])
                    .file_name(name),
            ),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                if let Err(err) = state.export_keymap_to(&path) {
                    state.report_error(err);
                }
            },
        );
    }

    /// Read the keymap file at `path`, save it among the sets under its file
    /// name, and install it.
    ///
    /// A set of that name already there is kept: the import is saved as
    /// `<name> 2`, `<name> 3` and so on, because an import that overwrote a
    /// set would lose one nobody exported.
    pub(crate) fn import_keymap_from(&mut self, path: &Path) -> Result<(), String> {
        let set = keymap_sets::read_keymap(path)?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Imported");
        let name = self.free_keymap_name(stem)?;
        keymap_sets::write_keymap(&keymap_sets::keymap_path(&self.data_dir, &name)?, &set)?;
        self.install_key_set(&name, &set);
        self.refresh_keymap_sets();
        Ok(())
    }

    /// Write the rebindings in force to `path`.
    pub(crate) fn export_keymap_to(&mut self, path: &Path) -> Result<(), String> {
        keymap_sets::write_keymap(path, &self.current_key_set())?;
        self.status_message = Some(format!("Exported the keys to {}", path.display()));
        Ok(())
    }

    /// The move waiting for an answer, while its row is still armed.
    ///
    /// Tied to the armed row so that clearing, reverting, or arming another
    /// row ends the question without each of them knowing it was asked.
    pub fn pending_key_move(&self) -> Option<KeyMove> {
        self.key_form
            .pending_move
            .filter(|pending| self.rebinding == Some(pending.id))
    }

    /// Move the chord to the binding that asked, and leave its holder with no
    /// key.
    pub fn move_key(&mut self) {
        let Some(pending) = self.pending_key_move() else {
            return;
        };
        self.key_form.pending_move = None;
        let Some(host) = self.key_host() else {
            return;
        };
        let before = host
            .overrides()
            .into_iter()
            .find(|(id, _)| *id == pending.holder);
        if let Err(err) = host.set_override(pending.holder, None) {
            self.rebinding = None;
            self.status_message = Some(self.refusal(pending.holder, pending.chord, &err));
            return;
        }
        if let Err(err) = host.set_override(pending.id, Some(pending.chord)) {
            // Put the holder back as it was, so a failed move changes nothing.
            match before {
                Some((_, chord)) => {
                    let _ = host.set_override(pending.holder, chord);
                }
                None => host.remove_override(pending.holder),
            }
            self.rebinding = None;
            self.status_message = Some(self.refusal(pending.id, pending.chord, &err));
            return;
        }
        self.rebinding = None;
        self.mirror_overrides();
        self.status_message = Some(format!(
            "{} is now {}, and {} has no key",
            self.binding_name(pending.id),
            pending.chord.label(),
            self.binding_name(pending.holder)
        ));
    }

    /// Leave the chord with its holder. The row stays armed for another key.
    pub fn keep_key(&mut self) {
        let Some(pending) = self.pending_key_move() else {
            return;
        };
        self.key_form.pending_move = None;
        self.status_message = Some(format!(
            "{} stays with {} · press another key for {}",
            pending.chord.label(),
            self.binding_name(pending.holder),
            self.binding_name(pending.id)
        ));
    }

    /// The modifiers the keyboard image shows chords for.
    pub fn key_image_mods(&self) -> Mods {
        self.key_form.image
    }

    pub fn toggle_key_image_modifier(&mut self, modifier: Modifier) {
        self.key_form.image = self.key_form.image.toggled(modifier);
    }

    /// Bind `id` to `key` with the image's modifiers down: what dropping a row
    /// on a key of the image does.
    pub fn drop_binding_on_key(&mut self, id: &'static str, key: &'static str) {
        self.take_chord(id, self.key_form.image.chord(key));
    }

    /// Give `id` the chord, or ask before taking it from the binding that has
    /// it. The one path a captured key and a dropped row share.
    pub(crate) fn take_chord(&mut self, id: &'static str, chord: Chord) {
        self.key_form.pending_move = None;
        match self.set_binding(id, Some(chord)) {
            Ok(()) => {
                self.rebinding = None;
                self.status_message = Some(format!(
                    "{} is now {}",
                    self.binding_name(id),
                    chord.label()
                ));
            }
            Err(KeyOverrideError::Taken(conflict)) => {
                // Armed, so Enter and Esc answer the question from the keyboard
                // the way Move and Keep answer it from the pointer.
                self.rebinding = Some(id);
                self.key_form.pending_move = Some(KeyMove {
                    id,
                    chord,
                    holder: conflict.held_by,
                });
                self.status_message = Some(format!(
                    "{} is held by {} · Enter moves it to {}, Esc keeps it",
                    chord.label(),
                    self.binding_name(conflict.held_by),
                    self.binding_name(id)
                ));
            }
            Err(KeyOverrideError::Unknown) => {
                self.rebinding = None;
                self.status_message = Some(format!("{id} is no longer a binding"));
            }
        }
    }

    /// The rebindings in force, as a set. Empty when no registry is mounted.
    fn current_key_set(&self) -> KeySet {
        self.key_host()
            .map(|host| host.current_set())
            .unwrap_or_default()
    }

    /// Install `set` as the set called `name`, mirror it, and report what was
    /// skipped.
    fn install_key_set(&mut self, name: &str, set: &KeySet) {
        self.rebinding = None;
        self.key_form.pending_move = None;
        let skipped = match self.key_host() {
            Some(host) => host.install_set(set),
            None => Vec::new(),
        };
        self.mirror_overrides();
        self.ui_settings.keymap = Some(name.to_owned());
        let installed = set.len().saturating_sub(skipped.len());
        let mut said = format!("Loaded {name}: {}", counted(installed, "key", "keys"));
        if !skipped.is_empty() {
            let reasons: Vec<String> = skipped.iter().map(|skip| self.skip_reason(skip)).collect();
            said.push_str(&format!(
                ", {} skipped ({})",
                skipped.len(),
                reasons.join("; ")
            ));
        }
        self.status_message = Some(said);
    }

    /// Why `id` could not take `chord`, as the status line reads it.
    fn refusal(&self, id: &str, chord: Chord, err: &KeyOverrideError) -> String {
        match err {
            KeyOverrideError::Unknown => format!("{id} is no longer a binding"),
            KeyOverrideError::Taken(conflict) => format!(
                "{} is held by {}",
                chord.label(),
                self.binding_name(conflict.held_by)
            ),
        }
    }

    fn skip_reason(&self, skip: &KeySetSkip) -> String {
        match skip.reason {
            SkipReason::Unknown => format!("{} is not a binding in this build", skip.id),
            SkipReason::Taken { chord, held_by } => format!(
                "{}: {} is held by {}",
                self.binding_name(&skip.id),
                chord.label(),
                self.binding_name(held_by)
            ),
        }
    }

    /// `stem`, or `stem 2`, `stem 3` and so on: the first name no set has.
    fn free_keymap_name(&self, stem: &str) -> Result<String, String> {
        let stem = stem.trim();
        for n in 1..=999 {
            let name = if n == 1 {
                stem.to_owned()
            } else {
                format!("{stem} {n}")
            };
            if !keymap_sets::keymap_path(&self.data_dir, &name)?.exists() {
                return Ok(name);
            }
        }
        Err(format!(
            "999 keymaps are already named after {stem}: delete some, or rename the file"
        ))
    }

    /// Write `ui-settings.json` now, for a change made where no window context
    /// can schedule the debounced write: a picker's answer.
    fn save_settings_now(&mut self) {
        if let Err(err) = self.ui_settings.save(&self.data_dir) {
            self.report_error(format!(
                "The keys are in force but could not be saved for the next launch: {err}"
            ));
        }
    }
}

#[cfg(test)]
#[path = "keymaps_tests.rs"]
mod tests;
