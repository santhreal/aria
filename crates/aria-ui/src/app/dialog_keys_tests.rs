//! A dialog that is up holds the keyboard.
//!
//! The class closed here is a key that reaches the editor through a dialog. The
//! shell consumed Enter for an open dialog and sent every other key on to the
//! bindings, so typing "razor" with Settings up switched the tool and set the
//! Out mark behind it. A second route of the same class: a key capture armed in
//! Settings outlived the dialog, so after Done every key was taken as a chord
//! and Ctrl+Q no longer quit.
//!
//! What is asserted:
//!
//! - Every dialog the overlay registry draws, on every route that opens it,
//!   lets no shipped binding run: the editor state behind it is unchanged after
//!   each binding's chord goes through the shell's key handler. Dialogs and
//!   bindings are read from their registries at run time, so a new dialog or a
//!   new binding is covered by construction.
//! - Every route off Settings ends a capture armed there, including a direct
//!   write of the dialog field that no close path reaches: the next key is a
//!   key, not a chord.
//!
//! Not covered: a key a dialog's own control answers (a focused box, Enter,
//! Escape), which `field_paste_tests`, `dialog_primary_tests` and
//! `menu_dialog_tests` own; and a platform menu accelerator, which macOS
//! dispatches as an action rather than a key.

use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::app::{AppState, DialogKind};
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{Chord, KeyRoute};
use crate::test_support::drawn_shell;
use crate::types::{ActivePanel, EditorTool, UiRoute};

/// The editor state a leaked binding changes.
#[derive(Debug, Clone, PartialEq)]
struct Behind {
    revision: u64,
    playhead: i64,
    selection: Vec<aria_models::ClipId>,
    tool: EditorTool,
    marks: (Option<i64>, Option<i64>),
    playing: bool,
    looping: bool,
    linked_selection: bool,
    route: UiRoute,
    panel: ActivePanel,
    dialog: DialogKind,
    menu_open: bool,
    context_open: bool,
    scroll_x: f32,
    status: Option<String>,
    rebinding: Option<&'static str>,
}

fn behind(state: &AppState) -> Behind {
    Behind {
        revision: state.revision,
        playhead: state.playhead.value,
        selection: state.selection.clone(),
        tool: state.tool,
        marks: (state.mark_in, state.mark_out),
        playing: state.playing,
        looping: state.looping,
        linked_selection: state.linked_selection,
        route: state.route,
        panel: state.active_panel,
        dialog: state.dialog,
        menu_open: state.open_menu.is_some(),
        context_open: state.context_menu.is_some(),
        scroll_x: state.scroll_x,
        status: state.status_message.clone(),
        rebinding: state.rebinding(),
    }
}

/// The dialogs the shell can put on screen, from the registry that draws them.
fn registered_kinds(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<DialogKind> {
    let kinds = visual.read(|app| {
        crate::plugins::overlay_host::host_of(&state.read(app).ctx)
            .expect("the shell mounts the overlay host")
            .dialog_kinds()
    });
    assert!(
        kinds.len() >= 6,
        "the shipped dialogs did not register: {kinds:?}"
    );
    kinds
}

/// The keystroke a window delivers for `chord`.
fn keystroke(chord: &Chord) -> gpui::Keystroke {
    gpui::Keystroke {
        modifiers: gpui::Modifiers {
            control: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
            ..Default::default()
        },
        key: chord.key.to_owned(),
        key_char: None,
    }
}

fn press(visual: &mut VisualTestContext, chord: &Chord) {
    visual.update(|window, app| window.dispatch_keystroke(keystroke(chord), app));
}

fn edit(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    change: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            change(state);
            cx.notify();
        })
    });
}

/// Enter and Escape are a dialog's own keys: Enter runs its primary action and
/// Escape closes it, so they are not leaks.
fn dialog_key(chord: &Chord) -> bool {
    (chord.key == "escape" && !chord.ctrl)
        || matches!(
            crate::popover::classify_key(chord.key, false, chord.ctrl),
            crate::popover::MenuKey::Enter
        )
}

