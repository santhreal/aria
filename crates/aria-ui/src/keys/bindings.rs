//! The key bindings the editor ships.
//!
//! A table, not a `match`: the tests below enumerate it, so a chord added here is
//! covered without a second list, and `KeyHost::add` refuses a chord that is
//! already taken rather than shadowing the first.
//!
//! The rows live in three sibling files, split by what a chord does:
//! `bindings_edit`, `bindings_tools`, and
//! `bindings_reach`. `SHIPPED_BINDINGS` is their concatenation, joined at
//! compile time from the groups' own lengths — no count is written down, so a
//! chord added to any group is in the table by construction, and the conflict test
//! below sees it.

use super::Binding;
use super::bindings_edit::EDIT_ROWS;
use super::bindings_reach::REACH_ROWS;
use super::bindings_tools::TOOL_ROWS;

/// The row groups, in registration order: editor first, Home last.
const GROUPS: &[&[Binding]] = &[EDIT_ROWS, TOOL_ROWS, REACH_ROWS];

/// How many bindings the groups hold between them, summed at compile time.
const TOTAL: usize = {
    let mut total = 0;
    let mut group = 0;
    while group < GROUPS.len() {
        total += GROUPS[group].len();
        group += 1;
    }
    total
};

/// Flatten the groups into one array.
///
/// A const fn rather than a fourth hand-written table: the registry and the
/// tooltip index both want one `&[Binding]`, and the rows want to live in files a
/// person can read.
const fn joined<const N: usize>(groups: &[&[Binding]]) -> [Binding; N] {
    let mut out = [groups[0][0]; N];
    let mut at = 0;
    let mut group = 0;
    while group < groups.len() {
        let rows = groups[group];
        let mut row = 0;
        while row < rows.len() {
            out[at] = rows[row];
            at += 1;
            row += 1;
        }
        group += 1;
    }
    out
}

