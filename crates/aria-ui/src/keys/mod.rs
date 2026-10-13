//! Where a key binding comes from.
//!
//! Key handling used to be one `match` in the shell, so a plugin could add a
//! command and have no way to bind a key to it, and nothing could list what the
//! app already used. A binding is data in this registry instead: the shipped
//! bindings register through the same call a plugin makes, and a chord that is
//! already taken is refused and recorded rather than shadowing the first.
//!
//! Text routing is not a binding, but it is data too: `fields` holds every text
//! box in precedence order, and the shell asks it before it asks this registry.

use aria_plugin::{PluginContext, Service};
use parking_lot::RwLock;

use crate::app::AppState;

mod binding_actions;
pub mod bindings;
mod bindings_edit;
mod bindings_reach;
mod bindings_tools;
pub mod fields;
mod overrides;
mod vocabulary;

#[cfg(test)]
mod doc_tests;

/// One chord, exactly as the shell sees it.
///
/// `ctrl` is control-or-command: the platform key is the same binding on macOS.
/// A chord matches only when every modifier agrees, so Alt+I and I are separate
/// bindings rather than one binding that inspects the modifier itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// The chord a keystroke names. The key is matched against the table's
    /// `&'static str` keys, so an unknown key resolves to nothing.
    pub fn matches(&self, key: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }

    /// How the binding reads in Settings and in the docs: `Ctrl+Shift+P`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(self.key);
        out
    }

    /// The chord a label names, or nothing when it names no bindable key.
    ///
    /// The reader for what [`Chord::label`] writes, so a rebinding survives a
    /// restart through the same spelling Settings shows. The key must be one the
    /// `keys_vocabulary` table holds: a chord is `Copy` and its key is
    /// `&'static str`, so there is nowhere for a typed key to live otherwise.
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
        let mut chord = Self::plain(vocabulary::bindable(&key.trim().to_ascii_lowercase())?);
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

/// Which surface a binding belongs to. Home shows no timeline, so its keys are a
/// separate set rather than a set of guards inside each action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRoute {
    Editor,
    Home,
}

impl KeyRoute {
    /// Which surface the binding belongs to, as the rebinding list says it.
    ///
    /// The list has to say it: two bindings may hold the same chord on different
    /// routes, and a row that did not name its surface would read as a conflict
    /// the registry allowed.
    pub fn label(self) -> &'static str {
        match self {
            KeyRoute::Editor => "Editor",
            KeyRoute::Home => "Home",
        }
    }
}

/// What the shell does after the action ran.
///
/// The two side effects a key can have beyond editing state: starting the
/// playback clock, and asking for a fresh preview frame. Both need the window
/// context, which an action does not get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    Play,
    Preview,
}

/// What a binding does when its chord fires.
///
/// Mirrors [`CommandBody`](crate::commands::host::CommandBody), and for
/// the same reason: most bindings act on the state alone and stay callable from
/// a headless test, and the ones that start work need the entity context to
/// spawn a task and hand the result back. A binding says which it is, so a key
/// for a job is one row rather than a signature every row pays for.
#[derive(Clone, Copy)]
pub enum KeyBody {
    Plain(fn(&mut AppState)),
    Windowed(crate::commands::host::WindowedRun),
}

/// One contributed binding.
#[derive(Clone, Copy)]
pub struct Binding {
    pub id: &'static str,
    pub chord: Chord,
    pub route: KeyRoute,
    pub run: KeyBody,
    pub after: After,
}
macro_rules! bind {
    ($id:literal, $chord:expr, $route:ident, $run:expr) => {
        $crate::keys::Binding {
            id: $id,
            chord: $chord,
            route: $crate::keys::KeyRoute::$route,
            run: $crate::keys::KeyBody::Plain($run),
            after: $crate::keys::After::Nothing,
        }
    };
    ($id:literal, $chord:expr, $route:ident, $run:expr, after: $after:ident) => {
        $crate::keys::Binding {
            id: $id,
            chord: $chord,
            route: $crate::keys::KeyRoute::$route,
            run: $crate::keys::KeyBody::Plain($run),
            after: $crate::keys::After::$after,
        }
    };
    ($id:literal, $chord:expr, $route:ident, windowed: $run:expr) => {
        $crate::keys::Binding {
            id: $id,
            chord: $chord,
            route: $crate::keys::KeyRoute::$route,
            run: $crate::keys::KeyBody::Windowed($run),
            after: $crate::keys::After::Nothing,
        }
    };
    ($id:literal, $chord:expr, $route:ident, windowed: $run:expr, after: $after:ident) => {
        $crate::keys::Binding {
            id: $id,
            chord: $chord,
            route: $crate::keys::KeyRoute::$route,
            run: $crate::keys::KeyBody::Windowed($run),
            after: $crate::keys::After::$after,
        }
    };
}
pub(crate) use bind;