#[gpui::test]
fn no_binding_runs_behind_an_open_dialog(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-keys-census");
    let kinds = registered_kinds(&state, visual);
    let chords: Vec<(&str, Chord)> = SHIPPED_BINDINGS
        .iter()
        .filter(|binding| !dialog_key(&binding.chord))
        .map(|binding| (binding.id, binding.chord))
        .collect();
    assert!(chords.len() > 50, "the shipped key table did not load");
    let mut opened = [0usize; 2];
    for (at, route) in [UiRoute::Editor, UiRoute::Home].into_iter().enumerate() {
        for &kind in &kinds {
            edit(&state, visual, |state| {
                state.enter_editor();
                if route == UiRoute::Home {
                    state.go_home();
                }
                state.open_dialog(kind);
            });
            // Home opens only the dialogs it draws; the rest are refused there.
            if visual.read(|app| state.read(app).dialog) != kind {
                continue;
            }
            opened[at] += 1;
            for (id, chord) in &chords {
                // A notifying edit redraws the whole shell; clear only a
                // status a press set, since an unset one is already `None`.
                if visual.read(|app| state.read(app).status_message.is_some()) {
                    edit(&state, visual, |state| state.status_message = None);
                }
                let before = visual.read(|app| behind(state.read(app)));
                press(visual, chord);
                let after = visual.read(|app| behind(state.read(app)));
                assert_eq!(
                    after,
                    before,
                    "{id} ({}) ran behind {kind:?} on {route:?}",
                    chord.label()
                );
            }
            visual.run_until_parked();
        }
    }
    assert_eq!(
        opened[0],
        kinds.len(),
        "the editor refused a registered dialog"
    );
    assert!(opened[1] > 0, "Home opened no dialog");
}

/// The binding the capture is armed on, and a key that no binding holds.
fn armed_and_free() -> (&'static str, &'static str) {
    let armed = SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.route == KeyRoute::Editor)
        .expect("an editor binding")
        .id;
    (armed, super::rebind::tests::free_key())
}

#[gpui::test]
fn every_route_off_settings_ends_a_capture_armed_there(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-keys-capture");
    let (armed, free) = armed_and_free();
    let held = visual.read(|app| state.read(app).chord_label(armed));
    assert!(held.is_some(), "{armed} holds no chord to keep");

    let mut routes: Vec<(String, Box<dyn Fn(&mut AppState)>)> = vec![
        (
            "Done".into(),
            Box::new(|state: &mut AppState| state.open_dialog(DialogKind::None)),
        ),
        ("Home".into(), Box::new(AppState::go_home)),
        ("the editor".into(), Box::new(AppState::enter_editor)),
        // What a path that writes the field rather than calling `open_dialog`
        // does, as the unsaved prompt and the install prompt do.
        (
            "a direct write".into(),
            Box::new(|state: &mut AppState| state.dialog = DialogKind::None),
        ),
    ];
    for kind in registered_kinds(&state, visual) {
        if kind != DialogKind::Settings {
            routes.push((
                format!("opening {kind:?}"),
                Box::new(move |state: &mut AppState| state.open_dialog(kind)),
            ));
        }
    }
    for (route, close) in &routes {
        edit(&state, visual, |state| {
            state.enter_editor();
            state.open_settings();
            state.begin_rebind(armed);
        });
        assert_eq!(
            visual.read(|app| state.read(app).rebinding()),
            Some(armed),
            "the capture did not arm"
        );
        edit(&state, visual, |state| close(state));
        // Every route but the direct write goes through a call that ends the
        // capture, so the Settings row stops waiting before any key arrives.
        // The direct write is left for the key handler to end.
        if route != "a direct write" {
            assert_eq!(
                visual.read(|app| state.read(app).rebinding()),
                None,
                "{route} left the capture armed"
            );
        }
        press(visual, &Chord::plain(free));
        let (rebinding, chord) = visual.read(|app| {
            let state = state.read(app);
            (state.rebinding(), state.chord_label(armed))
        });
        assert_eq!(rebinding, None, "the capture outlived {route}");
        assert_eq!(chord, held, "the key pressed after {route} rebound {armed}");
        visual.run_until_parked();
    }
}
