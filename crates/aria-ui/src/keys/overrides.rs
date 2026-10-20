//! Rebinding: the chords set in place of the shipped ones.
//!
//! Split out of [`super`] to stay under the file ceiling. A pure addition to
//! [`KeyHost`], written here because the registry file holds the shapes and this
//! one holds the one rule that matters: an override beats the shipped chord for
//! every reader, dispatch included.
//!
//! Both locks are taken in one order everywhere — overrides, then bindings — so
//! two threads rebinding and dispatching cannot wait on each other.

use super::{Binding, BindingRow, Chord, KeyConflict, KeyHost, KeyOverrideError};

impl KeyHost {
    /// The chord a binding answers to now, or nothing when it is unbound or the
    /// id names no binding.
    pub fn chord_of(&self, id: &str) -> Option<Chord> {
        let overrides = self.overrides.read();
        let bindings = self.bindings.read();
        let binding = bindings.iter().find(|binding| binding.id == id)?;
        Self::chord_for(overrides.as_slice(), binding)
    }

    /// Rebind, or unbind by passing `None`.
    ///
    /// A chord another binding already answers on the same route is refused and
    /// the holder named, so the surface can say what is in the way at the moment
    /// the key is pressed rather than shadowing a binding that still works. A
    /// binding rebound to the chord it already has costs nothing and is still
    /// recorded, because "this was chosen" is what Reset undoes.
    ///
    /// A plugin key refused for a taken chord is rebound the same way, and the
    /// next sync places it on the chord it was given.
    pub fn set_override(&self, id: &str, chord: Option<Chord>) -> Result<(), KeyOverrideError> {
        let mut overrides = self.overrides.write();
        let bindings = self.bindings.read();
        let Some(target) = bindings
            .iter()
            .find(|binding| binding.id == id)
            .copied()
            .or_else(|| self.refused_plugin_key(id))
        else {
            return Err(KeyOverrideError::Unknown);
        };
        if let Some(wanted) = chord {
            let held = bindings.iter().find(|other| {
                other.id != target.id
                    && other.route == target.route
                    && Self::chord_for(overrides.as_slice(), other) == Some(wanted)
            });
            if let Some(held) = held {
                return Err(KeyOverrideError::Taken(KeyConflict {
                    held_by: held.id,
                    refused: target.id,
                    chord: wanted,
                }));
            }
        }
        match overrides.iter_mut().find(|(held, _)| *held == target.id) {
            Some(row) => row.1 = chord,
            None => overrides.push((target.id, chord)),
        }
        self.resync_plugin_keys();
        Ok(())
    }

    /// Put one binding back to the chord it shipped with.
    pub fn remove_override(&self, id: &str) {
        self.overrides.write().retain(|(held, _)| *held != id);
        self.resync_plugin_keys();
    }

    /// Put every binding back to the chord it shipped with.
    pub fn reset_overrides(&self) {
        self.overrides.write().clear();
        self.resync_plugin_keys();
    }

    /// The rebindings in force, in the order they were made. What Settings
    /// persists.
    pub fn overrides(&self) -> Vec<(&'static str, Option<Chord>)> {
        self.overrides.read().clone()
    }

    /// Every binding with the chord it answers to now, in registration order,
    /// then each refused plugin key with no chord, so it can be given one.
    pub fn rows(&self) -> Vec<BindingRow> {
        let overrides = self.overrides.read();
        let bindings = self.bindings.read();
        let refused = self.plugin_refused.read();
        let overridden = |id: &str| overrides.iter().any(|(held, _)| *held == id);
        bindings
            .iter()
            .map(|binding| BindingRow {
                binding: *binding,
                chord: Self::chord_for(overrides.as_slice(), binding),
                overridden: overridden(binding.id),
            })
            .chain(refused.iter().map(|binding| BindingRow {
                binding: *binding,
                chord: None,
                overridden: overridden(binding.id),
            }))
            .collect()
    }

