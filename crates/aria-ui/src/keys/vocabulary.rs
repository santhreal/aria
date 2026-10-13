//! Which keys can carry a binding.
//!
//! A [`Chord`](super::Chord) holds a `&'static str`, because the shipped tables
//! are `const` and the registry's rows are `Copy`. An override the operator
//! typed arrives as text out of `ui-settings.json`, so it has to name a key this
//! table already holds: the table hands back its own `&'static str`, and from
//! there an override is an ordinary chord with the same matcher and the same
//! label as a shipped one.
//!
//! Leaking the typed key to get a `'static` lifetime would grow the process
//! every time a chord changed, so a key outside this table is refused and said
//! so, and the table is the thing to widen.
//!
//! The names are GPUI keystroke names, lower case, exactly as `on_key_down`
//! reports them.

/// Every key name a binding may use.
pub(super) const BINDABLE_KEYS: &[&str] = &[
    // Letters, as the tool and transport chords use them.
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    // Digits.
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    // Function keys.
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    // Navigation and editing.
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "space",
    "tab",
    "escape",
    "enter",
    "backspace",
    "delete",
    "insert",
    // Punctuation. `+` is a key in its own right on a numeric keypad, and the
    // zoom chord already uses it, so it is bindable and the label parser knows
    // it is the one key that is also the separator.
    "-",
    "=",
    "+",
    "[",
    "]",
    "\\",
    ";",
    "'",
    ",",
    ".",
    "/",
    "`",
];

/// This table's own `&'static str` for `name`, or nothing when the key cannot
/// carry a binding.
///
/// A modifier pressed on its own arrives as its own key name, which is not in
/// the table, so a capture still waiting for the real key refuses it instead of
/// binding "shift" to something.
pub(super) fn bindable(name: &str) -> Option<&'static str> {
    BINDABLE_KEYS.iter().copied().find(|key| *key == name)
}

/// Each shifted symbol on a US layout and the key under it.
///
/// GPUI on Linux reports Shift+6 as `^` with shift cleared, the way it reports
/// text, so a chord on Shift and a digit or punctuation key would never match.
/// `+` is listed for completeness and stays itself: it is bindable on its own.
pub(super) const SHIFTED_SYMBOLS: &[(&str, &str)] = &[
    ("!", "1"),
    ("@", "2"),
    ("#", "3"),
    ("$", "4"),
    ("%", "5"),
    ("^", "6"),
    ("&", "7"),
    ("*", "8"),
    ("(", "9"),
    (")", "0"),
    ("_", "-"),
    ("+", "="),
    ("{", "["),
    ("}", "]"),
    ("|", "\\"),
    (":", ";"),
    ("\"", "'"),
    ("<", ","),
    (">", "."),
    ("?", "/"),
    ("~", "`"),
];

/// The key under `name` when `name` is a shifted symbol no binding can name.
pub(super) fn unshifted(name: &str) -> Option<&'static str> {
    if bindable(name).is_some() {
        return None;
    }
    SHIFTED_SYMBOLS
        .iter()
        .find(|(symbol, _)| *symbol == name)
        .map(|(_, base)| *base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Chord;
    use crate::keys::bindings::SHIPPED_BINDINGS;
    use std::collections::BTreeSet;

    /// A shipped chord on a key this table does not name could be cleared and
    /// never typed back in: the row would be dead until the vocabulary caught
    /// up. Read out of the shipped table at run time, so a chord on a new key
    /// fails here rather than in a rebinding nobody tried.
    #[test]
    fn every_shipped_key_can_be_bound() {
        for binding in SHIPPED_BINDINGS {
            assert!(
                bindable(binding.chord.key).is_some(),
                "{} is bound to {:?}, which cannot be rebound",
                binding.id,
                binding.chord.key
            );
        }
    }

    #[test]
    fn the_vocabulary_names_each_key_once_in_lower_case() {
        let unique: BTreeSet<&str> = BINDABLE_KEYS.iter().copied().collect();
        assert_eq!(unique.len(), BINDABLE_KEYS.len(), "a key is listed twice");
        for key in BINDABLE_KEYS {
            assert!(!key.is_empty(), "an empty key name");
            assert_eq!(
                key.to_lowercase(),
                *key,
                "{key} is spelled in upper case; the shell reports lower case"
            );
        }
    }

    /// A modifier is not a key: a capture that took one would bind a chord the
    /// operator never finished typing.
    #[test]
    fn a_modifier_is_not_bindable() {
        for name in [
            "shift", "ctrl", "control", "alt", "cmd", "platform", "fn", "",
        ] {
            assert!(bindable(name).is_none(), "{name} was accepted as a key");
        }
    }

    /// The round trip the persisted override depends on: what Settings shows is
    /// what a saved chord parses back into. Enumerated, so a chord whose label
    /// cannot be read back fails here.
    #[test]
    fn every_shipped_chord_survives_its_own_label() {
        for binding in SHIPPED_BINDINGS {
            let label = binding.chord.label();
            assert_eq!(
                Chord::parse(&label),
                Some(binding.chord),
                "{} does not read back from {label:?}",
                binding.id
            );
        }
    }

    /// The plus key is the one that collides with the separator, in every
    /// modifier combination it can be typed with.
    #[test]
    fn the_plus_key_reads_back_with_every_modifier() {
        for chord in [
            Chord::plain("+"),
            Chord::ctrl("+"),
            Chord::shift("+"),
            Chord::alt("+"),
            Chord::ctrl_shift("+"),
            Chord::ctrl_alt("+"),
            Chord::shift_alt("+"),
        ] {
            assert_eq!(Chord::parse(&chord.label()), Some(chord));
        }
    }

    #[test]
    fn a_chord_that_names_nothing_is_refused() {
        for text in ["", "+++", "Ctrl+", "Ctrl+Meta+p", "f13", "Ctrl+a+b", "  "] {
            assert_eq!(Chord::parse(text), None, "{text:?} parsed into a chord");
        }
    }

    /// A chord typed with the platform key, or with macOS names, is the same
    /// chord: the registry has one control-or-command modifier, not two.
    #[test]
    fn the_platform_spellings_mean_the_same_chord() {
        for text in ["Cmd+s", "Command+s", "Control+s", "ctrl+s"] {
            assert_eq!(Chord::parse(text), Some(Chord::ctrl("s")), "{text}");
        }
        assert_eq!(Chord::parse("Option+m"), Some(Chord::alt("m")));
    }

    /// Every shifted symbol reads as Shift and a key the table can bind, and a
    /// bindable key is never rewritten. A symbol made bindable would stop
    /// reading as its Shift chord, so the only one allowed is `+`, which is a
    /// binding of its own.
    #[test]
    fn every_shifted_symbol_reads_as_a_bindable_shift_chord() {
        let symbols: BTreeSet<&str> = SHIFTED_SYMBOLS.iter().map(|(symbol, _)| *symbol).collect();
        assert_eq!(
            symbols.len(),
            SHIFTED_SYMBOLS.len(),
            "a symbol is listed twice"
        );
        for (symbol, under) in SHIFTED_SYMBOLS {
            assert!(bindable(under).is_some(), "{under} cannot carry a binding");
            if bindable(symbol).is_some() {
                assert_eq!(
                    *symbol, "+",
                    "{symbol} is bindable, so Shift+{under} is unreachable"
                );
            } else {
                assert_eq!(unshifted(symbol), Some(*under), "{symbol}");
            }
        }
        for key in BINDABLE_KEYS {
            assert_eq!(unshifted(key), None, "{key} is rewritten");
        }
    }
}
