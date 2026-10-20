//! The key bindings the editor ships.
//!
//! A table, not a `match`: the tests below enumerate it, so a chord added here is
//! covered without a second list, and `KeyHost::add` rejects a chord that is
//! already taken rather than shadowing the first.
//!
//! The rows are in four sibling files, split by what a chord does:
//! `bindings_edit`, `bindings_tools`, `bindings_reach` and `bindings_reveal`.
//! `SHIPPED_BINDINGS` is their concatenation, joined at compile time from the
//! groups' own lengths. No count is written down, so a chord added to any group
//! is in the table by construction, and the conflict test below checks it.

use super::Binding;
use super::bindings_edit::EDIT_ROWS;
use super::bindings_reach::REACH_ROWS;
use super::bindings_reveal::REVEAL_ROWS;
use super::bindings_tools::TOOL_ROWS;
use crate::const_table;

/// The row groups, in registration order: editor first, Home last.
const GROUPS: &[&[Binding]] = &[EDIT_ROWS, TOOL_ROWS, REACH_ROWS, REVEAL_ROWS];

/// How many bindings the groups hold between them.
const TOTAL: usize = const_table::total(GROUPS);

/// Editor-route bindings, then Home-route ones, then the reach chords.
pub const SHIPPED_BINDINGS: &[Binding] = &const_table::joined::<TOTAL, Binding>(GROUPS);

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

    /// Chords `docs/spec/keyboard.md` names resolve to the binding that does
    /// what the spec says, on the route the spec puts them on. A row moved to a
    /// different chord, or a chord taken by the wrong row, turns this red.
    #[test]
    fn the_documented_chords_still_resolve() {
        let host = KeyHost::default();
        for binding in SHIPPED_BINDINGS {
            host.add(*binding);
        }
        const C: bool = true;
        const N: bool = false;
        let cases: &[(KeyRoute, &str, bool, bool, bool, &str)] = &[
            (KeyRoute::Editor, "space", N, N, N, "key-play"),
            (KeyRoute::Editor, "enter", N, N, N, "key-render-in-to-out"),
            (KeyRoute::Editor, "k", C, N, N, "key-split"),
            (KeyRoute::Editor, "k", C, C, N, "key-split-all"),
            (KeyRoute::Editor, "s", N, N, N, "key-snap"),
            (KeyRoute::Editor, "s", C, N, N, "key-save"),
            (KeyRoute::Editor, "s", C, C, N, "key-save-as"),
            (KeyRoute::Editor, "s", C, N, C, "key-save-copy"),
            (KeyRoute::Editor, "z", C, N, N, "key-undo"),
            (KeyRoute::Editor, "z", C, C, N, "key-redo"),
            (KeyRoute::Editor, "y", C, N, N, "key-redo-y"),
            (KeyRoute::Editor, "p", C, C, N, "key-palette"),
            (KeyRoute::Editor, "i", N, N, N, "key-mark-in"),
            (KeyRoute::Editor, "i", N, C, N, "key-mark-in-go"),
            (KeyRoute::Editor, "i", C, C, N, "key-mark-in-clear"),
            (KeyRoute::Editor, "o", N, N, N, "key-mark-out"),
            (KeyRoute::Editor, "o", C, C, N, "key-mark-out-clear"),
            (KeyRoute::Editor, "x", C, C, N, "key-mark-clear"),
            (KeyRoute::Editor, "x", N, N, N, "key-mark-clip"),
            (KeyRoute::Editor, "o", C, N, N, "key-open"),
            (KeyRoute::Editor, "m", N, N, N, "key-marker-add"),
            (KeyRoute::Editor, "m", N, C, N, "key-marker-next"),
            (KeyRoute::Editor, "m", C, C, N, "key-marker-prev"),
            (KeyRoute::Editor, "m", C, N, C, "key-marker-remove"),
            (KeyRoute::Editor, "m", C, C, C, "key-marker-clear-all"),
            (KeyRoute::Editor, "f", N, N, N, "key-match-frame"),
            (KeyRoute::Editor, "\\", N, N, N, "key-fit"),
            (KeyRoute::Editor, "f", N, C, N, "key-fit-selection"),
            (KeyRoute::Editor, ";", N, N, N, "key-lift"),
            (KeyRoute::Editor, "'", N, N, N, "key-extract"),
            (KeyRoute::Editor, "q", N, N, N, "key-trim-in"),
            (KeyRoute::Editor, "w", N, N, N, "key-trim-out"),
            (KeyRoute::Editor, "[", N, N, C, "key-trim-in-keep-gap"),
            (KeyRoute::Editor, "]", N, N, C, "key-trim-out-keep-gap"),
            (KeyRoute::Editor, "delete", N, N, N, "key-delete-forward"),
            (
                KeyRoute::Editor,
                "delete",
                N,
                C,
                N,
                "key-ripple-delete-forward",
            ),
            (KeyRoute::Editor, "left", C, N, N, "key-trim-back"),
            (KeyRoute::Editor, "right", C, C, N, "key-trim-forward-fast"),
            (KeyRoute::Editor, "up", N, N, N, "key-edit-prev"),
            (KeyRoute::Editor, "down", N, C, N, "key-edit-next-all"),
            (KeyRoute::Editor, "d", N, N, N, "key-select-at-playhead"),
            (KeyRoute::Editor, "a", C, C, N, "key-deselect-all"),
            (KeyRoute::Editor, "l", C, N, N, "key-link"),
            (KeyRoute::Editor, "t", C, N, N, "key-transition"),
            (KeyRoute::Editor, "t", C, C, N, "key-audio-transition"),
            (KeyRoute::Editor, "]", N, N, N, "key-gain-up"),
            (KeyRoute::Editor, "[", N, C, N, "key-gain-down-fast"),
            (KeyRoute::Editor, "=", C, N, N, "key-tracks-taller"),
            (KeyRoute::Editor, "v", C, N, N, "key-paste"),
            (KeyRoute::Editor, "v", C, C, N, "key-paste-insert"),
            (KeyRoute::Editor, "v", C, N, C, "key-paste-attributes"),
            (KeyRoute::Editor, "x", C, N, N, "key-cut"),
            (KeyRoute::Editor, "i", C, N, N, "key-import"),
            (KeyRoute::Editor, "m", C, N, N, "key-export"),
            (KeyRoute::Editor, "n", C, N, C, "key-new"),
            (KeyRoute::Editor, "n", C, N, N, "key-new-sequence"),
            (KeyRoute::Editor, "e", C, C, N, "key-export-frame"),
            (KeyRoute::Editor, "e", N, C, N, "key-clip-enable"),
            (KeyRoute::Editor, "g", C, N, N, "key-clip-group"),
            (KeyRoute::Editor, "g", C, C, N, "key-clip-ungroup"),
            (KeyRoute::Editor, "r", C, N, N, "key-speed-duration"),
            (KeyRoute::Editor, "t", C, N, C, "key-time-remap"),
            (KeyRoute::Editor, "g", N, N, N, "key-audio-gain"),
            (KeyRoute::Editor, "r", C, C, N, "key-rulers"),
            (KeyRoute::Editor, "f10", N, N, N, "key-menu-bar"),
            (KeyRoute::Editor, "w", C, N, N, "key-pane-close"),
            (KeyRoute::Editor, "w", C, C, N, "key-close"),
            (KeyRoute::Editor, "f", C, N, N, "key-find"),
            (KeyRoute::Editor, "k", C, N, C, "key-keyboard-shortcuts"),
            (KeyRoute::Editor, "q", C, N, N, "key-quit"),
            (KeyRoute::Editor, "h", C, C, N, "key-history"),
            (KeyRoute::Editor, "left", N, C, N, "key-step-back-fast"),
            (KeyRoute::Editor, "left", N, C, C, "key-keyframe-prev"),
            (KeyRoute::Editor, "p", N, N, C, "key-reveal-position"),
            (KeyRoute::Editor, "p", N, C, C, "key-reveal-position-add"),
            (KeyRoute::Editor, "s", N, N, C, "key-reveal-scale"),
            (KeyRoute::Editor, "r", N, N, C, "key-reveal-rotation"),
            (KeyRoute::Editor, "t", N, N, C, "key-reveal-opacity"),
            (KeyRoute::Editor, "a", N, N, C, "key-reveal-anchor"),
            (KeyRoute::Editor, "e", N, N, C, "key-reveal-effects"),
            (KeyRoute::Editor, "m", N, N, C, "key-reveal-masks"),
            (KeyRoute::Editor, "l", N, N, C, "key-reveal-audio"),
            (KeyRoute::Editor, "u", N, N, C, "key-reveal-animated"),
            (KeyRoute::Editor, "`", N, N, C, "key-reveal-close"),
            (KeyRoute::Editor, "f9", N, N, N, "key-easy-ease"),
            (KeyRoute::Editor, "f9", N, C, N, "key-easy-ease-in"),
            (KeyRoute::Editor, "f9", C, C, N, "key-easy-ease-out"),
            (KeyRoute::Editor, "h", C, N, C, "key-toggle-hold"),
            (KeyRoute::Editor, "f3", N, C, N, "key-graph-editor"),
            (KeyRoute::Editor, "1", N, C, N, "key-pane-project"),
            (KeyRoute::Editor, "2", N, C, N, "key-pane-viewer"),
            (KeyRoute::Editor, "5", N, C, N, "key-pane-effects"),
            (KeyRoute::Editor, "6", N, C, N, "key-mixer"),
            (KeyRoute::Editor, "8", N, C, N, "key-pane-info"),
            (KeyRoute::Editor, "9", N, C, N, "key-pane-audio-meters"),
            (KeyRoute::Editor, "1", N, C, C, "key-workspace-1"),
            (KeyRoute::Editor, "tab", C, N, N, "key-pane-next"),
            (KeyRoute::Editor, "0", N, C, N, "key-assistant"),
            (KeyRoute::Editor, "tab", C, C, N, "key-pane-prev"),
            (KeyRoute::Editor, "`", N, N, N, "key-pane-maximize"),
            (KeyRoute::Editor, "a", N, N, N, "key-tool-track-forward"),
            (KeyRoute::Editor, "a", N, C, N, "key-tool-track-backward"),
            (KeyRoute::Editor, "b", N, N, N, "key-tool-ripple"),
            (KeyRoute::Editor, "c", N, N, N, "key-tool-razor"),
            (KeyRoute::Viewer, "left", N, N, N, "key-viewer-left"),
            (KeyRoute::Viewer, "up", N, C, N, "key-viewer-up-fast"),
            (KeyRoute::Viewer, "/", N, C, N, "key-viewer-fit"),
            (KeyRoute::Home, "n", C, N, C, "key-home-new"),
            (KeyRoute::Home, "o", C, N, N, "key-home-open"),
            (KeyRoute::Home, "q", C, N, N, "key-home-quit"),
            (KeyRoute::Home, "escape", N, N, N, "key-home-escape"),
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
                        "key-play"
                            | "key-play-work-area"
                            | "key-shuttle-back"
                            | "key-shuttle-forward"
                            | "key-shuttle-slower"
                            | "key-shuttle-faster"
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
