//! The menu bar driven from the keyboard.
//!
//! The class closed here: the bar had no keyboard path at all. F10 and a lone
//! Alt opened nothing, and an open drop ignored the arrows, so every menu row
//! was a pointer-only control. Every case reads its menus from
//! [`AppMenu::ALL`] and its rows from the registry the bar draws, so a menu or a
//! row added later is walked by the same assertions.
//!
//! Not covered here: the drawn light itself, and the shell's routing of a key
//! to [`AppState::menu_key`] ahead of a focused box and the bindings. The lone-Alt detector is
//! covered in `keys::lone_alt`.

use aria_plugin::PluginContext;
use gpui::AppContext;

use super::AppState;
use super::MenuNav;
use crate::app::DialogKind;
use crate::chrome::menu_items::{DropEntry, drop_entries};
use crate::keys::{self, KeyBody, KeyRoute};
use crate::plugins::UiPlugin;
use crate::types::{AppMenu, UiRoute};

fn state() -> AppState {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
    let mut state = AppState::new(ctx, crate::scratch_pool::scratch("menu-keys")).expect("state");
    state.enter_editor();
    state
}

/// Run `f` against the state as an entity, the way the shell holds it.
fn with_entity<R>(
    state: AppState,
    f: impl FnOnce(&mut AppState, &mut gpui::Context<AppState>) -> R,
) -> R {
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| entity.update(app, f))
}

/// What one drop draws: its entries' captions, a submenu counting as one.
fn captions(state: &AppState, menu: AppMenu) -> Vec<String> {
    let rows = state.menu_rows(menu);
    drop_entries(&rows)
        .iter()
        .map(|entry| entry.caption(&rows).to_string())
        .collect()
}

/// The drop entry of `menu` that is the row `id`.
fn entry_of(state: &AppState, menu: AppMenu, id: &str) -> usize {
    let rows = state.menu_rows(menu);
    drop_entries(&rows)
        .iter()
        .position(|entry| matches!(entry, DropEntry::Row(at) if rows[*at].id == id))
        .unwrap_or_else(|| panic!("{id} is a row of {}", menu.label()))
}

/// The drop entry of `menu` that is the submenu `caption`.
fn submenu_of(state: &AppState, menu: AppMenu, caption: &str) -> usize {
    let rows = state.menu_rows(menu);
    drop_entries(&rows)
        .iter()
        .position(|entry| matches!(entry, DropEntry::Sub { caption: c, .. } if *c == caption))
        .unwrap_or_else(|| panic!("{caption} is a submenu of {}", menu.label()))
}

fn press(state: &mut AppState, cx: &mut gpui::Context<AppState>, key: &str) -> bool {
    let nav = MenuNav::from_key(key).unwrap_or_else(|| panic!("{key} is a menu key"));
    state.menu_key(nav, cx)
}

#[test]
fn f10_opens_the_first_menu_lit_and_closes_it_again() {
    let mut state = state();
    let host = keys::host_of(&state.ctx).expect("the ui plugin mounts the registry");
    let binding = host
        .resolve(KeyRoute::Editor, "f10", false, false, false)
        .expect("F10 is bound on the editor route");
    let KeyBody::Plain(run) = binding.run else {
        panic!("F10 runs against the state alone");
    };
    run(&mut state);
    assert_eq!(state.open_menu, Some(AppMenu::ALL[0]));
    assert_eq!(state.menu_row, Some(0), "the first row is lit");
    run(&mut state);
    assert_eq!(state.open_menu, None, "F10 again closes the drop");
    assert_eq!(state.menu_row, None, "a closed drop lights nothing");
}

