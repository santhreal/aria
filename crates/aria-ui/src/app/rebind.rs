//! Rebinding a key, through the Keys settings.
//!
//! The registry holds the chords ([`crate::keys::KeyHost`]); this holds the
//! gesture. Arming a row makes the next keystroke the answer rather than a
//! command, which is the only way to name a chord without spelling it out: a
//! captured chord is exactly what the shell will see next time.
//!
//! The saved form is `ui-settings.json`, keyed by binding id, and the chord is
//! the same text Settings shows. Ids and keys are both closed sets, so a saved
//! override that names neither is dropped with a reason instead of quietly
//! rebinding nothing.

use aria_core::count::counted;

use super::AppState;
use crate::keys::keymap_sets::KeySet;
use crate::keys::{self, BindingRow, Chord, KeyOverrideError};

impl AppState {
    /// Every binding with the chord it answers to now, in registration order.
    ///
    /// Empty when no host is mounted, which is a shell with no key registry at
    /// all — the surface then has nothing to list rather than a shipped table
    /// nothing would dispatch.
    pub fn binding_rows(&self) -> Vec<BindingRow> {
        self.key_host().map(|host| host.rows()).unwrap_or_default()
    }

    /// The key registry with the plugin keys in place, or nothing when no host
    /// is mounted. Every reader of the registry in the window goes through
    /// this, so a plugin key that registered since the last read is listed and
    /// dispatched.
    pub(crate) fn key_host(&self) -> Option<std::sync::Arc<keys::KeyHost>> {
        let host = keys::host_of(&self.ctx)?;
        host.sync_plugin_keys(&self.ctx);
        Some(host)
    }

    /// What a binding is called: a plugin command's title, or a shipped id read
    /// as words.
    pub fn binding_name(&self, id: &str) -> String {
        self.key_host()
            .map_or_else(|| keys::binding_label(id), |host| host.label_of(id))
    }

    /// Bindings a plugin asked for and did not get, in the order they were
    /// refused.
    ///
    /// The registry has recorded these since it existed and nothing showed them,
    /// which made a plugin's dead key indistinguishable from a plugin that never
    /// registered one.
    pub fn binding_conflicts(&self) -> Vec<keys::KeyConflict> {
        self.key_host()
            .map(|host| host.conflicts())
            .unwrap_or_default()
    }

