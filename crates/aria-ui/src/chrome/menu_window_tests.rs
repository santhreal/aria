//! The Window menu's panel rows against the keys that bring panels forward.
//!
//! The class closed here: a pane key whose panel's Window row draws no chord,
//! which hides the shortcut from the menu that opens the same panel. The pane
//! keys are read off the shipped bindings, every Editor binding on Shift and a
//! digit, and the panel each one brings forward is measured by running it, so a
//! new pane key is red here until its panel's row draws it.
//!
//! Not covered: a panel key on another chord, such as Ctrl+Shift+H for History,
//! which `dock_panels_tests` lists by hand.

use crate::chrome::menu_items::items_of;
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{Binding, KeyBody, KeyRoute};
use crate::types::AppMenu;

use super::{PANEL_PREFIX, PANEL_ROWS};

/// The dock panel a Window row opens, read off its id.
fn row_panel(id: &str) -> Option<&str> {
    PANEL_ROWS
        .iter()
        .find(|(row, ..)| *row == id)
        .map(|&(_, panel, _)| panel)
        .or_else(|| id.strip_prefix(PANEL_PREFIX))
}

/// An Editor binding on Shift and one digit, the chords that bring a pane
/// forward.
fn is_pane_key(binding: &Binding) -> bool {
    let chord = binding.chord;
    binding.route == KeyRoute::Editor
        && chord.shift
        && !chord.ctrl
        && !chord.alt
        && chord.key.len() == 1
        && chord.key.bytes().all(|byte| byte.is_ascii_digit())
}

/// Each pane key, run in turn, brings a panel forward, and that panel's Window
/// row draws the key's chord.
#[gpui::test]
fn every_pane_key_is_drawn_beside_the_window_row_of_its_panel(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "window-pane-keys");
    let pane_keys: Vec<&Binding> = SHIPPED_BINDINGS
        .iter()
        .filter(|binding| is_pane_key(binding))
        .collect();
    assert!(
        !pane_keys.is_empty(),
        "no Editor binding sits on Shift and a digit"
    );
    let mut wrong = Vec::new();
    for binding in pane_keys {
        let KeyBody::Plain(run) = binding.run else {
            wrong.push(format!("{} needs the window to run", binding.id));
            continue;
        };
        let (panel, drawn, wanted) = visual.update(|_, app| {
            state.update(app, |state, _| {
                run(state);
                let panel = state.keyed_panel();
                let drawn = items_of(AppMenu::Window, state)
                    .iter()
                    .find(|item| row_panel(item.id) == Some(panel))
                    .map(|item| item.chord(state));
                (panel, drawn, state.chord_label(binding.id))
            })
        });
        match drawn {
            None => wrong.push(format!(
                "{} brings {panel} forward and Window has no row for it",
                binding.id
            )),
            Some(drawn) if wanted.is_none() || drawn != wanted => wrong.push(format!(
                "Window > {panel} draws {drawn:?}, and {} brings it forward on {wanted:?}",
                binding.id
            )),
            Some(_) => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
