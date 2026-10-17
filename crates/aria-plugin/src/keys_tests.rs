//! Contracts for the chord grammar and the key table.
//!
//! Closes the class "a chord reads one way and parses another": every chord the
//! label writes is read back here, across every key and modifier combination,
//! so a key added to the table without a round trip fails. The app's shipped
//! bindings are checked against this table by the app's own tests.

use std::collections::BTreeSet;

use super::*;

/// Every modifier combination a chord can carry.
fn every_modifier(key: &'static str) -> [Chord; 8] {
    let mut out = [Chord::plain(key); 8];
    for (ix, chord) in out.iter_mut().enumerate() {
        chord.ctrl = ix & 1 != 0;
        chord.shift = ix & 2 != 0;
        chord.alt = ix & 4 != 0;
    }
    out
}

#[test]
fn every_chord_on_every_key_survives_its_own_label() {
    for key in BINDABLE_KEYS {
        for chord in every_modifier(key) {
            assert_eq!(
                Chord::parse(&chord.label()),
                Some(chord),
                "{chord:?} does not read back from {:?}",
                chord.label()
            );
        }
    }
}

#[test]
fn a_chord_reads_as_its_shortcut() {
    assert_eq!(Chord::ctrl_shift("p").label(), "Ctrl+Shift+P");
    assert_eq!(Chord::alt("i").label(), "Alt+I");
    assert_eq!(Chord::shift_alt("f1").label(), "Shift+Alt+F1");
    assert_eq!(Chord::plain("space").label(), "Space");
    assert_eq!(Chord::ctrl("pageup").label(), "Ctrl+PageUp");
    assert_eq!(Chord::plain("pagedown").label(), "PageDown");
    assert_eq!(Chord::shift("backspace").label(), "Shift+Backspace");
    assert_eq!(Chord::ctrl("+").label(), "Ctrl++");
    assert_eq!(Chord::plain("[").label(), "[");
}

/// A key reads with an upper-case first letter, as a menu row and the docs
/// write it: `P`, not `p`; `Enter`, not `enter`. Read from the table, so a key
/// added to it is covered here.
#[test]
fn every_key_reads_with_an_upper_case_first_letter() {
    for key in BINDABLE_KEYS {
        let label = Chord::plain(key).label();
        assert!(
            !label.starts_with(|first: char| first.is_ascii_lowercase()),
            "{key:?} reads as {label:?}"
        );
        assert!(
            label.eq_ignore_ascii_case(key),
            "{key:?} reads as {label:?}"
        );
    }
}

#[test]
fn a_chord_that_names_nothing_is_refused() {
    for text in [
        "",
        "+++",
        "Ctrl+",
        "Ctrl+Meta+p",
        "f13",
        "Ctrl+a+b",
        "  ",
        "Shift",
        "Ctrl+Shift",
    ] {
        assert_eq!(Chord::parse(text), None, "{text:?} parsed into a chord");
    }
}

/// The platform key and the macOS names are the same modifier: the registry has
/// one control-or-command modifier, not two.
#[test]
fn the_platform_spellings_mean_the_same_chord() {
    for text in [
        "Cmd+s",
        "Command+s",
        "Control+s",
        "ctrl+s",
        "Platform+S",
        " Ctrl + s ",
    ] {
        assert_eq!(Chord::parse(text), Some(Chord::ctrl("s")), "{text}");
    }
    assert_eq!(Chord::parse("Option+m"), Some(Chord::alt("m")));
    assert_eq!(Chord::parse("alt+ctrl+L"), Some(Chord::ctrl_alt("l")));
}

#[test]
fn a_matched_keystroke_agrees_on_every_modifier() {
    let chord = Chord::ctrl_alt("l");
    assert!(chord.matches("l", true, false, true));
    for (ctrl, shift, alt) in [
        (false, false, true),
        (true, false, false),
        (true, true, true),
    ] {
        assert!(!chord.matches("l", ctrl, shift, alt));
    }
    assert!(!chord.matches("k", true, false, true));
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

/// A modifier is not a key: a capture that took one would bind a chord nobody
/// finished typing.
#[test]
fn a_modifier_is_not_bindable() {
    for name in [
        "shift", "ctrl", "control", "alt", "cmd", "platform", "fn", "",
    ] {
        assert!(bindable(name).is_none(), "{name} was accepted as a key");
    }
}

/// Every shifted symbol reads as Shift and a key the table can bind, and a
/// bindable key is never rewritten. A symbol made bindable would stop reading
/// as its Shift chord, so the only one allowed is `+`, a binding of its own.
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

#[test]
fn every_route_reads_back_from_its_label_in_any_case() {
    for route in KeyRoute::ALL {
        assert_eq!(KeyRoute::parse(route.label()), Some(route));
        assert_eq!(
            KeyRoute::parse(&route.label().to_ascii_uppercase()),
            Some(route)
        );
    }
    assert_eq!(KeyRoute::parse("inspector"), None);
    assert_eq!(KeyRoute::default(), KeyRoute::Editor);
}