    /// The binding waiting for a chord, if one is armed.
    pub fn rebinding(&self) -> Option<&'static str> {
        self.rebinding
    }

    /// Arm a row: the next keystroke is its chord.
    ///
    /// A question left from an earlier capture ends here, so Enter is a chord
    /// again until a taken chord asks a new one.
    pub fn begin_rebind(&mut self, id: &'static str) {
        self.key_form.forget_move();
        self.rebinding = Some(id);
        self.status_message = Some(format!(
            "Press a key for {} · Esc to cancel",
            self.binding_name(id)
        ));
    }

    pub fn cancel_rebind(&mut self) {
        if let Some(id) = self.rebinding.take() {
            self.status_message = Some(format!("{} is unchanged", self.binding_name(id)));
        }
    }

    /// Take a keystroke as the armed row's new chord.
    ///
    /// Escape cancels, because a capture with no way out would leave the
    /// keyboard dead. A key that cannot carry a binding (a modifier on its own, a
    /// key the vocabulary does not name) leaves the capture armed: nothing has
    /// been chosen yet. A chord another binding holds asks whether to move it,
    /// with the holder named and the capture still armed: Enter moves it, Escape
    /// ends the capture and leaves it where it is, and another key is another
    /// try.
    pub fn capture_rebind(&mut self, key: &str, ctrl: bool, shift: bool, alt: bool) {
        let Some(id) = self.rebinding else {
            return;
        };
        let bare = !ctrl && !shift && !alt;
        if key == "escape" && bare {
            self.cancel_rebind();
            return;
        }
        if key == "enter" && bare && self.pending_key_move().is_some() {
            self.move_key();
            return;
        }
        let (chord_key, shift) = keys::chord_key(key, shift);
        let Some(chord) = Chord::parse(&Self::chord_text(chord_key, ctrl, shift, alt)) else {
            self.status_message = Some(format!("{key} cannot carry a binding"));
            return;
        };
        self.take_chord(id, chord);
    }

    /// Leave a binding with no chord at all.
    ///
    /// Clearing carries no chord, so it cannot collide with one: the only
    /// refusal is a binding the registry does not hold.
    pub fn clear_binding(&mut self, id: &'static str) {
        self.rebinding = None;
        self.status_message = Some(match self.set_binding(id, None) {
            Ok(()) => format!("{} has no key", self.binding_name(id)),
            Err(KeyOverrideError::Unknown) | Err(KeyOverrideError::Taken(_)) => {
                format!("{id} is no longer a binding")
            }
        });
    }

    /// Put one binding back to the chord it shipped with.
    pub fn revert_binding(&mut self, id: &'static str) {
        self.rebinding = None;
        if let Some(host) = self.key_host() {
            host.remove_override(id);
        }
        self.mirror_overrides();
        self.status_message = Some(match self.chord_label(id) {
            Some(label) => format!("{} is {label} again", self.binding_name(id)),
            None => format!("{} is unbound", self.binding_name(id)),
        });
    }

    /// Put every binding back to the chord it shipped with.
    pub fn reset_bindings(&mut self) {
        self.rebinding = None;
        let restored = self.ui_settings.keys.len();
        if let Some(host) = self.key_host() {
            host.reset_overrides();
        }
        self.mirror_overrides();
        self.status_message = Some(match restored {
            0 => "Every key is already the shipped one".to_string(),
            n => format!("{} restored", counted(n, "key", "keys")),
        });
    }

    /// How one binding's chord reads now, or nothing when it has none.
    ///
    /// The surface draws from [`AppState::binding_rows`]; this is for the one
    /// place that names a single binding, which is what a revert reports.
    pub(crate) fn chord_label(&self, id: &str) -> Option<String> {
        self.key_host()?.chord_of(id).map(|chord| chord.label())
    }

    /// Install the saved overrides on the mounted registry.
    ///
    /// Called once, on the way out of the constructor: the registry is the only
    /// thing that resolves a chord, so the saved choices are in place before the
    /// first keystroke rather than consulted at dispatch. A saved row the
    /// registry cannot place — a plugin that is gone, a key this build no longer
    /// names — is logged and dropped rather than kept as a chord that does
    /// nothing.
    ///
    /// The rows go in as one set, through [`keys::KeyHost::install_set`]: two
    /// bindings that traded chords are saved as two rows, and installed one at a
    /// time the first row meets the other binding still on its shipped chord.
    pub(crate) fn apply_key_overrides(&self) {
        let Some(host) = self.key_host() else {
            return;
        };
        let mut set = KeySet::new();
        for (id, saved) in &self.ui_settings.keys {
            let chord = match saved {
                Some(text) => match Chord::parse(text) {
                    Some(chord) => Some(chord),
                    None => {
                        tracing::warn!(binding = %id, chord = %text, "saved key names no bindable chord");
                        continue;
                    }
                },
                None => None,
            };
            set.insert(id.clone(), chord);
        }
        for skip in host.install_set(&set) {
            tracing::warn!(binding = %skip.id, reason = ?skip.reason, "saved key could not be applied");
        }
    }

    /// The one write path: the registry decides, and the settings follow.
    ///
    /// A refused rebinding must not be persisted, or a restart would install a
    /// chord this session already rejected.
    pub(super) fn set_binding(
        &mut self,
        id: &'static str,
        chord: Option<Chord>,
    ) -> Result<(), KeyOverrideError> {
        let host = self.key_host().ok_or(KeyOverrideError::Unknown)?;
        host.set_override(id, chord)?;
        self.mirror_overrides();
        Ok(())
    }

    /// Copy the registry's rebindings into the settings that get saved.
    ///
    /// The registry owns them; the file is a projection of it. Kept that way so
    /// there is one place a rebinding can be refused, and so a reset cannot
    /// leave a row in the file that the running app no longer honours.
    pub(super) fn mirror_overrides(&mut self) {
        let Some(host) = self.key_host() else {
            return;
        };
        self.ui_settings.keys = host
            .overrides()
            .into_iter()
            .map(|(id, chord)| (id.to_string(), chord.map(|chord| chord.label())))
            .collect();
    }

    /// The keystroke, spelled the way a chord reads.
    ///
    /// Built as text and parsed back rather than assembled into a `Chord`
    /// directly, because the key has to be checked against the vocabulary either
    /// way and the parser is where that check lives.
    fn chord_text(key: &str, ctrl: bool, shift: bool, alt: bool) -> String {
        let mut out = String::with_capacity(key.len() + 16);
        if ctrl {
            out.push_str("Ctrl+");
        }
        if shift {
            out.push_str("Shift+");
        }
        if alt {
            out.push_str("Alt+");
        }
        out.push_str(key);
        out
    }
}

#[cfg(test)]
#[path = "rebind_tests.rs"]
pub(super) mod tests;
