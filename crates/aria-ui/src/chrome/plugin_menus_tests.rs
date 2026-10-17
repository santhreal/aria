//! Plugin menu rows as the menu bar draws them.
//!
//! The class closed here is a declared menu row that is drawn while nothing can
//! run it, drawn twice, or allowed to replace a shipped row. Locations and menus
//! are read from `MenuLocation::ALL` and `AppMenu::ALL`, so a menu added to
//! either side without the other fails here.
//!
//! Not covered: clicking a row, which needs a window; the row runs
//! `run_plugin_command`, which `app/plugin_commands_tests.rs` covers.

use aria_plugin::{CommandSpec, MenuCommand, MenuLocation, MenuPath, PluginContext, Registration};

use super::{PluginRow, bar_items, location_of, rows_in};
use crate::app::AppState;
use crate::types::AppMenu;

fn command(ctx: &PluginContext, id: &str, title: &str) -> Registration {
    ctx.register_command(CommandSpec::new(id, title), |_: &PluginContext| Ok(()))
        .expect("the command registers")
}

fn place(ctx: &PluginContext, command: &str, path: &str) -> Registration {
    ctx.register_menu_entry(MenuCommand {
        command: command.into(),
        path: MenuPath::parse(path).expect("a menu path"),
    })
    .expect("the menu entry registers")
}

#[test]
fn every_menu_bar_location_is_one_menu_of_the_bar() {
    for location in MenuLocation::ALL {
        let menus: Vec<AppMenu> = AppMenu::ALL
            .into_iter()
            .filter(|menu| location_of(*menu) == location)
            .collect();
        let expected = usize::from(location.in_menu_bar() || location == MenuLocation::Clip);
        assert_eq!(menus.len(), expected, "{location:?} is drawn by {menus:?}");
    }
}

#[test]
fn a_row_is_drawn_while_its_command_is_registered_once_per_command() {
    let ctx = PluginContext::new();
    let grouped = place(&ctx, "grade.reset", "Sequence/Color Tools");
    assert!(
        rows_in(&ctx, MenuLocation::Sequence).is_empty(),
        "a row was drawn before anything could run it"
    );

    let registered = command(&ctx, "grade.reset", "Reset Grade");
    let _again = place(&ctx, "grade.reset", "Sequence");
    let _other = command(&ctx, "grade.auto", "Auto Grade");
    let _other_row = place(&ctx, "grade.auto", "Sequence");
    assert_eq!(
        rows_in(&ctx, MenuLocation::Sequence),
        [
            PluginRow {
                command: "grade.reset",
                label: "Color Tools: Reset Grade",
                title: "Reset Grade",
                group: Some("Color Tools"),
                icon: None,
            },
            PluginRow {
                command: "grade.auto",
                label: "Auto Grade",
                title: "Auto Grade",
                group: None,
                icon: None,
            },
        ],
        "rows are drawn once per command, in registration order"
    );
    assert!(rows_in(&ctx, MenuLocation::File).is_empty());

    drop(grouped);
    assert_eq!(
        rows_in(&ctx, MenuLocation::Sequence)[0].label,
        "Reset Grade",
        "the row left with the registration that placed it"
    );
    drop(registered);
    assert_eq!(
        rows_in(&ctx, MenuLocation::Sequence),
        [PluginRow {
            command: "grade.auto",
            label: "Auto Grade",
            title: "Auto Grade",
            group: None,
            icon: None,
        }]
    );
}

/// The bar draws plugin rows after every shipped row, in every menu, and a
/// plugin row whose id a shipped row holds is dropped.
#[test]
fn the_bar_appends_plugin_rows_and_keeps_shipped_ids() {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("plugin-menus");
    let _ = crate::plugins::mount_all(&ctx, &dir);
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("a project");
    let host = crate::chrome::menu_host::host_of(&state.ctx).expect("a menu host");

    for menu in AppMenu::ALL {
        let shipped: Vec<&str> = host
            .items(menu, &state)
            .iter()
            .map(|item| item.id)
            .collect();
        let location = location_of(menu).label();
        let _command = command(&state.ctx, "grade.reset", "Reset Grade");
        let _row = place(&state.ctx, "grade.reset", &format!("{location}/Grade"));
        let _shadow = shipped.first().map(|id| {
            let registration = command(&state.ctx, id, "Stolen");
            (registration, place(&state.ctx, id, location))
        });

        let items = host.items(menu, &state);
        let ids: Vec<&str> = items.iter().map(|item| item.id).collect();
        let mut expected = shipped.clone();
        expected.push("grade.reset");
        assert_eq!(ids, expected, "{menu:?}");
        // The bar draws the group as a submenu holding the command's title.
        assert_eq!(
            items.last().map(|item| (item.label, item.under)),
            Some(("Reset Grade", Some("Grade")))
        );
        assert!(
            items.iter().all(|item| item.label != "Stolen"),
            "{menu:?}: a plugin row replaced a shipped row"
        );
        assert_eq!(
            bar_items(menu, &state).len(),
            usize::from(!shipped.is_empty()) + 1,
            "{menu:?}"
        );
    }
}
