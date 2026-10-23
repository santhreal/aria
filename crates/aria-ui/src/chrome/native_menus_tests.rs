//! The platform menu bar against the registry it is derived from.
//!
//! The class these close: a native bar written by hand drifts from the bar the
//! rest of the app draws, and the drift is invisible on the platform that does
//! not use it — a Linux gate would never see a macOS menu missing half its rows.
//! So the derivation is asserted here, on any platform: every menu the bar
//! titles is present with every row the registry offers, in registry order, and
//! a row dispatched by label and id runs the same code the CSD bar runs.
//!
//! Not covered: that macOS actually draws them. `set_menus` is a platform call
//! with no readable state, and this gate has no macOS.

use aria_plugin::PluginContext;
use gpui::AppContext as _;

use super::*;
use crate::chrome::menu_items;

fn state() -> AppState {
    let dir = crate::scratch_pool::scratch("native-menus");
    let ctx = PluginContext::new();
    // The shipped contributor is what the bar reads; mounting the ui plugin is
    // what registers it, and a headless machine can fail the GPU-bound mounts.
    let _ = crate::plugins::mount_all(&ctx, &dir);
    AppState::new(ctx, dir).expect("state")
}

#[test]
fn every_menu_carries_every_row_the_registry_offers() {
    let state = state();
    let menus = native_menus(&state);
    assert!(!menus.is_empty(), "no menu was derived at all");

    for menu in AppMenu::ALL {
        let rows = menu_items::items_of(menu, &state);
        let native = menus.iter().find(|m| m.name.as_ref() == menu.label());
        if rows.is_empty() {
            assert!(
                native.is_none(),
                "{} offers no row and must not be titled",
                menu.label()
            );
            continue;
        }
        let native = native.unwrap_or_else(|| panic!("{} is missing", menu.label()));
        let entries = menu_items::drop_entries(&rows);
        assert_eq!(
            native.items.len(),
            entries.len(),
            "{} has {} native items for {} drop entries",
            menu.label(),
            native.items.len(),
            entries.len()
        );
        for (item, entry) in native.items.iter().zip(entries.iter()) {
            match (item, entry) {
                (gpui::MenuItem::Action { .. }, menu_items::DropEntry::Row(at)) => {
                    assert_native_row(menu, item, &rows[*at], &state);
                }
                (
                    gpui::MenuItem::Submenu(sub),
                    menu_items::DropEntry::Sub {
                        caption,
                        rows: held,
                    },
                ) => {
                    assert_eq!(
                        sub.name.as_ref(),
                        *caption,
                        "{} named a submenu wrongly",
                        menu.label()
                    );
                    assert_eq!(
                        sub.items.len(),
                        held.len(),
                        "{} > {caption} lost rows",
                        menu.label()
                    );
                    for (item, at) in sub.items.iter().zip(held.iter()) {
                        assert_native_row(menu, item, &rows[*at], &state);
                    }
                }
                // `MenuItem` carries no `Debug`, so name the menu and the entry
                // the registry expected there instead of the item itself.
                _ => panic!(
                    "{} contributed the wrong kind of item where {} belongs",
                    menu.label(),
                    entry.caption(&rows)
                ),
            }
        }
    }
}

/// One native row against the registry row it was derived from: the caption the
/// CSD bar draws, which states a setting's current value beside the label, and
/// the same disabled and checked state.
fn assert_native_row(
    menu: AppMenu,
    item: &gpui::MenuItem,
    row: &menu_items::MenuItem,
    state: &AppState,
) {
    let gpui::MenuItem::Action {
        name,
        disabled,
        checked,
        ..
    } = item
    else {
        panic!(
            "{} drew {} as something other than a row",
            menu.label(),
            row.label
        );
    };
    assert_eq!(
        name.as_ref(),
        menu_items::row_caption(row, state),
        "{} drew its rows out of registry order",
        menu.label()
    );
    assert_eq!(
        *disabled,
        row.blocked_by(state).is_some(),
        "{} > {} is disabled in one bar and not the other",
        menu.label(),
        row.label
    );
    assert_eq!(
        *checked,
        row.is_checked(state),
        "{} > {} is checked in one bar and not the other",
        menu.label(),
        row.label
    );
}

#[test]
fn a_menu_label_names_the_menu_it_titles() {
    for menu in AppMenu::ALL {
        assert_eq!(menu_named(menu.label()), Some(menu));
    }
    assert_eq!(menu_named("Sequenced"), None, "a near miss is not a menu");
    assert_eq!(menu_named(""), None);
}

#[test]
fn a_dispatched_row_runs_the_registry_entry() {
    let mut cx = gpui::TestAppContext::single();
    let entity = cx.update(|cx| cx.new(|_| state()));
    // "New" is a File row every build offers, and it leaves an observable trace:
    // the session gets a project it did not have.
    let ran = entity.update(&mut cx, |state, cx| {
        dispatch(
            &RunMenuRow {
                menu: "File".to_string(),
                id: "file-new".to_string(),
            },
            state,
            cx,
        )
    });
    assert!(ran, "the row the registry offers did not run");

    let unknown_menu = entity.update(&mut cx, |state, cx| {
        dispatch(
            &RunMenuRow {
                menu: "Nowhere".to_string(),
                id: "file-new".to_string(),
            },
            state,
            cx,
        )
    });
    assert!(!unknown_menu, "a menu that does not exist ran a row");

    let unknown_row = entity.update(&mut cx, |state, cx| {
        dispatch(
            &RunMenuRow {
                menu: "File".to_string(),
                id: "file-nonesuch".to_string(),
            },
            state,
            cx,
        )
    });
    assert!(
        !unknown_row,
        "a row that does not exist reported that it ran"
    );
}