/// A binding that was refused because its chord was already taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict {
    /// The binding that keeps the chord.
    pub held_by: &'static str,
    /// The binding that was refused.
    pub refused: &'static str,
    pub chord: Chord,
}

/// One line of the rebinding surface.
#[derive(Clone, Copy)]
pub struct BindingRow {
    pub binding: Binding,
    /// The chord it answers to now, or nothing when the operator cleared it.
    pub chord: Option<Chord>,
    /// Whether that chord is the operator's choice rather than the shipped one.
    pub overridden: bool,
}

/// Why a rebinding was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOverrideError {
    /// No binding by that id, so there is nothing to rebind. A saved override
    /// for a plugin that is no longer installed lands here.
    Unknown,
    /// Another binding on the same route already answers that chord.
    Taken(KeyConflict),
}

/// How a binding reads in Settings: `key-shuttle-back` becomes "Shuttle back".
///
/// Derived rather than a fifth column on [`Binding`]: the id is the only name
/// the registry holds, and a label table would be a second list to keep in step.
/// A plugin's binding gets a readable name by naming its id the way the shipped
/// rows do.
pub fn binding_label(id: &str) -> String {
    let stem = id.strip_prefix("key-").unwrap_or(id);
    let mut out = String::with_capacity(stem.len());
    for word in stem.split('-').filter(|word| !word.is_empty()) {
        if out.is_empty() {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push(' ');
            out.push_str(word);
        }
    }
    out
}

/// Every binding the shell can dispatch, in registration order, and the chords
/// the operator put in place of the shipped ones.
///
/// The rebindings are held here rather than beside the surface that edits them,
/// because dispatch is the thing that has to agree with them: a chord shown in
/// Settings and a chord that fires come from one table either way. The rows are
/// in `keys_overrides.rs`.
#[derive(Default)]
pub struct KeyHost {
    bindings: RwLock<Vec<Binding>>,
    conflicts: RwLock<Vec<KeyConflict>>,
    /// Rebindings by binding id. A `None` chord is a binding the operator
    /// cleared, which is not the same as one that was never touched, so an
    /// absent row and a `None` row mean different things.
    overrides: RwLock<Vec<(&'static str, Option<Chord>)>>,
}

impl Service for KeyHost {}

impl KeyHost {
    /// Add a binding, or refuse it when its chord is taken on the same route.
    ///
    /// First registration wins, and the refusal is data: a plugin that tries to
    /// take Ctrl+S must not be able to silently break saving, and a user has to
    /// be able to see why its key does nothing.
    pub fn add(&self, binding: Binding) -> bool {
        let mut bindings = self.bindings.write();
        if let Some(held) = bindings
            .iter()
            .find(|held| held.route == binding.route && held.chord == binding.chord)
        {
            self.conflicts.write().push(KeyConflict {
                held_by: held.id,
                refused: binding.id,
                chord: binding.chord,
            });
            return false;
        }
        bindings.push(binding);
        true
    }

