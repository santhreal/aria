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

use std::sync::atomic::AtomicU64;

pub use aria_plugin::keys::{Chord, KeyRoute};
use aria_plugin::{PluginContext, Service};
use parking_lot::RwLock;

use crate::app::AppState;

mod binding_actions;
pub mod bindings;
mod bindings_edit;
mod bindings_reach;
mod bindings_reveal;
mod bindings_tools;
pub mod fields;
pub mod keyboard_image;
pub mod keymap_sets;
pub(crate) mod lone_alt;
mod overrides;
mod plugin_keys;

#[cfg(test)]
mod controls_doc_tests;
#[cfg(test)]
mod doc_tests;
#[cfg(test)]
mod guide_doc_tests;
#[cfg(test)]
mod reference_doc_tests;

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
    /// A plugin command's default chord: the binding's id is the command id,
    /// and the field is the command's title.
    Command(&'static str),
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
/// A plugin's binding reads as its command's title, through
/// [`KeyHost::label_of`].
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
    /// Every plugin key the last sync placed or refused, as `(command, title)`.
    plugin_keys: RwLock<Vec<(&'static str, &'static str)>>,
    /// The plugin keys the last sync refused because their chord was taken.
    /// Listed and rebindable, never dispatched.
    plugin_refused: RwLock<Vec<Binding>>,
    /// The plugin context's command revision at the last sync, plus one: zero
    /// is a host that has not synced.
    plugin_revision: AtomicU64,
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

    /// The binding a keystroke runs with `focus` holding the keyboard.
    ///
    /// A panel context resolves first and the window second, so a Viewer
    /// binding on an arrow outranks the window's arrow while the Viewer has
    /// focus. Bare letters and digits reach the Timeline and the Viewer only:
    /// with any other panel focused, the window's unmodified letter and digit
    /// chords do not fire. Home resolves against its own set alone.
    pub fn resolve_focused(
        &self,
        focus: KeyRoute,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Binding> {
        match focus {
            KeyRoute::Home => self.resolve(KeyRoute::Home, key, ctrl, shift, alt),
            KeyRoute::Timeline | KeyRoute::Viewer => self
                .resolve(focus, key, ctrl, shift, alt)
                .or_else(|| self.resolve(KeyRoute::Editor, key, ctrl, shift, alt)),
            KeyRoute::Editor => {
                let (named, shifted) = chord_key(key, shift);
                if is_bare(named, ctrl, shifted, alt) {
                    return None;
                }
                self.resolve(KeyRoute::Editor, key, ctrl, shift, alt)
            }
        }
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
    match aria_plugin::keys::unshifted(key) {
        Some(base) => (base, true),
        None => (key, shift),
    }
}

/// Whether a keystroke is a bare letter or digit: one alphanumeric key and no
/// modifier.
fn is_bare(key: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
    !(ctrl || shift || alt)
        && key.len() == 1
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The context a keystroke resolves against first: the focused panel's, or the
/// window's when the focused panel has no context of its own.
pub fn focus_route(state: &AppState) -> KeyRoute {
    use crate::types::{ActivePanel, UiRoute};
    if state.route == UiRoute::Home {
        return KeyRoute::Home;
    }
    match state.active_panel {
        ActivePanel::Timeline => KeyRoute::Timeline,
        ActivePanel::Preview => KeyRoute::Viewer,
        ActivePanel::Media | ActivePanel::Inspector | ActivePanel::Ai | ActivePanel::Mixer => {
            KeyRoute::Editor
        }
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

/// Run a plain binding for a key-down event, forwarding `is_held` to the J/K/L
/// transport handlers so `K` + `J`/`L` distinguishes a tap from a hold.
pub fn dispatch_plain_key_down(
    state: &mut AppState,
    binding: &Binding,
    is_held: bool,
    run: fn(&mut AppState),
) {
    match binding.id {
        "key-shuttle-stop" => state.k_down(is_held),
        "key-shuttle-back" => state.shuttle_key_down(-1, is_held),
        "key-shuttle-forward" => state.shuttle_key_down(1, is_held),
        id if id.starts_with("key-tool-") => {
            state.tool_key_down(binding.id, binding.chord.key, is_held, run);
        }
        _ => run(state),
    }
}

/// Handle a key-up event on the editor route, releasing any held `K` or
/// `K`+`J`/`L` slow-play hold. Returns `true` when transport state changed.
pub fn dispatch_key_up(
    state: &mut AppState,
    route: KeyRoute,
    key: &str,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> bool {
    if route != KeyRoute::Editor {
        return false;
    }
    let id = host_of(&state.ctx).and_then(|host| {
        host.resolve(route, key, ctrl, shift, alt)
            .or_else(|| host.resolve(route, key, false, false, false))
            .map(|b| b.id)
    });
    if state.tool_key_up(id, key) {
        return true;
    }
    let was_playing = state.playing;
    let was_k = state.k_held;
    match id.or(match key {
        "k" => Some("key-shuttle-stop"),
        "j" => Some("key-shuttle-back"),
        "l" => Some("key-shuttle-forward"),
        _ => None,
    }) {
        Some("key-shuttle-stop") => state.k_up(),
        Some("key-shuttle-back") => state.shuttle_key_up(-1),
        Some("key-shuttle-forward") => state.shuttle_key_up(1),
        _ => return false,
    }
    state.playing != was_playing || state.k_held != was_k
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

    /// A panel context outranks the window on a chord while that panel has
    /// focus, and the window answers every chord the panel leaves unbound.
    #[test]
    fn the_focused_panel_resolves_before_the_window() {
        let host = KeyHost::default();
        assert!(host.add(binding(
            "window-left",
            Chord::plain("left"),
            KeyRoute::Editor
        )));
        assert!(host.add(binding(
            "viewer-left",
            Chord::plain("left"),
            KeyRoute::Viewer
        )));
        assert!(host.add(binding("window-save", Chord::ctrl("s"), KeyRoute::Editor)));
        assert!(
            host.conflicts().is_empty(),
            "a panel chord is not a window conflict"
        );
        let left = |focus| {
            host.resolve_focused(focus, "left", false, false, false)
                .map(|b| b.id)
        };
        assert_eq!(left(KeyRoute::Viewer), Some("viewer-left"));
        assert_eq!(left(KeyRoute::Timeline), Some("window-left"));
        assert_eq!(left(KeyRoute::Editor), Some("window-left"));
        assert_eq!(left(KeyRoute::Home), None, "Home resolves its own set only");
        for focus in [KeyRoute::Viewer, KeyRoute::Timeline, KeyRoute::Editor] {
            assert_eq!(
                host.resolve_focused(focus, "s", true, false, false)
                    .map(|b| b.id),
                Some("window-save"),
                "{focus:?} lost the window chord"
            );
        }
    }

    /// Bare letters and digits reach the Timeline and the Viewer only. Every
    /// modified chord, and every bare key that is not a letter or a digit,
    /// reaches the window from any panel.
    #[test]
    fn bare_letters_and_digits_reach_the_timeline_and_viewer_only() {
        let host = KeyHost::default();
        let rows = [
            ("letter", Chord::plain("v"), "v", false, true),
            ("digit", Chord::plain("1"), "1", false, true),
            ("shift-digit", Chord::shift("1"), "1", true, false),
            ("alt-letter", Chord::alt("p"), "p", false, false),
            ("space", Chord::plain("space"), "space", false, false),
            ("bracket", Chord::plain("]"), "]", false, false),
        ];
        for (id, chord, ..) in rows {
            assert!(host.add(binding(id, chord, KeyRoute::Editor)));
        }
        for (id, chord, key, shift, bare) in rows {
            for focus in [KeyRoute::Timeline, KeyRoute::Viewer, KeyRoute::Editor] {
                let fired = host
                    .resolve_focused(focus, key, chord.ctrl, shift, chord.alt)
                    .map(|b| b.id);
                let reaches = !bare || focus != KeyRoute::Editor;
                assert_eq!(fired, reaches.then_some(id), "{id} with {focus:?} focused");
            }
        }
        // Shift+6 arrives as `^`, and is not a bare key.
        assert!(host.add(binding("mixer", Chord::shift("6"), KeyRoute::Editor)));
        assert_eq!(
            host.resolve_focused(KeyRoute::Editor, "^", false, false, false)
                .map(|b| b.id),
            Some("mixer")
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

    /// A shipped chord on a key the shared table does not hold could be cleared
    /// and never typed back in. Read out of the shipped table at run time, so a
    /// chord on a new key fails here rather than in a rebinding nobody tried.
    #[test]
    fn every_shipped_key_can_be_bound() {
        for binding in bindings::SHIPPED_BINDINGS {
            assert!(
                aria_plugin::keys::bindable(binding.chord.key).is_some(),
                "{} is bound to {:?}, which cannot be rebound",
                binding.id,
                binding.chord.key
            );
        }
    }

    /// The round trip a persisted override depends on: what Settings shows is
    /// what a saved chord parses back into.
    #[test]
    fn every_shipped_chord_survives_its_own_label() {
        for binding in bindings::SHIPPED_BINDINGS {
            let label = binding.chord.label();
            assert_eq!(
                Chord::parse(&label),
                Some(binding.chord),
                "{} does not read back from {label:?}",
                binding.id
            );
        }
    }

    #[test]
    fn shift_arrows_step_five_frames_and_transport_chords_dispatch() {
        let (mut state, _) = crate::app::premiere_tests::with_clip();
        state.clear_selection();
        state.set_playhead(10);

        let host = KeyHost::default();
        for binding in bindings::SHIPPED_BINDINGS {
            host.add(*binding);
        }
        let fire = |state: &mut AppState, key: &str, ctrl: bool, shift: bool, is_held: bool| {
            let binding = host
                .resolve(KeyRoute::Editor, key, ctrl, shift, false)
                .expect("binding exists");
            if let KeyBody::Plain(run) = binding.run {
                dispatch_plain_key_down(state, &binding, is_held, run);
            }
        };

        // Shift+Right and Shift+Left step 5 frames.
        fire(&mut state, "right", false, true, false);
        assert_eq!(state.playhead.value, 15);
        fire(&mut state, "left", false, true, false);
        assert_eq!(state.playhead.value, 10);

        // Ctrl+Shift+L toggles loop.
        assert!(!state.looping);
        fire(&mut state, "l", true, true, false);
        assert!(state.looping);
        fire(&mut state, "l", true, true, false);
        assert!(!state.looping);

        // Shift+L and Shift+J step by 10%.
        fire(&mut state, "l", false, true, false);
        assert_eq!(
            state.shuttle,
            crate::transport::ShuttleSpeed::hundredths(110)
        );
        fire(&mut state, "j", false, true, false);
        assert_eq!(
            state.shuttle,
            crate::transport::ShuttleSpeed::hundredths(100)
        );

        // K held + L tapped steps 1 frame; K held + L held plays at 1/4x until key up.
        state.set_playhead(10);
        fire(&mut state, "k", false, false, false);
        fire(&mut state, "l", false, false, false);
        assert_eq!(state.playhead.value, 11);
        assert!(!state.playing);

        fire(&mut state, "l", false, false, true);
        assert!(state.playing);
        assert_eq!(state.shuttle, crate::transport::ShuttleSpeed::QUARTER);
        assert!(dispatch_key_up(
            &mut state,
            KeyRoute::Editor,
            "l",
            false,
            false,
            false
        ));
        assert!(!state.playing);
        assert!(dispatch_key_up(
            &mut state,
            KeyRoute::Editor,
            "k",
            false,
            false,
            false
        ));
    }
}
