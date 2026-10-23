//! The keyboard Settings draws: where each key sits, and what each key holds.
//!
//! A US layout, because key names are read by their position on one
//! ([`aria_plugin::keys::SHIFTED_SYMBOLS`]). Every key a chord can name is on
//! the image once; the modifier caps choose which chords the image shows, and
//! the keys no binding can use are drawn and hold nothing.
//!
//! A key takes the colour of the kind of command it holds. The kind is the
//! route a binding answers on, or Plugin for a command a plugin contributed:
//! an Editor key works anywhere in the editor, a Timeline or Viewer key only
//! while that panel holds focus, a Home key only on the start screen. One key
//! can hold a binding on each route, and is drawn with one band per kind.

use std::collections::BTreeMap;

use super::{Binding, BindingRow, Chord, KeyBody, KeyRoute};

/// A modifier the image can hold down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

impl Modifier {
    pub const ALL: [Self; 3] = [Self::Ctrl, Self::Shift, Self::Alt];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Shift => "Shift",
            Self::Alt => "Alt",
        }
    }
}

/// The modifiers the image shows chords for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub fn held(self, modifier: Modifier) -> bool {
        match modifier {
            Modifier::Ctrl => self.ctrl,
            Modifier::Shift => self.shift,
            Modifier::Alt => self.alt,
        }
    }

    pub fn toggled(mut self, modifier: Modifier) -> Self {
        match modifier {
            Modifier::Ctrl => self.ctrl = !self.ctrl,
            Modifier::Shift => self.shift = !self.shift,
            Modifier::Alt => self.alt = !self.alt,
        }
        self
    }

    /// `key` with these modifiers.
    pub fn chord(self, key: &'static str) -> Chord {
        Chord {
            key,
            ctrl: self.ctrl,
            shift: self.shift,
            alt: self.alt,
        }
    }
}

/// What one cap on the image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapKind {
    /// A key a chord can name, by its name in the key vocabulary.
    Key(&'static str),
    /// A modifier: its cap holds the modifier down on the image.
    Modifier(Modifier),
    /// A key no binding can use.
    Inert,
    /// Space between groups of keys.
    Gap,
}

/// One cap: what it is, what it reads, and its width in key units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cap {
    pub kind: CapKind,
    pub label: &'static str,
    pub width: f32,
}

const fn key(name: &'static str, label: &'static str) -> Cap {
    wide(name, label, 1.0)
}

const fn wide(name: &'static str, label: &'static str, width: f32) -> Cap {
    Cap {
        kind: CapKind::Key(name),
        label,
        width,
    }
}

const fn modifier(modifier: Modifier, width: f32) -> Cap {
    Cap {
        kind: CapKind::Modifier(modifier),
        label: modifier.label(),
        width,
    }
}

const fn inert(label: &'static str, width: f32) -> Cap {
    Cap {
        kind: CapKind::Inert,
        label,
        width,
    }
}

const fn gap(width: f32) -> Cap {
    Cap {
        kind: CapKind::Gap,
        label: "",
        width,
    }
}

/// The width of every row of [`MAIN`], in key units.
pub const MAIN_UNITS: f32 = 15.0;