/// Editor-route bindings, then Home-route ones, then the reach chords.
pub const SHIPPED_BINDINGS: &[Binding] = &joined::<TOTAL>(GROUPS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{After, Chord, KeyHost, KeyRoute};
    use std::collections::BTreeSet;

    /// The table is enumerated at run time, so a binding added without a unique id
    /// turns this red. An id is what Settings shows and what a conflict names.
    #[test]
    fn every_shipped_binding_has_a_unique_namespaced_id() {
        assert!(!SHIPPED_BINDINGS.is_empty());
        let ids: BTreeSet<&str> = SHIPPED_BINDINGS.iter().map(|b| b.id).collect();
        assert_eq!(ids.len(), SHIPPED_BINDINGS.len(), "duplicate binding id");
        for binding in SHIPPED_BINDINGS {
            assert!(
                binding.id.starts_with("key-"),
                "{} is missing the key- prefix",
                binding.id
            );
            assert!(!binding.chord.key.is_empty(), "{} has no key", binding.id);
            assert_eq!(
                binding.chord.key.to_lowercase(),
                binding.chord.key,
                "{} spells its key in upper case; the shell reports lower case",
                binding.id
            );
        }
    }

    /// Two shipped bindings on one chord would mean one of them never fires, and
    /// which one would depend on table order. The host refuses the second, so this
    /// asserts the table never asks it to.
    #[test]
    fn no_two_shipped_bindings_take_the_same_chord() {
        let host = KeyHost::default();
        for binding in SHIPPED_BINDINGS {
            assert!(
                host.add(*binding),
                "{} collides with an earlier binding on {}",
                binding.id,
                binding.chord.label()
            );
        }
        assert!(host.conflicts().is_empty());
        assert_eq!(host.all().len(), SHIPPED_BINDINGS.len());
    }

    /// The chords the shell used before the registry existed still resolve, on the
    /// route they belonged to. This is the regression guard for the conversion: a
    /// binding dropped in the move would leave the key dead.
    #[test]
    fn the_documented_chords_still_resolve() {
        let host = KeyHost::default();
        for binding in SHIPPED_BINDINGS {
            host.add(*binding);
        }
        let cases: &[(KeyRoute, &str, bool, bool, bool, &str)] = &[
            (KeyRoute::Editor, "space", false, false, false, "key-play"),
            (KeyRoute::Editor, "s", false, false, false, "key-split"),
            (KeyRoute::Editor, "s", true, false, false, "key-save"),
            (KeyRoute::Editor, "s", true, true, false, "key-save-as"),
            (KeyRoute::Editor, "z", true, false, false, "key-undo"),
            (KeyRoute::Editor, "z", true, true, false, "key-redo"),
            (KeyRoute::Editor, "y", true, false, false, "key-redo-y"),
            (KeyRoute::Editor, "p", true, true, false, "key-palette"),
            (KeyRoute::Editor, "i", false, false, false, "key-mark-in"),
            (KeyRoute::Editor, "i", false, true, false, "key-mark-in-go"),
            (
                KeyRoute::Editor,
                "i",
                false,
                false,
                true,
                "key-mark-in-clear",
            ),
            (KeyRoute::Editor, "o", false, false, false, "key-mark-out"),
            (KeyRoute::Editor, "o", true, false, false, "key-open"),
            (KeyRoute::Editor, "m", false, false, false, "key-marker-add"),
            (
                KeyRoute::Editor,
                "m",
                false,
                false,
                true,
                "key-marker-remove",
            ),
            (KeyRoute::Editor, "m", false, true, false, "key-marker-next"),
            (KeyRoute::Editor, "m", false, true, true, "key-marker-prev"),
            (KeyRoute::Editor, "f", false, false, false, "key-fit"),
            (
                KeyRoute::Editor,
                "f",
                false,
                true,
                false,
                "key-fit-selection",
            ),
            (KeyRoute::Editor, "v", true, false, false, "key-paste"),
            (KeyRoute::Editor, "v", true, true, false, "key-paste-insert"),
            (
                KeyRoute::Editor,
                "v",
                true,
                false,
                true,
                "key-paste-attributes",
            ),
            (KeyRoute::Editor, "h", true, true, false, "key-history"),
            (
                KeyRoute::Editor,
                "left",
                false,
                true,
                false,
                "key-step-back-fast",
            ),
            (KeyRoute::Editor, "a", false, true, false, "key-assistant"),
            (KeyRoute::Editor, "6", false, true, false, "key-mixer"),
            (
                KeyRoute::Editor,
                "a",
                false,
                false,
                false,
                "key-tool-pointer-a",
            ),
            (KeyRoute::Home, "n", true, false, false, "key-home-new"),
            (KeyRoute::Home, "o", true, false, false, "key-home-open"),
            (
                KeyRoute::Home,
                "escape",
                false,
                false,
                false,
                "key-home-escape",
            ),
        ];
        for (route, key, ctrl, shift, alt, id) in cases {
            assert_eq!(
                host.resolve(*route, key, *ctrl, *shift, *alt).map(|b| b.id),
                Some(*id),
                "{} no longer resolves",
                id
            );
        }
    }

    /// The shifted row of a US layout, and the keys under it, as two strings
    /// read off the keyboard rather than out of the vocabulary's table.
    const US_SHIFTED: &str = "!@#$%^&*()_+{}|:\"<>?~";
    const US_UNDER: &str = "1234567890-=[]\\;',./`";

    /// What GPUI on Linux reports for a chord typed on a US layout: Shift and a
    /// digit or punctuation key arrive as the shifted symbol with shift
    /// cleared, the way text arrives. Nothing when no key produces the chord.
    fn reported(chord: Chord) -> Option<(String, bool)> {
        let mut chars = chord.key.chars();
        let (Some(under), None) = (chars.next(), chars.next()) else {
            return Some((chord.key.to_string(), chord.shift));
        };
        if !chord.shift || under.is_alphabetic() {
            return Some((chord.key.to_string(), chord.shift));
        }
        let at = US_UNDER.chars().position(|key| key == under)?;
        let symbol = US_SHIFTED.chars().nth(at)?;
        Some((symbol.to_string(), false))
    }

    /// The class closed here: a shipped chord the platform never reports. GPUI
    /// on Linux sends Shift+6 as `^` and Shift+, as `<`, both with shift
    /// cleared, so the mixer and ripple-insert keys never fired. Enumerated
    /// from the table, so a new Shift chord on a key no US keystroke produces,
    /// such as Shift+=, fails here. Layouts other than US are not modelled.
    #[test]
    fn every_shipped_chord_fires_from_the_keystroke_linux_reports() {
        let host = KeyHost::default();
        for binding in SHIPPED_BINDINGS {
            host.add(*binding);
        }
        for binding in SHIPPED_BINDINGS {
            let chord = binding.chord;
            let (key, shift) = reported(chord).unwrap_or_else(|| {
                panic!(
                    "{} is on {}, which no key produces",
                    binding.id,
                    chord.label()
                )
            });
            assert_eq!(
                host.resolve(binding.route, &key, chord.ctrl, shift, chord.alt)
                    .map(|fired| fired.id),
                Some(binding.id),
                "{} is on {}, which arrives as {key:?} and fires nothing",
                binding.id,
                chord.label()
            );
        }
    }

    /// The two keys the defect was found on, pinned, and the plus key, which
    /// is a shifted symbol and a binding of its own.
    #[test]
    fn the_shifted_symbols_fire_their_chords() {
        let host = KeyHost::default();
        for binding in SHIPPED_BINDINGS {
            host.add(*binding);
        }
        let fired = |key: &str| {
            host.resolve(KeyRoute::Editor, key, false, false, false)
                .map(|binding| binding.id)
        };
        assert_eq!(fired("^"), Some("key-mixer"));
        assert_eq!(fired("<"), Some("key-ripple-insert"));
        assert_eq!(fired("+"), Some("key-zoom-in-plus"));
    }

    /// Only the keys that need the window context carry an `After`, and the ones
    /// that do must be the transport, the mark keys, or the source cut-ins: a
    /// preview that never refreshes after a mark or an edit, or a clock that
    /// never starts after Space, is the failure this guards.
    #[test]
    fn the_side_effect_of_a_key_matches_what_it_does() {
        for binding in SHIPPED_BINDINGS {
            match binding.after {
                After::Play => assert!(
                    matches!(
                        binding.id,
                        "key-play" | "key-shuttle-back" | "key-shuttle-forward"
                    ),
                    "{} starts the clock",
                    binding.id
                ),
                After::Preview => assert!(
                    binding.id.starts_with("key-mark-")
                        || matches!(binding.id, "key-source-insert" | "key-source-overwrite"),
                    "{} asks for a preview frame",
                    binding.id
                ),
                After::Nothing => {}
            }
        }
    }
}