#[test]
fn up_and_down_wrap_in_every_menu() {
    with_entity(state(), |state, cx| {
        for menu in AppMenu::ALL {
            state.toggle_menu(menu);
            let len = captions(state, menu).len();
            assert!(len > 0, "{} has rows", menu.label());
            assert_eq!(state.menu_row, None, "a click lights nothing");
            assert!(press(state, cx, "up"));
            assert_eq!(
                state.menu_row,
                Some(len - 1),
                "Up from nothing lights the last row of {}",
                menu.label()
            );
            assert!(press(state, cx, "down"));
            assert_eq!(
                state.menu_row,
                Some(0),
                "Down past the last row wraps in {}",
                menu.label()
            );
            assert!(press(state, cx, "up"));
            assert_eq!(
                state.menu_row,
                Some(len - 1),
                "Up past the first row wraps in {}",
                menu.label()
            );
            assert!(press(state, cx, "home"));
            assert_eq!(state.menu_row, Some(0));
            assert!(press(state, cx, "end"));
            assert_eq!(state.menu_row, Some(len - 1));
            state.close_menu();
        }
    });
}

#[test]
fn left_and_right_walk_every_menu_and_wrap() {
    with_entity(state(), |state, cx| {
        state.toggle_menu_from_keyboard();
        let count = AppMenu::ALL.len();
        for step in 1..=count {
            let from = state.open_menu;
            assert!(press(state, cx, "right"));
            if state.open_menu == from {
                // A menu whose top entry is a submenu enters it first, lit at
                // its top; the next Right leaves for the neighbour.
                assert_eq!(
                    state.menu_flyout,
                    Some((0, Some(0))),
                    "Right stayed on {from:?} without entering its top submenu"
                );
                assert!(press(state, cx, "right"));
            }
            assert_eq!(state.open_menu, Some(AppMenu::ALL[step % count]));
            assert_eq!(
                state.menu_row,
                Some(0),
                "a menu reached by Right is lit at its top"
            );
        }
        for step in 1..=count {
            assert!(press(state, cx, "left"));
            assert_eq!(state.open_menu, Some(AppMenu::ALL[(count - step) % count]));
            assert_eq!(state.menu_row, Some(0));
        }
    });
}

#[test]
fn enter_runs_the_lit_row_and_closes_the_drop() {
    with_entity(state(), |state, cx| {
        state.toggle_menu_from_keyboard();
        let at = entry_of(state, AppMenu::ALL[0], "file-project-settings");
        for _ in 0..at {
            press(state, cx, "down");
        }
        assert_eq!(state.menu_row, Some(at));
        assert!(press(state, cx, "enter"));
        assert_eq!(state.dialog, DialogKind::Settings, "the lit row ran");
        assert_eq!(state.open_menu, None, "running a row closes the drop");
        assert_eq!(state.menu_row, None);
    });
}

/// Right on a lit submenu opens its flyout with the first row lit, Down walks
/// the flyout, Left closes it, and Enter runs the flyout's lit row.
#[test]
fn a_submenu_opens_to_the_right_and_runs_its_lit_row() {
    with_entity(state(), |state, cx| {
        state.toggle_menu_from_keyboard();
        let export = submenu_of(state, AppMenu::File, "Export");
        state.menu_row = Some(export);
        assert!(press(state, cx, "right"));
        assert_eq!(state.menu_flyout, Some((export, Some(0))));
        assert_eq!(state.open_menu, Some(AppMenu::File), "Right stays in File");
        assert!(press(state, cx, "left"));
        assert_eq!(state.menu_flyout, None, "Left closes the flyout");
        assert_eq!(state.menu_row, Some(export), "and leaves its entry lit");
        assert!(press(state, cx, "enter"));
        assert_eq!(
            state.menu_flyout,
            Some((export, Some(0))),
            "Enter opens it too"
        );
        // The flyout's first row is Media…, which opens the export dialog.
        assert!(press(state, cx, "enter"));
        assert_eq!(state.dialog, DialogKind::Export);
        assert_eq!(state.open_menu, None);
    });
}

/// A disabled row lit from the keyboard stays unrun: Enter leaves the drop
/// open and puts the reason on the status line.
#[test]
fn enter_on_a_disabled_row_reports_why_and_keeps_the_drop_open() {
    with_entity(state(), |state, cx| {
        state.toggle_menu_from_keyboard();
        let revert = entry_of(state, AppMenu::File, "file-revert");
        state.menu_row = Some(revert);
        assert!(press(state, cx, "enter"));
        assert_eq!(state.open_menu, Some(AppMenu::File));
        assert_eq!(
            state.status_message.as_deref(),
            Some("The project has never been saved")
        );
    });
}

