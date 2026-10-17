//! Menu rows plugins declare, as the menu bar and the context menus draw them.
//!
//! A plugin places a command in a menu through
//! [`PluginContext::register_menu_entry`] or a `menu` in its `plugin.toml`. The
//! row holds no code: choosing it runs the command through
//! [`AppState::run_plugin_command`]. A row is drawn while its command is
//! registered and its enablement condition holds, below the menu's own rows,
//! in registration order.

use aria_plugin::{MenuLocation, PluginContext};

use super::menu_items::MenuItem;
use crate::app::AppState;
use crate::interned::intern;
use crate::types::AppMenu;

/// The plugin location a menu bar menu is.
pub(crate) fn location_of(menu: AppMenu) -> MenuLocation {
    match menu {
        AppMenu::File => MenuLocation::File,
        AppMenu::Edit => MenuLocation::Edit,
        AppMenu::Clip => MenuLocation::Clip,
        AppMenu::Sequence => MenuLocation::Sequence,
        AppMenu::Markers => MenuLocation::Markers,
        AppMenu::Effect => MenuLocation::Effect,
        AppMenu::Animation => MenuLocation::Animation,
        AppMenu::View => MenuLocation::View,
        AppMenu::Window => MenuLocation::Window,
        AppMenu::Help => MenuLocation::Help,
    }
}

/// A plugin row: the command it runs, the text it draws, and the asset path
/// of its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PluginRow {
    pub(crate) command: &'static str,
    /// The text a flat menu draws: `Group: Title` for a row in a group.
    pub(crate) label: &'static str,
    /// The command's title alone, which the bar draws inside the group's
    /// submenu.
    pub(crate) title: &'static str,
    /// The group the row was placed in, which the bar draws as a submenu.
    pub(crate) group: Option<&'static str>,
    pub(crate) icon: Option<&'static str>,
}

/// Every row plugins placed in `location` whose command is registered and
/// enabled under the published context keys, in registration order. A command
/// placed twice in one location is drawn once.
pub(crate) fn rows_in(ctx: &PluginContext, location: MenuLocation) -> Vec<PluginRow> {
    let entries = ctx.menu_entries();
    if !entries.iter().any(|entry| entry.path.location == location) {
        return Vec::new();
    }
    let commands = ctx.commands();
    let keys = ctx.context_keys();
    let mut rows: Vec<PluginRow> = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.path.location == location)
    {
        let Some(spec) = commands.iter().find(|spec| spec.id == entry.command) else {
            continue;
        };
        if !spec.enabled(keys) || rows.iter().any(|row| row.command == spec.id) {
            continue;
        }
        rows.push(PluginRow {
            command: intern(&spec.id),
            label: intern(&entry.path.row_label(&spec.title)),
            title: intern(&spec.title),
            group: entry.path.group.as_deref().map(intern),
            icon: spec.icon.as_ref().map(crate::icons::plugin_icon_path),
        });
    }
    rows
}

/// The icon of `item` when it is one of `rows`. Rows are matched by the
/// interned id, so a shipped row with equal text is not one.
pub(crate) fn icon_of(rows: &[PluginRow], item: &MenuItem) -> Option<&'static str> {
    rows.iter()
        .find(|row| std::ptr::eq(row.command, item.id))
        .and_then(|row| row.icon)
}

/// The plugin rows of one menu bar menu, as menu items. A row placed in a
/// group sits in the group's submenu under its own title.
pub(crate) fn bar_items(menu: AppMenu, state: &AppState) -> Vec<MenuItem> {
    rows_in(&state.ctx, location_of(menu))
        .into_iter()
        .map(|row| match row.group {
            Some(group) => MenuItem::new(row.command, row.title, run_row).under(group),
            None => MenuItem::new(row.command, row.label, run_row),
        })
        .collect()
}

fn run_row(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    state.run_plugin_command(id);
    cx.notify();
}

#[cfg(test)]
#[path = "plugin_menus_tests.rs"]
mod tests;
