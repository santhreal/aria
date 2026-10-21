//! The Help menu: the shortcut sheet, the plugin guide, the data folder,
//! updates, and the about card.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::{AppState, DialogKind};
use crate::types::HomeRail;

use super::menu_items::{MenuItem, MenuItems};

static HELP_ITEMS: &[MenuItem] = &[
    MenuItem::new("help-learn", "Keyboard Shortcuts", |_, s, _| {
        s.set_home_rail(HomeRail::Learn);
        s.request_go_home();
    }),
    MenuItem::new("help-plugins", "Plugin Development", |_, _, cx| {
        cx.open_url(PLUGIN_GUIDE)
    }),
    MenuItem::new("help-data", "Open Data Folder", |_, s, _| {
        crate::app::recents::reveal_in_folder(&s.data_dir)
    }),
    MenuItem::new("help-updates", "Check for Updates", |_, s, cx| {
        s.check_for_updates(cx)
    }),
    MenuItem::new("help-about", "About", |_, s, _| {
        s.open_dialog(DialogKind::About)
    })
    .checked(|_, s| s.dialog == DialogKind::About),
];

/// The plugin developer guide, in the source repository of this build.
pub(crate) const PLUGIN_GUIDE: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/plugins/README.md"
);

pub(super) fn help_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(HELP_ITEMS)
}
