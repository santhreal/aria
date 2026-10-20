//! The platform menu bar, derived from the registry the CSD bar draws.
//!
//! macOS puts an application's menus in the system bar, so a client-side bar is
//! the wrong shape there: the top of the screen shows nothing, and the app reads
//! as a port. The rows are the same rows either way,
//! so this derives `gpui::Menu` from [`menu_host::MenuHost`] rather than from a
//! second table — a hand-written native list is exactly how a row comes to exist
//! in one bar and not the other.
//!
//! One action type carries the row it runs. Per-row action types would be 122
//! declarations that have to be kept in step with the registry by hand, which is
//! the drift this crate spends its registries avoiding.

use gpui::{App, Menu, MenuItem};

use super::menu_host;
use crate::app::AppState;
use crate::types::AppMenu;

/// Run one registry row, named the way the registry names it.
///
/// `menu` is the menu's label and `id` the row's id, because those are what
/// `menu_items::run_item` already takes: the
/// native bar and the palette reach the same code by the same name.
// `no_json`: the native bar builds this in Rust from the registry, and nothing
// deserializes it from a keymap, so the action needs no JSON schema.
#[derive(Clone, PartialEq, Debug, Default, gpui::Action)]
#[action(namespace = aria, no_json)]
pub struct RunMenuRow {
    pub menu: String,
    pub id: String,
}

/// The menus the platform bar should show, in bar order.
///
/// Empty when no contributor is mounted, which is a fixture without the ui
/// plugin rather than a shipped state; a menu with no rows is dropped, because
/// macOS draws an empty title as a dead end.
pub fn native_menus(state: &AppState) -> Vec<Menu> {
    let Some(host) = menu_host::host_of(&state.ctx) else {
        return Vec::new();
    };
    let mut menus = Vec::with_capacity(AppMenu::ALL.len());
    for menu in AppMenu::ALL {
        let rows = host.items(menu, state);
        let native = |item: &super::menu_items::MenuItem| {
            MenuItem::action(
                super::menu_items::row_caption(item, state),
                RunMenuRow {
                    menu: menu.label().to_string(),
                    id: item.id.to_string(),
                },
            )
            .disabled(item.blocked_by(state).is_some())
            .checked(item.is_checked(state))
        };
        let items: Vec<MenuItem> = super::menu_items::drop_entries(&rows)
            .into_iter()
            .map(|entry| match entry {
                super::menu_items::DropEntry::Row(at) => native(&rows[at]),
                super::menu_items::DropEntry::Sub {
                    caption,
                    rows: held,
                } => MenuItem::submenu(
                    Menu::new(caption).items(held.iter().map(|at| native(&rows[*at]))),
                ),
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        menus.push(Menu::new(menu.label()).items(items));
    }
    menus
}

/// The menu a label names, or `None` when it names no menu.
///
/// The native bar hands back a label because that is what it displayed; this is
/// the only place that mapping lives, so a renamed menu cannot leave the native
/// bar dispatching into nothing.
pub fn menu_named(label: &str) -> Option<AppMenu> {
    AppMenu::ALL.into_iter().find(|menu| menu.label() == label)
}

/// Run the registry row a native menu action names.
///
/// Reports whether it ran: a row that vanished between the bar being built and
/// the click is a no-op, not a wrong action, which is the same rule the palette
/// follows.
pub fn dispatch(
    action: &RunMenuRow,
    state: &mut AppState,
    cx: &mut gpui::Context<AppState>,
) -> bool {
    let Some(menu) = menu_named(&action.menu) else {
        return false;
    };
    super::menu_items::run_item(menu, &action.id, state, cx)
}

/// Install the platform menu bar, and build it again whenever a plugin's
/// commands or menu rows change, or the context keys their enablement
/// conditions read. A no-op where the platform has none.
///
/// Linux and Windows keep the client-side bar `chrome::menu_bar` draws, so
/// nothing is registered there: two live bars would double every row.
///
/// Takes the entity, not a borrowed state: deriving the menus and registering
/// them are one immutable and one mutable use of the same context, so the read
/// has to end before `set_menus` begins.
pub fn install(state: &gpui::Entity<AppState>, cx: &mut App) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let menus = native_menus(state.read(cx));
    cx.set_menus(menus);
    let drawn_from = |state: &AppState| (state.ctx.command_revision(), state.ctx.context_keys());
    let mut built = drawn_from(state.read(cx));
    cx.observe(state, move |state, cx| {
        let now = drawn_from(state.read(cx));
        if now == built {
            return;
        }
        built = now;
        let menus = native_menus(state.read(cx));
        cx.set_menus(menus);
    })
    .detach();
}

#[cfg(test)]
#[path = "native_menus_tests.rs"]
mod tests;
