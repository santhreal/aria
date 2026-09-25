//! Rebinding a key, from the operator's side.
//!
//! The registry holds the chords ([`crate::keys::KeyHost`]); this holds the
//! gesture. Arming a row makes the next keystroke the answer rather than a
//! command, which is the only way to name a chord without asking the operator to
//! spell it: a captured chord is exactly what the shell will see next time.
//!
//! The saved form is `ui-settings.json`, keyed by binding id, and the chord is
//! the same text Settings shows. Ids and keys are both closed sets, so a saved
//! override that names neither is dropped with a reason instead of quietly
//! rebinding nothing.

use super::AppState;
use crate::keys::{self, BindingRow, Chord, KeyOverrideError};

impl AppState {
    /// Every binding with the chord it answers to now, in registration order.
    ///
    /// Empty when no host is mounted, which is a shell with no key registry at
    /// all — the surface then has nothing to list rather than a shipped table
    /// nothing would dispatch.
    pub fn binding_rows(&self) -> Vec<BindingRow> {
        keys::host_of(&self.ctx)
            .map(|host| host.rows())
            .unwrap_or_default()
    }

    /// Bindings a plugin asked for and did not get, in the order they were
    /// refused.
    ///
    /// The registry has recorded these since it existed and nothing showed them,
    /// which made a plugin's dead key indistinguishable from a plugin that never
    /// registered one.
    pub fn binding_conflicts(&self) -> Vec<keys::KeyConflict> {
        keys::host_of(&self.ctx)
            .map(|host| host.conflicts())
            .unwrap_or_default()
    }

    /// The binding waiting for a chord, if the operator armed one.
    pub fn rebinding(&self) -> Option<&'static str> {
        self.rebinding
    }

    /// Arm a row: the next keystroke is its chord.
    pub fn begin_rebind(&mut self, id: &'static str) {
        self.rebinding = Some(id);
        self.status_message = Some(format!(
            "Press a key for {} · Esc to cancel",
            keys::binding_label(id)
        ));
    }

    pub fn cancel_rebind(&mut self) {
        if let Some(id) = self.rebinding.take() {
            self.status_message = Some(format!("{} is unchanged", keys::binding_label(id)));
        }
    }

    /// Take a keystroke as the armed row's new chord.
    ///
    /// Escape cancels, because a capture the operator cannot get out of would
    /// leave the keyboard dead. A key that cannot carry a binding (a modifier on
    /// its own, a key the vocabulary does not name) leaves the capture armed: the
    /// operator is mid-gesture and has not chosen anything yet. A chord another
    /// binding holds is refused with the holder named and the capture stays
    /// armed, so the next attempt costs one keystroke instead of a second click.
    pub fn capture_rebind(&mut self, key: &str, ctrl: bool, shift: bool, alt: bool) {
        let Some(id) = self.rebinding else {
            return;
        };
        if key == "escape" && !ctrl && !shift && !alt {
            self.cancel_rebind();
            return;
        }
        let Some(chord) = Chord::parse(&Self::chord_text(key, ctrl, shift, alt)) else {
            self.status_message = Some(format!("{key} cannot carry a binding"));
            return;
        };
        match self.set_binding(id, Some(chord)) {
            Ok(()) => {
                self.rebinding = None;
                self.status_message = Some(format!(
                    "{} is now {}",
                    keys::binding_label(id),
                    chord.label()
                ));
            }
            Err(KeyOverrideError::Taken(conflict)) => {
                self.status_message = Some(format!(
                    "{} is held by {}",
                    chord.label(),
                    keys::binding_label(conflict.held_by)
                ));
            }
            Err(KeyOverrideError::Unknown) => {
                self.rebinding = None;
                self.status_message = Some(format!("{id} is no longer a binding"));
            }
        }
    }

    /// Leave a binding with no chord at all.
    ///
    /// Clearing carries no chord, so it cannot collide with one: the only
    /// refusal is a binding the registry does not hold.
    pub fn clear_binding(&mut self, id: &'static str) {
        self.rebinding = None;
        self.status_message = Some(match self.set_binding(id, None) {
            Ok(()) => format!("{} has no key", keys::binding_label(id)),
            Err(KeyOverrideError::Unknown) | Err(KeyOverrideError::Taken(_)) => {
                format!("{id} is no longer a binding")
            }
        });
    }

    /// Put one binding back to the chord it shipped with.
    pub fn revert_binding(&mut self, id: &'static str) {
        self.rebinding = None;
        if let Some(host) = keys::host_of(&self.ctx) {
            host.remove_override(id);
        }
        self.mirror_overrides();
        self.status_message = Some(match self.chord_label(id) {
            Some(label) => format!("{} is {label} again", keys::binding_label(id)),
            None => format!("{} is unbound", keys::binding_label(id)),
        });
    }

    /// Put every binding back to the chord it shipped with.
    pub fn reset_bindings(&mut self) {
        self.rebinding = None;
        let restored = self.ui_settings.keys.len();
        if let Some(host) = keys::host_of(&self.ctx) {
            host.reset_overrides();
        }
        self.mirror_overrides();
        self.status_message = Some(match restored {
            0 => "Every key is already the shipped one".to_string(),
            1 => "1 key restored".to_string(),
            many => format!("{many} keys restored"),
        });
    }

    /// How one binding's chord reads now, or nothing when it has none.
    ///
    /// The surface draws from [`AppState::binding_rows`]; this is for the one
    /// place that names a single binding, which is what a revert reports.
    pub(crate) fn chord_label(&self, id: &str) -> Option<String> {
        keys::host_of(&self.ctx)?
            .chord_of(id)
            .map(|chord| chord.label())
    }

    /// Install the saved overrides on the mounted registry.
    ///
    /// Called once, on the way out of the constructor: the registry is the only
    /// thing that resolves a chord, so the operator's choices are in place before
    /// the first keystroke rather than consulted at dispatch. A saved row the
    /// registry cannot place — a plugin that is gone, a key this build no longer
    /// names — is logged and dropped rather than kept as a chord that does
    /// nothing.
    pub(crate) fn apply_key_overrides(&self) {
        let Some(host) = keys::host_of(&self.ctx) else {
            return;
        };
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
            if let Err(err) = host.set_override(id, chord) {
                tracing::warn!(binding = %id, ?err, "saved key could not be applied");
            }
        }
    }

    /// The one write path: the registry decides, and the settings follow.
    ///
    /// A refused rebinding must not be persisted, or a restart would install a
    /// chord this session already rejected.
    fn set_binding(
        &mut self,
        id: &'static str,
        chord: Option<Chord>,
    ) -> Result<(), KeyOverrideError> {
        let host = keys::host_of(&self.ctx).ok_or(KeyOverrideError::Unknown)?;
        host.set_override(id, chord)?;
        self.mirror_overrides();
        Ok(())
    }

    /// Copy the registry's rebindings into the settings that get saved.
    ///
    /// The registry owns them; the file is a projection of it. Kept that way so
    /// there is one place a rebinding can be refused, and so a reset cannot
    /// leave a row in the file that the running app no longer honours.
    fn mirror_overrides(&mut self) {
        let Some(host) = keys::host_of(&self.ctx) else {
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
mod tests;
