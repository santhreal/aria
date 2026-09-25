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
        assert_eq!(
            native.items.len(),
            rows.len(),
            "{} has {} native item(s) for {} registry row(s)",
            menu.label(),
            native.items.len(),
            rows.len()
        );
        for (item, row) in native.items.iter().zip(rows.iter()) {
            match item {
                gpui::MenuItem::Action { name, .. } => assert_eq!(
                    name.as_ref(),
                    row.label,
                    "{} drew its rows out of registry order",
                    menu.label()
                ),
                // `MenuItem` carries no `Debug`, so name the menu and the row
                // the registry expected there instead of the item itself.
                _ => panic!(
                    "{} contributed a non-action item where {} belongs",
                    menu.label(),
                    row.label
                ),
            }
        }
    }
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