    /// The binding for one keystroke, or nothing when the chord is unbound.
    ///
    /// Reads the effective chord, so a rebinding takes effect at the next
    /// keystroke and a cleared binding answers nothing at all. The keystroke is
    /// read through [`chord_key`], so Shift+6 reported as `^` finds Shift+6.
    pub fn resolve(
        &self,
        route: KeyRoute,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Binding> {
        let (key, shift) = chord_key(key, shift);
        let overrides = self.overrides.read();
        self.bindings
            .read()
            .iter()
            .find(|binding| {
                binding.route == route
                    && Self::chord_for(overrides.as_slice(), binding)
                        .is_some_and(|chord| chord.matches(key, ctrl, shift, alt))
            })
            .copied()
    }

    /// Every binding, in registration order. For Settings and the tests.
    pub fn all(&self) -> Vec<Binding> {
        self.bindings.read().clone()
    }

    /// Bindings that were refused, in the order they were refused.
    pub fn conflicts(&self) -> Vec<KeyConflict> {
        self.conflicts.read().clone()
    }
}

/// A reported keystroke as a chord names it: `(key, shift)`.
///
/// A shifted symbol no binding can name, such as `^` from Shift+6 on Linux,
/// reads as Shift and the key under it on a US layout. Any other keystroke is
/// unchanged. Text input does not pass through here: a text box wants the `^`.
pub fn chord_key(key: &str, shift: bool) -> (&str, bool) {
    match vocabulary::unshifted(key) {
        Some(base) => (base, true),
        None => (key, shift),
    }
}

/// The registry the shell reads, or nothing when no host mounted it.
pub fn host_of(ctx: &PluginContext) -> Option<std::sync::Arc<KeyHost>> {
    ctx.try_get::<KeyHost>()
}

/// The mounted registry, held where a reader with no plugin context can see it.
///
/// A tooltip is built from an element id and an `&mut App` and nothing else, so
/// without this it could only read the shipped table and would keep naming a
/// chord the operator has since rebound. One instance either way: the `Arc` the
/// plugin mount registered.
pub struct MountedKeys(std::sync::Arc<KeyHost>);

impl gpui::Global for MountedKeys {}

/// Publish the mounted registry. Called once, when the window's shell is built.
pub fn set_mounted(host: std::sync::Arc<KeyHost>, cx: &mut gpui::App) {
    cx.set_global(MountedKeys(host));
}

/// The registry a bare `App` can see, or nothing before a shell published one.
pub fn mounted(cx: &gpui::App) -> Option<&KeyHost> {
    cx.try_global::<MountedKeys>().map(|keys| &*keys.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_: &mut AppState) {}

    fn binding(id: &'static str, chord: Chord, route: KeyRoute) -> Binding {
        Binding {
            id,
            chord,
            route,
            run: KeyBody::Plain(noop),
            after: After::Nothing,
        }
    }

    /// A chord is exact: Alt+I and I are different keys, so an action cannot leak
    /// into a modifier combination nobody bound.
    #[test]
    fn a_chord_matches_only_its_own_modifiers() {
        let host = KeyHost::default();
        assert!(host.add(binding("mark-in", Chord::plain("i"), KeyRoute::Editor)));
        assert!(
            host.resolve(KeyRoute::Editor, "i", false, false, false)
                .is_some()
        );
        assert!(
            host.resolve(KeyRoute::Editor, "i", false, false, true)
                .is_none(),
            "Alt+I resolved to the plain I binding"
        );
        assert!(
            host.resolve(KeyRoute::Editor, "i", true, false, false)
                .is_none()
        );
    }

    /// Home and the editor bind the same chords to different actions, so the route
    /// is part of the lookup rather than a guard inside each action.
    #[test]
    fn the_same_chord_binds_once_per_route() {
        let host = KeyHost::default();
        assert!(host.add(binding("editor-open", Chord::ctrl("o"), KeyRoute::Editor)));
        assert!(host.add(binding("home-open", Chord::ctrl("o"), KeyRoute::Home)));
        assert!(host.conflicts().is_empty());
        assert_eq!(
            host.resolve(KeyRoute::Home, "o", true, false, false)
                .map(|b| b.id),
            Some("home-open")
        );
    }

    /// A second binding on a taken chord is refused, not silently preferred: a
    /// plugin must not be able to break Ctrl+S, and the reason has to be visible.
    #[test]
    fn a_taken_chord_is_refused_and_recorded() {
        let host = KeyHost::default();
        assert!(host.add(binding("save", Chord::ctrl("s"), KeyRoute::Editor)));
        assert!(!host.add(binding("plugin-save", Chord::ctrl("s"), KeyRoute::Editor)));
        assert_eq!(
            host.resolve(KeyRoute::Editor, "s", true, false, false)
                .map(|b| b.id),
            Some("save")
        );
        assert_eq!(
            host.conflicts(),
            vec![KeyConflict {
                held_by: "save",
                refused: "plugin-save",
                chord: Chord::ctrl("s"),
            }]
        );
    }

    /// The label is what Settings and the docs show.
    #[test]
    fn a_chord_reads_as_its_shortcut() {
        assert_eq!(Chord::ctrl_shift("p").label(), "Ctrl+Shift+p");
        assert_eq!(Chord::alt("i").label(), "Alt+i");
        assert_eq!(Chord::plain("space").label(), "space");
    }
}