    /// The chord one binding answers to, given the rebindings in force.
    ///
    /// The single join of the two tables: dispatch resolves through this and the
    /// surface draws it, so a key that fires and a key that is shown cannot
    /// disagree.
    pub(super) fn chord_for(
        overrides: &[(&'static str, Option<Chord>)],
        binding: &Binding,
    ) -> Option<Chord> {
        match overrides.iter().find(|(id, _)| *id == binding.id) {
            Some((_, chord)) => *chord,
            None => Some(binding.chord),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeyRoute, binding_label};

    fn host() -> KeyHost {
        let host = KeyHost::default();
        assert!(host.add(Binding::inert(
            "key-save",
            Chord::ctrl("s"),
            KeyRoute::Editor
        )));
        assert!(host.add(Binding::inert(
            "key-split",
            Chord::plain("s"),
            KeyRoute::Editor
        )));
        assert!(host.add(Binding::inert("key-open", Chord::ctrl("o"), KeyRoute::Home)));
        host
    }

    /// The point of the whole surface: the new chord fires and the old one stops
    /// firing, without a second dispatch path.
    #[test]
    fn a_rebound_chord_is_the_one_that_dispatches() {
        let host = host();
        assert_eq!(
            host.set_override("key-split", Some(Chord::plain("x"))),
            Ok(())
        );
        assert_eq!(
            host.resolve(KeyRoute::Editor, "x", false, false, false)
                .map(|binding| binding.id),
            Some("key-split")
        );
        assert!(
            host.resolve(KeyRoute::Editor, "s", false, false, false)
                .is_none(),
            "the shipped chord still fires after it was replaced"
        );
        assert_eq!(host.chord_of("key-split"), Some(Chord::plain("x")));
    }

    /// A cleared binding is a binding with no chord, not a binding back on its
    /// shipped chord: a cleared Ctrl+S must not save again after a restart.
    #[test]
    fn a_cleared_binding_answers_nothing_and_is_not_the_shipped_chord() {
        let host = host();
        assert_eq!(host.set_override("key-save", None), Ok(()));
        assert!(
            host.resolve(KeyRoute::Editor, "s", true, false, false)
                .is_none()
        );
        assert_eq!(host.chord_of("key-save"), None);
        let row = host
            .rows()
            .into_iter()
            .find(|row| row.binding.id == "key-save")
            .expect("the binding is still listed");
        assert!(row.overridden, "a cleared binding is an override");
        assert!(row.chord.is_none(), "a cleared binding shows no chord");
    }

    #[test]
    fn a_taken_chord_is_refused_and_names_what_holds_it() {
        let host = host();
        let refused = host.set_override("key-split", Some(Chord::ctrl("s")));
        assert_eq!(
            refused,
            Err(KeyOverrideError::Taken(KeyConflict {
                held_by: "key-save",
                refused: "key-split",
                chord: Chord::ctrl("s"),
            }))
        );
        assert_eq!(
            host.chord_of("key-split"),
            Some(Chord::plain("s")),
            "a refused rebinding left the binding as it was"
        );
    }

    /// The same chord on another route is not a conflict, because dispatch never
    /// asks both routes at once.
    #[test]
    fn a_chord_taken_on_another_route_is_free() {
        let host = host();
        assert_eq!(
            host.set_override("key-save", Some(Chord::ctrl("o"))),
            Ok(())
        );
        assert_eq!(
            host.resolve(KeyRoute::Editor, "o", true, false, false)
                .map(|binding| binding.id),
            Some("key-save")
        );
        assert_eq!(
            host.resolve(KeyRoute::Home, "o", true, false, false)
                .map(|binding| binding.id),
            Some("key-open")
        );
    }

    /// A chord freed by a rebinding is available immediately: the conflict test
    /// reads effective chords, not shipped ones.
    #[test]
    fn a_chord_the_holder_gave_up_can_be_taken() {
        let host = host();
        assert_eq!(
            host.set_override("key-save", Some(Chord::ctrl("k"))),
            Ok(())
        );
        assert_eq!(
            host.set_override("key-split", Some(Chord::ctrl("s"))),
            Ok(())
        );
        assert_eq!(
            host.resolve(KeyRoute::Editor, "s", true, false, false)
                .map(|binding| binding.id),
            Some("key-split")
        );
    }

    #[test]
    fn an_unknown_binding_cannot_be_rebound() {
        let host = host();
        assert_eq!(
            host.set_override("key-no-such-thing", Some(Chord::plain("q"))),
            Err(KeyOverrideError::Unknown)
        );
        assert!(host.overrides().is_empty());
        assert_eq!(host.chord_of("key-no-such-thing"), None);
    }

    #[test]
    fn reset_restores_every_shipped_chord() {
        let host = host();
        assert_eq!(
            host.set_override("key-split", Some(Chord::plain("x"))),
            Ok(())
        );
        assert_eq!(host.set_override("key-save", None), Ok(()));
        host.reset_overrides();
        assert!(host.overrides().is_empty());
        for row in host.rows() {
            assert!(!row.overridden, "{} is still overridden", row.binding.id);
            assert_eq!(row.chord, Some(row.binding.chord));
        }
        assert_eq!(
            host.resolve(KeyRoute::Editor, "s", true, false, false)
                .map(|binding| binding.id),
            Some("key-save")
        );
    }

    #[test]
    fn one_binding_can_be_put_back_without_touching_the_others() {
        let host = host();
        assert_eq!(
            host.set_override("key-split", Some(Chord::plain("x"))),
            Ok(())
        );
        assert_eq!(host.set_override("key-save", None), Ok(()));
        host.remove_override("key-split");
        assert_eq!(host.chord_of("key-split"), Some(Chord::plain("s")));
        assert_eq!(
            host.chord_of("key-save"),
            None,
            "the other one is untouched"
        );
    }

    /// Rebinding the same row twice replaces the row rather than stacking, or the
    /// saved settings would grow with every change and the first choice would
    /// win on reload.
    #[test]
    fn rebinding_twice_keeps_one_row() {
        let host = host();
        assert_eq!(
            host.set_override("key-split", Some(Chord::plain("x"))),
            Ok(())
        );
        assert_eq!(
            host.set_override("key-split", Some(Chord::plain("q"))),
            Ok(())
        );
        assert_eq!(host.overrides().len(), 1);
        assert_eq!(host.chord_of("key-split"), Some(Chord::plain("q")));
    }

    #[test]
    fn a_binding_reads_as_a_name_and_not_as_an_id() {
        assert_eq!(binding_label("key-play"), "Play");
        assert_eq!(binding_label("key-shuttle-back"), "Shuttle back");
        assert_eq!(binding_label("key-lane-shift-later"), "Lane shift later");
        // No prefix and no hyphen: a plugin that names its binding anything at
        // all still gets a readable row.
        assert_eq!(binding_label("plugin"), "Plugin");
        assert_eq!(binding_label(""), "");
    }
}
