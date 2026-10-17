//! Keys: chords, the keys a chord may name, and the default chord a plugin gives
//! a command.
//!
//! The grammar and the key table are defined here rather than in the app, so a
//! package's `key = "Ctrl+Alt+L"` is checked when its manifest is read, by
//! `aria plugin check` and by Aria alike, against the table the app binds from.
//!
//! A [`Chord`] holds a `&'static str` key, so the shipped binding tables are
//! `const` and the app's binding rows are `Copy`. A chord read from text (a
//! manifest, a saved rebinding) has to name a key [`BINDABLE_KEYS`] holds: the
//! table hands back its own `&'static str`. A key outside the table is refused,
//! and the table is the thing to widen. The names are GPUI keystroke names, lower
//! case, as `on_key_down` reports them; [`Chord::label`] writes them with an
//! upper-case first letter.

/// One chord, as the shell sees a keystroke.
///
/// `ctrl` is control-or-command: the platform key is the same binding on macOS.
/// A chord matches only when every modifier agrees, so Alt+I and I are separate
/// bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: &'static str,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub const fn plain(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl_shift(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
            alt: false,
        }
    }

    pub const fn ctrl_alt(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: false,
            alt: true,
        }
    }

    pub const fn shift(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: true,
            alt: false,
        }
    }

    pub const fn alt(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: true,
        }
    }

    pub const fn shift_alt(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: true,
            alt: true,
        }
    }

    pub const fn ctrl_shift_alt(key: &'static str) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
            alt: true,
        }
    }

    /// Whether a keystroke is this chord.
    pub fn matches(&self, key: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }

    /// How the chord reads on a menu row, in a tooltip, in Settings and in the
    /// docs: `Ctrl+Shift+P`, `Shift+Alt+F1`, `Space`, `PageUp`, `Ctrl++`. The
    /// key's first letter is upper case; [`Chord::parse`] reads a key name in
    /// any case, so the label reads back as the same chord.
    pub fn label(&self) -> String {
        let mut out = String::with_capacity(self.key.len() + 16);
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        match self.key {
            "pageup" => out.push_str("PageUp"),
            "pagedown" => out.push_str("PageDown"),
            key => {
                let mut chars = key.chars();
                if let Some(first) = chars.next() {
                    out.push(first.to_ascii_uppercase());
                    out.push_str(chars.as_str());
                }
            }
        }
        out
    }

    /// The chord `text` names, or nothing when it names no bindable key.
    ///
    /// The reader for what [`Chord::label`] writes. Modifiers are `Ctrl`,
    /// `Shift` and `Alt`, in any case and order; `Cmd`, `Command`, `Control` and
    /// `Platform` read as `Ctrl`, and `Option` as `Alt`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        // `+` is a key and the separator both. Taken off the end first: "+" is
        // the plus key, "Ctrl++" is the plus key with a modifier, and "Ctrl+"
        // names no key at all.
        let (modifiers, key) = match text.strip_suffix('+') {
            Some("") => ("", "+"),
            Some(head) if head.ends_with('+') => (&head[..head.len() - 1], "+"),
            _ => match text.rsplit_once('+') {
                Some((head, key)) => (head, key),
                None => ("", text),
            },
        };
        let mut chord = Self::plain(bindable(&key.trim().to_ascii_lowercase())?);
        if modifiers.is_empty() {
            return Some(chord);
        }
        for part in modifiers.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "platform" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                _ => return None,
            }
        }
        Some(chord)
    }
}

/// Which surface a binding answers on.
///
/// `Editor` is the editor window. `Timeline` and `Viewer` are panel contexts: a
/// keystroke resolves against the focused panel's context first and the
/// window's second, so a panel binding outranks a window binding on the same
/// chord while that panel holds focus. Home shows no timeline, so its keys are
/// a separate set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum KeyRoute {
    #[default]
    Editor,
    Timeline,
    Viewer,
    Home,
}

impl KeyRoute {
    /// Every route, in the order Settings lists them.
    pub const ALL: [Self; 4] = [Self::Editor, Self::Timeline, Self::Viewer, Self::Home];

    /// The surface's name, as Settings and `plugin.toml` spell it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Timeline => "Timeline",
            Self::Viewer => "Viewer",
            Self::Home => "Home",
        }
    }

    /// The route `text` names, in any case.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|route| route.label().eq_ignore_ascii_case(text.trim()))
    }
}

/// The chord a command answers to until the user rebinds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDefault {
    /// The command the chord runs.
    pub command: String,
    pub chord: Chord,
    pub route: KeyRoute,
}

/// Every key name a chord may use.
pub const BINDABLE_KEYS: &[&str] = &[
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
    // zoom chord uses it, so it is bindable and the parser treats it as the one
    // key that is also the separator.
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

/// The table's own `&'static str` for `name`, or nothing when the key cannot
/// carry a binding.
///
/// A modifier pressed on its own arrives as its own key name, which is not in
/// the table, so a capture still waiting for the real key refuses it.
pub fn bindable(name: &str) -> Option<&'static str> {
    BINDABLE_KEYS.iter().copied().find(|key| *key == name)
}

/// Each shifted symbol on a US layout and the key under it.
///
/// GPUI on Linux reports Shift+6 as `^` with shift cleared, the way it reports
/// text, so a chord on Shift and a digit or punctuation key would never match.
/// `+` is listed for completeness and stays itself: it is bindable on its own.
pub const SHIFTED_SYMBOLS: &[(&str, &str)] = &[
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
pub fn unshifted(name: &str) -> Option<&'static str> {
    if bindable(name).is_some() {
        return None;
    }
    SHIFTED_SYMBOLS
        .iter()
        .find(|(symbol, _)| *symbol == name)
        .map(|(_, base)| *base)
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