/// The main block, row by row.
pub const MAIN: [&[Cap]; 6] = [
    &[
        key("escape", "Esc"),
        gap(1.0),
        key("f1", "F1"),
        key("f2", "F2"),
        key("f3", "F3"),
        key("f4", "F4"),
        gap(0.5),
        key("f5", "F5"),
        key("f6", "F6"),
        key("f7", "F7"),
        key("f8", "F8"),
        gap(0.5),
        key("f9", "F9"),
        key("f10", "F10"),
        key("f11", "F11"),
        key("f12", "F12"),
    ],
    &[
        key("`", "`"),
        key("1", "1"),
        key("2", "2"),
        key("3", "3"),
        key("4", "4"),
        key("5", "5"),
        key("6", "6"),
        key("7", "7"),
        key("8", "8"),
        key("9", "9"),
        key("0", "0"),
        key("-", "-"),
        key("=", "="),
        wide("backspace", "Bksp", 2.0),
    ],
    &[
        wide("tab", "Tab", 1.5),
        key("q", "Q"),
        key("w", "W"),
        key("e", "E"),
        key("r", "R"),
        key("t", "T"),
        key("y", "Y"),
        key("u", "U"),
        key("i", "I"),
        key("o", "O"),
        key("p", "P"),
        key("[", "["),
        key("]", "]"),
        wide("\\", "\\", 1.5),
    ],
    &[
        inert("Caps", 1.75),
        key("a", "A"),
        key("s", "S"),
        key("d", "D"),
        key("f", "F"),
        key("g", "G"),
        key("h", "H"),
        key("j", "J"),
        key("k", "K"),
        key("l", "L"),
        key(";", ";"),
        key("'", "'"),
        wide("enter", "Enter", 2.25),
    ],
    &[
        modifier(Modifier::Shift, 2.25),
        key("z", "Z"),
        key("x", "X"),
        key("c", "C"),
        key("v", "V"),
        key("b", "B"),
        key("n", "N"),
        key("m", "M"),
        key(",", ","),
        key(".", "."),
        key("/", "/"),
        modifier(Modifier::Shift, 2.75),
    ],
    &[
        modifier(Modifier::Ctrl, 1.5),
        inert("Super", 1.5),
        modifier(Modifier::Alt, 1.5),
        wide("space", "Space", 4.5),
        modifier(Modifier::Alt, 1.5),
        inert("Super", 1.5),
        inert("Menu", 1.5),
        modifier(Modifier::Ctrl, 1.5),
    ],
];

/// The editing and arrow keys, and the keypad plus, under the main block.
pub const NAV: [&[Cap]; 2] = [
    &[
        wide("insert", "Ins", 1.5),
        wide("home", "Home", 1.5),
        wide("pageup", "PgUp", 1.5),
        gap(1.0),
        gap(1.0),
        key("up", "↑"),
        gap(1.0),
        gap(1.0),
        wide("+", "Num +", 2.0),
    ],
    &[
        wide("delete", "Del", 1.5),
        wide("end", "End", 1.5),
        wide("pagedown", "PgDn", 1.5),
        gap(1.0),
        key("left", "←"),
        key("down", "↓"),
        key("right", "→"),
    ],
];

/// Every row of the image, main block first.
pub fn rows() -> impl Iterator<Item = &'static [Cap]> {
    MAIN.into_iter().chain(NAV)
}

/// The kind of command a binding holds, which is the colour of its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Editor,
    Timeline,
    Viewer,
    Home,
    Plugin,
}

impl CommandKind {
    /// Every kind, in the order the legend lists them.
    pub const ALL: [Self; 5] = [
        Self::Editor,
        Self::Timeline,
        Self::Viewer,
        Self::Home,
        Self::Plugin,
    ];

    pub fn of(binding: &Binding) -> Self {
        if matches!(binding.run, KeyBody::Command(_)) {
            return Self::Plugin;
        }
        match binding.route {
            KeyRoute::Editor => Self::Editor,
            KeyRoute::Timeline => Self::Timeline,
            KeyRoute::Viewer => Self::Viewer,
            KeyRoute::Home => Self::Home,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Timeline => "Timeline",
            Self::Viewer => "Viewer",
            Self::Home => "Home",
            Self::Plugin => "Plugin",
        }
    }
}

/// One binding a key holds with the image's modifiers down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub id: &'static str,
    pub kind: CommandKind,
}

/// What each key holds with `mods` down, by key name, in registration order.
///
/// Read from the rows Settings lists, so a rebound key is drawn where it
/// dispatches and a cleared binding is on no key.
pub fn held_by_key(rows: &[BindingRow], mods: Mods) -> BTreeMap<&'static str, Vec<Held>> {
    let mut out: BTreeMap<&'static str, Vec<Held>> = BTreeMap::new();
    for row in rows {
        let Some(chord) = row.chord else {
            continue;
        };
        if chord.ctrl != mods.ctrl || chord.shift != mods.shift || chord.alt != mods.alt {
            continue;
        }
        out.entry(chord.key).or_default().push(Held {
            id: row.binding.id,
            kind: CommandKind::of(&row.binding),
        });
    }
    out
}

/// The kinds a key holds, each once, in legend order.
pub fn kinds(held: &[Held]) -> impl Iterator<Item = CommandKind> + '_ {
    CommandKind::ALL
        .into_iter()
        .filter(|kind| held.iter().any(|held| held.kind == *kind))
}

#[cfg(test)]
#[path = "keyboard_image_tests.rs"]
mod tests;