#[test]
fn enter_with_nothing_lit_runs_nothing() {
    with_entity(state(), |state, cx| {
        state.toggle_menu(AppMenu::ALL[0]);
        assert!(press(state, cx, "space"), "an open drop takes the key");
        assert_eq!(state.route, UiRoute::Editor, "no row ran");
        assert_eq!(
            state.open_menu,
            Some(AppMenu::ALL[0]),
            "the drop stays open"
        );
    });
}

#[test]
fn a_letter_lights_the_next_row_starting_with_it_and_wraps() {
    with_entity(state(), |state, cx| {
        for menu in AppMenu::ALL {
            state.toggle_menu(menu);
            let rows = captions(state, menu);
            let first = |row: usize| rows[row].chars().next().map(|ch| ch.to_ascii_lowercase());
            // Every letter some entry starts with, walked twice so the second
            // pass wraps past the end.
            let letters: std::collections::BTreeSet<char> =
                (0..rows.len()).filter_map(first).collect();
            for letter in letters.into_iter().filter(|ch| ch.is_alphanumeric()) {
                let hits: Vec<usize> = (0..rows.len())
                    .filter(|&row| first(row) == Some(letter))
                    .collect();
                state.menu_row = None;
                for round in 0..hits.len() * 2 {
                    assert!(press(state, cx, &letter.to_string()));
                    assert_eq!(
                        state.menu_row,
                        Some(hits[round % hits.len()]),
                        "{letter} in {} lights its rows in order",
                        menu.label()
                    );
                }
            }
            // A letter no entry starts with leaves the light where it was.
            let lit = state.menu_row;
            if let Some(stray) =
                ('a'..='z').find(|ch| (0..rows.len()).all(|row| first(row) != Some(*ch)))
            {
                press(state, cx, &stray.to_string());
                assert_eq!(
                    state.menu_row,
                    lit,
                    "{stray} matches no row of {}",
                    menu.label()
                );
            }
            state.close_menu();
        }
    });
}

#[test]
fn escape_closes_a_drop_however_it_was_opened() {
    with_entity(state(), |state, cx| {
        for open in [
            AppState::toggle_menu_from_keyboard as fn(&mut AppState),
            |s| s.toggle_menu(AppMenu::ALL[1]),
        ] {
            open(state);
            assert!(state.open_menu.is_some());
            assert!(press(state, cx, "escape"));
            assert_eq!(state.open_menu, None);
            assert_eq!(state.menu_row, None);
        }
    });
}

#[test]
fn a_closed_bar_passes_every_key_on() {
    with_entity(state(), |state, cx| {
        for key in [
            "up", "down", "left", "right", "home", "end", "enter", "space", "escape", "a",
        ] {
            assert!(
                !press(state, cx, key),
                "{key} with no drop open is the bindings'"
            );
        }
        assert_eq!(state.open_menu, None);
    });
}

#[test]
fn the_pointer_moves_a_keyboard_light_and_lights_nothing_on_its_own() {
    let mut state = state();
    state.toggle_menu(AppMenu::ALL[0]);
    state.point_menu_row(2);
    assert_eq!(
        state.menu_row, None,
        "a drop the pointer opened keeps its hover fade"
    );
    state.close_menu();
    state.toggle_menu_from_keyboard();
    state.point_menu_row(2);
    assert_eq!(state.menu_row, Some(2), "a lit drop follows the pointer");
}

#[test]
fn only_navigation_keys_are_menu_keys() {
    assert_eq!(MenuNav::from_key("enter"), Some(MenuNav::Run));
    assert_eq!(MenuNav::from_key("space"), Some(MenuNav::Run));
    assert_eq!(MenuNav::from_key("E"), Some(MenuNav::Letter('e')));
    assert_eq!(MenuNav::from_key("3"), Some(MenuNav::Letter('3')));
    for key in ["f10", "tab", "backspace", "-", ""] {
        assert_eq!(MenuNav::from_key(key), None, "{key} reaches the bindings");
    }
}
